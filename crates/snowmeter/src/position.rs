//! `WindowX` / `WindowY` / `AnchorX` / `AnchorY` text formats (`50%`, `100R`,
//! `20B`, `@2`, `(formula)`) and `KeepOnScreen` clamping. Pure functions.

use snowmeter_core::env::Rect;

/// One parsed coordinate string.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Coord {
    pub value: f64,
    pub percent: bool,
    /// `R` (X) or `B` (Y): measured from the right/bottom edge.
    pub opposite: bool,
    /// `@N` monitor number (0 = virtual screen).
    pub monitor: Option<usize>,
}

/// Evaluate a leading `(formula)` and substitute the number.
fn expand_formula(s: &str) -> String {
    let t = s.trim();
    if !t.starts_with('(') {
        return t.to_string();
    }
    let mut depth = 0;
    for (i, c) in t.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return match snowmeter_core::formula::evaluate(&t[..=i], &|_| None) {
                        Ok(v) => format!("{v}{}", &t[i + 1..]),
                        Err(_) => t.to_string(),
                    };
                }
            }
            _ => {}
        }
    }
    t.to_string()
}

/// Parse a position or anchor string. `vertical` selects `B` instead of `R`.
pub fn parse_coord(s: &str, vertical: bool) -> Coord {
    let expanded = expand_formula(s);
    let t = expanded.as_str();
    let num_end = t.find(|c: char| !(c == '-' || c == '.' || c.is_ascii_digit())).unwrap_or(t.len());
    let value = t[..num_end].parse::<f64>().unwrap_or(0.0);
    // Modifiers only count after the last '#'.
    let tail = match t.rfind('#') {
        Some(i) => &t[i + 1..],
        None => &t[num_end..],
    };
    let opp = if vertical { 'B' } else { 'R' };
    let monitor = tail.find('@').and_then(|i| {
        let digits: String = tail[i + 1..].chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse::<usize>().ok()
    });
    Coord { value, percent: tail.contains('%'), opposite: tail.contains(opp), monitor }
}

/// Bounding box of all monitors.
pub fn virtual_screen(monitors: &[Rect]) -> Rect {
    let Some(first) = monitors.first() else { return Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 } };
    let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.x + first.w, first.y + first.h);
    for m in monitors {
        x0 = x0.min(m.x);
        y0 = y0.min(m.y);
        x1 = x1.max(m.x + m.w);
        y1 = y1.max(m.y + m.h);
    }
    Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

fn pick_monitor(monitors: &[Rect], n: Option<usize>) -> Rect {
    match n {
        Some(0) => virtual_screen(monitors),
        Some(i) if i >= 1 && i <= monitors.len() => monitors[i - 1],
        _ => monitors.first().copied().unwrap_or(Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 }),
    }
}

/// Anchor offset in pixels (`10`, `25%`, `10R`) for a window of `size` pixels.
pub fn anchor_px(c: &Coord, size: f64) -> f64 {
    let v = c.value.max(0.0);
    let px = if c.percent { size * v / 100.0 } else { v };
    if c.opposite {
        size - px
    } else {
        px
    }
}

/// Resolve the window's top-left corner. `size` is the window size.
pub fn resolve_position(x: &str, y: &str, ax: &str, ay: &str, size: (f64, f64), monitors: &[Rect]) -> (f64, f64) {
    let (cx, cy) = (parse_coord(x, false), parse_coord(y, true));
    // An `@N` on one axis applies to both when the other has none.
    let n = cx.monitor.or(cy.monitor);
    let m = pick_monitor(monitors, n);
    let axis = |c: &Coord, origin: f64, extent: f64, anchor: f64| {
        let off = if c.percent { extent * c.value / 100.0 } else { c.value };
        origin + if c.opposite { extent - off } else { off } - anchor
    };
    let (a_x, a_y) = (anchor_px(&parse_coord(ax, false), size.0), anchor_px(&parse_coord(ay, true), size.1));
    (axis(&cx, m.x, m.w, a_x), axis(&cy, m.y, m.h, a_y))
}

