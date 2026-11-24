//! Settings (F1, or the book's Options): a panel over the game with a tab for each kind of
//! setting and one listing the controls. Keys, mouse, pad and touch move about it; drawn at the
//! screen's resolution in the books' typeface. The viewer supplies the rows' values.
use crate::pagefont::PageFont;
use u8render::{text, Canvas};
use winit::keyboard::KeyCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    Upscaler,
    Scale,
    Zoom,
    Fullscreen,
    Brightness,
    ColourFix,
    /// The view easing up and down (ours).
    SmoothCamera,
    FrameTime,
    Music,
    Volume,
    Song,
    Effects,
    /// Background sounds' level (fires, drips, fountains).
    Ambience,
    Buttons,
    /// A game controller: in use, its pointer's speed, its sticks' dead zone, which stick walks.
    Pad,
    PadSpeed,
    Deadzone,
    SwapSticks,
    /// The buttons a touch screen shows on the right edge.
    TouchButtons,
    TextStyle,
    Cutaway,
    Roofs,
    Follow,
    Cheats,
    /// Restored content (not the original game).
    Restored,
    Discord,
    Editor,
    /// The language played in (a translated install's, or English).
    Language,
}

/// The tabs, and the settings on each (the last tab lists the controls).
pub const TABS: [&str; 5] = ["Video", "Sound", "Controls", "Game", "Keys"];
pub const ON_TAB: [&[Setting]; 4] = {
    use Setting::*;
    [
        &[Upscaler, Scale, Zoom, Fullscreen, Brightness, ColourFix, SmoothCamera, FrameTime],
        &[Music, Volume, Song, Effects, Ambience],
        &[Buttons, Pad, PadSpeed, Deadzone, SwapSticks, TouchButtons],
        &[TextStyle, Cutaway, Roofs, Follow, Cheats, Restored, Discord, Editor, Language],
    ]
};
const KEYS_TAB: usize = 4;

/// The controls, by device (a heading has no key).
pub const KEYS: &[(&str, &str)] = &[
    ("", "Keyboard"),
    ("Arrows / WASD", "walk (Shift runs)"),
    ("Space", "jump"),
    ("C", "weapon out or away"),
    ("I / Z", "backpack / paperdoll"),
    ("J", "the diary: save and load"),
    ("1-9", "pick a reply"),
    ("Esc", "the game menu"),
    ("Backspace", "put away pages and speech"),
    ("Space, Left, Right", "turn a book's pages"),
    ("F1", "these settings"),
    ("F2", "the next look"),
    ("F3 / F4", "see inside / roofs"),
    ("F6, Home", "free camera / back to the Avatar"),
    ("F7 / F8", "quick save / quick load"),
    ("F9", "stop all scripts (if a scene sticks)"),
    ("F10, [ ]", "colour fix, brightness"),
    ("F11", "full screen"),
    ("F12", "frame time"),
    ("+ / -", "zoom"),
    ("P / Shift-P", "music / sound effects on or off"),
    ("N / Shift-N", "next / previous song"),
    ("Alt-H", "swap the mouse buttons"),
    ("Alt-X, Ctrl-Q", "quit"),
    ("", "Mouse"),
    ("Hold right", "walk to the pointer (far: run)"),
    ("Tap right", "one careful step"),
    ("Both buttons", "jump"),
    ("Left / double click", "look / use"),
    ("Left drag", "move a thing"),
    ("Left in combat", "swing (hold to block)"),
    ("Double right, Avatar", "weapon out or away"),
    ("Double right, combat", "kick"),
    ("Middle drag", "pan the camera"),
    ("Wheel", "zoom"),
    ("", "Controller"),
    ("Left stick", "walk (lean further to run)"),
    ("Right stick", "move the pointer"),
    ("A", "left button: look, use, drag, swing"),
    ("X", "use what's under the pointer"),
    ("RT / LT", "walk to the pointer / jump"),
    ("B", "put away pages; back in menus"),
    ("Y / LB / RB", "backpack / paperdoll / weapon"),
    ("Start / Back", "the game menu / the diary"),
    ("L3 / R3", "recentre / pointer to the Avatar"),
    ("D-pad", "walk; move the choice in menus"),
    ("", "Touch"),
    ("Tap / double tap", "look / use"),
    ("Press and slide", "drag a thing"),
    ("Hold on the ground", "walk to the finger"),
    ("Second finger tap", "jump, while walking"),
    ("Pinch", "zoom"),
    ("Two finger tap", "the right button's tap"),
];

