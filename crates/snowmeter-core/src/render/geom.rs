//! Small path helpers shared by RoundLine and Shape.

use std::f64::consts::PI;

/// Center parameterisation of an SVG-style endpoint arc:
/// `(cx, cy, rx, ry, theta1, delta_theta)` with radii scaled up when they are
/// too small. `None` for degenerate input (a straight segment is drawn then).
pub fn arc_center(
    from: (f64, f64),
    to: (f64, f64),
    rx: f64,
    ry: f64,
    rotation_deg: f64,
    large: bool,
    clockwise: bool,
) -> Option<(f64, f64, f64, f64, f64, f64)> {
    let (mut rx, mut ry) = (rx.max(0.0), ry.max(0.0));
    if rx == 0.0 || ry == 0.0 || from == to {
        return None;
    }
    let phi = rotation_deg.to_radians();
    let (sp, cp) = phi.sin_cos();
    let dx2 = (from.0 - to.0) / 2.0;
    let dy2 = (from.1 - to.1) / 2.0;
    let x1p = cp * dx2 + sp * dy2;
    let y1p = -sp * dx2 + cp * dy2;
    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let mut coef = if den == 0.0 { 0.0 } else { (num / den).max(0.0).sqrt() };
    // SVG: sign is + when large != sweep (sweep = clockwise on screen).
    if large == clockwise {
        coef = -coef;
    }
    let cxp = coef * (rx * y1p / ry);
    let cyp = coef * (-ry * x1p / rx);
    let cx = cp * cxp - sp * cyp + (from.0 + to.0) / 2.0;
    let cy = sp * cxp + cp * cyp + (from.1 + to.1) / 2.0;
    let ang = |ux: f64, uy: f64, vx: f64, vy: f64| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let t1 = ang(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut dt = ang((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry);
    if !clockwise && dt > 0.0 {
        dt -= 2.0 * PI;
    } else if clockwise && dt < 0.0 {
        dt += 2.0 * PI;
    }
    Some((cx, cy, rx, ry, t1, dt))
}

/// Append an SVG-style elliptical arc from the current point to `to`.
#[allow(clippy::too_many_arguments)]
pub fn arc_to(
    cr: &cairo::Context,
    from: (f64, f64),
    to: (f64, f64),
    rx: f64,
    ry: f64,
    rotation_deg: f64,
    large: bool,
    clockwise: bool,
) {
    match arc_center(from, to, rx, ry, rotation_deg, large, clockwise) {
        None => cr.line_to(to.0, to.1),
        Some((cx, cy, rx, ry, t1, dt)) => {
            let m = cr.matrix();
            cr.translate(cx, cy);
            cr.rotate(rotation_deg.to_radians());
            cr.scale(rx, ry);
            if dt >= 0.0 {
                cr.arc(0.0, 0.0, 1.0, t1, t1 + dt);
            } else {
                cr.arc_negative(0.0, 0.0, 1.0, t1, t1 + dt);
            }
            cr.set_matrix(m);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarter_circle_clockwise() {
        // (10,0) -> (0,10) around (0,0), clockwise on screen (positive angle).
        let (cx, cy, rx, ry, t1, dt) = arc_center((10.0, 0.0), (0.0, 10.0), 10.0, 10.0, 0.0, false, true).unwrap();
        assert!(cx.abs() < 1e-9 && cy.abs() < 1e-9 && (rx - 10.0).abs() < 1e-9 && (ry - 10.0).abs() < 1e-9);
        assert!(t1.abs() < 1e-9);
        assert!((dt - PI / 2.0).abs() < 1e-9);
    }

    #[test]
    fn counter_clockwise_goes_the_other_way() {
        let (_, _, _, _, _, dt) = arc_center((10.0, 0.0), (0.0, 10.0), 10.0, 10.0, 0.0, true, false).unwrap();
        assert!((dt + 1.5 * PI).abs() < 1e-9);
    }

    #[test]
    fn small_radius_is_scaled_up_and_zero_is_a_line() {
        let (cx, cy, rx, _, _, dt) = arc_center((0.0, 0.0), (10.0, 0.0), 1.0, 1.0, 0.0, false, true).unwrap();
        assert!((cx - 5.0).abs() < 1e-9 && cy.abs() < 1e-9 && (rx - 5.0).abs() < 1e-9);
        assert!((dt.abs() - PI).abs() < 1e-9);
        assert!(arc_center((0.0, 0.0), (10.0, 0.0), 0.0, 5.0, 0.0, false, true).is_none());
    }
}
