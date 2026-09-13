use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path};
use iced::{Color, Element, Length, Point, Rectangle, Size};

use crate::config::{HardwareDef, IconType};

#[derive(Debug, Clone)]
pub enum FaceplateMessage {
    ClickIcon(usize),
    ClickEmptyCell(u8, u8),
}

#[derive(Debug, Clone)]
pub struct FaceplateState {
    pub grid_columns: u8,
    pub grid_rows: u8,
    pub hardware: Vec<HardwareDef>,
    /// Faceplate value (0.0..=1.0) per icon, keyed by hardware id. Populated
    /// exclusively from each icon's `visual_source` — never from raw MIDI.
    pub visual_values: std::collections::HashMap<String, f64>,
    /// Icon labels from the active layer's software, keyed by hardware id.
    pub icon_labels: std::collections::HashMap<String, String>,
}

impl FaceplateState {
    pub(crate) fn cell_size(&self, bounds: Size) -> Size {
        let w = bounds.width / self.grid_columns.max(1) as f32;
        let h = bounds.height / self.grid_rows.max(1) as f32;
        let s = w.min(h).min(120.0);
        Size::new(s, s)
    }

    pub(crate) fn grid_origin(&self, bounds: Size) -> Point {
        let cs = self.cell_size(bounds);
        let tw = cs.width * self.grid_columns as f32;
        let th = cs.height * self.grid_rows as f32;
        Point::new((bounds.width - tw) / 2.0, (bounds.height - th) / 2.0)
    }

    pub(crate) fn icon_rect(&self, icon: &HardwareDef, bounds: Size) -> Rectangle {
        let cs = self.cell_size(bounds);
        let go = self.grid_origin(bounds);
        Rectangle::new(
            Point::new(
                go.x + icon.col as f32 * cs.width,
                go.y + icon.row as f32 * cs.height,
            ),
            Size::new(
                cs.width * icon.col_span as f32,
                cs.height * icon.row_span as f32,
            ),
        )
    }

    pub(crate) fn icon_at(
        &self,
        position: Point,
        bounds: Size,
        icons: &[HardwareDef],
    ) -> Option<(usize, Point)> {
        for (idx, icon) in icons.iter().enumerate() {
            let rect = self.icon_rect(icon, bounds);
            if rect.contains(position) {
                return Some((idx, rect.center()));
            }
        }
        None
    }
}

pub fn draw(state: FaceplateState) -> Element<'static, FaceplateMessage> {
    Canvas::new(state)
        .width(Length::Fill)
        .height(Length::Fixed(400.0))
        .into()
}

impl iced::widget::canvas::Program<FaceplateMessage> for FaceplateState {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let cs = self.cell_size(bounds.size());
        let go = self.grid_origin(bounds.size());

        self.draw_grid(&mut frame, bounds, cs, go);

        // Hardware controls are shared; labels come from the active layer.
        for icon in self.hardware.iter() {
            let rect = self.icon_rect(icon, bounds.size());

            let icon_label = self
                .icon_labels
                .get(&icon.id)
                .filter(|l| !l.is_empty())
                .cloned()
                .unwrap_or_else(|| icon.id.clone());
            let value = visual_value(&icon.id, &self.visual_values);

            match icon.hw_type {
                IconType::Button => draw_button(&mut frame, rect, &icon_label, value),
                IconType::Knob => draw_knob(&mut frame, rect, &icon_label, value),
                IconType::Fader => draw_fader(&mut frame, rect, &icon_label, value),
                IconType::Encoder => draw_encoder(&mut frame, rect, &icon_label, value),
            }
        }

        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        _state: &mut Self::State,
        event: &iced::widget::canvas::Event,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Option<iced::widget::canvas::Action<FaceplateMessage>> {
        if let iced::widget::canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(
            iced::mouse::Button::Left,
        )) = event
        {
            if let Some(cursor) = _cursor.position_in(bounds) {
                if let Some((idx, _)) = self.icon_at(cursor, bounds.size(), &self.hardware) {
                    return Some(
                        iced::widget::canvas::Action::publish(FaceplateMessage::ClickIcon(idx))
                            .and_capture(),
                    );
                }

                let cs = self.cell_size(bounds.size());
                let go = self.grid_origin(bounds.size());
                if let Some((col, row)) = cell_at(cursor, go, cs, self.grid_columns, self.grid_rows)
                {
                    let occupied = self.hardware.iter().any(|icon| {
                        col >= icon.col
                            && col < icon.col + icon.col_span
                            && row >= icon.row
                            && row < icon.row + icon.row_span
                    });
                    if !occupied {
                        return Some(
                            iced::widget::canvas::Action::publish(
                                FaceplateMessage::ClickEmptyCell(col, row),
                            )
                            .and_capture(),
                        );
                    }
                }
            }
        }
        None
    }
}

