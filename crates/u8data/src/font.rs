//! U8FONTS.FLX: each font is a shape whose frame N is character N.
use crate::{
    flex::Flex,
    palette::Palette,
    shape::{parse_shape, Shape},
};

pub struct Fonts(Vec<Shape>);

/// A run of text laid out in one font: RGBA pixels plus size.
pub struct TextImage {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

impl Fonts {
    pub fn parse(f: &Flex) -> Self {
        Self((0..f.len()).map(|i| f.get(i).and_then(|d| parse_shape(d).ok()).unwrap_or_default()).collect())
    }

    fn glyph(&self, font: usize, c: u8) -> Option<&crate::shape::Frame> {
        self.0.get(font)?.get(c as usize).filter(|f| f.width > 0)
    }

    /// Line height: tallest of a few reference glyphs.
    pub fn height(&self, font: usize) -> u32 {
        b"AgWy|".iter().filter_map(|&c| self.glyph(font, c)).map(|f| f.height).max().unwrap_or(12)
    }

    fn advance(&self, font: usize, c: u8) -> u32 {
        match self.glyph(font, c) {
            Some(f) => f.width,
            None => self.glyph(font, b'n').map_or(6, |f| f.width),
        }
    }

    pub fn width(&self, font: usize, s: &str) -> u32 {
        s.bytes().map(|c| self.advance(font, c)).sum()
    }

    /// Greedy word wrap to `max_w` pixels.
    pub fn wrap(&self, font: usize, s: &str, max_w: u32) -> Vec<String> {
        let mut lines = vec![];
        for para in s.split('\n') {
            let mut line = String::new();
            for word in para.split(' ').filter(|w| !w.is_empty()) {
                let cand = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
                if self.width(font, &cand) > max_w && !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                    line = word.to_string();
                } else {
                    line = cand;
                }
            }
            lines.push(line);
        }
        lines
    }

    /// Height as the original lays text out: the tallest character.
    pub fn full_height(&self, font: usize) -> u32 {
        self.0.get(font).map_or(12, |s| s.iter().map(|f| f.height).max().unwrap_or(12))
    }

    /// Where lines' baselines are: the lowest character hotspot.
    fn baseline(&self, font: usize) -> i32 {
        self.0.get(font).map_or(0, |s| s.iter().map(|f| f.yoff).max().unwrap_or(0))
    }

    /// From one line's top to the next's: height plus the font's lead (u8game.ini [fontleads]).
    pub fn line_skip(&self, font: usize) -> u32 {
        let lead = match font {
            4 | 9 | 16 => 0,
            10 | 11 => 4,
            _ => -1,
        };
        (self.full_height(font) as i32 + lead).max(1) as u32
    }

    /// Lay out game text in this font as the original does (see [`typeset`]); `height` 0: no
    /// limit. Returns the lines and how many bytes of `text` they took.
    pub fn typeset(
        &self,
        font: usize,
        text: &str,
        width: u32,
        height: u32,
        paging: bool,
        centre: bool,
    ) -> (Vec<String>, usize) {
        let w = |s: &[u8]| s.iter().map(|&c| self.advance(font, c)).sum::<u32>();
        let page = (height > 0).then_some(height);
        typeset(text, w, (self.full_height(font), self.line_skip(font)), (width, page), paging, centre)
    }

    /// Lines from [`typeset`](Self::typeset), `line_skip` apart, flush left or centred in `width`
    /// (0: the widest line), drawn in the font's own colours with no margin.
    pub fn render_set(&self, font: usize, lines: &[String], pal: &Palette, width: u32, centre: bool) -> TextImage {
        let skip = self.line_skip(font);
        let w = if width > 0 { width } else { lines.iter().map(|l| self.width(font, l)).max().unwrap_or(0) }.max(1);
        let h = (skip * lines.len().max(1) as u32).saturating_sub(skip) + self.full_height(font);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        let base = self.baseline(font);
        for (li, l) in lines.iter().enumerate() {
            let mut x = if centre { w.saturating_sub(self.width(font, l)) / 2 } else { 0 } as i32;
            let top = (li as u32 * skip) as i32 + base;
            for c in l.bytes() {
                if let Some(g) = self.glyph(font, c) {
                    for gy in 0..g.height as i32 {
                        for gx in 0..g.width as i32 {
                            let k = (gy * g.width as i32 + gx) as usize;
                            let (px, py) = (x - g.xoff + gx, top - g.yoff + gy);
                            if g.mask[k] && (0..w as i32).contains(&px) && (0..h as i32).contains(&py) {
                                let d = ((py as u32 * w + px as u32) * 4) as usize;
                                rgba[d..d + 4].copy_from_slice(&pal.0[g.pixels[k] as usize]);
                            }
                        }
                    }
                }
                x += self.advance(font, c) as i32;
            }
        }
        TextImage { w, h, rgba }
    }

