//! The game's book menus, drawn with the original's art from U8GUMPS.FLX: the main menu (Esc),
//! the diary for saving and loading (J), and the quit question (Alt-X). Its Options entry opens
//! the settings panel.
use crate::art::GumpArt;
use crate::credits::Roll;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use u8data::{
    font::{Fonts, TextImage},
    palette::Palette,
    save,
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
const LIT_FONT: usize = 6;
/// The diary, as DOS's (Read Diary loads, Write Diary saves): twelve entries, Entry n for save
/// slot n - 1 (Entry 1, slot 0, is "The Beginning...": loading it starts a new game; it can't be
/// written over), six to a spread, three to a page. The quick save (F7/F8) isn't in it.
const DIARY_ENTRIES: u32 = 12;
/// "Entry" (frame 0 English, 1 French, 2 German), the numbers 1 to 12 (frame n - 1), and the
/// page corners that turn the spreads (frame 0 back, at the top left; 1 on, at the top right).
const ENTRY_WORD: u32 = 46;
const ENTRY_NUMBERS: u32 = 36;
const TURN: u32 = 34;
/// Where they sit in the book (measured in DOS): each page's first heading, the rows; the
/// description under its heading ("The Beginning..." a little lower); the corners.
const ENTRY_AT: [(i32, i32); 2] = [(46, 10), (166, 10)];
const ENTRY_ROW: i32 = 40;
const DESC_DOWN: i32 = 15;
const BEGINNING_DOWN: i32 = 22;
const TURN_AT: [(i32, i32); 2] = [(34, 3), (245, 3)];
/// The descriptions' font (black, small), how wide one can be, and its colour pointed at.
const DESC_FONT: usize = 4;
const DESC_W: i32 = 95;
const DESC_LIT: [u8; 3] = [170, 30, 30];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diary {
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Main,
    Diary(Diary),
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
    /// The book's Options: the settings panel.
    Settings,
    Name(String),
    /// Roll the credits or the quotes.
    Roll(Roll),
    /// Show the intro or the ending.
    Film(crate::films::Which),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Hit {
    Entry(u32),
    /// A page corner: back or on a spread.
    Turn(i32),
    Slot(u32),
    Yes,
    No,
}

struct SlotInfo {
    slot: u32,
    desc: String,
    exists: bool,
}

pub struct Menu {
    fonts: Fonts,
    pal: Palette,
    pub page: Option<Page>,
    hits: Vec<([i32; 4], Hit)>,
    hover: Option<Hit>,
    slots: Vec<SlotInfo>,
    /// The diary's spread shown (0: entries 1 to 6).
    spread: u32,
    /// "Entry" in the language played (ENTRY_WORD's frame).
    entry_word: u32,
    /// Typing a description for this slot (Write Diary).
    edit: Option<(u32, String)>,
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
            spread: 0,
            entry_word: match u8data::lang::of(root) {
                'F' => 1,
                'G' => 2,
                _ => 0,
            },
            edit: None,
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
            self.spread = 0;
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
        self.slots = (0..DIARY_ENTRIES)
            .map(|slot| {
                let desc = match (slot, listed.get(&slot)) {
                    (0, _) => "The Beginning...".to_string(),
                    (_, Some(d)) => d.clone(),
                    _ => String::new(),
                };
                SlotInfo { slot, desc, exists: listed.contains_key(&slot) }
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
        let h = self.hit(x, y)?;
        self.act(h)
    }

    /// The pad: the choice highlighted on (`d` > 0) or back, round the page's lines, tabs and
    /// buttons.
    pub fn step(&mut self, d: i32) {
        let n = self.hits.len() as i32;
        if self.edit.is_some() || n == 0 {
            return;
        }
        let at = self.hover.and_then(|h| self.hits.iter().position(|x| x.1 == h));
        let i = at.map_or(if d > 0 { 0 } else { n - 1 }, |i| (i as i32 + d).rem_euclid(n));
        self.hover = Some(self.hits[i as usize].1);
    }

    /// The pad: take the highlighted choice.
    pub fn accept(&mut self) -> Option<Action> {
        let h = self.hover?;
        self.act(h)
    }

    /// The pad can't type: give what's being named a name, if it has none ("Avatar"; a save:
    /// `save`, or "Save N" when that won't fit). False if it has one already.
    pub fn suggest(&mut self, save: &str) -> bool {
        let (naming, (font, room)) = (self.page == Some(Page::Name), self.edit_room());
        let fonts = &self.fonts;
        let Some((slot, desc)) = self.edit.as_mut().filter(|e| e.1.is_empty()) else { return false };
        let name = if naming { "Avatar".to_string() } else { save.to_string() };
        let fits = |s: &String| fonts.width(font, &format!("{s}-")) < room as u32;
        *desc = [name, format!("Save {slot}")].into_iter().find(fits).unwrap_or_default();
        true
    }

    /// The font and room for what's being typed.
    fn edit_room(&self) -> (usize, i32) {
        if self.page == Some(Page::Name) {
            (LIT_FONT, RIGHT[2] - 8)
        } else {
            (DESC_FONT, DESC_W)
        }
    }

    fn act(&mut self, h: Hit) -> Option<Action> {
        let page = self.page?;
        match (h, page) {
            (Hit::Entry(k), _) => self.entry(k),
            (Hit::Turn(d), _) => {
                self.turn(d);
                None
            }
            (Hit::Slot(slot), Page::Diary(Diary::Read)) => {
                self.slots.iter().any(|s| s.slot == slot && s.exists).then_some(Action::Load(slot))
            }
            (Hit::Slot(0), Page::Diary(Diary::Write)) => None, // (the beginning stays)
            (Hit::Slot(slot), Page::Diary(Diary::Write)) => {
                let old = self.slots.iter().find(|s| s.slot == slot && s.exists).map(|s| s.desc.clone());
                self.edit = Some((slot, old.unwrap_or_default()));
                None
            }
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
            3 => return Some(Action::Settings),
            5 => self.open(Page::Quit),
            0 => return Some(Action::Film(crate::films::Which::Intro)),
            4 => return Some(Action::Roll(Roll::Credits)),
            6 => return Some(Action::Roll(Roll::Quotes)),
            _ => return Some(Action::Film(crate::films::Which::Endgame)),
        }
        None
    }

    /// The diary's other spread (round: there are two).
    fn turn(&mut self, d: i32) {
        let n = DIARY_ENTRIES.div_ceil(6) as i32;
        self.spread = (self.spread as i32 + d).rem_euclid(n) as u32;
        self.hover = None;
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
        let (naming, (font, room)) = (self.page == Some(Page::Name), self.edit_room());
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
            (KeyCode::ArrowLeft | KeyCode::PageUp, Some(Page::Diary(_))) => {
                self.turn(-1);
                None
            }
            (KeyCode::ArrowRight | KeyCode::PageDown, Some(Page::Diary(_))) => {
                self.turn(1);
                None
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
                // Headings and descriptions, then the corners over them (the back one covers the
                // first "E", as in DOS), which are found first.
                let edit = self.edit.clone();
                let shown = (self.spread * 6..(self.spread * 6 + 6).min(DIARY_ENTRIES)).enumerate();
                for (i, slot) in shown {
                    let (at, row) = (ENTRY_AT[i / 3], (i % 3) as i32);
                    let (x, y) = (bx + at.0, by + at.1 + row * ENTRY_ROW);
                    let word = art.draw(canvas, ENTRY_WORD, self.entry_word, x, y);
                    art.draw(canvas, ENTRY_NUMBERS, slot, x + word[2] + 2, y);
                    let h = Hit::Slot(slot);
                    let s = &self.slots[slot as usize];
                    let can = if mode == Diary::Write { slot != 0 } else { s.exists };
                    let (desc, tint) = match &edit {
                        Some((e, d)) if *e == slot => (format!("{d}-"), Some(DESC_LIT)),
                        _ => (s.desc.clone(), (can && lit(h)).then_some(DESC_LIT)),
                    };
                    if !desc.is_empty() {
                        let mut img = self.fonts.render(DESC_FONT, &[desc], &self.pal, None);
                        if let Some(c) = tint {
                            // (Its letters are black: all of them take the colour.)
                            img.rgba.chunks_exact_mut(4).filter(|p| p[3] > 0).for_each(|p| p[..3].copy_from_slice(&c));
                        }
                        let down = if slot == 0 { BEGINNING_DOWN } else { DESC_DOWN };
                        text::draw_plain(canvas, ss, &img, x, y + down);
                    }
                    hits.push(([x, y, DESC_W + 10, ENTRY_ROW - 2], h));
                }
                for (f, (d, at)) in [(-1, TURN_AT[0]), (1, TURN_AT[1])].into_iter().enumerate() {
                    let rect = art.draw(canvas, TURN, f as u32, bx + at.0, by + at.1);
                    hits.insert(0, (rect, Hit::Turn(d)));
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

    /// The book's fourth entry opens the settings panel.
    #[test]
    #[ignore]
    fn options_open_the_settings() {
        let Ok(dir) = std::env::var("U8_DATA") else { return };
        let root = PathBuf::from(dir);
        let pal =
            Palette::parse(&std::fs::read(u8data::find_file(&root, "STATIC/U8PAL.PAL").unwrap()).unwrap()).unwrap();
        let mut m = Menu::load(&root, &pal).unwrap();
        m.open(Page::Main);
        assert_eq!(m.key(KeyCode::Digit4, None), Some(Action::Settings));
    }

    /// The diary as DOS's: Read loads and Write saves; Entry 1 is the beginning (slot 0: a new
    /// game when read, never written), Entry n slot n - 1, six to a spread; the corners (and the
    /// arrow keys) turn to entries 7 to 12 and back.
    #[test]
    #[ignore]
    fn the_diary_is_the_originals() {
        let Ok(dir) = std::env::var("U8_DATA") else { return };
        let root = PathBuf::from(dir);
        let pal =
            Palette::parse(&std::fs::read(u8data::find_file(&root, "STATIC/U8PAL.PAL").unwrap()).unwrap()).unwrap();
        let (mut m, mut art) = (Menu::load(&root, &pal).unwrap(), GumpArt::load(&root, &pal).unwrap());
        let mut px = vec![0u8; 320 * 200 * 4];
        let mut draw = |m: &mut Menu| m.draw(&mut Canvas::new(&mut px, 320, 200), &mut art, (320, 200));
        let slots = |m: &Menu| {
            m.hits.iter().filter_map(|h| if let Hit::Slot(s) = h.1 { Some(s) } else { None }).collect::<Vec<_>>()
        };
        m.open(Page::Diary(Diary::Read));
        draw(&mut m);
        assert_eq!(slots(&m), [0, 1, 2, 3, 4, 5]);
        assert!(matches!(m.hits[0].1, Hit::Turn(_)) && matches!(m.hits[1].1, Hit::Turn(_)), "corners first");
        assert_eq!(m.slots[0].desc, "The Beginning...");
        let at = |m: &Menu, slot| m.hits.iter().find(|h| h.1 == Hit::Slot(slot)).unwrap().0;
        let entry = |m: &Menu, slot| {
            let r = at(m, slot);
            (r[0] + 20, r[1] + 30) // (below the corner over Entry 1's heading)
        };
        let (x, y) = entry(&m, 0);
        assert_eq!(m.click(x, y), m.slots[0].exists.then_some(Action::Load(0)), "the beginning: a new game");
        // Entry 4 heads the right page, 120 across; rows 40 apart.
        assert_eq!((at(&m, 3)[0] - at(&m, 0)[0], at(&m, 1)[1] - at(&m, 0)[1]), (120, 40));
        assert_eq!(m.key(KeyCode::ArrowRight, None), None);
        draw(&mut m);
        assert_eq!(slots(&m), [6, 7, 8, 9, 10, 11], "entries 7 to 12");
        let turn = m.hits.iter().find(|h| h.1 == Hit::Turn(1)).unwrap().0;
        m.click(turn[0] + 5, turn[1] + 5);
        draw(&mut m);
        assert_eq!(slots(&m), [0, 1, 2, 3, 4, 5], "round");
        m.open(Page::Diary(Diary::Write));
        draw(&mut m);
        let (x, y) = entry(&m, 0);
        assert_eq!((m.click(x, y), m.typing()), (None, false), "the beginning isn't written over");
        let (x, y) = entry(&m, 4);
        m.click(x, y);
        assert_eq!(m.edit.as_ref().map(|e| e.0), Some(4), "Entry 5 writes slot 4");
    }

    /// A pad can't type: the name and a save's description come ready made, and A takes them.
    #[test]
    #[ignore]
    fn the_pad_names_without_typing() {
        let Ok(dir) = std::env::var("U8_DATA") else { return };
        let root = PathBuf::from(dir);
        let pal =
            Palette::parse(&std::fs::read(u8data::find_file(&root, "STATIC/U8PAL.PAL").unwrap()).unwrap()).unwrap();
        let mut m = Menu::load(&root, &pal).unwrap();
        m.open(Page::Name);
        assert!(m.suggest("Docks, day 1"));
        assert!(!m.suggest("Docks, day 1"), "once");
        assert_eq!(m.key(KeyCode::Enter, None), Some(Action::Name("Avatar".into())));
        m.open(Page::Diary(Diary::Write));
        m.edit = Some((7, String::new()));
        assert!(m.suggest("Docks, day 3"));
        assert_eq!(m.key(KeyCode::Enter, None), Some(Action::Save { slot: 7, desc: "Docks, day 3".into() }));
        m.edit = Some((7, String::new()));
        m.suggest("The Hall of the Mountain King, day 12");
        assert_eq!(m.edit.as_ref().unwrap().1, "Save 7", "too long for the line");
        m.edit = Some((7, "Before the Titans".into()));
        assert!(!m.suggest("Docks, day 3"), "a name already there stays");
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
