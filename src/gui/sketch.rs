//! Drafting strokes for the blueprint look: dead-straight construction lines
//! that run past their corners, a blueprint grid, and straight hatching.

use gpui_kit::*;

fn pt(x: f32, y: f32) -> Point<Pixels> {
    point(px(x), px(y))
}

fn hline(window: &mut Window, x0: f32, x1: f32, y: f32, width: f32, color: Rgba) {
    window.paint_quad(fill(Bounds::new(pt(x0, y), size(px(x1 - x0), px(width))), color));
}

fn vline(window: &mut Window, x: f32, y0: f32, y1: f32, width: f32, color: Rgba) {
    window.paint_quad(fill(Bounds::new(pt(x, y0), size(px(width), px(y1 - y0))), color));
}

/// An outline for its (relative) parent, drawn as four straight lines that
/// overshoot each corner by `overshoot`, like construction lines on a drawing.
pub fn border(color: Rgba, width: f32, overshoot: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let (x0, y0) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
            let (x1, y1) = (x0 + f32::from(bounds.size.width) - width, y0 + f32::from(bounds.size.height) - width);
            let o = overshoot;
            hline(window, x0 - o, x1 + width + o, y0, width, color);
            hline(window, x0 - o, x1 + width + o, y1, width, color);
            vline(window, x0, y0 - o, y1 + width + o, width, color);
            vline(window, x1, y0 - o, y1 + width + o, width, color);
        },
    )
    .absolute()
    .inset_0()
}

/// A straight hairline across its parent.
pub fn rule(color: Rgba) -> Div {
    div().h(px(1.)).w_full().flex_none().bg(color)
}

/// Blueprint grid: hairlines every `step`, a heavier line every `major` steps.
pub fn grid(step: f32, major: usize, minor_color: Rgba, major_color: Rgba) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let (x0, y0) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
            let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            let color = |i: usize| if i % major == 0 { major_color } else { minor_color };
            let mut i = 0;
            while i as f32 * step <= w {
                vline(window, x0 + i as f32 * step, y0, y0 + h, 1., color(i));
                i += 1;
            }
            let mut j = 0;
            while j as f32 * step <= h {
                hline(window, x0, x0 + w, y0 + j as f32 * step, 1., color(j));
                j += 1;
            }
        },
    )
    .absolute()
    .inset_0()
}

/// A drafting ruler: a baseline on its inner edge with ticks every 10px,
/// longer every 50px and longest every 100px. Horizontal rulers sit above
/// content (baseline at the bottom), vertical ones to its left (baseline on
/// the right).
pub fn ruler(vertical: bool, color: Rgba) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let (x0, y0) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
            let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            let (length, depth) = if vertical { (h, w) } else { (w, h) };
            let tick = |i: usize| match i {
                i if i % 10 == 0 => depth * 0.9,
                i if i % 5 == 0 => depth * 0.6,
                _ => depth * 0.3,
            };
            if vertical {
                vline(window, x0 + w - 1., y0, y0 + h, 1., color);
            } else {
                hline(window, x0, x0 + w, y0 + h - 1., 1., color);
            }
            let mut i = 0;
            while i as f32 * 10. <= length {
                let at = i as f32 * 10.;
                let t = tick(i);
                if vertical {
                    hline(window, x0 + w - t, x0 + w, y0 + at, 1., color);
                } else {
                    vline(window, x0 + at, y0 + h - t, y0 + h, 1., color);
                }
                i += 1;
            }
        },
    )
    .size_full()
}

/// Segments of 45° hatching across a `w`×`h` box, `spacing` apart, clipped to it.
fn hatch_segments(w: f32, h: f32, spacing: f32) -> Vec<((f32, f32), (f32, f32))> {
    // Lines satisfy x - y = k; walk k across the box.
    let mut out = Vec::new();
    let mut k = -h + spacing / 2.;
    while k < w {
        let a = if k >= 0. { (k, 0.) } else { (0., -k) };
        let b = if k + h <= w { (k + h, h) } else { (w, w - k) };
        if b.0 - a.0 > 0.5 {
            out.push((a, b));
        }
        k += spacing;
    }
    out
}

/// Straight diagonal hatching filling its parent, like pencil shading.
pub fn hatch(color: Rgba, spacing: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let (x0, y0) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
            let segments = hatch_segments(f32::from(bounds.size.width), f32::from(bounds.size.height), spacing);
            let mut path = PathBuilder::stroke(px(1.));
            for (a, b) in segments {
                path.move_to(pt(x0 + a.0, y0 + a.1));
                path.line_to(pt(x0 + b.0, y0 + b.1));
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        },
    )
    .absolute()
    .inset_0()
}

#[cfg(test)]
mod tests {
    // Not `super::*`: that would pull in gpui's `test` macro and shadow `#[test]`.
    use super::hatch_segments;

    #[test]
    fn hatching_stays_inside_the_box() {
        let (w, h) = (200., 50.);
        let segs = hatch_segments(w, h, 6.);
        assert!(segs.len() > 30);
        for ((ax, ay), (bx, by)) in segs {
            for (x, y) in [(ax, ay), (bx, by)] {
                assert!((-0.01..=w + 0.01).contains(&x) && (-0.01..=h + 0.01).contains(&y));
            }
            // 45°: equal run and rise.
            assert!(((bx - ax) - (by - ay)).abs() < 0.01);
        }
    }
}
