use pipewire as pw;
use pipewire::device::{Device, DeviceListener};
use pipewire::link::{Link, LinkListener};
use pipewire::node::{Node, NodeListener};
use pipewire::spa::param::ParamType;
use pipewire::spa::pod::Pod;
use pipewire::spa::utils::Direction;
use pipewire::types::ObjectType;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use super::objects::{PwCommand, PwEvent, PwObject, PwObjectType};
use super::spa::{build_props_pod, build_route_pod, parse_props_volume, parse_route, RouteInfo};

/// How often the current object/edge snapshot is sent to the main thread.
const PUBLISH_INTERVAL: Duration = Duration::from_millis(50);
/// How often commands from the main thread are drained.
const COMMAND_INTERVAL: Duration = Duration::from_millis(10);
/// How long to wait for the first audio-graph snapshot before deciding the
/// PipeWire connection is not live and retrying.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);

/// Shared state, accessed only from the PipeWire main loop thread. All
/// listeners and timers run on that thread, so `Rc<RefCell>` is sufficient.
#[derive(Default)]
struct State {
    objects: Vec<PwObject>,
    /// Bound node proxies plus their listeners, keyed by node id. The proxy is
    /// used for native volume/mute writes; the listener keeps the subscription alive.
    nodes: HashMap<u32, (Node, NodeListener)>,
    /// Bound device proxies plus their listeners, keyed by device id.
    devices: HashMap<u32, (Rc<Device>, DeviceListener)>,
    /// Device id -> its routes, populated from `Route` param events.
    routes: HashMap<u32, Vec<RouteInfo>>,
    /// Node id -> (device id, ACP profile device index) for hardware endpoints.
    node_routes: HashMap<u32, (u32, u32)>,
    /// Port id -> (owning node id, direction, channel).
    ports: HashMap<u32, (u32, Direction, String)>,
    /// Link id -> (bound proxy, listener).
    links: HashMap<u32, (Link, LinkListener)>,
    /// Link id -> (output node id, input node id).
    link_nodes: HashMap<u32, (u32, u32)>,
}

fn node_name(state: &State, id: u32) -> Option<String> {
    state
        .objects
        .iter()
        .find(|o| o.id == id)
        .map(|o| o.name.clone())
}

fn resolve_node_id(state: &State, target: &str) -> Option<u32> {
    PwObject::find(&state.objects, target).map(|o| o.id)
}

fn apply_props(state: &State, node_id: u32, volume: Option<f64>, muted: Option<bool>) {
    let linear = volume.map(|v| v.max(0.0).powi(3) as f32);
    let bytes = build_props_pod(linear, muted);
    let Some(pod) = Pod::from_bytes(&bytes) else {
        return;
    };
    if let Some((node, _)) = state.nodes.get(&node_id) {
        node.set_param(ParamType::Props, 0, pod);
    }
}

fn node_route_info(state: &State, node_id: u32) -> Option<(u32, RouteInfo)> {
    let (device_id, profile_device) = *state.node_routes.get(&node_id)?;
    let route =
        state.routes.get(&device_id)?.iter().find(|route| {
            route.device == profile_device || route.devices.contains(&profile_device)
        })?;
    Some((device_id, route.clone()))
}

/// Mirror a volume/mute change onto the device route backing a hardware node,
/// so the session manager (and desktop settings UI) observes it. No-op for
/// nodes without a device route (streams, virtual sinks/sources).
fn apply_route(state: &State, node_id: u32, volume: Option<f64>, muted: Option<bool>) {
    let Some((device_id, route)) = node_route_info(state, node_id) else {
        return;
    };
    let linear = volume.map(|v| v.max(0.0).powi(3) as f32);
    let bytes = build_route_pod(&route, linear, muted);
    let Some(pod) = Pod::from_bytes(&bytes) else {
        return;
    };
    if let Some((device, _)) = state.devices.get(&device_id) {
        device.set_param(ParamType::Route, 0, pod);
    }
}

