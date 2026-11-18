//! The game's book menus, drawn with the original's art from U8GUMPS.FLX: the main menu (Esc),
//! the diary for saving and loading (J), the options page, and the quit question (Alt-X).
use crate::art::GumpArt;
use crate::credits::Roll;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use u8data::{
    font::{Fonts, TextImage},
    palette::Palette,
    save::{self, QUICK_SLOT},
};
use u8render::{text, Canvas};
use winit::keyboard::KeyCode;

/// U8GUMPS.FLX entries.
const BOOK: u32 = 35;
const LOGO: u32 = 32;
/// Menu entries: frame 2k is entry k, 2k + 1 the same lit (English; French and German follow).
const ENTRIES: u32 = 37;
/// Pages inside the book, relative to its top-left: [x, y, w, h].
const LEFT: [i32; 4] = [36, 8, 110, 132];
const RIGHT: [i32; 4] = [158, 8, 110, 132];
/// The original draws the book this much higher, and further right, than centred (measured in
/// DOS at 320×200).
const BOOK_LIFT: i32 = 11;
const BOOK_RIGHT: i32 = 4;
const TEXT_FONT: usize = 9;
const LIT_FONT: usize = 6;
/// Diary slots: the eleven the original's diary has descriptions for, and the quick save.
const SLOTS: [u32; 12] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, QUICK_SLOT];
const SLOT_H: i32 = 19;
/// An options line's text is about this tall (the last line has to fit above the page's foot).
const OPTION_H: i32 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diary {
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Main,
    Diary(Diary),
    Options,
    /// A new game asks for the Avatar's name ("Give thy name:").
    Name,
    /// "Quit the game?" with Yes and No (U8GUMPS 17, 18, 47, 50).
    Quit,
}

/// The quit question's art: window, question, Yes and No (frame 0 up, 1 pressed), and where the
/// buttons sit (in from each side, down from the top).
const QUIT_WINDOW: u32 = 17;
const QUIT_ASK: u32 = 18;
const QUIT_YES: u32 = 47;
const QUIT_NO: u32 = 50;
const QUIT_BUTTONS: (i32, i32) = (18, 41);

/// The options page's lines; the viewer supplies their current values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    Upscaler,
    Scale,
    TextStyle,
    Music,
    Effects,
    Volume,
    /// Background sounds' level (fires, drips, fountains).
    Ambience,
    Cutaway,
    /// The view easing up and down (ours).
    SmoothCamera,
    Fullscreen,
    Buttons,
    Cheats,
    Discord,
}

/// What the player chose.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Close,
    Quit,
    Save {
        slot: u32,
        desc: String,
    },
    Load(u32),
    Change(Setting),
    Name(String),
    /// Roll the credits or the quotes.
    Roll(Roll),
    /// Show the intro or the ending.
    Film(crate::films::Which),
}

#[derive(Clone, Copy, PartialEq)]
enum Hit {
    Entry(u32),
    Tab(Diary),
    Slot(u32),
    Option(Setting),
    Yes,
    No,
}

struct SlotInfo {
    slot: u32,
    desc: String,
    when: String,
    exists: bool,
}

pub struct Menu {
    fonts: Fonts,
    pal: Palette,
    pub page: Option<Page>,
    hits: Vec<([i32; 4], Hit)>,
    hover: Option<Hit>,
    slots: Vec<SlotInfo>,
    /// Typing a description for this slot (Write Diary).
    edit: Option<(u32, String)>,
    pub options: Vec<(Setting, String)>,
    root: PathBuf,
    /// The quit question came from the main menu (No goes back there, not out).
    quit_from_menu: bool,
    /// The Avatar is dead: the main menu can't be put away, only a game loaded or begun.
    pub stay: bool,
    /// The game's been finished: the main menu offers the quotes and the ending too (the
    /// original shows entries 7 and 8 only then).
    pub finished: bool,
    /// The Avatar's name, under the logo on the main page.
    pub avatar: String,
}

