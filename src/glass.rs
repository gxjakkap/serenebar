//! The glass styles: what is behind the bar, bent at the rim.
use crate::settings::Style;

#[derive(Clone, Copy)]
pub struct GlassParams {
    pub bend: f32,
    pub blur: f32,
    pub saturation: f32,
}

/// The glass a style is made of, or None for the solid ones.
pub fn params(style: Style) -> Option<GlassParams> {
    let glass = |bend, blur, saturation| {
        Some(GlassParams {
            bend,
            blur,
            saturation,
        })
    };
    match style {
        Style::Lagoon => glass(18.0, 0.6, 1.25), // clear water: the strongest bend at the rim
        Style::Frost => glass(12.0, 9.0, 1.7),   // frosted, with the colour behind pushed up
        Style::Dusk => glass(14.0, 2.5, 1.0),
        Style::Ember => glass(12.0, 8.0, 1.4),
        Style::Tide => glass(12.0, 8.0, 1.3),
        Style::Mist => glass(12.0, 10.0, 1.2),
        Style::Obsidian | Style::Slate | Style::Porcelain | Style::Sand | Style::Bare => None,
    }
}

impl GlassParams {
    /// Pixels sampled beyond the bar on each side, so the rim has something to bend in.
    pub fn margin(&self, k: f32) -> i32 {
        (self.bend * k).ceil() as i32 + 2
    }

    /// Width in pixels of the band inside the rim that bends.
    pub fn band(&self, k: f32) -> f32 {
        (self.bend * k).max(1.0) * 1.6
    }

    /// For Direct2D's displacement effect, output(p) = input(p + scale * (map(p) - 0.5)),
    /// and `displacement_map` spans ±0.5.
    pub fn scale(&self) -> f32 {
        2.0 * self.bend
    }
}

/// BGRA displacement map for a rounded rect at sx, sy, w, h inside a bw×bh region.
/// Pixels near the rim sample further outward, which pulls the scene beyond the edge
/// into the bar and bends it. Every other pixel stays at 0.5, meaning no offset.
pub fn displacement_map(
    bw: u32,
    bh: u32,
    sx: i32,
    sy: i32,
    w: i32,
    h: i32,
    r: f32,
    band: f32,
) -> Vec<u8> {
    let mut out = vec![0u8; (bw * bh * 4) as usize];
    let (hw, hh) = (w as f32 / 2.0, h as f32 / 2.0);
    let r = r.min(hw).min(hh);
    let (cx, cy) = (sx as f32 + hw, sy as f32 + hh);
    for y in 0..bh {
        for x in 0..bw {
            let (px, py) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let (qx, qy) = (px.abs() - (hw - r), py.abs() - (hh - r));
            let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
            let depth = -(outside + qx.max(qy).min(0.0) - r); // > 0 inside the shape
            let (mut dx, mut dy) = (0.0f32, 0.0f32);
            if depth > 0.0 && depth < band {
                let (nx, ny) = if qx > 0.0 && qy > 0.0 {
                    let l = (qx * qx + qy * qy).sqrt();
                    (qx / l * px.signum(), qy / l * py.signum())
                } else if qx > qy {
                    (px.signum(), 0.0)
                } else {
                    (0.0, py.signum())
                };
                let s = (1.0 - depth / band).powi(2);
                (dx, dy) = (nx * s, ny * s);
            }
            let i = ((y * bw + x) * 4) as usize;
            out[i] = 128;
            out[i + 1] = (128.0 + dy * 127.0) as u8; // G channel carries the y offset
            out[i + 2] = (128.0 + dx * 127.0) as u8; // R channel carries the x offset
            out[i + 3] = 255;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rim_bends_outward_centre_flat() {
        // 100x40 bar with radius 10, placed at 10,10 inside a 120x60 region
        let m = displacement_map(120, 60, 10, 10, 100, 40, 10.0, 8.0);
        let px = |x: u32, y: u32| {
            let i = ((y * 120 + x) * 4) as usize;
            (m[i + 2] as i32 - 128, m[i + 1] as i32 - 128)
        };
        assert_eq!(px(60, 30), (0, 0)); // centre untouched
        assert_eq!(px(2, 2), (0, 0)); // outside untouched
        assert!(px(60, 11).1 < -60); // just inside the top edge, it samples upward
        assert!(px(60, 48).1 > 60); // bottom edge samples downward
        assert!(px(11, 30).0 < -60); // left edge samples leftward
        assert!(px(108, 30).0 > 60); // right edge samples rightward
        assert_eq!(px(12, 12), (0, 0)); // cut-off corner is outside the shape
        let (cx, cy) = px(14, 14); // inside the rounded corner it samples diagonally outward
        assert!(cx < 0 && cy < 0);
    }
}