impl FaceplateState {
    fn draw_grid(&self, frame: &mut Frame, _bounds: Rectangle, cs: Size, go: Point) {
        let grid_color = Color::from_rgb(0.2, 0.2, 0.25);
        for col in 0..=self.grid_columns {
            let x = go.x + col as f32 * cs.width;
            let path = Path::line(
                Point::new(x, go.y),
                Point::new(x, go.y + cs.height * self.grid_rows as f32),
            );
            frame.stroke(
                &path,
                canvas::Stroke::default()
                    .with_color(grid_color)
                    .with_width(0.5),
            );
        }
        for row in 0..=self.grid_rows {
            let y = go.y + row as f32 * cs.height;
            let path = Path::line(
                Point::new(go.x, y),
                Point::new(go.x + cs.width * self.grid_columns as f32, y),
            );
            frame.stroke(
                &path,
                canvas::Stroke::default()
                    .with_color(grid_color)
                    .with_width(0.5),
            );
        }
    }
}

/// Grid cell under `position`, or `None` when the position falls outside the
/// grid (e.g. in the margins around the centered grid). Positions left or
/// above the origin must not clamp to (0, 0).
fn cell_at(
    position: Point,
    grid_origin: Point,
    cell: Size,
    cols: u8,
    rows: u8,
) -> Option<(u8, u8)> {
    if position.x < grid_origin.x || position.y < grid_origin.y {
        return None;
    }
    let col = (position.x - grid_origin.x) / cell.width;
    let row = (position.y - grid_origin.y) / cell.height;
    if col >= cols as f32 || row >= rows as f32 {
        return None;
    }
    Some((col as u8, row as u8))
}

/// Visual value for an icon: whatever its `visual_source` evaluated to, or 0
/// when no visual source is configured.
fn visual_value(hardware_id: &str, visual_values: &std::collections::HashMap<String, f64>) -> f64 {
    visual_values.get(hardware_id).copied().unwrap_or(0.0)
}

fn draw_label(frame: &mut Frame, rect: Rectangle, label: &str, size: f32) {
    frame.fill_text(canvas::Text {
        content: label.to_string(),
        position: Point::new(rect.x + 2.0, rect.y + rect.height - 4.0),
        color: Color::from_rgb(0.7, 0.7, 0.7),
        size: iced::Pixels(size),
        ..Default::default()
    });
}

fn draw_button(frame: &mut Frame, rect: Rectangle, label: &str, val: f64) {
    let fill = if val > 0.5 {
        Color::from_rgb(0.2, 0.7, 0.2)
    } else {
        Color::from_rgb(0.3, 0.3, 0.35)
    };

    let r = rect.width.min(rect.height) * 0.4;
    let center = rect.center();
    let circle = Path::circle(center, r);
    frame.fill(&circle, fill);

    draw_label(frame, rect, label, 10.0);
}

fn draw_knob(frame: &mut Frame, rect: Rectangle, label: &str, val: f64) {
    let r = rect.width.min(rect.height) * 0.35;
    let center = rect.center();
    let outer = Path::circle(center, r);
    frame.stroke(
        &outer,
        canvas::Stroke::default()
            .with_color(Color::from_rgb(0.5, 0.5, 0.6))
            .with_width(2.0),
    );

    let angle = std::f32::consts::PI * 0.75 + val as f32 * std::f32::consts::PI * 1.5;
    let tip = Point::new(
        center.x + angle.cos() * r * 0.7,
        center.y - angle.sin() * r * 0.7,
    );
    let indicator = Path::line(center, tip);
    frame.stroke(
        &indicator,
        canvas::Stroke::default()
            .with_color(Color::from_rgb(0.8, 0.8, 0.3))
            .with_width(2.0),
    );

    draw_label(frame, rect, label, 9.0);
}