impl Menu {
    pub fn load(root: &Path, pal: &Palette) -> Option<Self> {
        let read = |p: &str| std::fs::read(u8data::find_file(root, p).ok()?).ok();
        let fonts = Fonts::parse(&u8data::flex::Flex::parse(read("STATIC/U8FONTS.FLX")?).ok()?);
        Some(Self {
            fonts,
            pal: pal.clone(),
            page: None,
            hits: vec![],
            hover: None,
            slots: vec![],
            edit: None,
            options: vec![],
            root: root.to_path_buf(),
            quit_from_menu: false,
            stay: false,
            finished: false,
            avatar: String::new(),
        })
    }

    pub fn is_open(&self) -> bool {
        self.page.is_some()
    }

    pub fn open(&mut self, page: Page) {
        if let Page::Diary(_) = page {
            self.read_slots();
        }
        if page == Page::Quit {
            self.quit_from_menu = self.page == Some(Page::Main);
        }
        self.page = Some(page);
        self.edit = (page == Page::Name).then(|| (0, String::new()));
        self.hover = None;
    }

    pub fn close(&mut self) {
        self.page = None;
        self.edit = None;
    }

    /// Back one level (Esc, right click): typing → diary → main menu → closed.
    pub fn back(&mut self) {
        match self.page {
            Some(Page::Name) => {} // a name is needed to begin
            _ if self.edit.is_some() => self.edit = None,
            Some(Page::Quit) if !self.quit_from_menu => self.close(),
            Some(Page::Main) if self.stay => {}
            Some(Page::Main) | None => self.close(),
            Some(_) => self.page = Some(Page::Main),
        }
    }

    fn read_slots(&mut self) {
        let listed: HashMap<u32, String> = save::list(&self.root).into_iter().collect();
        self.slots = SLOTS
            .iter()
            .map(|&slot| {
                let exists = listed.contains_key(&slot);
                let ticks = exists.then(|| save::Save::load(&self.root, slot).ok()?.clock()).flatten();
                let when = ticks.map_or(String::new(), |t| {
                    let m = t / 450; // 450 ticks a game minute
                    format!("day {}, {:02}:{:02}", m / 1440 + 1, m / 60 % 24, m % 60)
                });
                let desc = match (slot, listed.get(&slot)) {
                    (QUICK_SLOT, _) => "Quick save (F7)".to_string(),
                    (_, Some(d)) if !d.is_empty() => d.clone(),
                    (_, Some(_)) => "(no name)".to_string(),
                    _ => String::new(),
                };
                SlotInfo { slot, desc, when, exists }
            })
            .collect();
    }