/// What the player did: change a setting by a step (`wrap`: past the end goes round, as Enter
/// and clicks do; the arrows stop at the ends), or put the panel away.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Out {
    Change(Setting, i32, bool),
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Hit {
    Tab(usize),
    /// A row, and which part: -1 its "less" arrow, 1 its "more" arrow, 0 elsewhere.
    Row(usize, i32),
    Close,
    Panel,
}

/// Sizes in game pixels (the interface's): the panel at most this big, text, rows.
const MAX: (i32, i32) = (330, 236);
const TITLE: f32 = 13.0;
const TEXT: f32 = 10.0;
const SMALL: f32 = 8.5;
const ROW: i32 = 15;
const KEY_ROW: i32 = 12;
const TOP: i32 = 44;
const FOOT: i32 = 16;

const GOLD: [u8; 3] = [236, 196, 120];
const LIGHT: [u8; 3] = [226, 220, 208];
const DIM: [u8; 3] = [150, 144, 136];

pub struct Panel {
    pub tab: usize,
    /// The highlighted row (or, on the keys tab, the first line shown).
    pub row: usize,
    scroll: usize,
    /// The current tab's rows: (setting, label, value), from the viewer.
    pub rows: Vec<(Setting, String, String)>,
    /// Opened from the book's Options: putting it away goes back there.
    pub from_menu: bool,
    hits: Vec<([i32; 4], Hit)>,
    font: PageFont,
}

impl Panel {
    pub fn new(from_menu: bool) -> Self {
        Self { tab: 0, row: 0, scroll: 0, rows: vec![], from_menu, hits: vec![], font: PageFont::new() }
    }

    /// How many lines the tab has.
    fn len(&self) -> usize {
        if self.tab == KEYS_TAB {
            KEYS.len()
        } else {
            self.rows.len()
        }
    }

    pub fn set_tab(&mut self, t: usize) {
        if t != self.tab {
            (self.tab, self.row, self.scroll) = (t, 0, 0);
        }
    }

    fn turn(&mut self, d: i32) {
        self.set_tab((self.tab as i32 + d).rem_euclid(TABS.len() as i32) as usize);
    }

    fn step(&mut self, d: i32) {
        let n = self.len() as i32;
        self.row = if n == 0 { 0 } else { (self.row as i32 + d).clamp(0, n - 1) as usize };
    }

    fn change(&self, d: i32, wrap: bool) -> Option<Out> {
        let s = self.rows.get(self.row).filter(|_| self.tab != KEYS_TAB)?;
        Some(Out::Change(s.0, d, wrap))
    }

    pub fn key(&mut self, k: KeyCode, shift: bool) -> Option<Out> {
        use KeyCode as K;
        match k {
            K::Escape | K::F1 | K::Backspace => return Some(Out::Close),
            K::Tab if shift => self.turn(-1),
            K::Tab | K::PageDown | K::KeyE => self.turn(1),
            K::PageUp | K::KeyQ => self.turn(-1),
            K::ArrowUp | K::KeyW => self.step(-1),
            K::ArrowDown | K::KeyS => self.step(1),
            K::Home => self.row = 0,
            K::End => self.step(i32::MAX / 2),
            K::ArrowLeft | K::KeyA => return self.change(-1, false),
            K::ArrowRight | K::KeyD => return self.change(1, false),
            K::Enter | K::NumpadEnter | K::Space => return self.change(1, true),
            K::Digit1 => self.set_tab(0),
            K::Digit2 => self.set_tab(1),
            K::Digit3 => self.set_tab(2),
            K::Digit4 => self.set_tab(3),
            K::Digit5 => self.set_tab(4),
            _ => {}
        }
        None
    }

