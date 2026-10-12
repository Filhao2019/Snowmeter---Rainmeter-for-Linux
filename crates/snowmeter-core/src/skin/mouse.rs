//! Mouse actions (skin-format.md section 9): option names, events, and the
//! hit-test / dispatch / over-leave logic of `Skin`.

use super::Skin;
use crate::parser::OptionReader;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    X1,
    X2,
}

/// Points are in skin-local logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MouseEvent {
    Down { button: MouseButton, x: f64, y: f64 },
    Up { button: MouseButton, x: f64, y: f64 },
    DoubleClick { button: MouseButton, x: f64, y: f64 },
    /// `dy > 0` scrolls up, `dx > 0` scrolls right.
    Scroll { dx: i32, dy: i32, x: f64, y: f64 },
    Move { x: f64, y: f64 },
    /// The pointer left the window.
    Leave,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ActionKind {
    Down(MouseButton),
    Up(MouseButton),
    DoubleClick(MouseButton),
    ScrollUp,
    ScrollDown,
    ScrollLeft,
    ScrollRight,
    Over,
    Leave,
}

const BUTTONS: [(MouseButton, &str); 5] = [
    (MouseButton::Left, "Left"),
    (MouseButton::Middle, "Middle"),
    (MouseButton::Right, "Right"),
    (MouseButton::X1, "X1"),
    (MouseButton::X2, "X2"),
];

fn all_kinds() -> Vec<(ActionKind, String)> {
    let mut v = Vec::new();
    for (b, n) in BUTTONS {
        v.push((ActionKind::Up(b), format!("{n}MouseUpAction")));
        v.push((ActionKind::Down(b), format!("{n}MouseDownAction")));
        v.push((ActionKind::DoubleClick(b), format!("{n}MouseDoubleClickAction")));
    }
    v.push((ActionKind::ScrollUp, "MouseScrollUpAction".into()));
    v.push((ActionKind::ScrollDown, "MouseScrollDownAction".into()));
    v.push((ActionKind::ScrollLeft, "MouseScrollLeftAction".into()));
    v.push((ActionKind::ScrollRight, "MouseScrollRightAction".into()));
    v.push((ActionKind::Over, "MouseOverAction".into()));
    v.push((ActionKind::Leave, "MouseLeaveAction".into()));
    v
}

/// The raw (unexpanded) action strings of a meter or of `[Rainmeter]`.
#[derive(Clone, Debug, Default)]
pub struct MouseActions {
    actions: Vec<(ActionKind, String)>,
    /// `!DisableMouseAction`: actions stay defined but are ignored.
    pub disabled: bool,
}

impl MouseActions {
    pub fn read(opt: &OptionReader) -> MouseActions {
        let actions = all_kinds()
            .into_iter()
            .filter_map(|(k, name)| {
                let v = opt.action(&name);
                (!v.trim().is_empty()).then_some((k, v))
            })
            .collect();
        MouseActions { actions, disabled: false }
    }

    pub fn get(&self, kind: ActionKind) -> Option<&str> {
        if self.disabled {
            return None;
        }
        self.actions.iter().find(|(k, _)| *k == kind).map(|(_, v)| v.as_str())
    }

    /// `!ClearMouseAction`: forget every action.
    pub fn clear(&mut self) {
        self.actions.clear();
    }

    pub fn has_any(&self) -> bool {
        !self.disabled && !self.actions.is_empty()
    }

    pub fn has_over_or_leave(&self) -> bool {
        self.get(ActionKind::Over).is_some() || self.get(ActionKind::Leave).is_some()
    }
}

/// Where an action runs: a meter (by index) or the skin itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Target {
    Meter(usize),
    Skin,
}

