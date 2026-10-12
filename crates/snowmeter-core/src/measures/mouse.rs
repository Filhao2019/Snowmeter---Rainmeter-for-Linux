//! Mouse plugin: skin-wide mouse actions with `$MouseX$` / `$MouseY$`, drag
//! actions that fire while a button is held, and a throttled move action.
//! GTK keeps delivering motion to the window during a button press, so the
//! drag works even when the pointer leaves the skin.

use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;
use crate::skin::mouse::{expand_mouse_vars, ActionKind, MouseActions};
use crate::skin::{MouseButton, MouseEvent};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct MouseMeasure {
    actions: MouseActions,
    move_action: String,
    /// Left, middle, right, X1, X2.
    drag: [String; 5],
    relative: bool,
    require_dragging: bool,
    dragging: bool,
    delay: Duration,
    last_move: Option<Instant>,
    held: [bool; 5],
    log: Vec<String>,
}

fn slot(b: MouseButton) -> usize {
    match b {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::X1 => 3,
        MouseButton::X2 => 4,
    }
}

impl MouseMeasure {
    pub fn new() -> MouseMeasure {
        MouseMeasure { relative: true, delay: Duration::from_millis(16), ..Default::default() }
    }

    fn active(&self) -> bool {
        !self.require_dragging || self.dragging
    }

    fn expand(&self, raw: &str, x: f64, y: f64, size: (f64, f64), pos: (f64, f64)) -> String {
        let (mx, my) = if self.relative { (x, y) } else { (x + pos.0, y + pos.1) };
        let dim = if self.relative { size } else { (0.0, 0.0) };
        expand_mouse_vars(raw, mx, my, dim)
    }
}

