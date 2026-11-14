//! On-screen script UI: speech barks over items, conversation choices, and readable text.
use crate::art::GumpArt;
use crate::pagefont::PageFont;
use std::time::{Duration, Instant};
use u8data::{
    font::{Fonts, TextImage},
    palette::Palette,
};
use u8engine::usecode::{Pid, ReadKind, UiEvent, Vm};
use u8render::{text, Canvas};

/// Speech wraps at this width (game pixels), as in the original.
const BARK_WIDTH: u32 = 194;
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
const CLASSIC_WIDTH: i32 = 200;
const BULLET: [u8; 4] = [210, 30, 24, 255];

/// How the original shows something to read (ScummVM's BookGump, ScrollGump, ReadableGump):
/// U8GUMPS shape, page size and where pages go in it (a book has two side by side). The text
/// is in [`PageFont`] where the original had its font 9.
struct Layout {
    gump: u32,
    page: (u32, u32),
    boxes: &'static [(i32, i32)],
}
const BOOK: Layout = Layout { gump: 6, page: (123, 129), boxes: &[(9, 5), (150, 5)] };
const SCROLL: Layout = Layout { gump: 19, page: (204, 115), boxes: &[(22, 29)] };
const GRAVE_FONT: usize = 11;
const PLAQUE_FONT: usize = 10;

/// Something open to read. Books and scrolls page on a click (or Space) and close on a double
/// click, Esc, or a click past the last page; graves and plaques close on any click or key.
struct Readable {
    pid: Pid,
    gump: u32,
    /// Top-left of each page shown at once, in the gump, and a page's size.
    boxes: Vec<(i32, i32)>,
    size: (i32, i32),
    pages: Pages,
    /// The page in the first box.
    first: usize,
    sign: bool,
    /// Put away by a key only (the Avatar's tombstone: clicks don't, in the DOS game).
    keys_only: bool,
    last_click: Option<Instant>,
}

/// What's on the pages: graves' and plaques' words in the game's own fonts (game pixels), or
/// book and scroll pages laid out for [`PageFont`], drawn at the screen's resolution (kept for
/// the scale they were drawn at).
enum Pages {
    Game(Vec<TextImage>),
    Type { lines: Vec<Vec<String>>, drawn: Option<(u32, Vec<TextImage>)> },
}

impl Pages {
    fn len(&self) -> usize {
        match self {
            Pages::Game(p) => p.len(),
            Pages::Type { lines, .. } => lines.len(),
        }
    }
}

impl Readable {
    fn new(
        (fonts, type_font): (&Fonts, &PageFont),
        pal: &Palette,
        pid: Pid,
        (kind, gump): (ReadKind, u16),
        text: &str,
        art: Option<&GumpArt>,
    ) -> Self {
        let lay = match kind {
            ReadKind::Book => BOOK,
            ReadKind::Scroll => SCROLL,
            ReadKind::Grave | ReadKind::Plaque => {
                // The whole text, centred in the gump less 8 each side (no paging: * breaks lines).
                let font = if kind == ReadKind::Grave { GRAVE_FONT } else { PLAQUE_FONT };
                let (gw, gh) = art.map_or((200, 120), |a| a.size(gump as u32));
                let w = (gw - 16).max(16) as u32;
                let (lines, _) = fonts.typeset(font, text, w, 0, false, true);
                let img = fonts.render_set(font, &lines, pal, w, true);
                let at = ((gw - img.w as i32) / 2, (gh - img.h as i32) / 2);
                let size = (img.w as i32, img.h as i32);
                let pages = Pages::Game(vec![img]);
                let boxes = vec![at];
                return Self {
                    pid,
                    gump: gump as u32,
                    boxes,
                    size,
                    pages,
                    first: 0,
                    sign: true,
                    keys_only: false,
                    last_click: None,
                };
            }
        };
        let pages = Pages::Type { lines: type_font.pages(text, lay.page), drawn: None };
        let size = (lay.page.0 as i32, lay.page.1 as i32);
        let boxes = lay.boxes.to_vec();
        Self { pid, gump: lay.gump, boxes, size, pages, first: 0, sign: false, keys_only: false, last_click: None }
    }

