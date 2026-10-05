//! Hand-drafted strokes for the blueprint look: wobbly double-stroked boxes
//! (in the spirit of rough.js), grid paper, hatching and sketched rings.
//!
//! Every shape is seeded, so it is drawn identically on every frame instead
//! of shimmering.

use gpui_kit::*;

/// Small deterministic generator (SplitMix64).
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0x9e37_79b9_7f4a_7c15)
    }

    /// Uniform in [-1, 1].
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        (z >> 40) as f32 / (1u64 << 23) as f32 - 1.
    }
}

/// Seed from any string, e.g. an element id.
pub fn seed(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

fn pt(x: f32, y: f32) -> Point<Pixels> {
    point(px(x), px(y))
}

/// A slightly bowed line with overshooting ends, like a quick pen stroke.
fn rough_line(window: &mut Window, a: (f32, f32), b: (f32, f32), rng: &mut Rng, wobble: f32, width: f32, color: Rgba) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt().max(1.);
    let (ux, uy) = (dx / len, dy / len);
    // Perpendicular unit vector.
    let (nx, ny) = (-uy, ux);
    let amp = wobble.min(len * 0.02);
    let over = 1.5 + rng.next().abs() * 1.5;
    let start = (a.0 - ux * over + nx * rng.next() * amp * 0.5, a.1 - uy * over + ny * rng.next() * amp * 0.5);
    let end = (b.0 + ux * over * 0.6 + nx * rng.next() * amp * 0.5, b.1 + uy * over * 0.6 + ny * rng.next() * amp * 0.5);
    let c1 = (a.0 + dx * 0.33 + nx * rng.next() * amp, a.1 + dy * 0.33 + ny * rng.next() * amp);
    let c2 = (a.0 + dx * 0.66 + nx * rng.next() * amp, a.1 + dy * 0.66 + ny * rng.next() * amp);

    let mut path = PathBuilder::stroke(px(width));
    path.move_to(pt(start.0, start.1));
    path.cubic_bezier_to(pt(end.0, end.1), pt(c1.0, c1.1), pt(c2.0, c2.1));
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

fn with_alpha(mut c: Rgba, a: f32) -> Rgba {
    c.a *= a;
    c
}

/// A hand-drawn rectangle outline that fills its (relative) parent.
/// Two passes per side: a firm stroke and a lighter second one beside it.
pub fn border(seed: u64, color: Rgba, width: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let o = bounds.origin;
            let (x0, y0) = (f32::from(o.x) + 1., f32::from(o.y) + 1.);
            let (x1, y1) = (x0 + f32::from(bounds.size.width) - 2., y0 + f32::from(bounds.size.height) - 2.);
            let mut rng = Rng::new(seed);
            let sides = [((x0, y0), (x1, y0)), ((x1, y0), (x1, y1)), ((x1, y1), (x0, y1)), ((x0, y1), (x0, y0))];
            for (pass, alpha, w) in [(0, 1.0, width), (1, 0.45, width * 0.7)] {
                for (a, b) in sides {
                    let shift = if pass == 0 { 0. } else { rng.next() * 0.9 };
                    rough_line(
                        window,
                        (a.0 + shift, a.1 + shift),
                        (b.0 - shift, b.1 + shift),
                        &mut rng,
                        2.2,
                        w,
                        with_alpha(color, alpha),
                    );
                }
            }
        },
    )
    .absolute()
    .inset_0()
}

/// A single hand-drawn horizontal rule across its parent.
pub fn rule(seed: u64, color: Rgba, width: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let mut rng = Rng::new(seed);
            let y = f32::from(bounds.origin.y) + f32::from(bounds.size.height) / 2.;
            let x0 = f32::from(bounds.origin.x);
            let x1 = x0 + f32::from(bounds.size.width);
            rough_line(window, (x0 + 2., y), (x1 - 2., y), &mut rng, 1.6, width, color);
        },
    )
    .h(px(6.))
    .w_full()
}

/// Drafting paper: a fine grid with a heavier line every fifth square.
pub fn grid(step: f32, minor: Rgba, major: Rgba) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let (x0, y0) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
            let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            let mut i = 0;
            let mut x = 0.;
            while x <= w {
                let color = if i % 5 == 0 { major } else { minor };
                window.paint_quad(fill(Bounds::new(pt(x0 + x, y0), size(px(1.), px(h))), color));
                x += step;
                i += 1;
            }
            let mut j = 0;
            let mut y = 0.;
            while y <= h {
                let color = if j % 5 == 0 { major } else { minor };
                window.paint_quad(fill(Bounds::new(pt(x0, y0 + y), size(px(w), px(1.))), color));
                y += step;
                j += 1;
            }
        },
    )
    .absolute()
    .inset_0()
}

/// Loose diagonal hatching across the parent, like a shaded region on a drawing.
pub fn hatch(seed: u64, color: Rgba, spacing: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let mut rng = Rng::new(seed);
            let (x0, y0) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
            let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            // Lines at 45°: x - y = k, clipped to the rectangle.
            let mut k = -h + spacing * 0.5;
            while k < w {
                let (ax, ay) = if k >= 0. { (k, 0.) } else { (0., -k) };
                let (bx, by) = if k + h <= w { (k + h, h) } else { (w, w - k) };
                if bx - ax > 4. {
                    rough_line(window, (x0 + ax, y0 + ay), (x0 + bx, y0 + by), &mut rng, 1.2, 1., color);
                }
                k += spacing;
            }
        },
    )
    .absolute()
    .inset_0()
}

/// A sketched ring (two loose passes) centered in its box.
pub fn ring(seed: u64, color: Rgba, width: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let mut rng = Rng::new(seed);
            let cx = f32::from(bounds.origin.x) + f32::from(bounds.size.width) / 2.;
            let cy = f32::from(bounds.origin.y) + f32::from(bounds.size.height) / 2.;
            let r = f32::from(bounds.size.width).min(f32::from(bounds.size.height)) / 2. - 1.5;
            for (pass, alpha) in [(0, 1.0f32), (1, 0.5)] {
                let mut path = PathBuilder::stroke(px(if pass == 0 { width } else { width * 0.7 }));
                let start = rng.next() * 0.6;
                let steps = 14;
                for i in 0..=steps + 1 {
                    // Slightly more than a full turn, so the ends cross like a pen loop.
                    let t = start + i as f32 / steps as f32 * std::f32::consts::TAU;
                    let rr = r + rng.next() * 0.6;
                    let p = pt(cx + rr * t.cos(), cy + rr * t.sin());
                    if i == 0 {
                        path.move_to(p);
                    } else {
                        path.line_to(p);
                    }
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, with_alpha(color, alpha));
                }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_is_deterministic_and_bounded() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        for _ in 0..1000 {
            let (x, y) = (a.next(), b.next());
            assert_eq!(x, y);
            assert!((-1.0..=1.0).contains(&x));
        }
        assert_ne!(Rng::new(1).next(), Rng::new(2).next());
    }

    #[test]
    fn seeds_differ() {
        assert_ne!(seed("details"), seed("logs"));
        assert_eq!(seed("x"), seed("x"));
    }
}