    /// Render wrapped, centred lines. `tint` replaces opaque glyph colours (None = font's own).
    pub fn render(&self, font: usize, lines: &[String], pal: &Palette, tint: Option<[u8; 3]>) -> TextImage {
        self.render_lines(font, lines, pal, tint, true)
    }

    /// Render lines flush left (speech, as the original shows it).
    pub fn render_left(&self, font: usize, lines: &[String], pal: &Palette) -> TextImage {
        self.render_lines(font, lines, pal, None, false)
    }

    fn render_lines(
        &self,
        font: usize,
        lines: &[String],
        pal: &Palette,
        tint: Option<[u8; 3]>,
        centre: bool,
    ) -> TextImage {
        let lh = self.height(font) + 1;
        let w = lines.iter().map(|l| self.width(font, l)).max().unwrap_or(0).max(1) + 2;
        let h = lh * lines.len().max(1) as u32 + 2;
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for (li, line) in lines.iter().enumerate() {
            let mut x = if centre { 1 + (w - 2 - self.width(font, line)) / 2 } else { 1 };
            let base = 1 + li as u32 * lh;
            for c in line.bytes() {
                if let Some(g) = self.glyph(font, c) {
                    for gy in 0..g.height {
                        for gx in 0..g.width {
                            let k = (gy * g.width + gx) as usize;
                            if !g.mask[k] {
                                continue;
                            }
                            // Glyphs hang from the line top; yoff is ignored for simplicity.
                            let (px, py) = (x + gx, base + gy);
                            if px >= w || py >= h {
                                continue;
                            }
                            let d = ((py * w + px) * 4) as usize;
                            let c = pal.0[g.pixels[k] as usize];
                            let c = match tint {
                                Some(t) if c[0] as u16 + c[1] as u16 + c[2] as u16 > 120 => [t[0], t[1], t[2], 255],
                                _ => c,
                            };
                            rgba[d..d + 4].copy_from_slice(&c);
                        }
                    }
                }
                x += self.advance(font, c);
            }
        }
        TextImage { w, h, rgba }
    }
}

