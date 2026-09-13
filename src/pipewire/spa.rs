use pipewire::spa::param::ParamType;
use pipewire::spa::pod::deserialize::PodDeserializer;
use pipewire::spa::pod::serialize::PodSerializer;
use pipewire::spa::pod::{Object, Pod, Property, Value, ValueArray};
use pipewire::spa::utils::{Id, SpaTypes};
use std::io::Cursor;

/// SPA property keys from spa/param/props.h (PipeWire >= 1.0), used to build
/// and parse the Props param of a node.
const SPA_PROP_MUTE: u32 = 0x10004;
const SPA_PROP_CHANNEL_VOLUMES: u32 = 0x10008;

/// SPA property keys from spa/param/route.h, used to address a device route
/// when mirroring node volume/mute changes onto the hardware endpoint.
const SPA_PARAM_ROUTE_INDEX: u32 = 1;
const SPA_PARAM_ROUTE_DIRECTION: u32 = 2;
const SPA_PARAM_ROUTE_DEVICE: u32 = 3;
const SPA_PARAM_ROUTE_PROPS: u32 = 10;
const SPA_PARAM_ROUTE_DEVICES: u32 = 11;

/// Static geometry of a device route, used to address it when mirroring a node
/// volume/mute change onto the hardware endpoint.
#[derive(Debug, Clone)]
pub(super) struct RouteInfo {
    pub(super) index: u32,
    pub(super) direction: u32,
    pub(super) device: u32,
    pub(super) devices: Vec<u32>,
}

/// Serialize a Props object carrying only the properties being set. The SPA
/// `Props` param is sparse, so omitted properties are left untouched; this lets
/// a mute-only write avoid resetting an unknown volume to 100%.
pub(super) fn build_props_pod(volume_linear: Option<f32>, muted: Option<bool>) -> Vec<u8> {
    let mut properties = Vec::new();
    if let Some(linear) = volume_linear {
        properties.push(Property::new(
            SPA_PROP_CHANNEL_VOLUMES,
            Value::ValueArray(ValueArray::Float(vec![linear])),
        ));
    }
    if let Some(muted) = muted {
        properties.push(Property::new(SPA_PROP_MUTE, Value::Bool(muted)));
    }

    let object = Value::Object(Object {
        type_: SpaTypes::ObjectParamProps.as_raw(),
        id: ParamType::Props.as_raw(),
        properties,
    });

    let (cursor, _) =
        PodSerializer::serialize(Cursor::new(Vec::new()), &object).expect("serialize Props pod");
    cursor.into_inner()
}

/// Parse a device `Route` param pod into its static geometry. Returns `None`
/// when the pod is not a Route object.
pub(super) fn parse_route(pod: &Pod) -> Option<RouteInfo> {
    let (_, value) = PodDeserializer::deserialize_from::<Value>(pod.as_bytes()).ok()?;
    let Value::Object(obj) = value else {
        return None;
    };
    if obj.id != ParamType::Route.as_raw() {
        return None;
    }
    let mut index = None;
    let mut direction = 0;
    let mut device = None;
    let mut devices = Vec::new();
    for prop in &obj.properties {
        match prop.key {
            k if k == SPA_PARAM_ROUTE_INDEX => {
                if let Value::Int(v) = &prop.value {
                    index = u32::try_from(*v).ok();
                }
            }
            k if k == SPA_PARAM_ROUTE_DIRECTION => {
                if let Value::Id(v) = &prop.value {
                    direction = v.0;
                }
            }
            k if k == SPA_PARAM_ROUTE_DEVICE => {
                if let Value::Int(v) = &prop.value {
                    device = u32::try_from(*v).ok();
                }
            }
            k if k == SPA_PARAM_ROUTE_DEVICES => {
                if let Value::ValueArray(ValueArray::Int(v)) = &prop.value {
                    devices = v.iter().filter_map(|x| u32::try_from(*x).ok()).collect();
                }
            }
            _ => {}
        }
    }
    let device = device.or_else(|| devices.first().copied())?;
    Some(RouteInfo {
        index: index?,
        direction,
        device,
        devices,
    })
}

