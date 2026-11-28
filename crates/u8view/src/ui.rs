//! On-screen script UI: speech barks over items, conversation choices, notices, and (in
//! `readable`) things to read.
use crate::art::GumpArt;
use crate::pagefont::PageFont;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use u8data::{
    font::{Fonts, TextImage},
    palette::Palette,
};
use u8engine::usecode::{Pid, ReadKind, UiEvent, Vm, ASK_TIMEOUT};
use u8engine::world::AVATAR;
use u8render::{text, Canvas};

mod readable;
#[cfg(test)]
mod tests;
pub use readable::PageKey;
use readable::*;

/// Speech wraps at this width (game pixels), as in the original.
const BARK_WIDTH: u32 = 194;
/// And a page of it is this high (ScummVM's BarkGump: five lines).
const BARK_HEIGHT: u32 = 55;
/// Notices from the viewer itself.
const SPEECH_FONT: usize = 8;

/// The Guardian's voice: the scripts bark it from object 666, which isn't on any map. (ScummVM
/// makes him an actor of shape 666 to do it: his recorded lines are SOUND/E666.FLX.)
pub const GUARDIAN: u16 = 666;

/// Who speaks in which colour, as in the original (ScummVM's BarkGump): the colour goes with the
/// speaker, not the mood — the Avatar and the Guardian red, other people by object number
/// (white, orange, green in turn — Devon, NPC 2, is green), things yellow.
fn speech_font(item: u16) -> usize {
    match item {
        1 | GUARDIAN => 6,
        257.. => 8,
        n => [0, 5, 7][n as usize % 3],
    }
}
const OPTION_FONT: usize = 0;
const OPTION_HOVER_FONT: usize = 8;
/// The original's conversation choices: red, bulleted, flowed above the Avatar's head.
const CLASSIC_FONT: usize = 6;
const CLASSIC_HOVER_FONT: usize = 5;
const CLASSIC_WIDTH: i32 = 160;

/// How conversation choices are laid out (Settings: Replies).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// A numbered list on a dark panel in the lower part of the screen.
    Modern,
    /// As in the original: red bulleted choices flowed over the Avatar's head.
    Classic,
}

/// An option rendered plain and highlighted.
type Choice = (Words, Words);

/// The readable type's sizes (game pixels to the em; Settings: Speech size), and how wide its
/// speech is let run: as many letters a line as the original's 194.
pub const SPEECH_SIZES: [(&str, f32); 4] = [("small", 9.0), ("medium", 11.0), ("large", 13.0), ("huge", 16.0)];
const fn speech_width(size: f32) -> u32 {
    (BARK_WIDTH as f32 * size / 11.0) as u32
}

/// Words on the overlay: in a game font (`ink` None), or in the readable type in that colour,
/// then drawn at the screen's own resolution (`ss`, the one it was last set for).
struct Words {
    lines: Vec<String>,
    ink: Option<[u8; 3]>,
    img: TextImage,
    ss: u32,
}

impl Words {
    fn game(img: TextImage) -> Self {
        Self { lines: vec![], ink: None, img, ss: 1 }
    }

    /// Its size in game pixels.
    fn size(&self) -> (i32, i32) {
        let s = self.ss as i32;
        ((self.img.w as i32 + s - 1) / s, (self.img.h as i32 + s - 1) / s)
    }

    /// Set again in the readable type at the overlay's resolution, if that's changed.
    fn fit(&mut self, font: Option<&PageFont>, ss: u32) {
        if let (Some(ink), Some(f)) = (self.ink, font) {
            if self.ss != ss {
                (self.img, self.ss) = (f.render_outlined(&self.lines, ss, ink), ss);
            }
        }
    }

    /// Draw it at game pixel (x, y); a game font `ringed` gets a dark outline.
    fn draw(&self, canvas: &mut Canvas, ss: u32, x: i32, y: i32, ringed: bool) {
        match self.ink {
            Some(_) => text::draw_blended(canvas, &self.img, x * ss as i32, y * ss as i32),
            None if ringed => text::draw(canvas, ss, &self.img, x, y),
            None => text::draw_plain(canvas, ss, &self.img, x, y),
        }
    }
}

