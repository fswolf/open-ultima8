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
type Choice = (TextImage, TextImage);

enum Panel {
    Bark {
        pid: Pid,
        item: u16,
        img: TextImage,
        /// Game time this page stays up for.
        left: Duration,
        /// The pages still to come, each with its time (long speech, as the original pages it).
        more: VecDeque<(TextImage, Duration)>,
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
    panels: Vec<Panel>,
    pub style: Style,
}

impl ScriptUi {
    pub fn new(fonts: Fonts) -> Self {
        Self { fonts, type_font: PageFont::new(), panels: vec![], style: Style::Modern }
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
                    .unwrap_or_else(|| (self.fonts.render_set(font, &[], pal, 0, false), Duration::ZERO));
                self.panels.push(Panel::Bark { pid, item, img, left, more: pages });
            }
            UiEvent::Ask { pid, options, .. } => {
                let choices = |fonts: &Fonts, (a, b): (usize, usize)| -> Vec<Choice> {
                    options
                        .iter()
                        .enumerate()
                        .map(|(i, o)| {
                            let t = vec![format!("{}. {}", i + 1, o.trim())];
                            (fonts.render(a, &t, pal, None), fonts.render(b, &t, pal, None))
                        })
                        .collect()
                };
                let imgs = choices(&self.fonts, (OPTION_FONT, OPTION_HOVER_FONT));
                // The original's: "@ " (its bullet) and the answer, in the Avatar's speech font.
                let classic = options
                    .iter()
                    .map(|o| {
                        let t = [format!("@ {}", o.trim())];
                        let f = &self.fonts;
                        (
                            f.render_set(CLASSIC_FONT, &t, pal, 0, false),
                            f.render_set(CLASSIC_HOVER_FONT, &t, pal, 0, false),
                        )
                    })
                    .collect();
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
    fn bark_pages(&self, font: usize, text: &str, pal: &Palette, spoken: Option<f32>) -> Vec<(TextImage, Duration)> {
        let mut pages = vec![];
        let mut rest = text;
        while !rest.is_empty() || pages.is_empty() {
            let (lines, used) = self.fonts.typeset(font, rest, BARK_WIDTH, BARK_HEIGHT, false, false);
            // (At least a letter, so it always moves on; never inside a multi-byte one.)
            let used = (used.max(1)..=rest.len()).find(|&i| rest.is_char_boundary(i)).unwrap_or(rest.len());
            let n = rest[..used].trim().len() as u64;
            let mut ms = 2900 + 16 * n;
            if self.style == Style::Modern {
                ms = ms.max((n * 70).min(8000));
            }
            if let Some(s) = spoken {
                ms = ms.max((s * 1000.0) as u64 * n / text.len().max(1) as u64);
            }
            pages.push((self.fonts.render_set(font, &lines, pal, 0, false), Duration::from_millis(ms)));
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
            Panel::Bark { pid, img, left, more, .. } => {
                *left = left.saturating_sub(step);
                if !left.is_zero() {
                    return true;
                }
                match more.pop_front() {
                    Some((next, t)) => (*img, *left) = (next, t),
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
            Panel::Bark { img, left, more, .. } if !more.is_empty() => {
                (*img, *left) = more.pop_front().unwrap();
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
        // Keep a w×h box inside the view (left/top edge wins if it can't fit).
        let fit = |v: i32, size: i32, room: i32| v.min(room - size - 2).max(2);
        for p in &mut self.panels {
            match p {
                Panel::Bark { item, img, .. } => {
                    let Some(r) = rect_of(*item) else { continue };
                    let (w, h) = (img.w as i32, img.h as i32);
                    let x = fit(r[0] + r[2] / 2 - w / 2, w, vw);
                    let y = fit(r[1] - h, h, vh);
                    text::draw_plain(canvas, ss, img, x, y); // speech fonts carry their own outline
                }
                Panel::Ask { classic, rects, hover, .. } if self.style == Style::Classic => {
                    // As ScummVM's AskGump: left to right 4 apart, a new row past 160 wide, rows
                    // a line apart; over the Avatar's head as its speech would be (below its
                    // feet if that's off the top, as DOS does), kept on screen.
                    let skip = self.fonts.line_skip(CLASSIC_FONT) as i32;
                    let mut at = Vec::with_capacity(classic.len());
                    let (mut px, mut py, mut w) = (0, 0, 0);
                    for (a, _) in classic.iter() {
                        if px > 0 && px + a.w as i32 > CLASSIC_WIDTH {
                            (px, py) = (0, py + skip);
                        }
                        at.push((px, py));
                        w = w.max(px + a.w as i32);
                        px += a.w as i32 + 4;
                    }
                    let h = py + classic.last().map_or(0, |(a, _)| a.h as i32);
                    let (x0, y0) = match rect_of(AVATAR) {
                        Some(r) if r[1] - h >= 0 => (r[0] + r[2] / 2 - w / 2, r[1] - h),
                        Some(r) => (r[0] + r[2] / 2 - w / 2, r[1] + r[3] + 4),
                        None => ((vw - w) / 2, low(h, vh)),
                    };
                    let (x0, y0) = (fit(x0, w, vw), fit(y0, h, vh));
                    rects.clear();
                    for (k, ((a, b), (dx, dy))) in classic.iter().zip(at).enumerate() {
                        let (x, y) = (x0 + dx, y0 + dy);
                        text::draw_plain(canvas, ss, if *hover == Some(k) { b } else { a }, x, y);
                        rects.push([x, y, a.w as i32, a.h as i32]);
                    }
                }
                Panel::Ask { imgs, rects, hover, .. } => {
                    let h: i32 = imgs.iter().map(|(a, _)| a.h as i32 + 1).sum();
                    let w = imgs.iter().map(|(a, _)| a.w as i32).max().unwrap_or(0);
                    let (x0, mut y) = ((vw - w) / 2, low(h, vh));
                    text::shade(canvas, ss, [x0 - 6, y - 4, w + 12, h + 8]);
                    rects.clear();
                    for (k, (a, b)) in imgs.iter().enumerate() {
                        text::draw(canvas, ss, if *hover == Some(k) { b } else { a }, x0, y);
                        rects.push([x0, y, w, a.h as i32 + 1]);
                        y += a.h as i32 + 1;
                    }
                }
                Panel::Notice { img, .. } => notice(canvas, ss, vw, img),
                Panel::Read(_) => {} // draw_reading, on top
            }
        }
    }
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
