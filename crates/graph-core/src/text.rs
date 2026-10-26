//! Text for axis labels: glyphs are rasterized with fontdue into a small
//! atlas the renderer uploads as a texture. Each texel holds the glyph's
//! coverage in red and a widened copy in green, so one draw gives every
//! label the white halo Desmos puts behind axis numbers.

use crate::tess::Vertex;
use std::collections::HashMap;

/// Width and height of the atlas texture, in texels.
pub const ATLAS_SIZE: u32 = 1024;
/// Texture coordinate of the always-covered 2x2 patch in the atlas corner.
/// Sampling its centre returns full coverage even with linear filtering.
pub const SOLID_UV: [f32; 2] = [1.0 / ATLAS_SIZE as f32, 1.0 / ATLAS_SIZE as f32];
/// Bytes per atlas texel: coverage and halo.
pub const ATLAS_CHANNELS: usize = 2;

/// Fonts to try for labels, in order. Arial is what Desmos uses.
const SYSTEM_FONTS: &[&str] = &[
    "C:\\Windows\\Fonts\\arial.ttf",
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/System/Library/Fonts/Supplemental/Arial.ttf",
];

#[derive(Clone, Copy, Debug)]
struct Glyph {
    /// Top-left corner of the padded bitmap in the atlas.
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    /// Offset from the pen position on the baseline to the bitmap's top-left.
    left: f32,
    top: f32,
    advance: f32,
}

pub struct TextAtlas {
    font: Option<fontdue::Font>,
    /// `ATLAS_SIZE`² texels of `ATLAS_CHANNELS` bytes.
    pub pixels: Vec<u8>,
    /// Bumped whenever `pixels` changes, so the renderer knows to upload.
    pub generation: u64,
    glyphs: HashMap<(char, u32, u32), Glyph>,
    cursor: (u32, u32),
    shelf_height: u32,
}

impl TextAtlas {
    /// An atlas with no font: text measures as zero width and draws nothing.
    pub fn empty() -> Self {
        let mut atlas = TextAtlas {
            font: None,
            pixels: vec![0; (ATLAS_SIZE * ATLAS_SIZE) as usize * ATLAS_CHANNELS],
            generation: 1,
            glyphs: HashMap::new(),
            cursor: (0, 0),
            shelf_height: 0,
        };
        atlas.reset();
        atlas
    }

    pub fn from_font_bytes(bytes: &[u8]) -> Result<Self, String> {
        let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .map_err(|e| e.to_string())?;
        let mut atlas = Self::empty();
        atlas.font = Some(font);
        Ok(atlas)
    }

    /// Loads the first label font found on this machine, or an empty atlas.
    pub fn system() -> Self {
        let windir = std::env::var("WINDIR").ok();
        let from_windir = windir.iter().flat_map(|w| {
            [
                format!("{w}\\Fonts\\arial.ttf"),
                format!("{w}\\Fonts\\segoeui.ttf"),
            ]
        });
        let candidates = from_windir.chain(SYSTEM_FONTS.iter().map(|s| s.to_string()));
        for path in candidates {
            if let Ok(bytes) = std::fs::read(&path) {
                if let Ok(atlas) = Self::from_font_bytes(&bytes) {
                    return atlas;
                }
            }
        }
        Self::empty()
    }

    pub fn has_font(&self) -> bool {
        self.font.is_some()
    }

