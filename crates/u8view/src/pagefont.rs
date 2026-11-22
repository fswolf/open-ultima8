//! Book and scroll text in a modern typeface (EB Garamond, SIL OFL: assets/OFL.txt), laid out
//! as the original lays its pages out but drawn at the screen's own resolution so it reads
//! easily. Sizes are in game pixels; layout is in 1/64ths of one so it's the same at any scale.
use ab_glyph::{point, Font, FontRef, GlyphId, PxScale, ScaleFont};
use u8data::font::{typeset, TextImage};

static TTF: &[u8] = include_bytes!("../assets/EBGaramond08-Regular.ttf");
/// Text size (em), and ink: a dark brown-black.
const SIZE: f32 = 9.0;
const INK: [u8; 3] = [38, 26, 20];
/// Fixed-point units per game pixel.
const UNIT: f32 = 64.0;

pub struct PageFont(FontRef<'static>);

impl PageFont {
    pub fn new() -> Self {
        Self(FontRef::try_from_slice(TTF).expect("bundled font"))
    }

    fn glyphs<'a>(&'a self, s: &'a str) -> impl Iterator<Item = GlyphId> + 'a {
        s.chars().map(|c| self.0.glyph_id(c))
    }

    /// Width of `s` at `px` per em, in pixels.
    fn width(&self, s: &str, px: f32) -> f32 {
        let f = self.0.as_scaled(PxScale::from(px));
        let mut prev = None;
        self.glyphs(s)
            .map(|g| {
                let k = prev.map_or(0.0, |p| f.kern(p, g));
                prev = Some(g);
                k + f.h_advance(g)
            })
            .sum()
    }

    /// Line height and spacing, in game pixels.
    fn lines(&self) -> (f32, f32) {
        let f = self.0.as_scaled(PxScale::from(SIZE));
        (f.ascent() - f.descent(), f.ascent() - f.descent() + f.line_gap())
    }

    /// The pages `text` fills, `page` (w, h) game pixels each — `*` starts a new one.
    pub fn pages(&self, text: &str, page: (u32, u32)) -> Vec<Vec<String>> {
        let w = |s: &[u8]| (self.width(&String::from_utf8_lossy(s), SIZE) * UNIT).ceil() as u32;
        let (h, skip) = self.lines();
        let units = ((h * UNIT) as u32, (skip * UNIT) as u32);
        let size = (page.0 * UNIT as u32, Some(page.1 * UNIT as u32));
        let mut pages = vec![];
        let mut rest = text;
        while !rest.is_empty() {
            let (lines, used) = typeset(rest, w, units, size, true, false);
            pages.push(lines);
            let mut used = used.clamp(1, rest.len());
            while !rest.is_char_boundary(used) {
                used += 1;
            }
            rest = &rest[used..];
        }
        pages
    }

    /// A page's lines drawn at `ss` screen pixels per game pixel: ink colour, coverage as alpha.
    pub fn render(&self, lines: &[String], ss: u32) -> TextImage {
        self.render_ink(lines, SIZE, ss, INK)
    }

    /// Lines at `size` game pixels per em, in `ink` (the settings panel's light type).
    pub fn render_ink(&self, lines: &[String], size: f32, ss: u32, ink: [u8; 3]) -> TextImage {
        let px = size * ss as f32;
        let f = self.0.as_scaled(PxScale::from(px));
        let (_, skip) = self.lines();
        let skip = skip * size / SIZE * ss as f32;
        let w = lines.iter().map(|l| self.width(l, px).ceil() as u32).max().unwrap_or(0).max(1) + 2;
        let h = (skip * lines.len().max(1) as f32 - skip + f.ascent() - f.descent()).ceil() as u32 + 2;
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for (i, l) in lines.iter().enumerate() {
            let base = f.ascent() + skip * i as f32;
            let (mut x, mut prev) = (0.0, None);
            for g in self.glyphs(l) {
                x += prev.map_or(0.0, |p| f.kern(p, g));
                prev = Some(g);
                if let Some(o) = self.0.outline_glyph(g.with_scale_and_position(px, point(x, base))) {
                    let b = o.px_bounds();
                    o.draw(|gx, gy, c| {
                        let (x, y) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
                        if (0..w as i32).contains(&x) && (0..h as i32).contains(&y) {
                            let d = ((y as u32 * w + x as u32) * 4) as usize;
                            let a = (c.clamp(0.0, 1.0) * 255.0) as u8;
                            let a = rgba[d + 3].max(a);
                            rgba[d..d + 4].copy_from_slice(&[ink[0], ink[1], ink[2], a]);
                        }
                    });
                }
                x += f.h_advance(g);
            }
        }
        TextImage { w, h, rgba }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_wrap_break_and_draw_crisp() {
        let f = PageFont::new();
        let pages = f.pages("A heading~~Body text that goes on well past the width of one line.*Page two.", (123, 129));
        assert_eq!(pages.len(), 2, "* starts a page");
        assert_eq!(pages[0][..2], ["A heading", ""]);
        assert!(pages[0].len() > 3, "wrapped: {:?}", pages[0]);
        assert_eq!(pages[1], ["Page two."]);
        // Lines fit the page, and drawing at 3× makes an image up to 3× the page.
        let img = f.render(&pages[0], 3);
        assert!(img.w <= 123 * 3 + 2 && img.h <= 129 * 3, "{}×{}", img.w, img.h);
        assert!(img.rgba.chunks(4).any(|p| p[3] > 0 && p[3] < 255), "anti-aliased");
    }
}