impl MeasureImpl for MouseMeasure {
    fn read_options(&mut self, opt: &OptionReader) {
        self.actions = MouseActions::read(opt);
        self.move_action = opt.action("MouseMoveAction");
        self.drag = [
            opt.action("LeftMouseDragAction"),
            opt.action("MiddleMouseDragAction"),
            opt.action("RightMouseDragAction"),
            opt.action("X1MouseDragAction"),
            opt.action("X2MouseDragAction"),
        ];
        self.relative = opt.bool("RelativeToSkin", true);
        self.require_dragging = opt.bool("RequireDragging", false);
        if !self.require_dragging {
            self.dragging = false;
        }
        self.delay = Duration::from_millis(opt.int("Delay", 16).max(0) as u64);
    }

    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        0.0
    }

    fn command(&mut self, args: &str) {
        let a = args.trim();
        if self.require_dragging && a.eq_ignore_ascii_case("start") {
            self.dragging = true;
        } else if self.require_dragging && a.eq_ignore_ascii_case("stop") {
            self.dragging = false;
        } else {
            self.log.push("!CommandMeasure: Not supported".into());
        }
    }

    fn pointer_event(&mut self, ev: &MouseEvent, size: (f64, f64), pos: (f64, f64)) -> Vec<String> {
        // Held buttons are tracked even while inactive so a drag can start later.
        match *ev {
            MouseEvent::Down { button, .. } | MouseEvent::DoubleClick { button, .. } => self.held[slot(button)] = true,
            MouseEvent::Up { button, .. } => self.held[slot(button)] = false,
            MouseEvent::Leave => {}
            _ => {}
        }
        if !self.active() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut push = |me: &MouseMeasure, raw: &str, x: f64, y: f64| {
            if !raw.trim().is_empty() {
                out.push(me.expand(raw, x, y, size, pos));
            }
        };
        match *ev {
            MouseEvent::Move { x, y } => {
                let due = self.last_move.map(|t| t.elapsed() >= self.delay).unwrap_or(true);
                if due {
                    self.last_move = Some(Instant::now());
                    let held = self.held;
                    for (i, h) in held.iter().enumerate() {
                        if *h {
                            let a = self.drag[i].clone();
                            push(self, &a, x, y);
                        }
                    }
                    let a = self.move_action.clone();
                    push(self, &a, x, y);
                }
            }
            MouseEvent::Down { button, x, y } => {
                let a = self.actions.get(ActionKind::Down(button)).unwrap_or("").to_string();
                push(self, &a, x, y);
            }
            MouseEvent::Up { button, x, y } => {
                let a = self.actions.get(ActionKind::Up(button)).unwrap_or("").to_string();
                push(self, &a, x, y);
            }
            MouseEvent::DoubleClick { button, x, y } => {
                let a = self
                    .actions
                    .get(ActionKind::DoubleClick(button))
                    .or_else(|| self.actions.get(ActionKind::Down(button)))
                    .unwrap_or("")
                    .to_string();
                push(self, &a, x, y);
            }
            MouseEvent::Scroll { dx, dy, x, y } => {
                let mut kinds = Vec::new();
                if dy != 0 {
                    kinds.push(if dy > 0 { ActionKind::ScrollUp } else { ActionKind::ScrollDown });
                }
                if dx != 0 {
                    kinds.push(if dx > 0 { ActionKind::ScrollRight } else { ActionKind::ScrollLeft });
                }
                for k in kinds {
                    let a = self.actions.get(k).unwrap_or("").to_string();
                    push(self, &a, x, y);
                }
            }
            MouseEvent::Leave => {}
        }
        out
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measures::testutil::skin;

    fn make(extra: &str) -> MouseMeasure {
        let s = skin(&format!("[M]\nMeasure=Plugin\nPlugin=Mouse\n{extra}"));
        let mut m = MouseMeasure::new();
        m.read_options(&OptionReader { parser: &s.parser, section: "M", sv: None });
        m
    }

    #[test]
    fn drag_runs_while_a_button_is_held() {
        let mut m = make("LeftMouseDragAction=[!SetVariable X $MouseX$][!SetVariable Y $MouseY$]\nDelay=0\n");
        let sz = (200.0, 100.0);
        assert!(m.pointer_event(&MouseEvent::Move { x: 5.0, y: 6.0 }, sz, (0.0, 0.0)).is_empty());
        m.pointer_event(&MouseEvent::Down { button: MouseButton::Left, x: 5.0, y: 6.0 }, sz, (0.0, 0.0));
        let a = m.pointer_event(&MouseEvent::Move { x: 40.7, y: -3.0 }, sz, (0.0, 0.0));
        assert_eq!(a, ["[!SetVariable X 40][!SetVariable Y -3]"]);
        m.pointer_event(&MouseEvent::Up { button: MouseButton::Left, x: 0.0, y: 0.0 }, sz, (0.0, 0.0));
        assert!(m.pointer_event(&MouseEvent::Move { x: 1.0, y: 1.0 }, sz, (0.0, 0.0)).is_empty());
    }

    #[test]
    fn screen_coordinates_and_move_action_throttle() {
        let mut m = make("MouseMoveAction=[!Log $MouseX$,$MouseY$]\nRelativeToSkin=0\nDelay=100000\n");
        let a = m.pointer_event(&MouseEvent::Move { x: 10.0, y: 20.0 }, (50.0, 50.0), (1000.0, 500.0));
        assert_eq!(a, ["[!Log 1010,520]"]);
        assert!(m.pointer_event(&MouseEvent::Move { x: 11.0, y: 20.0 }, (50.0, 50.0), (1000.0, 500.0)).is_empty());
    }

    #[test]
    fn button_actions_and_require_dragging() {
        let mut m = make("LeftMouseUpAction=[!Log up $MouseX:%$]\nRequireDragging=1\nMouseScrollUpAction=[!Log up]\n");
        let sz = (100.0, 100.0);
        assert!(m.pointer_event(&MouseEvent::Up { button: MouseButton::Left, x: 49.0, y: 0.0 }, sz, (0.0, 0.0)).is_empty());
        m.command("Start");
        assert_eq!(m.pointer_event(&MouseEvent::Up { button: MouseButton::Left, x: 49.0, y: 0.0 }, sz, (0.0, 0.0)), ["[!Log up 50]"]);
        assert_eq!(m.pointer_event(&MouseEvent::Scroll { dx: 0, dy: 1, x: 0.0, y: 0.0 }, sz, (0.0, 0.0)), ["[!Log up]"]);
        m.command("Stop");
        assert!(m.pointer_event(&MouseEvent::Scroll { dx: 0, dy: 1, x: 0.0, y: 0.0 }, sz, (0.0, 0.0)).is_empty());
        m.command("Bogus");
        assert_eq!(m.take_log().len(), 1);
    }
}