fn compute_edges(state: &State) -> Vec<(String, String)> {
    state
        .link_nodes
        .values()
        .filter_map(|(out_id, in_id)| {
            let out = node_name(state, *out_id)?;
            let inn = node_name(state, *in_id)?;
            Some((out, inn))
        })
        .collect()
}

fn handle_command(
    state: &mut State,
    core: &pw::core::Core,
    registry: &pw::registry::Registry,
    cmd: PwCommand,
) {
    match cmd {
        PwCommand::Shutdown => {}
        PwCommand::Volume { target, volume } => {
            if let Some(id) = resolve_node_id(state, &target) {
                apply_props(state, id, Some(volume), None);
                apply_route(state, id, Some(volume), None);
            }
        }
        PwCommand::Mute { target, muted } => {
            if let Some(id) = resolve_node_id(state, &target) {
                apply_props(state, id, None, Some(muted));
                apply_route(state, id, None, Some(muted));
            }
        }
        PwCommand::Route {
            stream,
            sink,
            connect,
        } => set_route(state, core, registry, &stream, &sink, connect),
    }
}

fn set_route(
    state: &mut State,
    core: &pw::core::Core,
    registry: &pw::registry::Registry,
    stream: &str,
    sink: &str,
    connect: bool,
) {
    let Some(stream_id) = resolve_node_id(state, stream) else {
        return;
    };
    let Some(sink_id) = resolve_node_id(state, sink) else {
        return;
    };

    if connect {
        let out_ports: Vec<(u32, String)> = state
            .ports
            .iter()
            .filter(|(_, (node, dir, _))| *node == stream_id && *dir == Direction::Output)
            .map(|(port, (_, _, ch))| (*port, ch.clone()))
            .collect();
        let in_ports: Vec<(u32, String)> = state
            .ports
            .iter()
            .filter(|(_, (node, dir, _))| *node == sink_id && *dir == Direction::Input)
            .map(|(port, (_, _, ch))| (*port, ch.clone()))
            .collect();

        for (out_port, out_ch) in &out_ports {
            let in_port = in_ports
                .iter()
                .find(|(_, in_ch)| in_ch == out_ch)
                .map(|(port, _)| *port)
                .or_else(|| in_ports.first().map(|(port, _)| *port));
            let Some(in_port) = in_port else { continue };

            let mut props = pw::properties::PropertiesBox::new();
            props.insert("link.output.node", stream_id.to_string());
            props.insert("link.output.port", out_port.to_string());
            props.insert("link.input.node", sink_id.to_string());
            props.insert("link.input.port", in_port.to_string());
            props.insert("link.passive", "true");
            props.insert("object.linger", "true");
            let _ = core.create_object::<Link>("link-factory", &props);
        }
    } else {
        let to_destroy: Vec<u32> = state
            .link_nodes
            .iter()
            .filter(|(_, (out_id, in_id))| *out_id == stream_id && *in_id == sink_id)
            .map(|(link_id, _)| *link_id)
            .collect();
        for link_id in to_destroy {
            state.link_nodes.remove(&link_id);
            state.links.remove(&link_id);
            let _ = registry.destroy_global(link_id);
        }
    }
}

fn media_class(props: Option<&pw::spa::utils::dict::DictRef>) -> &str {
    props.and_then(|p| p.get("media.class")).unwrap_or("")
}

fn object_type_for(media_class: &str) -> Option<PwObjectType> {
    if media_class.contains("Audio/Sink") {
        Some(PwObjectType::Sink)
    } else if media_class.contains("Audio/Source") {
        Some(PwObjectType::Source)
    } else if media_class.contains("Stream") && media_class.contains("Audio") {
        Some(PwObjectType::Stream)
    } else {
        None
    }
}