/// Text to store after the user moved the window to `pos` (same style as the
/// authored `old` value: `%`, `R`/`B` and `@N` are kept).
pub fn format_coord(old: &str, pos: f64, anchor: f64, origin: f64, extent: f64, vertical: bool) -> String {
    let c = parse_coord(old, vertical);
    let mut off = pos + anchor - origin;
    if c.opposite {
        off = extent - off;
    }
    let mut s = if c.percent && extent > 0.0 { format!("{:.5}%", off / extent * 100.0) } else { format!("{}", off.round() as i64) };
    if c.opposite {
        s.push(if vertical { 'B' } else { 'R' });
    }
    if let Some(n) = c.monitor {
        s.push_str(&format!("@{n}"));
    }
    s
}

/// Convenience for both axes: returns `(WindowX, WindowY)` text.
pub fn format_position(old: (&str, &str), anchors: (&str, &str), pos: (f64, f64), size: (f64, f64), monitors: &[Rect]) -> (String, String) {
    let (cx, cy) = (parse_coord(old.0, false), parse_coord(old.1, true));
    let m = pick_monitor(monitors, cx.monitor.or(cy.monitor));
    let (ax, ay) = (anchor_px(&parse_coord(anchors.0, false), size.0), anchor_px(&parse_coord(anchors.1, true), size.1));
    let mut sx = format_coord(old.0, pos.0, ax, m.x, m.w, false);
    let mut sy = format_coord(old.1, pos.1, ay, m.y, m.h, true);
    // Keep `@N` on both axes if either had it.
    if let Some(n) = cx.monitor.or(cy.monitor) {
        if cx.monitor.is_none() {
            sx.push_str(&format!("@{n}"));
        }
        if cy.monitor.is_none() {
            sy.push_str(&format!("@{n}"));
        }
    }
    (sx, sy)
}

fn overlap(a: &Rect, b: &Rect) -> f64 {
    let w = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
    let h = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
    if w > 0.0 && h > 0.0 {
        w * h
    } else {
        0.0
    }
}