impl Skin {
    /// Alpha of the last rendered frame at `(x, y)`; 0 means the pointer
    /// passes through to the window below (Windows layered-window rule).
    /// Points outside the skin return 0.
    pub fn hit_mask_alpha(&self, x: f64, y: f64) -> u8 {
        let (w, h) = self.size();
        if x < 0.0 || y < 0.0 || x >= w as f64 || y >= h as f64 {
            return 0;
        }
        self.with_frame(|surf| {
            let stride = surf.stride() as usize;
            let (px, py) = (x as usize, y as usize);
            surf.data()
                .ok()
                .map(|d| {
                    let o = py * stride + px * 4;
                    let v = u32::from_ne_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]);
                    (v >> 24) as u8
                })
                .unwrap_or(0)
        })
        .unwrap_or(0)
    }

    /// Bytes `0..=255` of alpha, row-major `width*height`, for building an
    /// input region.
    pub fn hit_mask(&self) -> (i32, i32, Vec<u8>) {
        let (w, h) = self.size();
        let mut out = vec![0u8; (w.max(0) * h.max(0)) as usize];
        let _ = self.with_frame(|surf| {
            let stride = surf.stride() as usize;
            if let Ok(d) = surf.data() {
                for y in 0..h.max(0) as usize {
                    for x in 0..w.max(0) as usize {
                        let o = y * stride + x * 4;
                        out[y * w as usize + x] = d[o + 3];
                    }
                }
            }
        });
        (w, h, out)
    }

    /// Hit test of meter `i`; a contained meter also needs to hit its container.
    fn meter_hit(&self, i: usize, x: f64, y: f64) -> bool {
        let m = &self.meters[i];
        if let Some(c) = m.base.container_idx {
            if !self.meters[c].hit_test(x, y) {
                return false;
            }
        }
        m.hit_test(x, y)
    }

    /// Topmost meter (reverse order, visible) with an enabled action of
    /// `kind` whose rectangle contains the point.
    fn find_target(&self, kind: ActionKind, x: f64, y: f64) -> Option<Target> {
        for i in (0..self.meters.len()).rev() {
            let m = &self.meters[i];
            if m.base.hidden || m.base.mouse.get(kind).is_none() {
                continue;
            }
            if self.meter_hit(i, x, y) {
                return Some(Target::Meter(i));
            }
        }
        let (w, h) = self.size();
        (self.options.mouse.get(kind).is_some() && x >= 0.0 && y >= 0.0 && x < w as f64 && y < h as f64)
            .then_some(Target::Skin)
    }

    fn run_target(&mut self, t: Target, kind: ActionKind, x: f64, y: f64) {
        let (raw, section, off, dim) = match t {
            Target::Meter(i) => {
                let m = &self.meters[i];
                (
                    m.base.mouse.get(kind).unwrap_or("").to_string(),
                    Some(m.base.name.clone()),
                    (m.base.rect_x, m.base.rect_y),
                    (m.base.w, m.base.h),
                )
            }
            Target::Skin => {
                let (w, h) = self.size();
                (self.options.mouse.get(kind).unwrap_or("").to_string(), None, (0.0, 0.0), (w as f64, h as f64))
            }
        };
        let raw = expand_mouse_vars(&raw, x - off.0, y - off.1, dim);
        self.execute_in(&raw, section.as_deref());
    }

    /// Dispatch a mouse event. Returns true when the event was consumed (an
    /// action exists at the point), which also suppresses the context menu.
    pub fn mouse(&mut self, ev: MouseEvent) -> bool {
        self.mouse_measures(&ev);
        match ev {
            MouseEvent::Leave => {
                self.update_over(None);
                false
            }
            MouseEvent::Move { x, y } => {
                let inside = self.hit_mask_alpha(x, y) > 0;
                self.update_over(inside.then_some((x, y)));
                false
            }
            MouseEvent::Down { button, x, y } => self.click(ActionKind::Down(button), x, y),
            MouseEvent::Up { button, x, y } => {
                let handled = self.click(ActionKind::Up(button), x, y);
                if button == MouseButton::Right && !handled {
                    // Defining any right-button action at the point suppresses the menu.
                    return self.has_action_at(ActionKind::Down(button), x, y)
                        || self.has_action_at(ActionKind::DoubleClick(button), x, y);
                }
                handled
            }
            MouseEvent::DoubleClick { button, x, y } => {
                if self.hit_mask_alpha(x, y) == 0 {
                    return false;
                }
                if self.click(ActionKind::DoubleClick(button), x, y) {
                    return true;
                }
                self.click(ActionKind::Down(button), x, y)
            }
            MouseEvent::Scroll { dx, dy, x, y } => {
                let mut handled = false;
                if dy != 0 {
                    handled |= self.click(if dy > 0 { ActionKind::ScrollUp } else { ActionKind::ScrollDown }, x, y);
                }
                if dx != 0 {
                    handled |= self.click(if dx > 0 { ActionKind::ScrollRight } else { ActionKind::ScrollLeft }, x, y);
                }
                handled
            }
        }
    }

    /// Feed the pointer event to Mouse plugin measures and run what they return.
    fn mouse_measures(&mut self, ev: &MouseEvent) {
        let (w, h) = self.size();
        let (size, pos) = ((w as f64, h as f64), self.screen_pos);
        for i in 0..self.measures.len() {
            let acts = match self.measures[i].m.try_borrow_mut() {
                Ok(mut m) => m.pointer_event(ev, size, pos),
                Err(_) => continue,
            };
            if !acts.is_empty() {
                let name = self.measures[i].name.clone();
                for a in acts {
                    self.execute_in(&a, Some(&name));
                }
            }
        }
    }

    fn has_action_at(&self, kind: ActionKind, x: f64, y: f64) -> bool {
        self.hit_mask_alpha(x, y) > 0 && self.find_target(kind, x, y).is_some()
    }

    fn click(&mut self, kind: ActionKind, x: f64, y: f64) -> bool {
        if self.hit_mask_alpha(x, y) == 0 {
            return false;
        }
        match self.find_target(kind, x, y) {
            Some(t) => {
                self.run_target(t, kind, x, y);
                true
            }
            None => false,
        }
    }

    /// Over/leave processing for a pointer position (`None` = outside).
    fn update_over(&mut self, pos: Option<(f64, f64)>) {
        // Leave first.
        for i in 0..self.meters.len() {
            if !self.over[i] {
                continue;
            }
            let m = &self.meters[i];
            let still = pos.map(|(x, y)| !m.base.hidden && self.meter_hit(i, x, y)).unwrap_or(false);
            if !still {
                self.over[i] = false;
                if m.base.mouse.get(ActionKind::Leave).is_some() {
                    let (x, y) = pos.unwrap_or((-32768.0, -32768.0));
                    self.run_target(Target::Meter(i), ActionKind::Leave, x, y);
                }
            }
        }
        if self.skin_over && pos.is_none() {
            self.skin_over = false;
            if self.options.mouse.get(ActionKind::Leave).is_some() {
                self.run_target(Target::Skin, ActionKind::Leave, -32768.0, -32768.0);
            }
        }
        let Some((x, y)) = pos else { return };
        if !self.skin_over {
            self.skin_over = true;
            if self.options.mouse.get(ActionKind::Over).is_some() {
                self.run_target(Target::Skin, ActionKind::Over, x, y);
            }
        }
        for i in (0..self.meters.len()).rev() {
            let m = &self.meters[i];
            if self.over[i] || m.base.hidden || !m.base.mouse.has_over_or_leave() || !self.meter_hit(i, x, y) {
                continue;
            }
            self.over[i] = true;
            if m.base.mouse.get(ActionKind::Over).is_some() {
                self.run_target(Target::Meter(i), ActionKind::Over, x, y);
            }
        }
    }
}