enum Panel {
    Bark {
        pid: Pid,
        item: u16,
        img: Words,
        /// Game time this page stays up for.
        left: Duration,
        /// The pages still to come, each with its time (long speech, as the original pages it).
        more: VecDeque<(Words, Duration)>,
        /// Where this page went, from the point over the speaker (see [`place_bark`]); it then
        /// moves with the speaker. None: not placed yet.
        off: Option<(i32, i32)>,
    },
    Ask {
        pid: Pid,
        options: Vec<String>,
        imgs: Vec<Choice>,
        classic: Vec<Choice>,
        rects: Vec<[i32; 4]>,
        hover: Option<usize>,
        /// Game time it has waited for an answer (see [`ASK_TIMEOUT`]).
        waited: Duration,
    },
    Read(Readable),
    /// A message from the viewer itself ("Game saved."), at the top for a moment.
    Notice {
        img: TextImage,
        until: Instant,
    },
}

pub struct ScriptUi {
    fonts: Fonts,
    type_font: PageFont,
    /// Speech and replies in the readable type (None: the game's fonts), and its size.
    speech: Option<PageFont>,
    pub speech_size: usize,
    panels: Vec<Panel>,
    pub style: Style,
}

impl ScriptUi {
    pub fn new(fonts: Fonts) -> Self {
        Self { fonts, type_font: PageFont::new(), speech: None, speech_size: 1, panels: vec![], style: Style::Modern }
    }

    /// Speech and replies in the readable type (`on`) at one of [`SPEECH_SIZES`], or in the
    /// game's fonts, from the next words said.
    pub fn set_speech(&mut self, on: bool, size: usize) {
        self.speech_size = size.min(SPEECH_SIZES.len() - 1);
        self.speech = on.then(|| PageFont::speech(SPEECH_SIZES[self.speech_size].1));
    }

    /// Is speech set in the readable type (drawn at the screen's resolution)?
    pub fn typed(&self) -> bool {
        self.speech.is_some()
    }

    /// The colour a game font's letters are (the commonest one but its black outline).
    fn ink(&self, font: usize, pal: &Palette) -> [u8; 3] {
        let img = self.fonts.render_set(font, &["MWHOEA".to_string()], pal, 0, false);
        let mut seen = std::collections::HashMap::new();
        for p in img.rgba.chunks(4).filter(|p| p[3] > 0 && p[..3] != [0, 0, 0]) {
            *seen.entry([p[0], p[1], p[2]]).or_insert(0) += 1;
        }
        seen.into_iter().max_by_key(|&(c, n)| (n, c)).map_or([255; 3], |(c, _)| c)
    }

    /// One line in game font `font` (`set`: as speech is set, else as the numbered replies), or
    /// in the readable type in its colour.
    fn words(&self, font: usize, line: String, pal: &Palette, set: bool) -> Words {
        match &self.speech {
            Some(f) => {
                let (lines, ink) = (vec![line], self.ink(font, pal));
                Words { img: f.render_outlined(&lines, 1, ink), lines, ink: Some(ink), ss: 1 }
            }
            None if set => Words::game(self.fonts.render_set(font, &[line], pal, 0, false)),
            None => Words::game(self.fonts.render(font, &[line], pal, None)),
        }
    }
    pub fn toggle_style(&mut self) {
        self.style = if self.style == Style::Modern { Style::Classic } else { Style::Modern };
    }

    pub fn clear(&mut self) {
        self.panels.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.panels.is_empty()
    }

    /// A question or a page is open: the player has to answer it first.
    pub fn blocking(&self) -> bool {
        self.panels.iter().any(|p| matches!(p, Panel::Ask { .. } | Panel::Read(_)))
    }

