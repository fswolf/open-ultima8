//! Pixels for Ultima 8: sprites (palette → RGBA, upscaling, caching), the isometric painter that
//! sorts and draws a [`u8engine::World`], and text. CPU-only; output is an RGBA [`Canvas`] that
//! any presenter (the wgpu viewer, a PNG writer) can show.
pub mod painter;
pub mod sprite;
pub mod text;
pub mod upscale;

pub use painter::{project_fine, DrawOpts, Painter};
pub use sprite::{Sprite, SpriteCache};
pub use upscale::Upscaler;

/// World → screen, in unscaled game pixels: each term divided on its own, as the original does
/// (measured in DOS: things off the 8-unit grid land a pixel apart from `(x - y) / 4` against
/// the scenery; ScummVM's sort boxes round the same way).
pub fn project(x: i32, y: i32, z: i32) -> (i32, i32) {
    (x / 4 - y / 4, x / 8 + y / 8 - z)
}

/// An RGBA8 pixel buffer being drawn into, alpha premultiplied: a world frame is opaque; an
/// overlay (the windows and menus over it) starts clear and keeps what's drawn on it.
pub struct Canvas<'a> {
    pub px: &'a mut [u8],
    pub w: u32,
    pub h: u32,
}

impl<'a> Canvas<'a> {
    pub fn new(px: &'a mut [u8], w: u32, h: u32) -> Self {
        assert_eq!(px.len(), (w * h * 4) as usize, "canvas buffer size mismatch");
        Self { px, w, h }
    }

    pub fn fill(&mut self, c: [u8; 4]) {
        self.px.chunks_exact_mut(4).for_each(|p| p.copy_from_slice(&c));
    }

    /// All see-through (an overlay, before drawing on it).
    pub fn clear(&mut self) {
        self.px.fill(0);
    }

    /// Alpha-blend a sprite with its top-left at (x, y): its opaque runs are copied, the
    /// see-through pixels between them skipped, the rest blended.
    pub fn blit(&mut self, sp: &Sprite, x: i32, y: i32) {
        let (bw, bh) = (self.w as i32, self.h as i32);
        if x >= bw || y >= bh || x + sp.w as i32 <= 0 || y + sp.h as i32 <= 0 {
            return;
        }
        let row = |r: u32| (r as i32 + y >= 0 && r as i32 + y < bh).then(|| (r as i32 + y) as usize * bw as usize);
        for &(r, a, b) in &sp.runs {
            let Some(drow) = row(r) else { continue };
            let (a, b) = ((x + a as i32).max(0), (x + b as i32).min(bw));
            if a < b {
                let s0 = ((r * sp.w) as usize + (a - x) as usize) * 4;
                let n = (b - a) as usize * 4;
                let d0 = (drow + a as usize) * 4;
                self.px[d0..d0 + n].copy_from_slice(&sp.rgba[s0..s0 + n]);
            }
        }
        for &(r, c) in &sp.blends {
            let (Some(drow), px) = (row(r), x + c as i32) else { continue };
            if !(0..bw).contains(&px) {
                continue;
            }
            let (s, d) = (((r * sp.w + c) * 4) as usize, (drow + px as usize) * 4);
            let a = sp.rgba[s + 3] as u16;
            for k in 0..3 {
                self.px[d + k] = ((sp.rgba[s + k] as u16 * a + self.px[d + k] as u16 * (255 - a)) / 255) as u8;
            }
            self.px[d + 3] = (a + self.px[d + 3] as u16 * (255 - a) / 255) as u8;
        }
    }

    /// Set one pixel (ignored outside the canvas).
    pub fn put(&mut self, x: i32, y: i32, c: [u8; 4]) {
        if x >= 0 && y >= 0 && (x as u32) < self.w && (y as u32) < self.h {
            let d = ((y as u32 * self.w + x as u32) * 4) as usize;
            self.px[d..d + 4].copy_from_slice(&c);
        }
    }
}

/// Do two [x, y, w, h] rectangles overlap?
pub fn overlap(a: &[i32; 4], b: &[i32; 4]) -> bool {
    a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blitting_by_runs_matches_pixel_by_pixel() {
        // A 7×5 sprite of every kind of pixel, put down half off each edge of a 9×6 canvas.
        let (w, h) = (7u32, 5u32);
        let rgba: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                [(i * 37) as u8, (i * 11) as u8, (i * 5) as u8, [0, 255, 255, 128, 255, 0, 64][i as usize % 7]]
            })
            .collect();
        let sp = Sprite::new(w, h, 0, 0, rgba.clone());
        for (x, y) in [(-3, -2), (0, 0), (4, 3), (6, -4), (-6, 4), (9, 0)] {
            let mut a = vec![90u8; 9 * 6 * 4];
            Canvas::new(&mut a, 9, 6).blit(&sp, x, y);
            let mut b = vec![90u8; 9 * 6 * 4];
            for (py, px) in (0..h as i32).flat_map(|py| (0..w as i32).map(move |px| (py, px))) {
                let (cx, cy) = (x + px, y + py);
                if !(0..9).contains(&cx) || !(0..6).contains(&cy) {
                    continue;
                }
                let (s, d) = (((py as u32 * w + px as u32) * 4) as usize, ((cy * 9 + cx) * 4) as usize);
                let al = rgba[s + 3] as u16;
                for k in 0..3 {
                    b[d + k] = ((rgba[s + k] as u16 * al + b[d + k] as u16 * (255 - al)) / 255) as u8;
                }
                b[d + 3] = (al + b[d + 3] as u16 * (255 - al) / 255) as u8;
            }
            assert_eq!(a, b, "at ({x}, {y})");
        }
        assert_eq!(project_fine(1234, 567, 40, 1), project(1234, 567, 40));
        // Each term rounds on its own, as the original: scenery at 7 mod 8 doesn't move down a row.
        assert_eq!(project(15, 15, 0), (0, 2));
        assert_eq!(project(16, 13, 0), (1, 3));
    }
}