    /// Turn the page; false if there was no more.
    fn next(&mut self) -> bool {
        let n = self.boxes.len();
        if self.sign || self.first + n >= self.pages.len() {
            return false;
        }
        self.first += n;
        true
    }

    fn draw(
        &mut self,
        canvas: &mut Canvas,
        art: Option<&mut GumpArt>,
        type_font: &PageFont,
        ss: u32,
        (vw, vh): (i32, i32),
    ) {
        let (gw, gh) = art.as_ref().map_or((0, 0), |a| a.size(self.gump));
        let (x0, y0) = match art {
            Some(art) if gw > 0 => {
                let at = ((vw - gw) / 2, (vh - gh) / 2);
                art.draw(canvas, self.gump, 0, at.0, at.1);
                at
            }
            _ => {
                // No window art: the text on a dark panel.
                let w = self.boxes.iter().map(|b| b.0).max().unwrap_or(0) + self.size.0;
                let h = self.boxes.iter().map(|b| b.1).max().unwrap_or(0) + self.size.1;
                let at = ((vw - w) / 2, (vh - h) / 2);
                text::shade(canvas, ss, [at.0 - 8, at.1 - 6, w + 16, h + 12]);
                at
            }
        };
        let first = self.first;
        match &mut self.pages {
            Pages::Game(pages) => {
                for (b, p) in self.boxes.iter().zip(pages.iter().skip(first)) {
                    text::draw_plain(canvas, ss, p, x0 + b.0, y0 + b.1);
                }
            }
            Pages::Type { lines, drawn } => {
                if drawn.as_ref().is_none_or(|d| d.0 != ss) {
                    *drawn = Some((ss, lines.iter().map(|l| type_font.render(l, ss)).collect()));
                }
                let s = ss as i32;
                for (b, p) in self.boxes.iter().zip(drawn.as_ref().unwrap().1.iter().skip(first)) {
                    text::draw_blended(canvas, p, (x0 + b.0) * s, (y0 + b.1) * s);
                }
            }
        }
    }
}