    /// Show what a script asked for. `spoken`: seconds of recorded speech playing with a bark
    /// (it stays up at least that long).
    pub fn push(&mut self, ev: UiEvent, pal: &Palette, spoken: Option<f32>, art: Option<&GumpArt>) {
        match ev {
            UiEvent::Bark { pid, item, text } => {
                let font = speech_font(item);
                let mut pages: VecDeque<_> = self.bark_pages(font, text.trim(), pal, spoken).into();
                let (img, left) = pages
                    .pop_front()
                    .unwrap_or_else(|| (Words::game(self.fonts.render_set(font, &[], pal, 0, false)), Duration::ZERO));
                self.panels.push(Panel::Bark { pid, item, img, left, more: pages, off: None });
            }
            UiEvent::Ask { pid, options, .. } => {
                let choices =
                    |(a, b): (usize, usize), set: bool, line: &dyn Fn(usize, &str) -> String| -> Vec<Choice> {
                        let one = |f, i, o: &String| self.words(f, line(i, o.trim()), pal, set);
                        options.iter().enumerate().map(|(i, o)| (one(a, i, o), one(b, i, o))).collect()
                    };
                let imgs = choices((OPTION_FONT, OPTION_HOVER_FONT), false, &|i, o| format!("{}. {o}", i + 1));
                // The original's: "@ " (its bullet, a dot in the readable type) and the answer,
                // in the Avatar's speech font.
                let bullet = if self.typed() { "\u{2022}" } else { "@" };
                let classic = choices((CLASSIC_FONT, CLASSIC_HOVER_FONT), true, &|_, o| format!("{bullet} {o}"));
                self.panels.push(Panel::Ask {
                    pid,
                    options,
                    imgs,
                    classic,
                    rects: vec![],
                    hover: None,
                    waited: Duration::ZERO,
                });
            }
            UiEvent::Read { pid, kind, gump, text, .. } => {
                let r = Readable::new((&self.fonts, &self.type_font), pal, pid, (kind, gump), &text, art);
                self.panels.push(Panel::Read(r));
            }
            _ => {} // sounds are for the audio side
        }
    }

    /// Text in one of the game's fonts.
    pub fn text(&self, font: usize, s: &str, pal: &Palette) -> TextImage {
        self.fonts.render_left(font, &[s.to_string()], pal)
    }

    /// Show a message from the viewer for a few seconds.
    pub fn notice(&mut self, text: &str, pal: &Palette) {
        self.panels.retain(|p| !matches!(p, Panel::Notice { .. }));
        let img = self.fonts.render(SPEECH_FONT, &[text.to_string()], pal, None);
        self.panels.push(Panel::Notice { img, until: Instant::now() + Duration::from_millis(2500) });
    }

    /// Speech laid out as the original does (ScummVM's BarkGump: a text box 194 × 55, so five
    /// lines a page), each page with how long it stays up: in the classic style the original's
    /// time (measured in DOS at text speed 5: about 2.9 s plus 16 ms a letter: "Hello there."
    /// 3.2 s, 81 letters 4.2 s); in the modern one at least ~70 ms a letter to read it, up to
    /// 8 s. Recorded speech keeps each page up for its share of it (the last 0.3 s longer).
    /// In the readable type: the same, five lines a page as wide as its size lets them run.
    fn bark_pages(&self, font: usize, text: &str, pal: &Palette, spoken: Option<f32>) -> Vec<(Words, Duration)> {
        let time = |n: u64| {
            let mut ms = 2900 + 16 * n;
            if self.style == Style::Modern {
                ms = ms.max((n * 70).min(8000));
            }
            if let Some(s) = spoken {
                ms = ms.max((s * 1000.0) as u64 * n / text.len().max(1) as u64);
            }
            Duration::from_millis(ms)
        };
        let mut pages = vec![];
        if let Some(f) = &self.speech {
            let ink = self.ink(font, pal);
            let mut set = f.speech_pages(text, speech_width(f.size()), 5);
            if set.is_empty() {
                set.push((vec![], "")); // (nothing said still stays its while, as the original's)
            }
            for (lines, took) in set {
                let img = f.render_outlined(&lines, 1, ink);
                pages.push((Words { lines, ink: Some(ink), img, ss: 1 }, time(took.trim().len() as u64)));
            }
        }
        let mut rest = text;
        while self.speech.is_none() && (!rest.is_empty() || pages.is_empty()) {
            let (lines, used) = self.fonts.typeset(font, rest, BARK_WIDTH, BARK_HEIGHT, false, false);
            // (At least a letter, so it always moves on; never inside a multi-byte one.)
            let used = (used.max(1)..=rest.len()).find(|&i| rest.is_char_boundary(i)).unwrap_or(rest.len());
            let n = rest[..used].trim().len() as u64;
            pages.push((Words::game(self.fonts.render_set(font, &lines, pal, 0, false)), time(n)));
            rest = rest[used..].trim_start();
            if lines.is_empty() {
                break;
            }
        }
        if let (Some(_), Some(last)) = (spoken, pages.last_mut()) {
            last.1 += Duration::from_millis(300);
        }
        pages
    }