    fn hit(&self, x: i32, y: i32) -> Option<Hit> {
        self.hits.iter().find(|(r, _)| x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]).map(|h| h.1)
    }

    /// The pointer moved: the row under it is highlighted. True if that changed.
    pub fn hover(&mut self, x: i32, y: i32) -> bool {
        match self.hit(x, y) {
            Some(Hit::Row(i, _)) if i != self.row && self.tab != KEYS_TAB => {
                self.row = i;
                true
            }
            _ => false,
        }
    }

    /// A click (or tap): a tab, a row's arrows or the row; outside the panel, away it goes.
    pub fn click(&mut self, x: i32, y: i32) -> Option<Out> {
        match self.hit(x, y) {
            Some(Hit::Tab(t)) => self.set_tab(t),
            Some(Hit::Row(i, part)) if self.tab != KEYS_TAB => {
                self.row = i;
                return self.change(if part == 0 { 1 } else { part }, part == 0);
            }
            Some(Hit::Close) | None => return Some(Out::Close),
            _ => {}
        }
        None
    }

    /// The wheel: down the rows (the keys tab scrolls).
    pub fn wheel(&mut self, d: i32) {
        self.step(d);
    }

    /// The panel's rect in game pixels, centred in the view.
    fn rect((vw, vh): (u32, u32)) -> [i32; 4] {
        let (w, h) = (MAX.0.min(vw as i32 - 8), MAX.1.min(vh as i32 - 8));
        [(vw as i32 - w) / 2, (vh as i32 - h) / 2, w, h]
    }

    /// Lines that fit between the tabs and the foot.
    fn shown(r: [i32; 4], keys: bool) -> usize {
        ((r[3] - TOP - FOOT) / if keys { KEY_ROW } else { ROW }).max(1) as usize
    }

    /// Draw it over everything at `ss` canvas pixels per game pixel.
    pub fn draw(&mut self, canvas: &mut Canvas, ss: u32, view: (u32, u32)) {
        let s = ss as i32;
        let r = Self::rect(view);
        let keys = self.tab == KEYS_TAB;
        // Everything behind darkened; the panel, its edge.
        fill(canvas, [0, 0, view.0 as i32 * s, view.1 as i32 * s], [0, 0, 0, 110]);
        let px = |v: [i32; 4]| [v[0] * s, v[1] * s, v[2] * s, v[3] * s];
        fill(canvas, px([r[0] - 1, r[1] - 1, r[2] + 2, r[3] + 2]), [120, 88, 48, 255]);
        fill(canvas, px(r), [22, 19, 28, 240]);
        let mut hits = vec![(r, Hit::Panel)];
        self.put(canvas, ss, "Settings", TITLE, GOLD, (r[0] + 10, r[1] + 5), Align::Left);
        let close = [r[0] + r[2] - 18, r[1] + 4, 14, 14];
        self.put(canvas, ss, "×", TITLE, DIM, (close[0] + 7, close[1]), Align::Centre);
        hits.push((close, Hit::Close));
        // Tabs.
        let tw = (r[2] - 16) / TABS.len() as i32;
        for (t, name) in TABS.iter().enumerate() {
            let at = [r[0] + 8 + t as i32 * tw, r[1] + 23, tw - 2, 15];
            let on = t == self.tab;
            fill(canvas, px(at), if on { [86, 62, 34, 255] } else { [36, 32, 44, 255] });
            let ink = if on { GOLD } else { DIM };
            self.put(canvas, ss, name, TEXT, ink, (at[0] + at[2] / 2, at[1] + 2), Align::Centre);
            hits.push((at, Hit::Tab(t)));
        }
        // The lines, scrolled to keep the highlighted one in sight.
        let n = Self::shown(r, keys);
        if keys {
            self.row = self.row.min(KEYS.len().saturating_sub(n));
            self.scroll = self.row;
        } else {
            self.row = self.row.min(self.rows.len().saturating_sub(1));
            self.scroll = self.scroll.min(self.row).max((self.row + 1).saturating_sub(n));
        }
        let (x0, x1) = (r[0] + 10, r[0] + r[2] - 10);
        if keys {
            for (i, &(k, does)) in KEYS.iter().enumerate().skip(self.scroll).take(n) {
                let y = r[1] + TOP + (i - self.scroll) as i32 * KEY_ROW;
                if k.is_empty() {
                    self.put(canvas, ss, does, TEXT, GOLD, (x0, y - 1), Align::Left);
                } else {
                    self.put(canvas, ss, k, SMALL, LIGHT, (x0 + 4, y), Align::Left);
                    self.put(canvas, ss, does, SMALL, DIM, (x0 + 104, y), Align::Left);
                }
            }
        } else if self.rows.is_empty() {
            self.put(
                canvas,
                ss,
                "Not available here (no sound device found?)",
                TEXT,
                DIM,
                (x0, r[1] + TOP),
                Align::Left,
            );
        } else {
            let rows = self.rows.clone();
            for (i, (_, label, value)) in rows.iter().enumerate().skip(self.scroll).take(n) {
                let y = r[1] + TOP + (i - self.scroll) as i32 * ROW;
                let row = [x0 - 4, y - 1, x1 - x0 + 8, ROW - 1];
                let on = i == self.row;
                if on {
                    fill(canvas, px(row), [62, 46, 30, 255]);
                }
                self.put(canvas, ss, label, TEXT, if on { LIGHT } else { DIM }, (x0, y), Align::Left);
                // "‹ value ›": the arrows step it, the rest of the row goes on round.
                let value_w = 96;
                let vx = x1 - value_w;
                self.put(canvas, ss, value, TEXT, if on { GOLD } else { LIGHT }, (vx + value_w / 2, y), Align::Centre);
                if on {
                    self.put(canvas, ss, "‹", TEXT, GOLD, (vx + 2, y), Align::Centre);
                    self.put(canvas, ss, "›", TEXT, GOLD, (x1 - 2, y), Align::Centre);
                }
                hits.push((row, Hit::Row(i, 0)));
                hits.push(([vx - 6, row[1], 18, row[3]], Hit::Row(i, -1)));
                hits.push(([x1 - 12, row[1], 16, row[3]], Hit::Row(i, 1)));
            }
        }
        // More above or below; and how it works.
        let more = (self.scroll > 0, self.scroll + n < self.len());
        // (Small triangles: the typeface has no arrows.)
        let tri = |canvas: &mut Canvas, (x, y): (i32, i32), up: bool| {
            for i in 0..3 * s {
                let w = if up { i } else { 3 * s - 1 - i };
                fill(canvas, [x * s - w, y * s + i, 2 * w + 1, 1], [DIM[0], DIM[1], DIM[2], 255]);
            }
        };
        if more.0 {
            tri(canvas, (x1, r[1] + TOP - 5), true);
        }
        if more.1 {
            tri(canvas, (x1, r[1] + r[3] - FOOT - 2), false);
        }
        let hint = if keys {
            "Up, Down: scroll   Tab, Q, E: other tabs   Esc: close"
        } else {
            "Up, Down: choose   Left, Right: change   Tab, Q, E: tabs   Esc: close"
        };
        self.put(canvas, ss, hint, SMALL, DIM, (r[0] + r[2] / 2, r[1] + r[3] - FOOT + 3), Align::Centre);
        // (The rows first: a row's arrows are found before the row.)
        hits.reverse();
        self.hits = hits;
    }

    /// `s` at size `px` (game pixels per em) with its top at `at` (game pixels).
    #[allow(clippy::too_many_arguments)]
    fn put(&self, canvas: &mut Canvas, ss: u32, s: &str, px: f32, ink: [u8; 3], at: (i32, i32), a: Align) {
        let img = self.font.render_ink(&[s.to_string()], px, ss, ink);
        let x = at.0 * ss as i32 - if a == Align::Centre { img.w as i32 / 2 } else { 0 };
        text::draw_blended(canvas, &img, x, at.1 * ss as i32);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Align {
    Left,
    Centre,
}

/// Cover canvas pixels [x, y, w, h] with a colour (alpha `c[3]`), over what's there.
fn fill(canvas: &mut Canvas, r: [i32; 4], c: [u8; 4]) {
    let (w, h) = (canvas.w as i32, canvas.h as i32);
    let a = c[3] as u32;
    for y in r[1].max(0)..(r[1] + r[3]).min(h) {
        for x in r[0].max(0)..(r[0] + r[2]).min(w) {
            let d = ((y * w + x) * 4) as usize;
            for (k, &ck) in c[..3].iter().enumerate() {
                canvas.px[d + k] = ((ck as u32 * a + canvas.px[d + k] as u32 * (255 - a)) / 255) as u8;
            }
            canvas.px[d + 3] = (a + canvas.px[d + 3] as u32 * (255 - a) / 255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel() -> Panel {
        let mut p = Panel::new(false);
        p.rows = ON_TAB[0].iter().map(|&s| (s, format!("{s:?}"), "on".into())).collect();
        p
    }

    /// Every setting is on one tab, once.
    #[test]
    fn every_setting_has_a_tab() {
        let all: Vec<Setting> = ON_TAB.iter().flat_map(|t| t.iter().copied()).collect();
        for (i, s) in all.iter().enumerate() {
            assert!(!all[i + 1..].contains(s), "{s:?} twice");
        }
        assert_eq!(all.len(), 28);
    }

    /// The keys: up and down choose, left and right change (stopping at the ends), Enter goes on
    /// round, Tab and Q/E switch tabs, Esc and F1 put it away.
    #[test]
    fn keys_move_about_and_change() {
        let mut p = panel();
        assert_eq!(p.key(KeyCode::ArrowRight, false), Some(Out::Change(Setting::Upscaler, 1, false)));
        p.key(KeyCode::ArrowDown, false);
        p.key(KeyCode::ArrowDown, false);
        assert_eq!(p.key(KeyCode::ArrowLeft, false), Some(Out::Change(Setting::Zoom, -1, false)));
        assert_eq!(p.key(KeyCode::Enter, false), Some(Out::Change(Setting::Zoom, 1, true)));
        p.key(KeyCode::ArrowUp, false);
        p.key(KeyCode::ArrowUp, false);
        p.key(KeyCode::ArrowUp, false);
        assert_eq!(p.row, 0, "stops at the top");
        p.key(KeyCode::Tab, false);
        assert_eq!((p.tab, p.row), (1, 0));
        p.key(KeyCode::Tab, true);
        p.key(KeyCode::KeyQ, false);
        assert_eq!(p.tab, 4, "round to the keys");
        assert_eq!(p.key(KeyCode::Enter, false), None, "the keys tab changes nothing");
        assert_eq!(p.key(KeyCode::Digit2, false), None);
        assert_eq!(p.tab, 1);
        assert_eq!(p.key(KeyCode::F1, false), Some(Out::Close));
        assert_eq!(p.key(KeyCode::Escape, false), Some(Out::Close));
    }

    /// The mouse: a row's arrows step it, the row goes on round, a tab switches, outside closes.
    /// Everything fits a 320×200 view, and the keys scroll.
    #[test]
    fn the_mouse_clicks_rows_tabs_and_arrows() {
        let mut p = panel();
        let (vw, vh) = (320, 200);
        let mut px = vec![0u8; (vw * vh * 4) as usize];
        p.draw(&mut Canvas::new(&mut px, vw, vh), 1, (vw, vh));
        let r = Panel::rect((vw, vh));
        assert!(r[0] >= 0 && r[1] >= 0 && r[0] + r[2] <= vw as i32 && r[1] + r[3] <= vh as i32);
        let y = r[1] + TOP + ROW + 5; // the second row
        assert!(p.hover(r[0] + 40, y));
        assert_eq!(p.row, 1);
        assert_eq!(p.click(r[0] + 40, y), Some(Out::Change(Setting::Scale, 1, true)));
        let x1 = r[0] + r[2] - 10;
        assert_eq!(p.click(x1 - 2, y), Some(Out::Change(Setting::Scale, 1, false)));
        assert_eq!(p.click(x1 - 96, y), Some(Out::Change(Setting::Scale, -1, false)));
        let tw = (r[2] - 16) / TABS.len() as i32;
        assert_eq!(p.click(r[0] + 8 + 3 * tw + 5, r[1] + 28), None);
        assert_eq!(p.tab, 3);
        assert_eq!(p.click(r[0] + 5, r[1] + 5), None, "the panel itself does nothing");
        assert_eq!(p.click(2, 2), Some(Out::Close), "outside puts it away");
        // The keys: more than fit; scrolling shows the last.
        p.set_tab(4);
        p.draw(&mut Canvas::new(&mut px, vw, vh), 1, (vw, vh));
        let n = Panel::shown(r, true);
        assert!(KEYS.len() > n);
        for _ in 0..KEYS.len() {
            p.wheel(1);
        }
        p.draw(&mut Canvas::new(&mut px, vw, vh), 1, (vw, vh));
        assert_eq!(p.scroll + n, KEYS.len());
        assert!(px.chunks(4).any(|c| c[3] > 0 && c[0] > 200), "drawn, gold and light letters");
    }

    /// The controls list's two columns don't run into each other or off the panel.
    #[test]
    fn the_keys_fit_their_columns() {
        let f = PageFont::new();
        for &(k, does) in KEYS {
            let w = |s: &str, px| f.render_ink(&[s.to_string()], px, 1, LIGHT).w as i32;
            if k.is_empty() {
                continue;
            }
            assert!(w(k, SMALL) < 100, "{k}");
            assert!(104 + w(does, SMALL) < MAX.0 - 20, "{does}");
        }
    }
}