/// `[$MouseX]`, `[$MouseY]`, `[$MouseX:%]`, `[$MouseY:%]` and the old
/// `$MouseX$` forms (case-insensitive).
pub(crate) fn expand_mouse_vars(s: &str, mx: f64, my: f64, dim: (f64, f64)) -> String {
    if !s.contains('$') {
        return s.to_string();
    }
    let pct = |p: f64, d: f64| if d == 0.0 { 0 } else { ((p + 1.0) / d * 100.0) as i64 };
    let table: [(&str, String); 8] = [
        ("[$MouseX:%]", pct(mx, dim.0).to_string()),
        ("[$MouseY:%]", pct(my, dim.1).to_string()),
        ("[$MouseX]", (mx as i64).to_string()),
        ("[$MouseY]", (my as i64).to_string()),
        ("$MouseX:%$", pct(mx, dim.0).to_string()),
        ("$MouseY:%$", pct(my, dim.1).to_string()),
        ("$MouseX$", (mx as i64).to_string()),
        ("$MouseY$", (my as i64).to_string()),
    ];
    let mut out = String::with_capacity(s.len());
    let lower = s.to_ascii_lowercase();
    let mut i = 0;
    'outer: while i < s.len() {
        for (pat, rep) in &table {
            if lower[i..].starts_with(&pat.to_ascii_lowercase()) {
                out.push_str(rep);
                i += pat.len();
                continue 'outer;
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}
