//! Shape frames as RGBA sprites at a chosen scale, upscaled once and cached.
use crate::upscale::{self, Upscaler};
use std::collections::HashMap;
use std::rc::Rc;
use u8data::{typeflag::flag, GameData};

/// RGBA sprite at the cache's scale; (xoff, yoff) is the hotspot in scaled pixels.
pub struct Sprite {
    pub w: u32,
    pub h: u32,
    pub xoff: i32,
    pub yoff: i32,
    pub rgba: Vec<u8>,
    /// Where it's opaque — (row, first, end) runs, copied whole — and the pixels that blend.
    pub(crate) runs: Vec<(u32, u32, u32)>,
    pub(crate) blends: Vec<(u32, u32)>,
}

impl Sprite {
    pub fn new(w: u32, h: u32, xoff: i32, yoff: i32, rgba: Vec<u8>) -> Self {
        let (mut runs, mut blends) = (vec![], vec![]);
        for y in 0..h {
            let row = &rgba[(y * w * 4) as usize..((y + 1) * w * 4) as usize];
            let mut x = 0;
            while x < w {
                match row[(x * 4 + 3) as usize] {
                    0 => x += 1,
                    255 => {
                        let from = x;
                        while x < w && row[(x * 4 + 3) as usize] == 255 {
                            x += 1;
                        }
                        runs.push((y, from, x));
                    }
                    _ => {
                        blends.push((y, x));
                        x += 1;
                    }
                }
            }
        }
        Self { w, h, xoff, yoff, rgba, runs, blends }
    }
}

struct Slot {
    sprite: Option<Rc<Sprite>>,
    used: u64,
}

/// Default memory budget for cached sprites (upscaled sprites get big: ×6 is 36× the pixels).
pub const DEFAULT_BUDGET: usize = 512 << 20;

pub struct SpriteCache {
    pub scale: u32,
    up: Upscaler,
    map: HashMap<(u32, u32, bool), Slot>,
    bytes: usize,
    budget: usize,
    frame: u64,
}

impl SpriteCache {
    pub fn new(scale: u32, up: Upscaler) -> Self {
        Self { scale: scale.max(1), up, map: HashMap::new(), bytes: 0, budget: DEFAULT_BUDGET, frame: 0 }
    }
    /// Plain pixels at 1×.
    pub fn unscaled() -> Self {
        Self::new(1, Box::new(upscale::nearest))
    }
    pub fn with_budget(mut self, bytes: usize) -> Self {
        self.budget = bytes;
        self
    }
    pub fn set_scale(&mut self, scale: u32, up: Upscaler) {
        self.scale = scale.max(1);
        self.up = up;
        self.map.clear();
        self.bytes = 0;
    }
    /// Bytes of pixel data held.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Call once per drawn frame: over budget, forget sprites the previous frame didn't use.
    pub fn begin_frame(&mut self) {
        self.frame += 1;
        if self.bytes > self.budget {
            let keep_from = self.frame - 1;
            self.map.retain(|_, s| s.used >= keep_from);
            self.bytes = self.map.values().filter_map(|s| s.sprite.as_ref()).map(|s| s.rgba.len()).sum();
        }
    }

    /// Sprite for a frame; `flip` mirrors it about its hotspot (U8 reuses frames for mirrored directions).
    pub fn get(&mut self, gd: &GameData, shape: u32, frame: u32, flip: bool) -> Option<Rc<Sprite>> {
        let now = self.frame;
        if let Some(s) = self.map.get_mut(&(shape, frame, flip)) {
            s.used = now;
            return s.sprite.clone();
        }
        let sprite = self.make(gd, shape, frame, flip).map(Rc::new);
        self.bytes += sprite.as_ref().map_or(0, |s| s.rgba.len());
        self.map.insert((shape, frame, flip), Slot { sprite: sprite.clone(), used: now });
        sprite
    }

    fn make(&self, gd: &GameData, shape: u32, frame: u32, flip: bool) -> Option<Sprite> {
        let s = self.scale;
        let f = gd.shapes.frame(shape, frame)?;
        let transl = gd.typeflags.get(shape).has(flag::TRANSL);
        let src = f.to_rgba(&gd.palette, transl.then_some(&gd.xform));
        let mut rgba = if s > 1 { upscale_seamless(&src, f.width, f.height, s, transl, &self.up) } else { src };
        let (w, h) = (f.width * s, f.height * s);
        let mut xoff = f.xoff * s as i32;
        if flip {
            for row in rgba.chunks_exact_mut((w * 4) as usize) {
                row.as_chunks_mut::<4>().0.reverse();
            }
            xoff = w as i32 - (f.xoff + 1) * s as i32; // mirror about the hotspot pixel
        }
        Some(Sprite::new(w, h, xoff, f.yoff * s as i32, rgba))
    }
}

/// Upscale one sprite without seams: colours are bled into the transparent border first, and the
/// result never covers less than the original pixels did, so neighbouring floor/wall tiles still meet.
fn upscale_seamless(src: &[u8], w: u32, h: u32, s: u32, transl: bool, up: &Upscaler) -> Vec<u8> {
    let mut out = up(&bleed(src, w, h), w, h, s);
    if transl {
        return out; // keep effect-pixel alpha as-is
    }
    let (w2, h2) = (w * s, h * s);
    for y in 0..h2 {
        for x in 0..w2 {
            let orig = src[(((y / s) * w + x / s) * 4 + 3) as usize];
            let a = &mut out[((y * w2 + x) * 4 + 3) as usize];
            *a = if orig == 255 || *a >= 128 { 255 } else { 0 };
        }
    }
    out
}

/// Copy the average colour of opaque neighbours into fully transparent pixels (alpha stays 0).
fn bleed(src: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut out = src.to_vec();
    let (w, h) = (w as i32, h as i32);
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            if src[i + 3] != 0 {
                continue;
            }
            let (mut sum, mut n) = ([0u32; 3], 0);
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w || ny >= h {
                    continue;
                }
                let j = ((ny * w + nx) * 4) as usize;
                if src[j + 3] == 255 {
                    (0..3).for_each(|k| sum[k] += src[j + k] as u32);
                    n += 1;
                }
            }
            if n > 0 {
                (0..3).for_each(|k| out[i + k] = (sum[k] / n) as u8);
            }
        }
    }
    out
}
