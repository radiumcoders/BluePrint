//! Drafting strokes for the blueprint look: dead-straight construction lines
//! that run past their corners, a dot grid, and straight hatching.

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

/// A dot grid, like engineering paper.
pub fn dots(step: f32, radius: f32, color: Rgba) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let (x0, y0) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
            let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            let d = radius * 2.;
            let mut y = step / 2.;
            while y < h {
                let mut x = step / 2.;
                while x < w {
                    let b = Bounds::new(pt(x0 + x - radius, y0 + y - radius), size(px(d), px(d)));
                    window.paint_quad(fill(b, color).corner_radii(px(radius)));
                    x += step;
                }
                y += step;
            }
        },
    )
    .absolute()
    .inset_0()
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
