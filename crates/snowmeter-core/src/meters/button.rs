//! The Button meter (meters.md section 9, `MeterButton.cpp`).

use super::imgslot::ImageSlot;
use super::{MeterBase, MeterImpl, MeterInputs};
use crate::parser::OptionReader;
use crate::render::image::paint_region;
use crate::render::Resources;

pub const FRAMES: i32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ButtonState {
    #[default]
    Normal,
    Down,
    Hover,
}

impl ButtonState {
    pub fn frame(self) -> i32 {
        match self {
            ButtonState::Normal => 0,
            ButtonState::Down => 1,
            ButtonState::Hover => 2,
        }
    }
}

/// What the skin tells a button about the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ButtonInput {
    Move,
    Down,
    /// Left button released; `execute` is false when the release was a drag.
    Up { execute: bool },
}

/// Where the pointer is relative to the button.
#[derive(Clone, Copy, Debug)]
pub struct Pointer {
    pub x: f64,
    pub y: f64,
    /// The pointer is inside the meter rectangle.
    pub over: bool,
    /// This is the topmost button under the pointer.
    pub focus: bool,
    pub left_down: bool,
}

#[derive(Default)]
pub struct ButtonMeter {
    image: ImageSlot,
    command: String,
    state: ButtonState,
    clicked: bool,
    log: Vec<String>,
}

/// Source rectangle `(x, y, w, h)` of frame `i` and the frame size.
pub fn frame_rect(img_w: i32, img_h: i32, i: i32) -> (f64, f64, f64, f64) {
    if img_h > img_w {
        let fh = img_h / FRAMES;
        (0.0, (fh * i) as f64, img_w as f64, fh as f64)
    } else {
        let fw = img_w / FRAMES;
        ((fw * i) as f64, 0.0, fw as f64, img_h as f64)
    }
}

impl ButtonMeter {
    pub fn state(&self) -> ButtonState {
        self.state
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    /// `HitTest2`: pointer over, and opaque (solid colour, no image, or an
    /// opaque pixel of the current frame).
    pub fn hit_test2(&self, base: &MeterBase, p: &Pointer) -> bool {
        if !p.over {
            return false;
        }
        if base.solid_color.a != 0 || base.solid_color2.a != 0 {
            return true;
        }
        let Some((iw, ih)) = self.image.size() else { return true };
        let (left, top, _, _) = base.content_rect();
        let (px, py) = ((p.x - left).floor() as i32, (p.y - top).floor() as i32);
        let (fx, fy, _, _) = frame_rect(iw, ih, self.state.frame());
        self.image.alpha_at(px + fx as i32, py + fy as i32) > 0
    }

    /// Port of `MouseMove`/`MouseDown`/`MouseUp`. Returns `(redraw, command)`.
    pub fn mouse(&mut self, base: &MeterBase, input: ButtonInput, p: &Pointer) -> (bool, Option<String>) {
        match input {
            ButtonInput::Down => {
                if p.focus && self.hit_test2(base, p) {
                    self.state = ButtonState::Down;
                    self.clicked = true;
                    return (true, None);
                }
                (false, None)
            }
            ButtonInput::Up { execute } => {
                if self.state == ButtonState::Down {
                    let run = execute && self.clicked && p.focus && self.hit_test2(base, p);
                    self.state = ButtonState::Normal;
                    self.clicked = false;
                    return (true, run.then(|| self.command.clone()));
                }
                self.clicked = false;
                (false, None)
            }
            ButtonInput::Move => {
                let hit = self.hit_test2(base, p);
                if self.clicked {
                    if hit {
                        if self.state == ButtonState::Normal {
                            self.state = ButtonState::Down;
                            return (true, None);
                        }
                    } else {
                        if !p.left_down {
                            self.clicked = false;
                        }
                        if self.state == ButtonState::Down {
                            self.state = ButtonState::Normal;
                            return (true, None);
                        }
                    }
                } else if hit {
                    if self.state == ButtonState::Normal {
                        self.state = ButtonState::Hover;
                        return (true, None);
                    }
                } else if self.state == ButtonState::Hover {
                    self.state = ButtonState::Normal;
                    return (true, None);
                }
                (false, None)
            }
        }
    }
}

impl MeterImpl for ButtonMeter {
    fn read_options(&mut self, opt: &OptionReader) {
        let mut log = Vec::new();
        self.image.read(opt, "Button", &mut log);
        self.log.extend(log);
        self.command = opt.action("ButtonCommand");
    }