    fn hit(&self, x: i32, y: i32) -> Option<Hit> {
        self.hits.iter().find(|(r, _)| x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]).map(|h| h.1)
    }

    /// Mouse moved (game pixels in the view). True if the highlight changed.
    pub fn hover(&mut self, x: i32, y: i32) -> bool {
        let h = self.hit(x, y);
        std::mem::replace(&mut self.hover, h) != h
    }

    pub fn click(&mut self, x: i32, y: i32) -> Option<Action> {
        let page = self.page?;
        match (self.hit(x, y)?, page) {
            (Hit::Entry(k), _) => self.entry(k),
            (Hit::Tab(d), _) => {
                self.open(Page::Diary(d));
                None
            }
            (Hit::Slot(slot), Page::Diary(Diary::Read)) => {
                self.slots.iter().any(|s| s.slot == slot && s.exists).then_some(Action::Load(slot))
            }
            (Hit::Slot(slot), Page::Diary(Diary::Write)) => {
                if slot == QUICK_SLOT {
                    return Some(Action::Save { slot, desc: String::new() });
                }
                let old = self.slots.iter().find(|s| s.slot == slot && s.exists).map(|s| s.desc.clone());
                self.edit = Some((slot, old.filter(|d| d != "(no name)").unwrap_or_default()));
                None
            }
            (Hit::Option(s), _) => Some(Action::Change(s)),
            (Hit::Yes, _) => Some(Action::Quit),
            (Hit::No, _) => {
                self.back();
                (!self.is_open()).then_some(Action::Close)
            }
            _ => None,
        }
    }

    /// A main menu entry (0-based, as numbered in the book from 1).
    fn entry(&mut self, k: u32) -> Option<Action> {
        match k {
            1 => self.open(Page::Diary(Diary::Read)),
            2 => self.open(Page::Diary(Diary::Write)),
            3 => self.open(Page::Options),
            5 => self.open(Page::Quit),
            0 => return Some(Action::Film(crate::films::Which::Intro)),
            4 => return Some(Action::Roll(Roll::Credits)),
            6 => return Some(Action::Roll(Roll::Quotes)),
            _ => return Some(Action::Film(crate::films::Which::Endgame)),
        }
        None
    }

    /// How many main menu entries show: six, eight once the game's been finished.
    fn entries(&self) -> u32 {
        if self.finished {
            8
        } else {
            6
        }
    }

    /// A key while the menu is open (`text`: what it types).
    pub fn key(&mut self, k: KeyCode, text: Option<&str>) -> Option<Action> {
        let naming = self.page == Some(Page::Name);
        let (font, room) = if naming { (LIT_FONT, RIGHT[2] - 8) } else { (TEXT_FONT, LEFT[2] - 14) };
        if let Some((slot, desc)) = &mut self.edit {
            match k {
                KeyCode::Enter | KeyCode::NumpadEnter if naming => {
                    let name = desc.trim().to_string();
                    if !name.is_empty() {
                        self.close();
                        return Some(Action::Name(name));
                    }
                }
                KeyCode::Enter | KeyCode::NumpadEnter => {
                    let (slot, desc) = (*slot, desc.trim().to_string());
                    self.edit = None;
                    return Some(Action::Save { slot, desc });
                }
                KeyCode::Escape if naming => {}
                KeyCode::Escape => self.edit = None,
                KeyCode::Backspace => {
                    desc.pop();
                }
                _ => {
                    let typed = text.unwrap_or("").chars().filter(|c| c.is_ascii() && !c.is_ascii_control());
                    for c in typed {
                        if self.fonts.width(font, &format!("{desc}{c}-")) < room as u32 {
                            desc.push(c);
                        }
                    }
                }
            }
            return None;
        }
        match (k, self.page) {
            (KeyCode::KeyY | KeyCode::Enter, Some(Page::Quit)) => Some(Action::Quit),
            (KeyCode::KeyN, Some(Page::Quit)) | (KeyCode::Escape, _) => {
                self.back();
                (!self.is_open()).then_some(Action::Close)
            }
            (_, Some(Page::Main)) => {
                use KeyCode::*;
                let k =
                    [Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8].iter().position(|&d| d == k)?;
                (k < self.entries() as usize).then(|| self.entry(k as u32)).flatten()
            }
            _ => None,
        }
    }

    /// The game's fonts and palette (the credits draw with them).
    pub fn fonts(&self) -> (&Fonts, &Palette) {
        (&self.fonts, &self.pal)
    }

    pub fn typing(&self) -> bool {
        self.edit.is_some()
    }

    fn label(&self, font: usize, s: &str) -> TextImage {
        self.fonts.render(font, &[s.to_string()], &self.pal, None)
    }

    /// Draw over the frame: the book centred in a vw×vh view, with `art` at the canvas's scale.
    pub fn draw(&mut self, canvas: &mut Canvas, art: &mut GumpArt, (vw, vh): (u32, u32)) {
        let Some(page) = self.page else { return };
        let ss = art.scale();
        if page == Page::Quit {
            // Over the book when it came from there, as the original draws it.
            if self.quit_from_menu {
                self.page = Some(Page::Main);
                self.draw(canvas, art, (vw, vh));
                self.page = Some(Page::Quit);
            } else {
                text::shade(canvas, ss, [0, 0, vw as i32, vh as i32]);
            }
            self.draw_quit(canvas, art, (vw, vh));
            return;
        }
        text::shade(canvas, ss, [0, 0, vw as i32, vh as i32]);
        let (bx, by) = ((vw as i32 - 307) / 2 + BOOK_RIGHT, ((vh as i32 - 158) / 2 - BOOK_LIFT).max(0));
        art.draw(canvas, BOOK, 0, bx, by);
        let (l, r) = ([bx + LEFT[0], by + LEFT[1]], [bx + RIGHT[0], by + RIGHT[1]]);
        let mut hits = vec![];
        let hover = self.hover;
        let lit = |h: Hit| hover == Some(h);
        match page {
            Page::Main => {
                // As the original's (ScummVM's MenuGump): the logo at the top of the left page with
                // the Avatar's name under it, the entries 14 apart from 18 down the right one.
                let logo = (l[0] + (LEFT[2] - 99) / 2, by + 10);
                art.draw(canvas, LOGO, 0, logo.0, logo.1);
                if !self.avatar.is_empty() {
                    let name = self.label(LIT_FONT, &self.avatar.clone());
                    text::draw_plain(canvas, ss, &name, logo.0 + (99 - name.w as i32) / 2, by + 117);
                }
                for k in 0..self.entries() {
                    let h = Hit::Entry(k);
                    let on = lit(h);
                    let rect = art.draw(canvas, ENTRIES, 2 * k + on as u32, bx + 307 / 2 + 13, by + 18 + k as i32 * 14);
                    hits.push((rect, h));
                }
            }
            Page::Diary(mode) => {
                for (d, at, k) in [(Diary::Read, l, 1), (Diary::Write, r, 2)] {
                    let h = Hit::Tab(d);
                    let on = mode == d || lit(h);
                    let rect = art.draw(canvas, ENTRIES, 2 * k + on as u32, at[0] + 4, at[1]);
                    hits.push((rect, h));
                }
                let edit = self.edit.clone();
                for (i, s) in self.slots.iter().enumerate() {
                    let at = if i < 6 { l } else { r };
                    let (x, y) = (at[0] + 4, at[1] + 17 + (i % 6) as i32 * SLOT_H);
                    let h = Hit::Slot(s.slot);
                    let number = if s.slot == QUICK_SLOT { String::new() } else { format!("{}.", s.slot) };
                    let (desc, font) = match &edit {
                        Some((slot, d)) if *slot == s.slot => (format!("{d}-"), LIT_FONT),
                        _ if lit(h) && (mode == Diary::Write || s.exists) => (s.desc.clone(), LIT_FONT),
                        _ => (s.desc.clone(), TEXT_FONT),
                    };
                    if !number.is_empty() {
                        let a = self.label(font, &number);
                        text::draw_plain(canvas, ss, &a, x, y);
                    }
                    let b = self.label(font, if desc.is_empty() { "-" } else { &desc });
                    text::draw_plain(canvas, ss, &b, x + 14, y);
                    if !s.when.is_empty() {
                        let c = self.label(4, &s.when);
                        text::draw_plain(canvas, ss, &c, x + 14, y + b.h as i32 - 1);
                    }
                    hits.push(([x, y, LEFT[2] - 4, SLOT_H], h));
                }
            }
            Page::Name => {
                art.draw(canvas, LOGO, 0, l[0] + (LEFT[2] - 99) / 2, l[1] + (LEFT[3] - 94) / 2);
                let ask = self.label(LIT_FONT, "Give thy name:");
                text::draw_plain(canvas, ss, &ask, r[0] + 4, r[1] + 6);
                let name = self.edit.as_ref().map_or(String::new(), |e| format!("{}-", e.1));
                let typed = self.label(LIT_FONT, &name);
                text::draw_plain(canvas, ss, &typed, r[0] + 4, r[1] + 6 + ask.h as i32 + 2);
            }
            Page::Options => {
                art.draw(canvas, ENTRIES, 7, l[0] + 4, l[1]);
                art.draw(canvas, LOGO, 0, l[0] + (LEFT[2] - 99) / 2, l[1] + 24);
                // Lines 14 apart, closer if that's what it takes to stay on the page.
                let step = ((RIGHT[3] - OPTION_H) / self.options.len().max(1) as i32).min(14);
                for (i, (s, v)) in self.options.clone().iter().enumerate() {
                    let h = Hit::Option(*s);
                    let img = self.label(if lit(h) { LIT_FONT } else { TEXT_FONT }, v);
                    let (x, y) = (r[0] + 4, r[1] + 4 + i as i32 * step);
                    text::draw_plain(canvas, ss, &img, x, y);
                    hits.push(([x, y, RIGHT[2] - 4, step], h));
                }
            }
            Page::Quit => {} // drawn on its own (above)
        }
        self.hits = hits;
    }

    /// "Quit the game?": the window centred, the question at its top, Yes and No below.
    fn draw_quit(&mut self, canvas: &mut Canvas, art: &mut GumpArt, (vw, vh): (u32, u32)) {
        let (w, h) = art.size(QUIT_WINDOW);
        let (x, y) = ((vw as i32 - w) / 2, (vh as i32 - h) / 2);
        art.draw(canvas, QUIT_WINDOW, 0, x, y);
        let ask = art.size(QUIT_ASK);
        art.draw(canvas, QUIT_ASK, 0, x + (w - ask.0) / 2, y + 5);
        let no_w = art.size(QUIT_NO).0;
        let (lit_yes, lit_no) = (self.hover == Some(Hit::Yes), self.hover == Some(Hit::No));
        let yes = art.draw(canvas, QUIT_YES, lit_yes as u32, x + QUIT_BUTTONS.0, y + QUIT_BUTTONS.1);
        let no = art.draw(canvas, QUIT_NO, lit_no as u32, x + w - QUIT_BUTTONS.0 - no_w, y + QUIT_BUTTONS.1);
        self.hits = vec![(yes, Hit::Yes), (no, Hit::No)];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The main page as the original's: six entries 14 apart (eight once the game's finished),
    /// the book 11 higher than centred, the Avatar's name under the logo; quitting from it asks
    /// over the book.
    #[test]
    #[ignore]
    fn the_main_menu_is_the_originals() {
        let Ok(dir) = std::env::var("U8_DATA") else { return };
        let root = PathBuf::from(dir);
        let pal =
            Palette::parse(&std::fs::read(u8data::find_file(&root, "STATIC/U8PAL.PAL").unwrap()).unwrap()).unwrap();
        let (mut m, mut art) = (Menu::load(&root, &pal).unwrap(), GumpArt::load(&root, &pal).unwrap());
        let mut px = vec![0u8; 320 * 200 * 4];
        m.open(Page::Main);
        m.draw(&mut Canvas::new(&mut px, 320, 200), &mut art, (320, 200));
        let tops: Vec<i32> = m.hits.iter().map(|h| h.0[1]).collect();
        assert_eq!(tops.len(), 6);
        assert!(tops.windows(2).all(|w| w[1] - w[0] == 14), "{tops:?}");
        assert_eq!(m.key(KeyCode::Digit7, None), None, "no 7 before the end");
        m.finished = true;
        m.draw(&mut Canvas::new(&mut px, 320, 200), &mut art, (320, 200));
        assert_eq!(m.hits.len(), 8);
        // The name: red letters under the logo on the left page.
        m.avatar = "Ryan".into();
        px.fill(0);
        m.draw(&mut Canvas::new(&mut px, 320, 200), &mut art, (320, 200));
        let by = (200 - 158) / 2 - BOOK_LIFT;
        let red = (by + 117..by + 129)
            .flat_map(|y| (60..115).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let p = &px[((y * 320 + x) * 4) as usize..][..3];
                p[0] > 150 && p[1] < 80 && p[2] < 80
            })
            .count();
        assert!(red > 10, "the name is drawn ({red} red pixels)");
        // Quit from the main page: the question over the book.
        m.open(Page::Quit);
        px.fill(0);
        m.draw(&mut Canvas::new(&mut px, 320, 200), &mut art, (320, 200));
        assert_eq!(m.hits.len(), 2, "only Yes and No answer");
        let book = &px[(((by + 5) * 320 + 160) * 4) as usize..][..3];
        assert!(book.iter().any(|&c| c > 100), "the book is still there behind it: {book:?}");
    }

    /// Every option stays on the page, however many there are. With `U8_SHOTS` set, the page is
    /// written there as PPM.
    #[test]
    #[ignore]
    fn options_stay_on_the_page() {
        let Ok(dir) = std::env::var("U8_DATA") else { return };
        let root = PathBuf::from(dir);
        let pal =
            Palette::parse(&std::fs::read(u8data::find_file(&root, "STATIC/U8PAL.PAL").unwrap()).unwrap()).unwrap();
        let (mut m, mut art) = (Menu::load(&root, &pal).unwrap(), GumpArt::load(&root, &pal).unwrap());
        m.open(Page::Options);
        m.options = (0..12).map(|k| (Setting::Music, format!("Music volume: {k} of 10"))).collect();
        // (How far down the letters reach, descenders included.)
        let ink = |img: &TextImage| {
            (0..img.h).rev().find(|&y| (0..img.w).any(|x| img.rgba[((y * img.w + x) * 4 + 3) as usize] > 0))
        };
        let deepest = m
            .options
            .iter()
            .flat_map(|(_, t)| [m.label(LIT_FONT, t), m.label(TEXT_FONT, t)])
            .filter_map(|i| ink(&i))
            .max();
        assert!((deepest.unwrap() as i32) < OPTION_H, "letters reach {deepest:?}");
        let mut px = vec![0u8; 320 * 200 * 4];
        m.draw(&mut Canvas::new(&mut px, 320, 200), &mut art, (320, 200));
        let foot = (200 - 158) / 2 - BOOK_LIFT + RIGHT[1] + RIGHT[3];
        let last = m.hits.last().unwrap().0;
        assert!(last[1] + OPTION_H <= foot, "the last line ends at {}, the page at {foot}", last[1] + OPTION_H);
        if let Ok(out) = std::env::var("U8_SHOTS") {
            let mut ppm = b"P6 320 200 255\n".to_vec();
            px.chunks(4).for_each(|p| ppm.extend(&p[..3]));
            std::fs::write(format!("{out}/options.ppm"), ppm).unwrap();
        }
    }

    /// The quit question draws from the game's art; Yes quits, No backs out. With `U8_SHOTS`
    /// set, the picture is written there as PPM.
    #[test]
    #[ignore]
    fn quit_asks_first() {
        let Ok(dir) = std::env::var("U8_DATA") else { return };
        let root = PathBuf::from(dir);
        let pal =
            Palette::parse(&std::fs::read(u8data::find_file(&root, "STATIC/U8PAL.PAL").unwrap()).unwrap()).unwrap();
        let (mut m, mut art) = (Menu::load(&root, &pal).unwrap(), GumpArt::load(&root, &pal).unwrap());
        m.open(Page::Main);
        assert_eq!(m.entry(5), None);
        assert_eq!(m.page, Some(Page::Quit));
        let mut px = vec![0u8; 320 * 200 * 4];
        m.draw(&mut Canvas::new(&mut px, 320, 200), &mut art, (320, 200));
        if let Ok(out) = std::env::var("U8_SHOTS") {
            let mut ppm = b"P6 320 200 255\n".to_vec();
            px.chunks(4).for_each(|p| ppm.extend(&p[..3]));
            std::fs::write(format!("{out}/quit.ppm"), ppm).unwrap();
        }
        let centre = |r: [i32; 4]| (r[0] + r[2] / 2, r[1] + r[3] / 2);
        let (yes, no) = (m.hits[0].0, m.hits[1].0);
        assert!(yes[2] > 0 && no[0] > yes[0], "two buttons, Yes on the left");
        let n = centre(no);
        assert_eq!(m.click(n.0, n.1), None);
        assert_eq!(m.page, Some(Page::Main), "No: back to the menu");
        m.open(Page::Quit);
        let y = centre(yes);
        assert_eq!(m.click(y.0, y.1), Some(Action::Quit));
    }
}