fn draw_fader(frame: &mut Frame, rect: Rectangle, label: &str, val: f64) {
    let track_w = rect.width * 0.15;
    let track_x = rect.center().x - track_w / 2.0;
    let track_h = rect.height * 0.75;
    let track_y = rect.y + rect.height * 0.125;

    let track = Path::rectangle(Point::new(track_x, track_y), Size::new(track_w, track_h));
    frame.fill(&track, Color::from_rgb(0.2, 0.2, 0.25));
    frame.stroke(
        &track,
        canvas::Stroke::default()
            .with_color(Color::from_rgb(0.5, 0.5, 0.6))
            .with_width(1.0),
    );

    let fill_h = track_h * val as f32;
    let fill_rect = Path::rectangle(
        Point::new(track_x, track_y + track_h - fill_h),
        Size::new(track_w, fill_h),
    );
    frame.fill(&fill_rect, Color::from_rgb(0.2, 0.6, 0.8));

    draw_label(frame, rect, label, 9.0);
}

fn draw_encoder(frame: &mut Frame, rect: Rectangle, label: &str, val: f64) {
    let r = rect.width.min(rect.height) * 0.35;
    let center = rect.center();

    let segments = 11;
    for i in 0..segments {
        let start_angle = std::f32::consts::PI * 0.75
            + i as f32 * std::f32::consts::PI * 1.5 / (segments - 1) as f32;
        let end_angle = start_angle + 0.2;
        let on = (i as f64) < (val * segments as f64);

        for a in 0..8 {
            let a_norm = start_angle + (end_angle - start_angle) * a as f32 / 7.0;
            let px = center.x + a_norm.cos() * r;
            let py = center.y - a_norm.sin() * r;
            let dot = Path::circle(Point::new(px, py), 2.0);
            let color = if on {
                Color::from_rgb(0.2, 0.8, 0.2)
            } else {
                Color::from_rgb(0.15, 0.15, 0.2)
            };
            frame.fill(&dot, color);
        }
    }

    draw_label(frame, rect, label, 9.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_bounds() -> Rectangle {
        Rectangle::new(Point::new(0.0, 0.0), Size::new(800.0, 400.0))
    }

    fn test_state(grid_columns: u8, grid_rows: u8) -> FaceplateState {
        FaceplateState {
            grid_columns,
            grid_rows,
            hardware: Vec::new(),
            visual_values: std::collections::HashMap::new(),
            icon_labels: std::collections::HashMap::new(),
        }
    }

    /// #feature FACE-GRID — clicks outside the grid must not create icons.
    #[test]
    fn test_cell_at_off_grid_click_regression() {
        let state = test_state(4, 2);
        let bounds = sample_bounds();
        let cs = state.cell_size(bounds.size());
        let go = state.grid_origin(bounds.size());
        let cols = state.grid_columns;
        let rows = state.grid_rows;

        // Left of the grid origin used to clamp to (0, y) and create an icon.
        let left_of_grid = Point::new(go.x - 1.0, go.y + cs.height / 2.0);
        assert_eq!(cell_at(left_of_grid, go, cs, cols, rows), None);

        // Above the grid origin used to clamp to (x, 0).
        let above_grid = Point::new(go.x + cs.width / 2.0, go.y - 1.0);
        assert_eq!(cell_at(above_grid, go, cs, cols, rows), None);

        // Beyond the right/bottom edge of the grid is outside too.
        let beyond_right = Point::new(go.x + cs.width * 4.5, go.y + cs.height / 2.0);
        assert_eq!(cell_at(beyond_right, go, cs, cols, rows), None);

        // Valid cells map to their column/row.
        let inside = Point::new(go.x + cs.width * 2.5, go.y + cs.height * 0.5);
        assert_eq!(cell_at(inside, go, cs, cols, rows), Some((2, 0)));
    }

    #[test]
    fn test_icon_at() {
        let state = test_state(8, 4);
        let icons = vec![HardwareDef {
            id: "btn1".into(),
            hw_type: IconType::Button,
            col: 0,
            row: 0,
            col_span: 1,
            row_span: 1,
            inputs: Vec::new(),
            outputs: Vec::new(),
        }];
        let cs = state.cell_size(Size::new(800.0, 400.0));
        let go = state.grid_origin(Size::new(800.0, 400.0));
        let center = Point::new(go.x + cs.width / 2.0, go.y + cs.height / 2.0);
        let result = state.icon_at(center, Size::new(800.0, 400.0), &icons);
        assert!(result.is_some());
    }
}