    fn update(&mut self, m: &MeterInputs) -> bool {
        self.image.update(m)
    }

    fn auto_size(&self, _base: &MeterBase, _res: &Resources) -> (f64, f64) {
        (0.0, 0.0)
    }

    fn fixed_size(&self) -> Option<(f64, f64)> {
        self.image.size().map(|(w, h)| {
            let r = frame_rect(w, h, 0);
            (r.2, r.3)
        })
    }

    fn draw(&self, cr: &cairo::Context, base: &MeterBase, _res: &Resources) {
        let Some(img) = self.image.surface() else { return };
        let (left, top, _, _) = base.content_rect();
        let s = frame_rect(img.width(), img.height(), self.state.frame());
        paint_region(cr, img, s, (left, top, base.w, base.h));
    }

    fn is_button(&self) -> bool {
        true
    }

    fn button_mouse(&mut self, base: &MeterBase, input: ButtonInput, p: &Pointer) -> (bool, Option<String>) {
        self.mouse(base, input, p)
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_layout_follows_the_image_shape() {
        assert_eq!(frame_rect(30, 90, 1), (0.0, 30.0, 30.0, 30.0));
        assert_eq!(frame_rect(90, 30, 2), (60.0, 0.0, 30.0, 30.0));
        assert_eq!(frame_rect(30, 30, 1), (10.0, 0.0, 10.0, 30.0));
        assert_eq!(ButtonState::Hover.frame(), 2);
    }

    fn base() -> MeterBase {
        let mut b = MeterBase::new("B", "Button");
        b.w = 30.0;
        b.h = 30.0;
        b
    }

    fn at(over: bool, focus: bool, left_down: bool) -> Pointer {
        Pointer { x: 5.0, y: 5.0, over, focus, left_down }
    }

    #[test]
    fn state_machine_hover_down_release_executes() {
        let mut b = ButtonMeter { command: "[!Cmd]".into(), ..Default::default() };
        let base = base();
        assert_eq!(b.mouse(&base, ButtonInput::Move, &at(true, true, false)), (true, None));
        assert_eq!(b.state(), ButtonState::Hover);
        assert_eq!(b.mouse(&base, ButtonInput::Down, &at(true, true, true)), (true, None));
        assert_eq!(b.state(), ButtonState::Down);
        // Dragging off returns to Normal but keeps `clicked` while the button is held.
        assert_eq!(b.mouse(&base, ButtonInput::Move, &at(false, true, true)), (true, None));
        assert_eq!(b.state(), ButtonState::Normal);
        assert_eq!(b.mouse(&base, ButtonInput::Move, &at(true, true, true)), (true, None));
        assert_eq!(b.state(), ButtonState::Down);
        let (redraw, cmd) = b.mouse(&base, ButtonInput::Up { execute: true }, &at(true, true, false));
        assert!(redraw);
        assert_eq!(cmd.as_deref(), Some("[!Cmd]"));
        assert_eq!(b.state(), ButtonState::Normal);
    }

    #[test]
    fn release_elsewhere_does_not_execute() {
        let mut b = ButtonMeter { command: "[!Cmd]".into(), ..Default::default() };
        let base = base();
        b.mouse(&base, ButtonInput::Down, &at(true, true, true));
        let (_, cmd) = b.mouse(&base, ButtonInput::Up { execute: true }, &at(false, true, false));
        assert_eq!(cmd, None);
        // No focus, no press.
        assert_eq!(b.mouse(&base, ButtonInput::Down, &at(true, false, true)), (false, None));
    }
}