/// `KeepOnScreen`: move the window fully into the monitor it overlaps most
/// (nearest monitor when it is entirely off-screen).
pub fn keep_on_screen(pos: (f64, f64), size: (f64, f64), monitors: &[Rect]) -> (f64, f64) {
    if monitors.is_empty() {
        return pos;
    }
    let win = Rect { x: pos.0, y: pos.1, w: size.0, h: size.1 };
    let mut best = &monitors[0];
    let mut best_score = f64::NEG_INFINITY;
    for m in monitors {
        let mut score = overlap(&win, m);
        if score == 0.0 {
            let (cx, cy) = (win.x + win.w / 2.0 - (m.x + m.w / 2.0), win.y + win.h / 2.0 - (m.y + m.h / 2.0));
            score = -(cx * cx + cy * cy).sqrt() - 1.0;
        }
        if score > best_score {
            best_score = score;
            best = m;
        }
    }
    let clamp = |p: f64, s: f64, o: f64, e: f64| {
        if s >= e {
            o
        } else {
            p.max(o).min(o + e - s)
        }
    };
    (clamp(pos.0, size.0, best.x, best.w), clamp(pos.1, size.1, best.y, best.h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon() -> Vec<Rect> {
        vec![Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 }, Rect { x: 1920.0, y: 0.0, w: 1000.0, h: 800.0 }]
    }

    fn one(origin: f64, extent: f64) -> Vec<Rect> {
        vec![Rect { x: origin, y: origin, w: extent, h: extent }]
    }

    #[test]
    fn rainmeter_vectors() {
        // 10R (origin 100, extent 1000) -> 1090
        assert_eq!(resolve_position("10R", "0", "0", "0", (0.0, 0.0), &one(100.0, 1000.0)).0, 1090.0);
        // 25% (100, 800) -> 300
        assert_eq!(resolve_position("25%", "0", "0", "0", (0.0, 0.0), &one(100.0, 800.0)).0, 300.0);
        // 10B (50, 600) -> 640
        assert_eq!(resolve_position("0", "10B", "0", "0", (0.0, 0.0), &one(50.0, 600.0)).1, 640.0);
        // -100 (10, 1000) -> -90
        assert_eq!(resolve_position("-100", "0", "0", "0", (0.0, 0.0), &one(10.0, 1000.0)).0, -90.0);
        // -100R (50, 1000) -> 1150
        assert_eq!(resolve_position("-100R", "0", "0", "0", (0.0, 0.0), &one(50.0, 1000.0)).0, 1150.0);
    }

    #[test]
    fn anchors() {
        // Anchor 10 (w 200): 100 -> 90
        assert_eq!(resolve_position("100", "0", "10", "0", (200.0, 10.0), &one(0.0, 1000.0)).0, 90.0);
        // Anchor 10R: anchorPos 190, pos -90
        assert_eq!(resolve_position("100", "0", "10R", "0", (200.0, 10.0), &one(0.0, 1000.0)).0, -90.0);
        // Anchor 25%: 50, pos 50
        assert_eq!(resolve_position("100", "0", "25%", "0", (200.0, 10.0), &one(0.0, 1000.0)).0, 50.0);
        // Anchor 25%R: 150, pos -50
        assert_eq!(resolve_position("100", "0", "25%R", "0", (200.0, 10.0), &one(0.0, 1000.0)).0, -50.0);
    }

    #[test]
    fn monitors_and_formulas() {
        let m = mon();
        assert_eq!(resolve_position("10@2", "20", "0", "0", (1.0, 1.0), &m), (1930.0, 20.0));
        assert_eq!(resolve_position("20", "10@2", "0", "0", (1.0, 1.0), &m), (1940.0, 10.0));
        assert_eq!(resolve_position("0", "0", "0", "0", (1.0, 1.0), &m), (0.0, 0.0));
        assert_eq!(resolve_position("50%@2", "0", "0", "0", (1.0, 1.0), &m).0, 2420.0);
        assert_eq!(resolve_position("(10*2)", "(5+5)", "0", "0", (1.0, 1.0), &m), (20.0, 10.0));
        // Invalid monitor falls back to the primary.
        assert_eq!(resolve_position("5@9", "0", "0", "0", (1.0, 1.0), &m).0, 5.0);
        // Virtual screen.
        assert_eq!(resolve_position("100R@0", "0", "0", "0", (1.0, 1.0), &m).0, 2820.0);
    }

    #[test]
    fn modifiers_after_hash_only() {
        let c = parse_coord("#Foo#R", false);
        assert!(c.opposite);
        let c = parse_coord("5R", true);
        assert!(!c.opposite);
        assert_eq!(parse_coord("abc", false).value, 0.0);
    }

    #[test]
    fn formatting_round_trips() {
        let m = one(0.0, 1000.0);
        assert_eq!(format_coord("12", 77.4, 0.0, 0.0, 1000.0, false), "77");
        assert_eq!(format_coord("10R", 900.0, 0.0, 0.0, 1000.0, false), "100R");
        assert_eq!(format_coord("25%", 500.0, 0.0, 0.0, 1000.0, true), "50.00000%");
        let (x, y) = format_position(("5@2", "5"), ("0", "0"), (1930.0, 30.0), (10.0, 10.0), &mon());
        assert_eq!((x.as_str(), y.as_str()), ("10@2", "30@2"));
        let _ = m;
    }

    #[test]
    fn keep_on_screen_clamps() {
        let m = mon();
        assert_eq!(keep_on_screen((-50.0, -20.0), (100.0, 100.0), &m), (0.0, 0.0));
        assert_eq!(keep_on_screen((1900.0, 1000.0), (100.0, 100.0), &m), (1820.0, 980.0));
        // Mostly on the second monitor.
        assert_eq!(keep_on_screen((2800.0, 10.0), (200.0, 100.0), &m), (2720.0, 10.0));
        // Bigger than the monitor: pinned to the origin.
        assert_eq!(keep_on_screen((30.0, 30.0), (3000.0, 100.0), &m).0, 0.0);
        // Entirely off-screen goes to the nearest monitor.
        assert_eq!(keep_on_screen((5000.0, 50.0), (100.0, 100.0), &m).0, 2820.0);
    }
}
