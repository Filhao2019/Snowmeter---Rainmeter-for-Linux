//! Input region from the rendered alpha channel: pixels with alpha > 0 receive
//! pointer events, the rest is click-through.

/// `(x, y, w, h)`.
pub type RectI = (i32, i32, i32, i32);

/// Row runs of non-zero alpha, with identical consecutive rows merged into
/// taller rectangles.
pub fn mask_rects(w: i32, h: i32, alpha: &[u8]) -> Vec<RectI> {
    let (w, h) = (w.max(0) as usize, h.max(0) as usize);
    if w == 0 || h == 0 || alpha.len() < w * h {
        return Vec::new();
    }
    let mut out: Vec<RectI> = Vec::new();
    // Rects still growing downwards: (x, w, start_y).
    let mut open: Vec<(i32, i32, i32)> = Vec::new();
    for y in 0..h {
        let row = &alpha[y * w..(y + 1) * w];
        let mut runs: Vec<(i32, i32)> = Vec::new();
        let mut x = 0;
        while x < w {
            if row[x] > 0 {
                let s = x;
                while x < w && row[x] > 0 {
                    x += 1;
                }
                runs.push((s as i32, (x - s) as i32));
            } else {
                x += 1;
            }
        }
        let mut next_open = Vec::with_capacity(runs.len());
        for &(rx, rw) in &runs {
            if let Some(pos) = open.iter().position(|&(ox, ow, _)| ox == rx && ow == rw) {
                next_open.push(open.remove(pos));
            } else {
                next_open.push((rx, rw, y as i32));
            }
        }
        for (ox, ow, oy) in open.drain(..) {
            out.push((ox, oy, ow, y as i32 - oy));
        }
        open = next_open;
    }
    for (ox, ow, oy) in open {
        out.push((ox, oy, ow, h as i32 - oy));
    }
    out.sort_by_key(|&(x, y, _, _)| (y, x));
    out
}

/// Build the cairo region used with `gdk::Surface::set_input_region`.
pub fn to_region(rects: &[RectI]) -> gtk4::cairo::Region {
    let list: Vec<gtk4::cairo::RectangleInt> =
        rects.iter().map(|&(x, y, w, h)| gtk4::cairo::RectangleInt::new(x, y, w, h)).collect();
    gtk4::cairo::Region::create_rectangles(&list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_full() {
        assert!(mask_rects(4, 3, &[0; 12]).is_empty());
        assert_eq!(mask_rects(4, 3, &[255; 12]), vec![(0, 0, 4, 3)]);
        assert!(mask_rects(0, 0, &[]).is_empty());
        assert!(mask_rects(4, 4, &[1; 3]).is_empty());
    }

    #[test]
    fn runs_are_merged_vertically() {
        #[rustfmt::skip]
        let m = [
            0, 1, 1, 0,
            0, 1, 1, 0,
            1, 0, 0, 1,
        ];
        let r = mask_rects(4, 3, &m);
        assert_eq!(r, vec![(1, 0, 2, 2), (0, 2, 1, 1), (3, 2, 1, 1)]);
    }

    #[test]
    fn partial_alpha_counts() {
        let r = mask_rects(3, 1, &[0, 1, 0]);
        assert_eq!(r, vec![(1, 0, 1, 1)]);
    }

    #[test]
    fn region_has_area() {
        let reg = to_region(&[(0, 0, 4, 2), (0, 2, 2, 1)]);
        assert!(reg.contains_point(1, 1));
        assert!(!reg.contains_point(3, 2));
    }
}