/// Serialize a `Route` param addressed at `route`, carrying only the properties
/// being set. Sibling routes and unrelated properties are left untouched.
pub(super) fn build_route_pod(
    route: &RouteInfo,
    volume_linear: Option<f32>,
    muted: Option<bool>,
) -> Vec<u8> {
    let mut properties = Vec::new();
    if let Some(linear) = volume_linear {
        properties.push(Property::new(
            SPA_PROP_CHANNEL_VOLUMES,
            Value::ValueArray(ValueArray::Float(vec![linear])),
        ));
    }
    if let Some(muted) = muted {
        properties.push(Property::new(SPA_PROP_MUTE, Value::Bool(muted)));
    }
    let route_props = Value::Object(Object {
        type_: SpaTypes::ObjectParamProps.as_raw(),
        id: ParamType::Route.as_raw(),
        properties,
    });

    let object = Value::Object(Object {
        type_: SpaTypes::ObjectParamRoute.as_raw(),
        id: ParamType::Route.as_raw(),
        properties: vec![
            Property::new(SPA_PARAM_ROUTE_INDEX, Value::Int(route.index as i32)),
            Property::new(SPA_PARAM_ROUTE_DIRECTION, Value::Id(Id(route.direction))),
            Property::new(SPA_PARAM_ROUTE_DEVICE, Value::Int(route.device as i32)),
            Property::new(SPA_PARAM_ROUTE_PROPS, route_props),
        ],
    });

    let (cursor, _) =
        PodSerializer::serialize(Cursor::new(Vec::new()), &object).expect("serialize Route pod");
    cursor.into_inner()
}

/// A partial `Props` update parsed from a node param pod. `None` fields mean
/// "unchanged" and must not overwrite the object's current value.
pub(super) struct PropUpdate {
    pub(super) volume: Option<f64>,
    pub(super) muted: Option<bool>,
}

/// Extract the volume and mute carried by a Props object. The per-channel
/// volumes are the effective linear volumes; SPA converts to the normalized
/// volume with a cubic curve, so `normalized = cbrt(linear)`.
fn parse_props_object(obj: &Object) -> (Option<f64>, Option<bool>) {
    let mut volume = None;
    let mut muted = None;
    for prop in &obj.properties {
        match prop.key {
            k if k == SPA_PROP_MUTE => {
                if let Value::Bool(b) = &prop.value {
                    muted = Some(*b);
                }
            }
            k if k == SPA_PROP_CHANNEL_VOLUMES => {
                if let Value::ValueArray(ValueArray::Float(v)) = &prop.value {
                    volume = v.first().map(|f| f64::from(*f).cbrt());
                }
            }
            _ => {}
        }
    }
    (volume, muted)
}

