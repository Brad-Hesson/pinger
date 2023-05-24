use std::ops::RangeBounds;

use float_ord::FloatOrd;
use piston_window::{
    math, Button, ButtonArgs, ButtonState, Context, Event, Input, Motion, MouseButton, Transformed,
};

#[derive(Debug)]
pub struct PanZoom {
    max_zoom: Option<f64>,
    min_zoom: Option<f64>,
    zoom_factor: f64,
    zoom: f64,
    pan: [f64; 2],
    mouse_down: bool,
}
impl PanZoom {
    pub fn new(zoom_factor: f64, limits: impl RangeBounds<f64>) -> Self {
        let max_zoom = match limits.end_bound() {
            std::ops::Bound::Included(v) => Some(*v),
            std::ops::Bound::Excluded(v) => Some(*v),
            std::ops::Bound::Unbounded => None,
        };
        let min_zoom = match limits.start_bound() {
            std::ops::Bound::Included(v) => Some(*v),
            std::ops::Bound::Excluded(v) => Some(*v),
            std::ops::Bound::Unbounded => None,
        };
        Self {
            zoom_factor,
            zoom: 1.,
            pan: [0.; 2],
            mouse_down: false,
            max_zoom,
            min_zoom,
        }
    }
    pub fn update(&mut self, event: &Event) {
        let input = match event {
            Event::Input(input, _) => input,
            _ => return,
        };
        match input {
            Input::Move(Motion::MouseScroll([_, scroll])) => {
                self.zoom *= self.zoom_factor.powf(*scroll);
                self.zoom = self.zoom.clamp(
                    self.min_zoom.unwrap_or(f64::MIN_POSITIVE),
                    self.max_zoom.unwrap_or(f64::MAX),
                );
            }
            Input::Button(ButtonArgs {
                state,
                button: Button::Mouse(MouseButton::Left),
                ..
            }) => self.mouse_down = *state == ButtonState::Press,
            Input::Move(Motion::MouseRelative([dx, dy])) if self.mouse_down => {
                self.pan[0] += dx / self.zoom;
                self.pan[1] += dy / self.zoom;
            }
            _ => {}
        }
    }
    pub fn apply_transform(&self, ctx: &mut Context) {
        let screen_size = ctx.get_view_size();
        let screen_offset = math::mul_scalar(screen_size, 0.5);
        let screen_scale = screen_size
            .into_iter()
            .min_by_key(|f| FloatOrd(*f))
            .unwrap()
            / 2.;
        *ctx = ctx.trans_pos(screen_offset);
        *ctx = ctx.scale(self.zoom, self.zoom);
        *ctx = ctx.trans_pos(self.pan);
        *ctx = ctx.scale(screen_scale, screen_scale);
    }
}