    /// Retire notices whose time is up (while the game is paused). True if any went.
    pub fn expire_notices(&mut self) -> bool {
        let now = Instant::now();
        let before = self.panels.len();
        self.panels.retain(|p| !matches!(p, Panel::Notice { until, .. } if *until <= now));
        before != self.panels.len()
    }

    /// Retire barks whose time is up, and questions left unanswered for [`ASK_TIMEOUT`] of game
    /// time `dt` (an empty answer, as the original gives). Returns true if anything changed.
    pub fn expire(&mut self, vm: &mut Vm, dt: f32) -> bool {
        let (now, step) = (Instant::now(), Duration::from_secs_f32(dt));
        let before = self.panels.len();
        self.panels.retain_mut(|p| match p {
            Panel::Bark { pid, img, left, more, off, .. } => {
                *left = left.saturating_sub(step);
                if !left.is_zero() {
                    return true;
                }
                match more.pop_front() {
                    Some((next, t)) => (*img, *left, *off) = (next, t, None),
                    None => vm.finish(*pid, 0),
                }
                !left.is_zero()
            }
            Panel::Ask { pid, waited, .. } => {
                *waited += step;
                let over = *waited >= ASK_TIMEOUT;
                if over {
                    vm.answer_nothing(*pid);
                }
                !over
            }
            Panel::Notice { until, .. } => *until > now,
            _ => true,
        });
        before != self.panels.len()
    }