/// Lay out game text as the original does (ScummVM's typesetText with U8's specials), in any
/// font: `width` measures a run of text. `~` and `*` break lines, `%` is a tab (four spaces,
/// dropped at the start of a centred line), `^` a space; words wrap at `max_w` (0: never),
/// overlong ones break mid-word. Lines are `skip` apart and `line_h` high; stops when the next
/// wouldn't fit in `page` or, with `paging`, after a `*`. Returns the lines and how many bytes
/// of `text` they took.
pub fn typeset(
    text: &str,
    width: impl Fn(&[u8]) -> u32,
    (line_h, skip): (u32, u32),
    (max_w, page): (u32, Option<u32>),
    paging: bool,
    centre: bool,
) -> (Vec<String>, usize) {
    let t = text.as_bytes();
    let is_space = |c: u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b'%' | b'~' | b'*' | b'^');
    let is_break = |c: u8| matches!(c, b'\n' | b'~' | b'*');
    let (mut lines, mut cur, mut i, mut total, mut break_here) = (vec![], vec![], 0, 0, false);
    let line = |l: &mut Vec<u8>| String::from_utf8_lossy(&std::mem::take(l)).into_owned();
    loop {
        if i == t.len() || break_here || is_break(t[i]) {
            let page_break = paging && !break_here && i < t.len() && t[i] == b'*';
            lines.push(line(&mut cur));
            total += skip;
            if i == t.len() {
                return (lines, i);
            }
            if !std::mem::take(&mut break_here) {
                i += 1;
            }
            if page_break || page.is_some_and(|h| total + line_h > h) {
                return (lines, i);
            }
            continue;
        }
        let next = i + t[i..].iter().take_while(|&&c| is_space(c)).count();
        let mut spaces = vec![];
        while i < next && !is_break(t[i]) {
            match t[i] {
                b'\t' | b'%' if !(cur.is_empty() && centre) => spaces.extend_from_slice(b"    "),
                b'\t' | b'%' => {}
                _ if !cur.is_empty() => spaces.push(b' '),
                _ => {}
            }
            i += 1;
        }
        if i < next || next == t.len() {
            continue; // at a line break, or only spaces left
        }
        let end = next + t[next..].iter().take_while(|&&c| !is_space(c)).count();
        let cand = [&cur[..], &spaces, &t[next..end]].concat();
        if max_w == 0 || width(&cand) <= max_w {
            (cur, i) = (cand, end);
            continue;
        }
        if cur.is_empty() {
            // A word wider than the line: as much as fits (at least a character).
            let mut k = next + 1;
            while k < end && width(&[&spaces[..], &t[next..k + 1]].concat()) <= max_w {
                k += 1;
            }
            cur = [&spaces[..], &t[next..k]].concat();
            i = k;
        } else {
            i = next;
        }
        break_here = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shape::Frame;

    /// Font 9 of a made-up set: every character 5 wide, 9 high (so lines are 9 apart).
    fn fonts() -> Fonts {
        let glyph = Frame { width: 5, height: 9, xoff: 0, yoff: 7, pixels: vec![1; 45], mask: vec![true; 45] };
        let mut v = vec![Shape::default(); 10];
        v[9] = vec![glyph; 256];
        Fonts(v)
    }

    fn set(text: &str, w: u32, h: u32, paging: bool) -> (Vec<String>, usize) {
        fonts().typeset(9, text, w, h, paging, false)
    }

    #[test]
    fn tildes_and_stars_break_lines() {
        assert_eq!(set("GUARD LOG: ~~Bloodwatch: quiet.", 0, 0, false).0, ["GUARD LOG:", "", "Bloodwatch: quiet."]);
        assert_eq!(set("SIVLE*THE MINSTREL", 0, 0, false).0, ["SIVLE", "THE MINSTREL"]);
    }

    #[test]
    fn a_star_ends_a_book_page() {
        let (lines, used) = set("one*two", 0, 0, true);
        assert_eq!((lines, used), (vec!["one".to_string()], 4));
    }

    #[test]
    fn percent_indents_and_caret_is_a_space() {
        assert_eq!(set("a.%Next^word", 0, 0, false).0, ["a.    Next word"]);
        assert_eq!(set("%Indented", 0, 0, false).0, ["    Indented"]);
        assert_eq!(fonts().typeset(9, "%Centred", 0, 0, false, true).0, ["Centred"]);
    }

    #[test]
    fn words_wrap_and_long_ones_break() {
        // 30 px: six characters a line.
        assert_eq!(set("aa bb cc dd", 30, 0, false).0, ["aa bb", "cc dd"]);
        assert_eq!(set("abcdefghij", 30, 0, false).0, ["abcdef", "ghij"]);
    }

    #[test]
    fn a_full_page_says_where_the_next_starts() {
        // 20 px high: two 9-px lines fit, not three.
        let text = "one~two~three~four";
        let (lines, used) = set(text, 0, 20, true);
        assert_eq!(lines, ["one", "two"]);
        assert_eq!(&text[used..], "three~four");
    }

    #[test]
    fn set_text_is_line_skip_apart() {
        let f = fonts();
        let img = f.render_set(9, &["ab".into(), "c".into()], &Palette([[9, 9, 9, 255]; 256]), 0, false);
        assert_eq!((img.w, img.h), (10, 18));
    }
}