    /// Clears every glyph and restores the solid patch.
    fn reset(&mut self) {
        self.pixels.fill(0);
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let i = (y * ATLAS_SIZE + x) as usize * ATLAS_CHANNELS;
            self.pixels[i] = 255;
            self.pixels[i + 1] = 255;
        }
        self.glyphs.clear();
        self.cursor = (4, 0);
        self.shelf_height = 4;
        self.generation += 1;
    }

    fn glyph(&mut self, c: char, size: f32, halo: f32) -> Option<Glyph> {
        let key = (c, (size * 4.0).round() as u32, (halo * 4.0).round() as u32);
        if let Some(g) = self.glyphs.get(&key) {
            return Some(*g);
        }
        let font = self.font.as_ref()?;
        let (metrics, coverage) = font.rasterize(c, size);
        let pad = halo.ceil() as u32 + 1;
        let (w, h) = (
            metrics.width as u32 + 2 * pad,
            metrics.height as u32 + 2 * pad,
        );
        if w > ATLAS_SIZE / 4 || h > ATLAS_SIZE / 4 {
            return None;
        }
        if self.cursor.0 + w > ATLAS_SIZE {
            self.cursor = (0, self.cursor.1 + self.shelf_height);
            self.shelf_height = 0;
        }
        if self.cursor.1 + h > ATLAS_SIZE {
            // Full: start over. Labels re-add what they need on the next frame.
            self.reset();
        }
        let (x, y) = self.cursor;
        self.cursor.0 += w;
        self.shelf_height = self.shelf_height.max(h);

        let at = |px: i64, py: i64| -> f32 {
            if px < 0 || py < 0 || px >= metrics.width as i64 || py >= metrics.height as i64 {
                0.0
            } else {
                coverage[py as usize * metrics.width + px as usize] as f32 / 255.0
            }
        };
        let reach = halo.ceil() as i64;
        for by in 0..h as i64 {
            for bx in 0..w as i64 {
                let (gx, gy) = (bx - pad as i64, by - pad as i64);
                let ink = at(gx, gy);
                // The halo is the coverage dilated by a disc of radius `halo`,
                // with a soft one-pixel rim.
                let mut glow = ink;
                for dy in -reach..=reach {
                    for dx in -reach..=reach {
                        let d = ((dx * dx + dy * dy) as f32).sqrt();
                        let weight = (halo + 0.5 - d).clamp(0.0, 1.0);
                        if weight > 0.0 {
                            glow = glow.max(at(gx + dx, gy + dy) * weight);
                        }
                    }
                }
                let i = ((y + by as u32) * ATLAS_SIZE + x + bx as u32) as usize * ATLAS_CHANNELS;
                self.pixels[i] = (ink * 255.0).round() as u8;
                self.pixels[i + 1] = (glow * 255.0).round() as u8;
            }
        }
        self.generation += 1;
        let glyph = Glyph {
            x,
            y,
            w,
            h,
            left: metrics.xmin as f32 - pad as f32,
            top: -(metrics.ymin as f32 + metrics.height as f32) - pad as f32,
            advance: metrics.advance_width,
        };
        self.glyphs.insert(key, glyph);
        Some(glyph)
    }

    /// Width of `text` at `size` pixels.
    pub fn measure(&self, text: &str, size: f32) -> f32 {
        let Some(font) = &self.font else { return 0.0 };
        text.chars()
            .map(|c| font.metrics(c, size).advance_width)
            .sum()
    }

    /// Draws `text` with its pen starting at (`x`, `baseline`) and returns
    /// the pen position after the last glyph. Pixel snapping keeps small text
    /// sharp.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        text: &str,
        size: f32,
        halo: f32,
        x: f32,
        baseline: f32,
        color: [f32; 4],
        out: &mut Vec<Vertex>,
    ) -> f32 {
        let mut pen = x;
        let baseline = baseline.round();
        for c in text.chars() {
            let Some(g) = self.glyph(c, size, halo) else {
                continue;
            };
            let x0 = (pen + g.left).round();
            let y0 = baseline + g.top;
            let (x1, y1) = (x0 + g.w as f32, y0 + g.h as f32);
            let s = ATLAS_SIZE as f32;
            let (u0, v0) = (g.x as f32 / s, g.y as f32 / s);
            let (u1, v1) = ((g.x + g.w) as f32 / s, (g.y + g.h) as f32 / s);
            let corners = [
                ([x0, y0], [u0, v0]),
                ([x1, y0], [u1, v0]),
                ([x0, y1], [u0, v1]),
                ([x1, y1], [u1, v1]),
            ];
            for i in [0, 1, 2, 2, 1, 3] {
                out.push(Vertex {
                    pos: corners[i].0,
                    uv: corners[i].1,
                    color,
                });
            }
            pen += g.advance;
        }
        pen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atlas() -> Option<TextAtlas> {
        let atlas = TextAtlas::system();
        if !atlas.has_font() {
            eprintln!("no system font found; skipping");
            return None;
        }
        Some(atlas)
    }

    #[test]
    fn solid_patch_survives_reset() {
        let mut a = TextAtlas::empty();
        a.reset();
        assert_eq!(&a.pixels[..2], &[255, 255]);
        assert_eq!(a.measure("12", 14.0), 0.0, "no font means no width");
    }

    #[test]
    fn digits_rasterize_with_halo() {
        let Some(mut a) = atlas() else { return };
        let w = a.measure("10", 14.0);
        assert!(w > 10.0 && w < 30.0, "width {w}");
        let mut out = Vec::new();
        let gen = a.generation;
        a.draw("−8", 14.0, 2.0, 0.0, 20.0, [0.0, 0.0, 0.0, 1.0], &mut out);
        assert_eq!(out.len(), 12);
        assert!(a.generation > gen);
        // Halo coverage is never less than ink coverage.
        assert!(a.pixels.chunks(2).all(|p| p[1] >= p[0]));
        assert!(a.pixels.chunks(2).skip(4).any(|p| p[0] > 200));
    }
}