fn app_name(props: Option<&pw::spa::utils::dict::DictRef>) -> Option<String> {
    let props = props?;
    for key in [
        "application.name",
        "application.process.binary",
        "node.nick",
        "media.name",
    ] {
        if let Some(value) = props.get(key) {
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

pub fn spawn(
    event_tx: mpsc::Sender<PwEvent>,
    command_rx: mpsc::Receiver<PwCommand>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let command_rx = std::sync::Arc::new(std::sync::Mutex::new(command_rx));
        loop {
            pw::init();

            let mainloop = match pw::main_loop::MainLoopRc::new(None) {
                Ok(ml) => ml,
                Err(_) => {
                    let _ = event_tx.send(PwEvent::Disconnected);
                    thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };

            let context = match pw::context::ContextRc::new(&mainloop, None) {
                Ok(ctx) => ctx,
                Err(_) => {
                    let _ = event_tx.send(PwEvent::Disconnected);
                    thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };

            let core = match context.connect_rc(None) {
                Ok(c) => c,
                Err(_) => {
                    let _ = event_tx.send(PwEvent::Disconnected);
                    thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };

            let registry = match core.get_registry_rc() {
                Ok(r) => r,
                Err(_) => {
                    let _ = event_tx.send(PwEvent::Disconnected);
                    thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };

            let _ = event_tx.send(PwEvent::Connected);

            let state: Rc<RefCell<State>> = Rc::new(RefCell::new(State::default()));

            let state_global = state.clone();
            let state_remove = state.clone();
            let registry_global = registry.clone();

            let _reg_listener = registry
                .add_listener_local()
                .global(move |global| match global.type_ {
                    ObjectType::Node => {
                        let Some(object_type) = object_type_for(media_class(global.props)) else {
                            return;
                        };
                        let node_id = global.id;
                        let name = global
                            .props
                            .and_then(|p| p.get("node.name"))
                            .unwrap_or("")
                            .to_string();

                        // Hardware endpoints belong to a Device and are backed by
                        // one of its routes. Record the mapping from the node
                        // `info` event below, since `card.profile.device` is not
                        // present in the registry global props.

                        let description = if object_type == PwObjectType::Stream {
                            match app_name(global.props) {
                                Some(app) => format!("{app} ({name})"),
                                None => name.clone(),
                            }
                        } else {
                            global
                                .props
                                .and_then(|p| p.get("node.description"))
                                .unwrap_or(&name)
                                .to_string()
                        };

                        {
                            let mut s = state_global.borrow_mut();
                            if let Some(obj) = s.objects.iter_mut().find(|o| o.id == node_id) {
                                obj.object_type = object_type;
                                obj.name = name.clone();
                                obj.description = description;
                            } else {
                                s.objects.push(PwObject {
                                    id: node_id,
                                    object_type,
                                    name: name.clone(),
                                    description,
                                    volume: None,
                                    muted: false,
                                });
                            }
                        }

                        let node: Node = match registry_global.bind(global) {
                            Ok(n) => n,
                            Err(_) => return,
                        };

                        let state_info = state_global.clone();
                        let state_param = state_global.clone();
                        let listener = node
                            .add_listener_local()
                            .info(move |info| {
                                let Some(props) = info.props() else { return };
                                let Some(device) =
                                    props.get("device.id").and_then(|v| v.parse::<u32>().ok())
                                else {
                                    return;
                                };
                                let Some(profile_device) = props
                                    .get("card.profile.device")
                                    .and_then(|v| v.parse::<u32>().ok())
                                else {
                                    return;
                                };
                                state_info
                                    .borrow_mut()
                                    .node_routes
                                    .insert(node_id, (device, profile_device));
                            })
                            .param(move |_seq, ptype, _index, _next, pod| {
                                if ptype != ParamType::Props {
                                    return;
                                }
                                let Some(pod) = pod else { return };
                                let Some(update) = parse_props_volume(pod) else {
                                    return;
                                };
                                let mut s = state_param.borrow_mut();
                                if let Some(obj) = s.objects.iter_mut().find(|o| o.id == node_id) {
                                    if let Some(volume) = update.volume {
                                        obj.volume = Some(volume);
                                    }
                                    if let Some(muted) = update.muted {
                                        obj.muted = muted;
                                    }
                                }
                            })
                            .register();
                        node.subscribe_params(&[ParamType::Props]);
                        state_global
                            .borrow_mut()
                            .nodes
                            .insert(node_id, (node, listener));
                    }
                    ObjectType::Port => {
                        let node_id = global
                            .props
                            .and_then(|p| p.get("node.id"))
                            .and_then(|v| v.parse::<u32>().ok());
                        let direction = match global.props.and_then(|p| p.get("port.direction")) {
                            Some("in") => Direction::Input,
                            Some("out") => Direction::Output,
                            _ => return,
                        };
                        let channel = global
                            .props
                            .and_then(|p| p.get("audio.channel"))
                            .unwrap_or("")
                            .to_string();
                        if let Some(node_id) = node_id {
                            state_global
                                .borrow_mut()
                                .ports
                                .insert(global.id, (node_id, direction, channel));
                        }
                    }
                    ObjectType::Link => {
                        let link: Link = match registry_global.bind(global) {
                            Ok(l) => l,
                            Err(_) => return,
                        };
                        let link_id = global.id;

                        // Populate the link's endpoint node ids from its props
                        // immediately. `set_route` writes `link.output.node` and
                        // `link.input.node` as numeric ids, so this is
                        // authoritative for links we create and makes routes
                        // visible without waiting for the link `info` event.
                        let props_nodes = global.props.and_then(|p| {
                            let out = p.get("link.output.node")?.parse::<u32>().ok()?;
                            let inn = p.get("link.input.node")?.parse::<u32>().ok()?;
                            Some((out, inn))
                        });
                        if let Some((out, inn)) = props_nodes {
                            state_global
                                .borrow_mut()
                                .link_nodes
                                .insert(link_id, (out, inn));
                        }

                        let state_link = state_global.clone();
                        let listener = link
                            .add_listener_local()
                            .info(move |info| {
                                let out = info.output_node_id();
                                let inn = info.input_node_id();
                                // Ignore unresolved endpoints (`SPA_ID_INVALID`
                                // and zero) so a premature info event doesn't
                                // overwrite the resolved ids.
                                if out != 0 && inn != 0 && out != u32::MAX && inn != u32::MAX {
                                    state_link
                                        .borrow_mut()
                                        .link_nodes
                                        .insert(link_id, (out, inn));
                                }
                            })
                            .register();
                        state_global
                            .borrow_mut()
                            .links
                            .insert(link_id, (link, listener));
                    }
                    ObjectType::Device => {
                        let device_id = global.id;
                        let device: Device = match registry_global.bind(global) {
                            Ok(d) => d,
                            Err(_) => return,
                        };

                        // Subscribe only after the device advertises a Route param:
                        // subscribing on a device without one emits a fatal core error.
                        let device = Rc::new(device);
                        let subscribed = Rc::new(Cell::new(false));
                        let device_info = Rc::clone(&device);
                        let subscribed_info = Rc::clone(&subscribed);
                        let state_route = state_global.clone();
                        let listener = device
                            .add_listener_local()
                            .info(move |info| {
                                if subscribed_info.get() {
                                    return;
                                }
                                if info.params().iter().any(|p| p.id() == ParamType::Route) {
                                    subscribed_info.set(true);
                                    device_info.subscribe_params(&[ParamType::Route]);
                                }
                            })
                            .param(move |_seq, ptype, _index, _next, pod| {
                                if ptype != ParamType::Route {
                                    return;
                                }
                                let Some(pod) = pod else { return };
                                let Some(route) = parse_route(pod) else {
                                    return;
                                };
                                let mut s = state_route.borrow_mut();
                                let routes = s.routes.entry(device_id).or_default();
                                if let Some(slot) =
                                    routes.iter_mut().find(|r| r.index == route.index)
                                {
                                    *slot = route;
                                } else {
                                    routes.push(route);
                                }
                            })
                            .register();
                        state_global
                            .borrow_mut()
                            .devices
                            .insert(device_id, (device, listener));
                    }
                    _ => {}
                })
                .global_remove(move |id| {
                    let mut s = state_remove.borrow_mut();
                    s.objects.retain(|o| o.id != id);
                    s.nodes.remove(&id);
                    s.devices.remove(&id);
                    s.routes.remove(&id);
                    s.node_routes.remove(&id);
                    s.ports.remove(&id);
                    s.links.remove(&id);
                    s.link_nodes.remove(&id);
                })
                .register();

            let got_objects = Rc::new(Cell::new(false));
            let state_publish = state.clone();
            let event_tx_publish = event_tx.clone();
            let got_objects_publish = got_objects.clone();
            let publish_timer = mainloop.loop_().add_timer(move |_| {
                let s = state_publish.borrow();
                if !s.objects.is_empty() {
                    got_objects_publish.set(true);
                }
                let _ = event_tx_publish.send(PwEvent::ObjectsUpdated {
                    objects: s.objects.clone(),
                    edges: compute_edges(&s),
                });
            });
            publish_timer.update_timer(Some(PUBLISH_INTERVAL), Some(PUBLISH_INTERVAL));

            let state_cmd = state.clone();
            let core_cmd = core.clone();
            let registry_cmd = registry.clone();
            let command_rx = command_rx.clone();
            let shutdown_requested = Arc::new(AtomicBool::new(false));
            let shutdown_for_timer = Arc::clone(&shutdown_requested);
            let mainloop_for_commands = mainloop.downgrade();
            let command_timer = mainloop.loop_().add_timer(move |_| {
                while let Ok(cmd) = command_rx.lock().unwrap().try_recv() {
                    if matches!(&cmd, PwCommand::Shutdown) {
                        shutdown_for_timer.store(true, Ordering::Release);
                        if let Some(ml) = mainloop_for_commands.upgrade() {
                            ml.quit();
                        }
                        break;
                    }
                    handle_command(&mut state_cmd.borrow_mut(), &core_cmd, &registry_cmd, cmd);
                }
            });
            command_timer.update_timer(Some(COMMAND_INTERVAL), Some(COMMAND_INTERVAL));

            // `connect_rc` connects lazily, so a successful connection does not
            // prove a live server. Quit and retry if no audio graph has been
            // published within the timeout.
            let mainloop_weak = mainloop.downgrade();
            let got_objects_watchdog = got_objects.clone();
            let watchdog_weak = mainloop.downgrade();
            let watchdog = mainloop.loop_().add_timer(move |_| {
                if !got_objects_watchdog.get() {
                    if let Some(ml) = watchdog_weak.upgrade() {
                        ml.quit();
                    }
                }
            });
            watchdog.update_timer(Some(STARTUP_TIMEOUT), None);

            let event_tx_conn = event_tx.clone();
            let _conn_listener = core
                .add_listener_local()
                .error(move |_id, _seq, _res, _msg| {
                    let _ = event_tx_conn.send(PwEvent::Disconnected);
                    if let Some(ml) = mainloop_weak.upgrade() {
                        ml.quit();
                    }
                })
                .register();

            let run_result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| mainloop.run()));
            if run_result.is_err() {
                log::error!("PipeWire main loop panicked; reconnecting");
            }

            let _ = event_tx.send(PwEvent::Disconnected);
            if shutdown_requested.load(Ordering::Acquire) {
                return;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(id: u32, name: &str, volume: Option<f64>) -> PwObject {
        PwObject {
            id,
            object_type: PwObjectType::Sink,
            name: name.into(),
            description: name.into(),
            volume,
            muted: false,
        }
    }

    #[test]
    fn test_resolve_node_id() {
        let state = State {
            objects: vec![
                obj(1, "alsa_output.hdmi", Some(0.5)),
                obj(2, "alsa_input.usb", Some(0.8)),
            ],
            ..Default::default()
        };
        assert_eq!(resolve_node_id(&state, "1"), Some(1));
        assert_eq!(resolve_node_id(&state, "alsa_output.hdmi"), Some(1));
        assert_eq!(resolve_node_id(&state, "hdmi"), Some(1));
        assert_eq!(resolve_node_id(&state, "missing"), None);
    }

    #[test]
    fn test_resolve_node_id_prefers_exact_over_substring() {
        // The media-sink's name contains the sink's name as a substring, so a
        // substring-first lookup would resolve the sink to the media-sink.
        let state = State {
            objects: vec![
                obj(1, "output.media-sink_alsa_output.hdmi", Some(0.5)),
                obj(2, "alsa_output.hdmi", Some(0.5)),
            ],
            ..Default::default()
        };
        assert_eq!(resolve_node_id(&state, "alsa_output.hdmi"), Some(2));
        assert_eq!(
            resolve_node_id(&state, "output.media-sink_alsa_output.hdmi"),
            Some(1)
        );
    }

    #[test]
    fn test_object_type_for() {
        assert_eq!(object_type_for("Audio/Sink"), Some(PwObjectType::Sink));
        assert_eq!(object_type_for("Audio/Source"), Some(PwObjectType::Source));
        assert_eq!(
            object_type_for("Stream/Input/Audio"),
            Some(PwObjectType::Stream)
        );
        assert_eq!(object_type_for("Video/Source"), None);
        assert_eq!(object_type_for(""), None);
    }

    #[test]
    fn test_compute_edges() {
        let mut state = State {
            objects: vec![obj(1, "sink", Some(0.5)), obj(2, "stream", Some(0.8))],
            ..Default::default()
        };
        state.link_nodes.insert(10, (2, 1));
        assert_eq!(
            compute_edges(&state),
            vec![("stream".to_string(), "sink".to_string())]
        );
    }

    /// #feature PW-VOL — a node is matched to the route that covers its ACP
    /// profile device, so multi-route devices (e.g. HDMI) resolve correctly.
    #[test]
    fn test_node_route_info_matches_profile_device() {
        let mut state = State::default();
        state.node_routes.insert(165, (73, 4));
        state.routes.insert(
            73,
            vec![
                RouteInfo {
                    index: 0,
                    direction: 1,
                    device: 4,
                    devices: vec![4, 5, 6],
                },
                RouteInfo {
                    index: 1,
                    direction: 1,
                    device: 7,
                    devices: vec![7, 8, 9],
                },
            ],
        );

        let (device_id, route) = node_route_info(&state, 165).expect("route resolved");
        assert_eq!(device_id, 73);
        assert_eq!(route.index, 0);
        assert!(node_route_info(&state, 999).is_none());
    }

    fn wpctl_volume(id: u32) -> Option<f64> {
        let out = std::process::Command::new("wpctl")
            .args(["get-volume", &id.to_string()])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        // Output is "Volume: 0.52" or "Volume: 0.52 [MUTED]".
        text.split_whitespace().nth(1)?.parse::<f64>().ok()
    }

    /// #feature PW-VOL — a volume change to a hardware endpoint must reach the
    /// session manager (which reads the device route), not just the node Props.
    /// Requires a live PipeWire session, so it is ignored by default:
    /// run with `cargo test -- --ignored --nocapture`.
    #[test]
    #[ignore = "requires a live PipeWire session"]
    fn test_live_device_route_sync() {
        use std::sync::mpsc;
        use std::time::Duration;

        let (tx, rx) = mpsc::channel();
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let _handle = spawn(tx, cmd_rx);

        // Find the first hardware sink that reports a node volume.
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        let mut sink: Option<(u32, String, f64)> = None;
        while std::time::Instant::now() < deadline {
            if let Ok(PwEvent::ObjectsUpdated { objects, .. }) =
                rx.recv_timeout(Duration::from_millis(500))
            {
                if let Some(o) = objects
                    .iter()
                    .find(|o| {
                        o.object_type == PwObjectType::Sink && o.name.starts_with("alsa_output")
                    })
                    .and_then(|o| o.volume.map(|v| (o.id, o.name.clone(), v)))
                {
                    sink = Some(o);
                    break;
                }
            }
        }
        let Some((id, name, node_volume)) = sink else {
            panic!("no hardware sink with a reported volume found");
        };

        let before = wpctl_volume(id).expect("wpctl get-volume works");
        // Pick a target clearly different from the session manager's value.
        let target = if before > 0.5 { 0.35 } else { 0.65 };
        // Let device `Route` subscriptions arrive before issuing the command.
        std::thread::sleep(Duration::from_millis(500));
        let _ = cmd_tx.send(PwCommand::Volume {
            target: name.clone(),
            volume: target,
        });

        let deadline = std::time::Instant::now() + Duration::from_millis(2000);
        let (mut node_seen, mut route_seen) = (false, false);
        while std::time::Instant::now() < deadline {
            if let Ok(PwEvent::ObjectsUpdated { objects, .. }) =
                rx.recv_timeout(Duration::from_millis(300))
            {
                if let Some(o) = objects.iter().find(|o| o.id == id) {
                    if o.volume.is_some_and(|v| (v - target).abs() < 0.02) {
                        node_seen = true;
                    }
                }
            }
            if wpctl_volume(id).is_some_and(|v| (v - target).abs() < 0.02) {
                route_seen = true;
            }
            if node_seen && route_seen {
                break;
            }
        }

        // Restore the original audible volume before asserting.
        let _ = cmd_tx.send(PwCommand::Volume {
            target: name,
            volume: node_volume,
        });
        std::thread::sleep(Duration::from_millis(300));

        assert!(node_seen, "node Props volume not updated");
        assert!(
            route_seen,
            "device route not updated; the session manager never saw the change"
        );
    }

    /// #feature PW-VOL — end-to-end smoke test of the live node listeners:
    /// volume changes must be reported promptly (as events), not on a poll.
    /// Requires a live PipeWire session, so it is ignored by default:
    /// run with `cargo test -- --ignored`.
    #[test]
    #[ignore = "requires a live PipeWire session"]
    fn test_live_volume_listener_smoke() {
        use std::sync::mpsc;
        use std::time::Duration;

        let (tx, rx) = mpsc::channel();
        let (_cmd_tx, cmd_rx) = mpsc::channel();
        let _handle = spawn(tx, cmd_rx);

        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        let mut before: Option<f64> = None;
        while std::time::Instant::now() < deadline {
            if let Ok(PwEvent::ObjectsUpdated { objects, .. }) =
                rx.recv_timeout(Duration::from_millis(500))
            {
                if let Some(v) = objects
                    .iter()
                    .find(|o| o.id == 44 || o.name.contains("default-sink"))
                    .and_then(|o| o.volume)
                {
                    before = Some(v);
                    break;
                }
            }
        }
        let Some(before) = before else {
            panic!("no volume reported for the default sink via listeners");
        };

        let target = (before + 0.05).min(1.0);
        let ok = std::process::Command::new("wpctl")
            .args(["set-volume", "44", &format!("{target:.3}")])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(ok, "wpctl set-volume failed");

        let deadline = std::time::Instant::now() + Duration::from_millis(2000);
        let mut updated = false;
        while std::time::Instant::now() < deadline {
            if let Ok(PwEvent::ObjectsUpdated { objects, .. }) =
                rx.recv_timeout(Duration::from_millis(300))
            {
                if let Some(v) = objects
                    .iter()
                    .find(|o| o.id == 44 || o.name.contains("default-sink"))
                    .and_then(|o| o.volume)
                {
                    if (v - target).abs() < 0.02 {
                        updated = true;
                        break;
                    }
                }
            }
        }
        let _ = std::process::Command::new("wpctl")
            .args(["set-volume", "44", &format!("{before:.3}")])
            .status();
        assert!(updated, "volume change not reported within 2 s");
    }
}