    /// A click (game-pixel coords) while a question or page is open. Returns true if the UI used it.
    pub fn click(&mut self, x: i32, y: i32, vm: &mut Vm) -> bool {
        if let Some(i) = self.panels.iter().position(|p| matches!(p, Panel::Ask { .. })) {
            if let Panel::Ask { pid, options, rects, .. } = &self.panels[i] {
                if let Some(k) = rects.iter().position(|r| x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3])
                {
                    vm.answer(*pid, options, k);
                    self.panels.remove(i);
                }
            }
            return true; // clicks elsewhere don't leak into the world while asking
        }
        if let Some(i) = self.panels.iter().position(|p| matches!(p, Panel::Read(_))) {
            let Panel::Read(r) = &mut self.panels[i] else { unreachable!() };
            if r.keys_only {
                return true;
            }
            let now = Instant::now();
            let double = r.last_click.is_some_and(|t| now - t < crate::DOUBLE_CLICK);
            if !double && r.on_back((x, y)) {
                r.back();
                r.last_click = Some(now);
            } else if double || !r.next() {
                self.close_reading(i, vm);
            } else {
                r.last_click = Some(now);
            }
            return true; // anywhere: the page has the mouse until it's put away
        }
        false
    }

    fn close_reading(&mut self, i: usize, vm: &mut Vm) {
        if let Panel::Read(r) = self.panels.remove(i) {
            vm.finish(r.pid, 0);
        }
    }

    /// A key while something is open to read: Space (or Right) turns a page on (closing past
    /// the last), Left back; any key puts away a grave or plaque. True if used.
    pub fn read_key(&mut self, key: PageKey, vm: &mut Vm) -> bool {
        let Some(i) = self.panels.iter().position(|p| matches!(p, Panel::Read(_))) else { return false };
        let Panel::Read(r) = &mut self.panels[i] else { return false };
        match key {
            PageKey::Other if !r.sign => return false,
            PageKey::Back if !r.sign => _ = r.back(),
            _ if !r.next() => self.close_reading(i, vm),
            _ => {}
        }
        true
    }

    /// Is a conversation asking for a reply?
    pub fn asking(&self) -> bool {
        self.panels.iter().any(|p| matches!(p, Panel::Ask { .. }))
    }

    /// What was last opened to read goes away only with a key.
    pub fn keys_only(&mut self) {
        if let Some(Panel::Read(r)) = self.panels.iter_mut().rev().find(|p| matches!(p, Panel::Read(_))) {
            r.keys_only = true;
        }
    }

    /// Is something open to read? (It's drawn at the screen's full resolution.)
    pub fn reading(&self) -> bool {
        self.panels.iter().any(|p| matches!(p, Panel::Read(_)))
    }

    /// Draw what's open to read, over everything else.
    pub fn draw_reading(&mut self, canvas: &mut Canvas, art: Option<&mut GumpArt>, ss: u32, (vw, vh): (u32, u32)) {
        for p in &mut self.panels {
            if let Panel::Read(r) = p {
                r.draw(canvas, art, &self.type_font, ss, (vw as i32, vh as i32));
                return;
            }
        }
    }

    /// Put away pages and speech; questions stay (their script waits for an answer). True if any
    /// pages or speech were put away.
    pub fn close(&mut self, vm: &mut Vm) -> bool {
        let mut closed = false;
        self.panels.retain(|p| match p {
            Panel::Bark { pid, .. } | Panel::Read(Readable { pid, .. }) => {
                vm.finish(*pid, 0);
                closed = true;
                false
            }
            Panel::Ask { .. } => true,
            Panel::Notice { .. } => false, // goes too, but doesn't count: Esc still opens the menu
        });
        closed
    }

    /// Clicking skips speech. Returns true if there was any.
    pub fn skip_barks(&mut self, vm: &mut Vm) -> bool {
        let mut used = false;
        self.panels.retain_mut(|p| match p {
            // Onto the next page, if there is one (ScummVM's BarkGump::onMouseDown).
            Panel::Bark { img, left, more, off, .. } if !more.is_empty() => {
                (*img, *left) = more.pop_front().unwrap();
                *off = None;
                used = true;
                true
            }
            Panel::Bark { pid, .. } => {
                vm.finish(*pid, 0);
                used = true;
                false
            }
            _ => true,
        });
        used
    }

    /// Number keys pick conversation options.
    pub fn key(&mut self, n: usize, vm: &mut Vm) -> bool {
        if let Some(i) = self.panels.iter().position(|p| matches!(p, Panel::Ask { .. })) {
            if let Panel::Ask { pid, options, .. } = &self.panels[i] {
                if n < options.len() {
                    vm.answer(*pid, options, n);
                    self.panels.remove(i);
                    return true;
                }
            }
        }
        false
    }

    /// Where the answer `d` on from the highlighted one is (from the first or last, if none is):
    /// the pad's D-pad points at it.
    pub fn answer_step(&self, d: i32) -> Option<[i32; 4]> {
        self.panels.iter().find_map(|p| match p {
            Panel::Ask { rects, hover, .. } if !rects.is_empty() => {
                let n = rects.len() as i32;
                let i = hover.map_or(if d > 0 { 0 } else { n - 1 }, |h| (h as i32 + d).rem_euclid(n));
                Some(rects[i as usize])
            }
            _ => None,
        })
    }

    /// Mouse moved (game-pixel coords). Returns true if a highlight changed.
    pub fn hover(&mut self, x: i32, y: i32) -> bool {
        let mut changed = false;
        for p in &mut self.panels {
            if let Panel::Ask { rects, hover, .. } = p {
                let h = rects.iter().position(|r| x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]);
                changed |= h != *hover;
                *hover = h;
            }
        }
        changed
    }

    /// Draw just the notices (over a menu).
    pub fn draw_notices(&self, canvas: &mut Canvas, ss: u32, vw: u32) {
        for p in &self.panels {
            if let Panel::Notice { img, .. } = p {
                notice(canvas, ss, vw as i32, img);
            }
        }
    }

    /// Draw over the frame. The canvas is `vw*ss × vh*ss`; `rect_of` gives where an item is, in
    /// the same game pixels: speech goes centred on it with its bottom on the rect's top (on the
    /// map a point over the head, the rect reaching down to the feet).
    pub fn draw(
        &mut self,
        canvas: &mut Canvas,
        ss: u32,
        (vw, vh): (u32, u32),
        rect_of: impl Fn(u16) -> Option<[i32; 4]>,
    ) {
        let (vw, vh) = (vw as i32, vh as i32);
        let font = self.speech.as_ref();
        for p in &mut self.panels {
            match p {
                Panel::Bark { img, .. } => img.fit(font, ss),
                Panel::Ask { imgs, classic, .. } => {
                    imgs.iter_mut().chain(classic.iter_mut()).for_each(|(a, b)| (a.fit(font, ss), b.fit(font, ss)).1)
                }
                _ => {}
            }
        }
        // Keep a w×h box inside the view (left/top edge wins if it can't fit).
        let fit = |v: i32, size: i32, room: i32| v.min(room - size - 2).max(2);
        // Speech: pages already placed keep their place by their speaker; new ones find theirs
        // clear of those (BARK.C).
        let mut taken: Vec<[i32; 4]> = self.panels.iter().filter_map(|p| bark_rect(p, &rect_of)).collect();
        for p in &mut self.panels {
            let Panel::Bark { item, img, off: off @ None, .. } = p else { continue };
            let Some(r) = rect_of(*item) else { continue };
            let (w, h) = img.size();
            let at = if r[2] == 0 && r[3] == 0 {
                (r[0] - w / 2, r[1] - h) // (nobody to stand over: the Guardian, low in the middle)
            } else {
                place_bark((r[0] + r[2] / 2, r[1]), r, (w, h), (vw, vh), &taken)
            };
            *off = Some((at.0 - r[0] - r[2] / 2, at.1 - r[1]));
            taken.push([at.0, at.1, w, h]);
        }
        for p in &mut self.panels {
            let spot = bark_rect(p, &rect_of);
            match p {
                Panel::Bark { img, .. } => {
                    let Some([x, y, ..]) = spot else { continue };
                    img.draw(canvas, ss, x, y, false); // speech fonts carry their own outline
                }
                Panel::Ask { classic, rects, hover, .. } if self.style == Style::Classic => {
                    // As ScummVM's AskGump: left to right 4 apart, a new row past 160 wide, rows
                    // a line apart; over the Avatar's head as its speech would be (below its
                    // feet if that's off the top, as DOS does), kept on screen.
                    // (In the readable type: rows its own height apart, as wide as its size runs.)
                    let (skip, most) = match font {
                        Some(f) => {
                            (classic.first().map_or(0, |c| c.0.size().1), speech_width(f.size()) as i32 * 160 / 194)
                        }
                        None => (self.fonts.line_skip(CLASSIC_FONT) as i32, CLASSIC_WIDTH),
                    };
                    let mut at = Vec::with_capacity(classic.len());
                    let (mut px, mut py, mut w) = (0, 0, 0);
                    for (a, _) in classic.iter() {
                        let aw = a.size().0;
                        if px > 0 && px + aw > most {
                            (px, py) = (0, py + skip);
                        }
                        at.push((px, py));
                        w = w.max(px + aw);
                        px += aw + 4;
                    }
                    let h = py + classic.last().map_or(0, |(a, _)| a.size().1);
                    let (x0, y0) = match rect_of(AVATAR) {
                        Some(r) if r[1] - h >= 0 => (r[0] + r[2] / 2 - w / 2, r[1] - h),
                        Some(r) => (r[0] + r[2] / 2 - w / 2, r[1] + r[3] + 4),
                        None => ((vw - w) / 2, low(h, vh)),
                    };
                    let (x0, y0) = (fit(x0, w, vw), fit(y0, h, vh));
                    rects.clear();
                    for (k, ((a, b), (dx, dy))) in classic.iter().zip(at).enumerate() {
                        let (x, y) = (x0 + dx, y0 + dy);
                        if *hover == Some(k) { b } else { a }.draw(canvas, ss, x, y, false);
                        let (aw, ah) = a.size();
                        rects.push([x, y, aw, ah]);
                    }
                }
                Panel::Ask { imgs, rects, hover, .. } => {
                    let h: i32 = imgs.iter().map(|(a, _)| a.size().1 + 1).sum();
                    let w = imgs.iter().map(|(a, _)| a.size().0).max().unwrap_or(0);
                    let (x0, mut y) = ((vw - w) / 2, low(h, vh));
                    text::shade(canvas, ss, [x0 - 6, y - 4, w + 12, h + 8]);
                    rects.clear();
                    for (k, (a, b)) in imgs.iter().enumerate() {
                        if *hover == Some(k) { b } else { a }.draw(canvas, ss, x0, y, true);
                        rects.push([x0, y, w, a.size().1 + 1]);
                        y += a.size().1 + 1;
                    }
                }
                Panel::Notice { img, .. } => notice(canvas, ss, vw, img),
                Panel::Read(_) => {} // draw_reading, on top
            }
        }
    }
}

