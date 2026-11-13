//! The credits and quotes (main menu): the decoded text scrolled up a black 320×200 screen under
//! its section title and a red rule, as the original's CreditsGump lays it out — names in
//! yellow, roles in red, both centred; quotes' speakers indented; blank lines 48 pixels; runs of
//! `&` a short red rule; a new section pauses on a gap before its title takes over.
use u8data::font::{Fonts, TextImage};
use u8data::palette::Palette;
use u8render::{text, Canvas};

const RED_FONT: usize = 6;
const YELLOW_FONT: usize = 8;
/// The scroll window inside the 320×200 screen, and the title above it.
const SCROLL: [i32; 4] = [32, 44, 256, 156];
const TITLE: (i32, i32, u32) = (64, 34, 192);
const RULE_COLOUR: [u8; 4] = [0xD4, 0x30, 0x30, 0xFF];
/// Scrolling speed (pixels a second: one every other tick) and the pause before closing.
const SPEED: f32 = 15.0;
const LINGER: f32 = 4.0;
/// Space a blank line, and a new section, take.
const BLANK: i32 = 48;
const SECTION_GAP: i32 = 160;

/// Which list: the credits (paragraphs close together) or the quotes (spaced out).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Roll {
    Credits,
    Quotes,
}

impl Roll {
    pub fn file(self) -> &'static str {
        match self {
            Roll::Credits => "STATIC/ECREDITS.DAT",
            Roll::Quotes => "STATIC/QUOTES.DAT",
        }
    }
    /// MUSIC.FLX song played under it.
    #[cfg_attr(not(feature = "audio"), allow(dead_code))]
    pub fn song(self) -> u8 {
        match self {
            Roll::Credits => 51,
            Roll::Quotes => 113,
        }
    }
    fn paragraph_gap(self) -> i32 {
        match self {
            Roll::Credits => 24,
            Roll::Quotes => 80,
        }
    }
}

enum Mark {
    Text(TextImage, i32),
    Rule(i32, i32),
}

pub struct Scroller {
    /// What's laid out, at heights from the top of the roll: text (at x) and rules (x, width).
    marks: Vec<(i32, Mark)>,
    /// Section titles and where they take over.
    titles: Vec<(i32, TextImage)>,
    height: i32,
    offset: f32,
    linger: f32,
}

impl Scroller {
    pub fn new(roll: Roll, lines: &[String], fonts: &Fonts, pal: &Palette) -> Self {
        let mut s = Scroller { marks: vec![], titles: vec![], height: 0, offset: 0.0, linger: 0.0 };
        let mut y = 0;
        for line in lines.iter().filter(|l| !l.is_empty()) {
            if let Some(t) = line.strip_prefix('+') {
                let title = t.split('*').next().unwrap_or("");
                let img = fonts.render(RED_FONT, &fonts.wrap(RED_FONT, title, TITLE.2), pal, None);
                if !s.titles.is_empty() {
                    s.titles.push((y, img));
                    y += SECTION_GAP;
                } else {
                    s.titles.push((0, img));
                }
                continue;
            }
            let (mut font, mut centred, mut indent) = (RED_FONT, false, 0);
            let mut rest = line.as_str();
            loop {
                let marked = rest.chars().next().filter(|c| "+&}~@".contains(*c));
                if let Some(m) = marked {
                    rest = &rest[1..];
                    match m {
                        '&' => (font, centred, indent) = (YELLOW_FONT, true, 0),
                        '}' => (font, centred, indent) = (RED_FONT, true, 0),
                        '~' => (font, centred, indent) = (YELLOW_FONT, false, 32),
                        _ => {} // '@': the end
                    }
                }
                let (part, more) = match rest.find('*') {
                    Some(k) => (&rest[..k], Some(&rest[k + 1..])),
                    None => (rest, None),
                };
                if marked.is_none() && part.is_empty() {
                    y += BLANK;
                } else if part.starts_with('&') {
                    let w = (part.len() as i32 * 8).min(192);
                    s.marks.push((y + 3, Mark::Rule(SCROLL[2] / 2 - w / 2, w)));
                    y += 7;
                } else if !part.is_empty() {
                    let room = (SCROLL[2] - indent) as u32;
                    let wrapped = fonts.wrap(font, part, room);
                    let img = if centred {
                        fonts.render(font, &wrapped, pal, None)
                    } else {
                        fonts.render_left(font, &wrapped, pal)
                    };
                    let x = if centred { (SCROLL[2] - img.w as i32) / 2 } else { indent };
                    let h = img.h as i32;
                    s.marks.push((y, Mark::Text(img, x)));
                    y += h;
                }
                match more {
                    Some(m) if !m.is_empty() => rest = m,
                    _ => break,
                }
            }
            y += roll.paragraph_gap();
        }
        s.height = y;
        s
    }