/// How conversation choices are laid out (F1 switches).
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
        until: Instant,
    },
    Ask {
        pid: Pid,
        item: u16,
        options: Vec<String>,
        imgs: Vec<Choice>,
        classic: Vec<Choice>,
        rects: Vec<[i32; 4]>,
        hover: Option<usize>,
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
                let text = text.trim().to_string();
                let font = speech_font(item);
                let lines = self.fonts.wrap(font, &text, BARK_WIDTH);
                let img = self.fonts.render_left(font, &lines, pal);
                // Long enough to read: ~70 ms per character, 1.5–8 s; or as long as it's spoken.
                let read = (text.len() as u64 * 70).clamp(1500, 8000);
                let ms = spoken.map_or(read, |s| read.max((s * 1000.0) as u64 + 300));
                self.panels.push(Panel::Bark { pid, item, img, until: Instant::now() + Duration::from_millis(ms) });
            }
            UiEvent::Ask { pid, item, options } => {
                let choices = |fonts: &Fonts, (a, b): (usize, usize), numbered: bool| -> Vec<Choice> {
                    options
                        .iter()
                        .enumerate()
                        .map(|(i, o)| {
                            let t =
                                vec![if numbered { format!("{}. {}", i + 1, o.trim()) } else { o.trim().to_string() }];
                            (fonts.render(a, &t, pal, None), fonts.render(b, &t, pal, None))
                        })
                        .collect()
                };
                let imgs = choices(&self.fonts, (OPTION_FONT, OPTION_HOVER_FONT), true);
                let classic = choices(&self.fonts, (CLASSIC_FONT, CLASSIC_HOVER_FONT), false);
                self.panels.push(Panel::Ask { pid, item, options, imgs, classic, rects: vec![], hover: None });
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

    /// Retire notices whose time is up (while the game is paused). True if any went.
    pub fn expire_notices(&mut self) -> bool {
        let now = Instant::now();
        let before = self.panels.len();
        self.panels.retain(|p| !matches!(p, Panel::Notice { until, .. } if *until <= now));
        before != self.panels.len()
    }

    /// Retire barks whose time is up. Returns true if anything changed.
    pub fn expire(&mut self, vm: &mut Vm) -> bool {
        let now = Instant::now();
        let before = self.panels.len();
        self.panels.retain(|p| match p {
            Panel::Bark { pid, until, .. } if *until <= now => {
                vm.finish(*pid, 0);
                false
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
            if double || !r.next() {
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

    /// A key while something is open to read: Space turns a page (or closes past the last); any
    /// key puts away a grave or plaque. True if it was used.
    pub fn read_key(&mut self, space: bool, vm: &mut Vm) -> bool {
        let Some(i) = self.panels.iter().position(|p| matches!(p, Panel::Read(_))) else { return false };
        let Panel::Read(r) = &mut self.panels[i] else { return false };
        if !(r.sign || space) {
            return false;
        }
        if !r.next() {
            self.close_reading(i, vm);
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
        self.panels.retain(|p| match p {
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

    /// Draw over the frame. The canvas is `vw*ss × vh*ss`; `rect_of` gives where an item is
    /// drawn, in the same game pixels (to put speech over it).
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
                    let y = fit(r[1] - h - 2, h, vh);
                    text::draw_plain(canvas, ss, img, x, y); // speech fonts carry their own outline
                }
                Panel::Ask { item, classic, rects, hover, .. } if self.style == Style::Classic => {
                    // Greedy rows of "■ choice" up to CLASSIC_WIDTH, centred over the asker's head.
                    let (gap, bullet) = (8, 7);
                    let mut rows: Vec<Vec<usize>> = vec![vec![]];
                    let mut used = 0;
                    for (k, (a, _)) in classic.iter().enumerate() {
                        let w = bullet + a.w as i32;
                        if used > 0 && used + gap + w > CLASSIC_WIDTH {
                            rows.push(vec![]);
                            used = 0;
                        }
                        used += if used > 0 { gap } else { 0 } + w;
                        rows.last_mut().unwrap().push(k);
                    }
                    let lh = classic.iter().map(|(a, _)| a.h as i32).max().unwrap_or(8) + 2;
                    let row_w = |r: &[usize]| {
                        r.iter().map(|&k| bullet + classic[k].0.w as i32).sum::<i32>()
                            + gap * (r.len() as i32 - 1).max(0)
                    };
                    let widest = rows.iter().map(|r| row_w(r)).max().unwrap_or(0);
                    let total = lh * rows.len() as i32;
                    let (cx, top) = match rect_of(*item) {
                        Some(r) => (r[0] + r[2] / 2, r[1] - total - 4),
                        None => (vw / 2, low(total, vh)),
                    };
                    let x0 = fit(cx - widest / 2, widest, vw);
                    let mut y = fit(top, total, vh);
                    rects.clear();
                    rects.resize(classic.len(), [0; 4]);
                    for r in &rows {
                        let mut x = x0 + (widest - row_w(r)) / 2;
                        for &k in r {
                            let (a, b) = &classic[k];
                            let by = y + a.h as i32 / 2 - 2;
                            text::block(canvas, ss, [x - 1, by - 1, 5, 5], [0, 0, 0, 255]);
                            text::block(canvas, ss, [x, by, 3, 3], BULLET);
                            text::draw(canvas, ss, if *hover == Some(k) { b } else { a }, x + bullet, y);
                            rects[k] = [x - 1, y - 1, bullet + a.w as i32 + 2, a.h as i32 + 2];
                            x += bullet + a.w as i32 + gap;
                        }
                        y += lh;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use u8data::flex::Flex;

    struct Data {
        fonts: Fonts,
        pal: Palette,
        art: GumpArt,
        uc: u8data::usecode::Usecode,
    }
    struct Shots {
        pal: Palette,
        art: GumpArt,
    }

    fn data() -> Option<Data> {
        let root = PathBuf::from(std::env::var("U8_DATA").ok()?);
        let read = |p: &str| std::fs::read(u8data::find_file(&root, p).unwrap()).unwrap();
        let pal = Palette::parse(&read("STATIC/U8PAL.PAL")).unwrap();
        let fonts = Fonts::parse(&Flex::parse(read("STATIC/U8FONTS.FLX")).unwrap());
        let art = GumpArt::load(&root, &pal).unwrap();
        Some(Data { fonts, pal, art, uc: u8data::load_usecode(&root).unwrap().0 })
    }

    /// The usecode's string constant starting with `start`.
    fn game_text(uc: &u8data::usecode::Usecode, start: &str) -> String {
        (0..uc.class_count() as u32)
            .find_map(|k| {
                let d = uc.class(k)?;
                let at = d.windows(start.len()).position(|w| w == start.as_bytes())?;
                let n = d[at..].iter().position(|&b| b == 0)?;
                Some(String::from_utf8_lossy(&d[at..at + n]).into_owned())
            })
            .unwrap()
    }

    /// Speech colour goes with who's speaking (ScummVM's dialogFontForActor): the Avatar and
    /// the Guardian red, NPCs white/orange/green by number (Corinth, 15, white; a castle guard,
    /// 50, green — as Ryan's DOS shots show them), anything else yellow.
    #[test]
    fn speech_colour_goes_with_the_speaker() {
        assert_eq!((speech_font(1), speech_font(GUARDIAN)), (6, 6));
        assert_eq!((speech_font(15), speech_font(2), speech_font(50)), (0, 7, 7));
        assert_eq!((speech_font(256), speech_font(257), speech_font(4000)), (5, 8, 8));
    }

    /// Draw over a 320×200 view at 3 screen pixels a game pixel; with `U8_SHOTS` set, written
    /// there as `name`.ppm.
    fn shot(ui: &mut ScriptUi, art: &mut GumpArt, name: &str) {
        let mut px = vec![0u8; 960 * 600 * 4];
        art.set_style(3, false);
        ui.draw_reading(&mut Canvas::new(&mut px, 960, 600), Some(art), 3, (320, 200));
        if let Ok(out) = std::env::var("U8_SHOTS") {
            let mut ppm = b"P6 960 600 255\n".to_vec();
            px.chunks(4).for_each(|p| ppm.extend(&p[..3]));
            std::fs::write(format!("{out}/{name}.ppm"), ppm).unwrap();
        }
    }

    fn reading(ui: &ScriptUi) -> &Readable {
        ui.panels.iter().find_map(|p| if let Panel::Read(r) = p { Some(r) } else { None }).unwrap()
    }

    /// A book opens on the book gump, two pages at a time, `~` breaking lines; a click turns
    /// the pages, and a click past the last (or a double click) puts it away.
    #[test]
    #[ignore]
    fn books_read_like_books() {
        let Some(Data { fonts, pal, art, uc }) = data() else { return };
        let (mut d, mut vm, mut ui) = (Shots { pal, art }, Vm::new(&uc, None), ScriptUi::new(fonts));
        let text = game_text(&uc, "GUARD LOG: ~~Bloodwatch: Guard changed.");
        let read = |kind, gump, text: &str| UiEvent::Read { pid: 1, kind, item: 0, gump, text: text.into() };
        ui.push(read(ReadKind::Book, 0, &text), &d.pal, None, Some(&d.art));
        let r = reading(&ui);
        assert_eq!((r.gump, r.boxes.len()), (6, 2));
        let Pages::Type { lines, .. } = &r.pages else { panic!("in the page font") };
        let lines = &lines[0];
        assert!(lines[..2] == ["GUARD LOG:", ""] && lines[2].starts_with("Bloodwatch: "), "{lines:?}");
        assert!(lines.iter().all(|l| !l.contains('~')));
        // Short enough for two pages: the first click is past the end.
        assert!(r.pages.len() <= 2, "one spread");
        shot(&mut ui, &mut d.art, "book");
        assert!(ui.blocking());
        ui.click(0, 0, &mut vm);
        assert!(!ui.blocking(), "closed");

        // A long one pages; a double click closes it.
        let long = game_text(&uc, "The Spell of Resurrection~~");
        ui.push(read(ReadKind::Book, 0, &long), &d.pal, None, Some(&d.art));
        assert!(reading(&ui).pages.len() > 4, "{} pages", reading(&ui).pages.len());
        shot(&mut ui, &mut d.art, "book_long");
        assert!(ui.read_key(true, &mut vm), "Space turns the page");
        assert_eq!(reading(&ui).first, 2);
        shot(&mut ui, &mut d.art, "book_long2");
        assert!(!ui.read_key(false, &mut vm), "other keys don't");
        ui.click(0, 0, &mut vm);
        assert_eq!(reading(&ui).first, 4);
        ui.click(0, 0, &mut vm);
        assert!(!ui.blocking(), "double click: closed");

        // A scroll: one page in the scroll gump.
        ui.push(read(ReadKind::Scroll, 0, &long), &d.pal, None, Some(&d.art));
        assert_eq!((reading(&ui).gump, reading(&ui).boxes.len()), (19, 1));
        shot(&mut ui, &mut d.art, "scroll");
        ui.close(&mut vm);
    }

    /// Graves and plaques: the script's gump, `*` breaking centred lines; any key puts them away.
    #[test]
    #[ignore]
    fn graves_and_plaques_centre_their_words() {
        let Some(Data { fonts, pal, art, uc }) = data() else { return };
        let (mut d, mut vm, mut ui) = (Shots { pal, art }, Vm::new(&uc, None), ScriptUi::new(fonts));
        let ev = UiEvent::Read {
            pid: 1,
            kind: ReadKind::Plaque,
            item: 0,
            gump: 16,
            text: "THE MONASTERY*OF THE*THEURGISTS ".into(),
        };
        ui.push(ev, &d.pal, None, Some(&d.art));
        let r = reading(&ui);
        assert!(r.sign && r.gump == 16 && r.pages.len() == 1);
        let (gw, gh) = d.art.size(16);
        let Pages::Game(pages) = &r.pages else { panic!("in the plaque's own font") };
        let (at, img) = (r.boxes[0], &pages[0]);
        assert_eq!((at.0, img.w as i32), (8, gw - 16), "centred, 8 in from each side");
        assert!(at.1 >= 0 && at.1 + img.h as i32 <= gh);
        assert_eq!(img.h, 2 * 18 + 14, "three lines, 18 apart");
        shot(&mut ui, &mut d.art, "plaque");
        assert!(ui.read_key(false, &mut vm));
        assert!(!ui.blocking());
        let ev = UiEvent::Read {
            pid: 1,
            kind: ReadKind::Grave,
            item: 0,
            gump: 0x1b,
            text: "SIVLE*THE MINSTREL*WANDERS NO MORE".into(),
        };
        ui.push(ev, &d.pal, None, Some(&d.art));
        shot(&mut ui, &mut d.art, "grave");
        ui.click(5, 5, &mut vm);
        assert!(!ui.blocking(), "a click puts it away");
        // The Avatar's own tombstone: as the DOS game, clicks don't put it away; a key does.
        let ev = UiEvent::Read {
            pid: Pid::MAX,
            kind: ReadKind::Grave,
            item: 0,
            gump: 27,
            text: "HERE LIES*THE AVATAR*REST IN PEACE".into(),
        };
        ui.push(ev, &d.pal, None, Some(&d.art));
        ui.keys_only();
        ui.click(5, 5, &mut vm);
        ui.click(5, 5, &mut vm);
        assert!(ui.reading(), "clicks, even double, leave it");
        assert!(ui.read_key(false, &mut vm) && !ui.reading(), "a key puts it away");
    }
}