/// Where a page of speech placed by [`place_bark`] is now: kept beside its speaker as they (or
/// the view) move.
fn bark_rect(p: &Panel, rect_of: &impl Fn(u16) -> Option<[i32; 4]>) -> Option<[i32; 4]> {
    let Panel::Bark { item, img, off: Some(off), .. } = p else { return None };
    let r = rect_of(*item)?;
    let (w, h) = img.size();
    Some([r[0] + r[2] / 2 + off.0, r[1] + off.1, w, h])
}

/// Where a page of speech `size` big goes, as the original places it (BARK.C findPlace): of
/// every spot in the view 8 apart, the one whose middle is nearest `to` (over the speaker),
/// clear of other speech (any spot clear beats one that isn't) and of the speaker (`item`).
fn place_bark(
    to: (i32, i32),
    item: [i32; 4],
    (w, h): (i32, i32),
    (vw, vh): (i32, i32),
    taken: &[[i32; 4]],
) -> (i32, i32) {
    let meets =
        |a: [i32; 4], b: [i32; 4]| a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3];
    let mut best = ((true, i32::MAX), (0, 0));
    for x in (0..(vw - w).max(1)).step_by(8) {
        for y in (0..(vh - h).max(1)).step_by(8) {
            let r = [x, y, w, h];
            let mut score = (to.0 - (x + w / 2)).abs() + (to.1 - (y + h / 2)).abs();
            score += if meets(r, item) { 500 } else { 0 };
            let crowded = taken.iter().any(|&t| meets(r, t));
            best = best.min(((crowded, score + if crowded { 3000 } else { 0 }), (x, y)));
        }
    }
    best.1
}

/// A notice centred at the top of the view.
fn notice(canvas: &mut Canvas, ss: u32, vw: i32, img: &TextImage) {
    let x = (vw - img.w as i32) / 2;
    text::shade(canvas, ss, [x - 6, 6, img.w as i32 + 12, img.h as i32 + 8]);
    text::draw(canvas, ss, img, x, 10);
}

/// Where a reply panel `h` high starts: below the middle (where the Avatar stands), clear of the
/// bottom edge.
fn low(h: i32, vh: i32) -> i32 {
    (vh * 5 / 8).min(vh - h - 12).max(0)
}