    /// Advance `dt` seconds. True once it has all gone by (and lingered): close it.
    pub fn tick(&mut self, dt: f32) -> bool {
        if self.offset < self.height as f32 {
            self.offset += SPEED * dt;
            return false;
        }
        self.linger += dt;
        self.linger >= LINGER
    }

    /// Draw the whole screen, centred in a vw×vh view.
    pub fn draw(&self, canvas: &mut Canvas, ss: u32, (vw, vh): (u32, u32)) {
        canvas.fill([0, 0, 0, 255]);
        let (ox, oy) = ((vw as i32 - 320) / 2, (vh as i32 - 200) / 2);
        let top = oy + SCROLL[1];
        // The roll starts below the window and rises through it.
        let at = |y: i32| top + SCROLL[3] + y - self.offset as i32;
        for (y, m) in &self.marks {
            let sy = at(*y);
            if sy > oy + 200 || sy < top - 64 {
                continue;
            }
            match m {
                Mark::Text(img, x) => text::draw_plain(canvas, ss, img, ox + SCROLL[0] + x, sy),
                Mark::Rule(x, w) => text::block(canvas, ss, [ox + SCROLL[0] + x, sy, *w, 1], RULE_COLOUR),
            }
        }
        // Black above and below the window, then the title and its rule.
        text::block(canvas, ss, [0, 0, vw as i32, top], [0, 0, 0, 255]);
        text::block(canvas, ss, [0, oy + 200, vw as i32, vh as i32], [0, 0, 0, 255]);
        let shown = self.offset as i32 - SCROLL[3];
        if let Some((_, title)) = self.titles.iter().rev().find(|(y, _)| *y <= shown.max(0)) {
            let x = ox + TITLE.0 + (TITLE.2 as i32 - title.w as i32) / 2;
            text::draw_plain(canvas, ss, title, x, oy + TITLE.1 - title.h as i32);
        }
        text::block(canvas, ss, [ox + TITLE.0, oy + 41, TITLE.2 as i32, 1], RULE_COLOUR);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both rolls lay out from the real files; with `U8_SHOTS` set, frames are written there as PPM.
    #[test]
    #[ignore]
    fn rolls_lay_out() {
        let Ok(dir) = std::env::var("U8_DATA") else { return };
        let root = std::path::PathBuf::from(dir);
        let read = |p: &str| std::fs::read(u8data::find_file(&root, p).unwrap()).unwrap();
        let fonts = Fonts::parse(&u8data::flex::Flex::parse(read("STATIC/U8FONTS.FLX")).unwrap());
        let pal = Palette::parse(&read("STATIC/U8PAL.PAL")).unwrap();
        for roll in [Roll::Credits, Roll::Quotes] {
            let mut s = Scroller::new(roll, &u8data::credits::decode(&read(roll.file())), &fonts, &pal);
            assert!(!s.titles.is_empty() && s.height > 1000, "{roll:?}: {} px", s.height);
            for (k, at) in [300.0f32, 900.0, 2400.0].iter().enumerate() {
                s.offset = *at;
                let mut px = vec![0u8; 320 * 200 * 4];
                s.draw(&mut Canvas::new(&mut px, 320, 200), 1, (320, 200));
                if let Ok(out) = std::env::var("U8_SHOTS") {
                    let mut ppm = b"P6 320 200 255\n".to_vec();
                    px.chunks(4).for_each(|p| ppm.extend(&p[..3]));
                    std::fs::write(format!("{out}/{roll:?}{k}.ppm"), ppm).unwrap();
                }
            }
            // It ends: all the way through, then a pause.
            s.offset = s.height as f32;
            assert!(!s.tick(1.0) && s.tick(LINGER));
        }
    }
}