/// Parse a node's `Props` param pod into a `PropUpdate`. Returns `None` when
/// the pod carries neither volume nor mute (e.g. empty/clear events).
pub(super) fn parse_props_volume(pod: &Pod) -> Option<PropUpdate> {
    let (_, value) = PodDeserializer::deserialize_from::<Value>(pod.as_bytes()).ok()?;
    let Value::Object(obj) = value else {
        return None;
    };
    let (volume, muted) = parse_props_object(&obj);
    if volume.is_none() && muted.is_none() {
        return None;
    }
    Some(PropUpdate { volume, muted })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #feature PW-MUTE — a mute-only Props pod must update `muted` without
    /// clobbering the (unknown) volume.
    #[test]
    fn test_parse_props_volume_mute_only() {
        let object = Value::Object(Object {
            type_: SpaTypes::ObjectParamProps.as_raw(),
            id: ParamType::Props.as_raw(),
            properties: vec![Property::new(SPA_PROP_MUTE, Value::Bool(true))],
        });
        let (cursor, _) =
            PodSerializer::serialize(Cursor::new(Vec::new()), &object).expect("serialize mute pod");
        let bytes = cursor.into_inner();
        let pod = Pod::from_bytes(&bytes).expect("deserialize pod");

        let update = parse_props_volume(pod).expect("mute-only pod yields an update");
        assert_eq!(update.volume, None);
        assert_eq!(update.muted, Some(true));
    }

    /// #feature PW-MUTE — a mute-only pod must omit channelVolumes entirely,
    /// so muting a node with unknown volume (e.g. a stream) doesn't reset it
    /// to 100%.
    #[test]
    fn test_build_props_pod_sparse() {
        let bytes = build_props_pod(None, Some(true));
        let pod = Pod::from_bytes(&bytes).expect("deserialize pod");
        let update = parse_props_volume(pod).expect("mute-only pod yields an update");
        assert_eq!(update.volume, None);
        assert_eq!(update.muted, Some(true));

        let bytes = build_props_pod(Some(0.125), None);
        let pod = Pod::from_bytes(&bytes).expect("deserialize pod");
        let update = parse_props_volume(pod).expect("volume-only pod yields an update");
        assert_eq!(update.muted, None);
        let volume = update.volume.expect("volume present");
        assert!((volume - 0.5).abs() < 1e-6);
    }

    /// #feature PW-VOL — a Props pod with neither volume nor mute yields no
    /// update (so an empty/clear event doesn't touch the object).
    #[test]
    fn test_parse_props_volume_empty_is_none() {
        let object = Value::Object(Object {
            type_: SpaTypes::ObjectParamProps.as_raw(),
            id: ParamType::Props.as_raw(),
            properties: vec![],
        });
        let (cursor, _) = PodSerializer::serialize(Cursor::new(Vec::new()), &object)
            .expect("serialize empty pod");
        let bytes = cursor.into_inner();
        let pod = Pod::from_bytes(&bytes).expect("deserialize pod");

        assert!(parse_props_volume(pod).is_none());
    }

    /// #feature PW-VOL — a built Route pod addresses the route by index,
    /// direction and device, and carries the requested volume/mute.
    #[test]
    fn test_build_route_pod_roundtrip() {
        let route = RouteInfo {
            index: 3,
            direction: 0,
            device: 3,
            devices: vec![3],
        };
        let bytes = build_route_pod(&route, Some(0.125), Some(true));
        let pod = Pod::from_bytes(&bytes).expect("deserialize pod");
        let (_, value) =
            PodDeserializer::deserialize_from::<Value>(pod.as_bytes()).expect("deserialize value");
        let Value::Object(obj) = value else {
            panic!("route pod is an object")
        };
        assert_eq!(obj.type_, SpaTypes::ObjectParamRoute.as_raw());
        assert_eq!(obj.id, ParamType::Route.as_raw());

        let mut index = None;
        let mut direction = None;
        let mut device = None;
        let mut volume = None;
        let mut muted = None;
        for prop in &obj.properties {
            match prop.key {
                k if k == SPA_PARAM_ROUTE_INDEX => {
                    if let Value::Int(v) = &prop.value {
                        index = Some(*v);
                    }
                }
                k if k == SPA_PARAM_ROUTE_DIRECTION => {
                    if let Value::Id(v) = &prop.value {
                        direction = Some(v.0);
                    }
                }
                k if k == SPA_PARAM_ROUTE_DEVICE => {
                    if let Value::Int(v) = &prop.value {
                        device = Some(*v);
                    }
                }
                k if k == SPA_PARAM_ROUTE_PROPS => {
                    if let Value::Object(props) = &prop.value {
                        let (vol, m) = parse_props_object(props);
                        volume = vol;
                        muted = m;
                    }
                }
                _ => {}
            }
        }
        assert_eq!(index, Some(3));
        assert_eq!(direction, Some(0));
        assert_eq!(device, Some(3));
        assert_eq!(muted, Some(true));
        assert!((volume.expect("volume present") - 0.5).abs() < 1e-6);
    }

    /// #feature PW-VOL — a mute-only Route pod must omit channelVolumes so
    /// muting a hardware endpoint doesn't reset its route volume to 100%.
    #[test]
    fn test_build_route_pod_mute_only() {
        let route = RouteInfo {
            index: 0,
            direction: 1,
            device: 4,
            devices: vec![4, 5, 6],
        };
        let bytes = build_route_pod(&route, None, Some(false));
        let pod = Pod::from_bytes(&bytes).expect("deserialize pod");
        let (_, value) =
            PodDeserializer::deserialize_from::<Value>(pod.as_bytes()).expect("deserialize value");
        let Value::Object(obj) = value else {
            panic!("route pod is an object")
        };
        let props = obj
            .properties
            .iter()
            .find(|p| p.key == SPA_PARAM_ROUTE_PROPS)
            .and_then(|p| match &p.value {
                Value::Object(props) => Some(parse_props_object(props)),
                _ => None,
            })
            .expect("nested props");
        assert_eq!(props.0, None);
        assert_eq!(props.1, Some(false));
    }

    /// #feature PW-VOL — a Route pod is parsed into the geometry used to address
    /// the route, including the devices it covers.
    #[test]
    fn test_parse_route() {
        let object = Value::Object(Object {
            type_: SpaTypes::ObjectParamRoute.as_raw(),
            id: ParamType::Route.as_raw(),
            properties: vec![
                Property::new(SPA_PARAM_ROUTE_INDEX, Value::Int(0)),
                Property::new(SPA_PARAM_ROUTE_DIRECTION, Value::Id(Id(1))),
                Property::new(SPA_PARAM_ROUTE_DEVICE, Value::Int(4)),
                Property::new(
                    SPA_PARAM_ROUTE_DEVICES,
                    Value::ValueArray(ValueArray::Int(vec![4, 5, 6])),
                ),
            ],
        });
        let (cursor, _) = PodSerializer::serialize(Cursor::new(Vec::new()), &object)
            .expect("serialize route pod");
        let bytes = cursor.into_inner();
        let pod = Pod::from_bytes(&bytes).expect("deserialize pod");

        let route = parse_route(pod).expect("route pod parses");
        assert_eq!(route.index, 0);
        assert_eq!(route.direction, 1);
        assert_eq!(route.device, 4);
        assert_eq!(route.devices, vec![4, 5, 6]);
    }
}
