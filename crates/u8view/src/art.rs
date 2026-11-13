//! The original's window art (U8GUMPS.FLX) as sprites at the view's scale, shared by the menus and
//! the item windows. Also GUMPAGE.DAT: where each window shows its items.
use std::collections::HashMap;
use std::path::Path;
use u8data::{palette::Palette, shape::ShapeArchive};
use u8render::{upscale, Canvas, Sprite};

pub struct GumpArt {
    pub shapes: ShapeArchive,
    pal: Palette,
    /// (shape, frame, times bigger, outline trimmed) → sprite.
    sprites: HashMap<(u32, u32, u32, bool), Sprite>,
    /// (canvas pixels per game pixel, xBRZ?)
    style: (u32, bool),
    /// GUMPAGE.DAT: gump n's item area [x, y, w, h] at n - 1.
    areas: Vec<[i32; 4]>,
}

impl GumpArt {
    pub fn load(root: &Path, pal: &Palette) -> Option<Self> {
        let read = |p: &str| std::fs::read(u8data::find_file(root, p).ok()?).ok();
        let shapes = ShapeArchive::new(u8data::flex::Flex::parse(read("STATIC/U8GUMPS.FLX")?).ok()?);
        let areas = read("STATIC/GUMPAGE.DAT")
            .unwrap_or_default()
            .as_chunks::<8>()
            .0
            .iter()
            .map(|r| {
                let v = |k: usize| i16::from_le_bytes([r[k], r[k + 1]]) as i32;
                [v(0), v(2), v(4) - v(0), v(6) - v(2)]
            })
            .collect();
        Some(Self { shapes, pal: pal.clone(), sprites: HashMap::new(), style: (1, false), areas })
    }

    /// Where gump `shape` shows its contents, relative to its top-left.
    pub fn area(&self, shape: u32) -> [i32; 4] {
        self.areas
            .get(shape.wrapping_sub(1) as usize)
            .copied()
            .filter(|a| a[2] > 0 && a[3] > 0)
            .unwrap_or([8, 8, 80, 60])
    }

    /// Draw at this scale from now on (sprites are remade when it changes).
    pub fn set_style(&mut self, scale: u32, smooth: bool) {
        if self.style != (scale.max(1), smooth) {
            self.style = (scale.max(1), smooth);
            self.sprites.clear();
        }
    }

    pub fn scale(&self) -> u32 {
        self.style.0
    }

    fn sprite(&mut self, shape: u32, frame: u32, k: u32, trim: bool) -> Option<&Sprite> {
        let (s, smooth) = (self.style.0 * k, self.style.1);
        let key = (shape, frame, k, trim);
        if !self.sprites.contains_key(&key) {
            let f = self.shapes.frame(shape, frame)?;
            let mut rgba = f.to_rgba(&self.pal, false);
            if trim {
                // Without its outermost pixels (a dark outline).
                for y in 0..f.height {
                    for x in 0..f.width {
                        if x == 0 || y == 0 || x + 1 == f.width || y + 1 == f.height {
                            rgba[((y * f.width + x) * 4) as usize..][..4].fill(0);
                        }
                    }
                }
            }
            let up = if smooth { upscale::xbrz } else { upscale::nearest };
            let sp = Sprite::new(
                f.width * s,
                f.height * s,
                f.xoff * k as i32,
                f.yoff * k as i32,
                up(&rgba, f.width, f.height, s),
            );
            self.sprites.insert(key, sp);
        }
        self.sprites.get(&key)
    }

    /// Draw gump `shape` frame `frame` with its top-left (less its hotspot) at game pixel (x, y).
    /// Returns where it went, [x, y, w, h].
    pub fn draw(&mut self, canvas: &mut Canvas, shape: u32, frame: u32, x: i32, y: i32) -> [i32; 4] {
        self.draw_scaled(canvas, shape, frame, (x, y), 1, false)
    }

    /// [`draw`](Self::draw), `k` times bigger (a size in game pixels at that size), and with
    /// `trim`, without the art's outermost ring of pixels.
    pub fn draw_scaled(
        &mut self,
        canvas: &mut Canvas,
        shape: u32,
        frame: u32,
        (x, y): (i32, i32),
        k: u32,
        trim: bool,
    ) -> [i32; 4] {
        let s = self.style.0 as i32;
        let Some(sp) = self.sprite(shape, frame, k.max(1), trim) else { return [x, y, 0, 0] };
        let (xo, yo, w, h) = (sp.xoff, sp.yoff, sp.w as i32 / s, sp.h as i32 / s);
        canvas.blit(sp, (x - xo) * s, (y - yo) * s);
        [x - xo, y - yo, w, h]
    }

    /// Is gump `shape` (frame 0) opaque at (x, y) from its top-left?
    pub fn solid(&self, shape: u32, x: i32, y: i32) -> bool {
        self.shapes.frame(shape, 0).is_some_and(|f| f.hit(x + f.xoff, y + f.yoff))
    }

    /// Size of gump `shape` (frame 0).
    pub fn size(&self, shape: u32) -> (i32, i32) {
        self.shapes.frame(shape, 0).map_or((0, 0), |f| (f.width as i32, f.height as i32))
    }
}
