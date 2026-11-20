//! A touch screen (the Steam Deck's, the ROG Ally's): fingers drive the mouse's own functions, as
//! the pad does. The first finger is the pointer and the left button (tap: look, double tap: use,
//! press and slide: drag); held still on open ground it walks towards itself, as the right button
//! does. A second finger taps to jump while walking, pinches to zoom, or taps with the first for
//! the right button's tap. A few buttons on the screen's edge stand in for keys.
use crate::input::Command;
use std::time::{Duration, Instant};

/// What the fingers do, as the mouse would do it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Out {
    /// The pointer to there (window pixels).
    Move((f64, f64)),
    Left(bool),
    /// Let go of the left button without a click or a drop.
    Cancel,
    /// Walking towards the pointer starts or stops (as the right button held).
    Walk(bool),
    Jump,
    Zoom(f32),
    RightTap,
}

/// Held this long on open ground, a finger walks.
const HOLD: Duration = Duration::from_millis(200);
/// A second finger up within this, without sliding, is a tap.
const TAP: Duration = Duration::from_millis(300);
/// Mouse events this soon after a touch, where the touch was, are the system's copy of it.
const ECHO: Duration = Duration::from_millis(500);

/// How far a finger slides before it's a drag, not a tap (window pixels): fingers wobble more
/// than a mouse does (6).
pub fn slop(height: f64) -> f64 {
    (height / 40.0).max(12.0)
}

#[derive(Clone, Copy)]
struct Finger {
    id: u64,
    from: (f64, f64),
    at: (f64, f64),
    since: Instant,
    far: bool,
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

#[derive(Default)]
pub struct Touch {
    first: Option<Finger>,
    second: Option<Finger>,
    slop: f64,
    /// The first finger may walk (it started on open ground); it walks.
    walkable: bool,
    walking: bool,
    /// Two fingers on (the first's press let go); the pinch's first and last spread.
    two: bool,
    pinch: Option<(f64, f64)>,
    /// A finger on a screen button: nothing more comes of it.
    spent: Option<u64>,
    /// The last touch, for telling the system's mouse copy of it apart.
    last: Option<(Instant, (f64, f64))>,
    /// Touch has been seen (the screen buttons show), and the setting for them.
    shown: bool,
    pub buttons: bool,
}

impl Touch {
    pub fn new(buttons: bool) -> Self {
        Self { buttons, ..Default::default() }
    }

    fn seen(&mut self, now: Instant, p: (f64, f64)) {
        (self.last, self.shown) = (Some((now, p)), true);
    }

    /// A mouse event at `p` now is the system's copy of a touch.
    pub fn echo(&self, now: Instant, p: (f64, f64)) -> bool {
        self.last.is_some_and(|(t, q)| now - t < ECHO && dist(p, q) < 2.0)
    }

    /// The screen buttons show (touch seen; the mouse or pad hides them again).
    pub fn buttons_shown(&self) -> bool {
        self.buttons && self.shown
    }
    pub fn hide_buttons(&mut self) {
        self.shown = false;
    }

    pub fn walking(&self) -> bool {
        self.walking
    }
    /// A finger is down (keep the frames coming).
    pub fn busy(&self) -> bool {
        self.first.is_some()
    }

    /// Finger `id` landed on a screen button: the rest of it is ignored.
    pub fn spend(&mut self, id: u64, now: Instant, p: (f64, f64)) {
        self.seen(now, p);
        self.spent = Some(id);
    }

    /// The first finger may walk if held (after its press: not on a window, not on something
    /// the Avatar could pick up, the player in control).
    pub fn walkable(&mut self, yes: bool) {
        self.walkable = yes;
    }

    pub fn start(&mut self, id: u64, p: (f64, f64), now: Instant, slop: f64) -> Vec<Out> {
        self.seen(now, p);
        let f = Finger { id, from: p, at: p, since: now, far: false };
        match (self.first, self.second) {
            (None, _) => {
                (self.first, self.slop, self.walkable, self.two, self.pinch) = (Some(f), slop, false, false, None);
                vec![Out::Move(p), Out::Left(true)]
            }
            (Some(a), None) if a.id != id => {
                self.second = Some(f);
                if self.walking {
                    return vec![];
                }
                let d = dist(a.at, p);
                (self.two, self.pinch) = (true, Some((d, d)));
                vec![Out::Cancel]
            }
            _ => vec![],
        }
    }

    pub fn moved(&mut self, id: u64, p: (f64, f64), now: Instant) -> Vec<Out> {
        self.seen(now, p);
        let slop = self.slop;
        let mut out = vec![];
        for f in [&mut self.first, &mut self.second].into_iter().flatten().filter(|f| f.id == id) {
            f.at = p;
            f.far |= dist(f.from, p) > slop;
        }
        let (Some(a), second) = (self.first, self.second) else { return out };
        match (self.two, second) {
            (true, Some(b)) => {
                // Pinching: the spread's change in rungs of the zoom (×1.25 each).
                let d = dist(a.at, b.at);
                if let Some((d0, last)) = self.pinch {
                    if (d - d0).abs() > slop && last > 0.0 && d > 0.0 {
                        // (About their middle, where a free camera zooms about the pointer.)
                        out.push(Out::Move(((a.at.0 + b.at.0) / 2.0, (a.at.1 + b.at.1) / 2.0)));
                        out.push(Out::Zoom(((d / last).ln() / 1.25f64.ln()) as f32));
                        self.pinch = Some((f64::NEG_INFINITY, d)); // (zooming now: every change counts)
                    }
                }
            }
            (true, None) => {}
            _ if a.id == id && (self.walking || a.far) => out.push(Out::Move(p)),
            _ => {}
        }
        out
    }

    pub fn ended(&mut self, id: u64, now: Instant) -> Vec<Out> {
        self.seen(now, self.first.filter(|f| f.id == id).or(self.second).map_or((0.0, 0.0), |f| f.at));
        if self.spent.take_if(|s| *s == id).is_some() {
            return vec![];
        }
        let tapped = |f: Finger| now - f.since < TAP && !f.far;
        let zoomed = self.pinch.is_some_and(|(d0, _)| d0 == f64::NEG_INFINITY);
        if let Some(b) = self.second.take_if(|f| f.id == id) {
            return match (self.walking, self.two) {
                (true, _) if tapped(b) => vec![Out::Jump],
                (false, true) if tapped(b) && !zoomed => vec![Out::RightTap],
                _ => vec![],
            };
        }
        let Some(_) = self.first.take_if(|f| f.id == id) else { return vec![] };
        let out = match (self.walking, self.two) {
            (true, _) => vec![Out::Walk(false)],
            (false, true) => vec![],
            _ => vec![Out::Left(false)],
        };
        (self.second, self.walking, self.two, self.pinch) = (None, false, false, None);
        out
    }

    /// A touch the system took back: no click, no drop, no walk.
    pub fn cancelled(&mut self, id: u64) -> Vec<Out> {
        if self.second.take_if(|f| f.id == id).is_some() || self.spent.take_if(|s| *s == id).is_some() {
            return vec![];
        }
        let Some(_) = self.first.take_if(|f| f.id == id) else { return vec![] };
        let walked = std::mem::take(&mut self.walking);
        (self.second, self.two, self.pinch) = (None, false, None);
        if walked {
            vec![Out::Walk(false)]
        } else {
            vec![Out::Cancel]
        }
    }

    /// Time passing: a first finger held on open ground starts walking.
    pub fn tick(&mut self, now: Instant) -> Vec<Out> {
        match self.first {
            Some(a) if self.walkable && !self.walking && !self.two && now - a.since >= HOLD => {
                self.walking = true;
                vec![Out::Cancel, Out::Walk(true), Out::Move(a.at)]
            }
            _ => vec![],
        }
    }
}

/// The screen buttons: what each does and says.
pub const BUTTONS: [(Command, &str); 4] = [
    (Command::Menu, "Menu"),
    (Command::Backpack, "Backpack"),
    (Command::Paperdoll, "Paperdoll"),
    (Command::Combat, "Combat"),
];

/// Where the screen buttons are on a `vw`×`vh` overlay (game pixels): a column on the right
/// edge, in the middle.
pub fn button_rects((vw, vh): (u32, u32)) -> [[i32; 4]; 4] {
    let (w, h, gap) = (64, 18, 6);
    let y0 = (vh as i32 - (4 * h + 3 * gap)) / 2;
    std::array::from_fn(|k| [vw as i32 - w - 6, y0 + k as i32 * (h + gap), w, h])
}

impl crate::App {
    /// A finger on the screen.
    pub(super) fn touched(&mut self, t: winit::event::Touch, el: &winit::event_loop::ActiveEventLoop) {
        use winit::event::TouchPhase as P;
        let (now, p, id) = (Instant::now(), (t.location.x, t.location.y), t.id);
        self.pad_aim = None; // (a finger aims by the pointer, as the mouse)
        let out = match t.phase {
            P::Started => {
                let s = self.view.scale as f64;
                let at = ((p.0 / s) as i32, (p.1 / s) as i32);
                let hit = self.touch_buttons_up().then(|| button_rects(self.view_size())).and_then(|r| {
                    r.iter().position(|r| (r[0]..r[0] + r[2]).contains(&at.0) && (r[1]..r[1] + r[3]).contains(&at.1))
                });
                if let Some(k) = hit {
                    self.touch.spend(id, now, p);
                    self.command(BUTTONS[k].0);
                    self.keep_settings();
                    return;
                }
                let out = self.touch.start(id, p, now, slop(self.window_size().1 as f64));
                self.apply_touch(out, el);
                let may = self.press.is_some()
                    && self.press_item.is_none_or(|i| self.game.can_pick_up(i).is_err())
                    && self.controlling()
                    && !self.ui_blocking()
                    && !self.menu_open();
                self.touch.walkable(may);
                self.dirty = true;
                return;
            }
            P::Moved => self.touch.moved(id, p, now),
            P::Ended => self.touch.ended(id, now),
            P::Cancelled => self.touch.cancelled(id),
        };
        self.apply_touch(out, el);
    }

    /// The screen buttons are up (touch seen, the setting on, nothing over the game).
    pub(super) fn touch_buttons_up(&self) -> bool {
        self.touch.buttons_shown() && !self.menu_open()
    }

    pub(super) fn apply_touch(&mut self, out: Vec<Out>, el: &winit::event_loop::ActiveEventLoop) {
        use winit::event::MouseButton as M;
        let (left, right) = if self.swapped { (M::Right, M::Left) } else { (M::Left, M::Right) };
        for o in out {
            match o {
                Out::Move(p) => {
                    self.owner = crate::Owner::Touch;
                    self.moved_to(p);
                }
                Out::Left(down) => self.mouse_button(left, down, el),
                Out::Cancel => self.cancel_press(),
                Out::Walk(_) => {}
                Out::Jump if self.controlling() && !self.ui_blocking() => self.both_buttons(),
                Out::Jump => {}
                Out::Zoom(steps) => self.zoom_by(steps),
                Out::RightTap => {
                    self.mouse_button(right, true, el);
                    self.mouse_button(right, false, el);
                }
            }
            self.dirty = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    /// A tap is the left button pressed and let go where the finger landed (look; two of them,
    /// a double click: use); a slide within the slop stays a tap (no pointer move, so no drag,
    /// no drop, nobody told); past it, a drag.
    #[test]
    fn taps_and_drags() {
        let (t0, mut t) = (Instant::now(), Touch::new(true));
        let s = slop(800.0);
        assert_eq!(s, 20.0);
        assert_eq!(slop(400.0), 12.0, "at least 12");
        assert_eq!(t.start(1, (100.0, 100.0), t0, s), vec![Out::Move((100.0, 100.0)), Out::Left(true)]);
        assert!(t.moved(1, (109.0, 100.0), at(t0, 50)).is_empty(), "a 9 pixel slide: still a tap");
        assert_eq!(t.ended(1, at(t0, 80)), vec![Out::Left(false)]);
        for k in [2, 3] {
            assert_eq!(t.start(k, (100.0, 100.0), at(t0, 100 * k), s).len(), 2);
            assert_eq!(t.ended(k, at(t0, 100 * k + 50)), vec![Out::Left(false)], "double tap: two clicks");
        }
        t.start(4, (100.0, 100.0), at(t0, 1000), s);
        assert_eq!(t.moved(4, (160.0, 100.0), at(t0, 1050)), vec![Out::Move((160.0, 100.0))], "a drag");
        assert_eq!(t.moved(4, (170.0, 100.0), at(t0, 1060)), vec![Out::Move((170.0, 100.0))]);
        assert_eq!(t.ended(4, at(t0, 1100)), vec![Out::Left(false)], "dropped where it ended");
    }

    /// Held on open ground a finger walks (not a click), and stops when let go; a second
    /// finger's tap jumps meanwhile. Somewhere it may not walk, holding is just a long press.
    #[test]
    fn holding_walks_and_a_second_finger_jumps() {
        let (t0, mut t) = (Instant::now(), Touch::new(true));
        t.start(1, (300.0, 200.0), t0, 20.0);
        t.walkable(true);
        assert!(t.tick(at(t0, 150)).is_empty(), "not yet");
        assert_eq!(t.tick(at(t0, 210)), vec![Out::Cancel, Out::Walk(true), Out::Move((300.0, 200.0))]);
        assert!(t.walking());
        assert_eq!(t.moved(1, (305.0, 200.0), at(t0, 250)), vec![Out::Move((305.0, 200.0))], "steering");
        assert!(t.start(2, (500.0, 300.0), at(t0, 300), 20.0).is_empty());
        assert_eq!(t.ended(2, at(t0, 350)), vec![Out::Jump]);
        assert_eq!(t.ended(1, at(t0, 600)), vec![Out::Walk(false)], "no click");
        t.start(3, (300.0, 200.0), at(t0, 1000), 20.0);
        t.walkable(false);
        assert!(t.tick(at(t0, 1500)).is_empty());
        assert_eq!(t.ended(3, at(t0, 1600)), vec![Out::Left(false)]);
    }

    /// Two fingers spreading zoom in (about the spread's change), a two finger tap is the right
    /// button's tap, and a touch the system takes back does nothing.
    #[test]
    fn pinches_two_finger_taps_and_cancelling() {
        let (t0, mut t) = (Instant::now(), Touch::new(true));
        t.start(1, (400.0, 400.0), t0, 20.0);
        assert_eq!(t.start(2, (500.0, 400.0), at(t0, 20), 20.0), vec![Out::Cancel], "the first's press let go");
        assert!(t.moved(2, (510.0, 400.0), at(t0, 40)).is_empty(), "within the slop");
        let z = t.moved(2, (650.0, 400.0), at(t0, 80));
        assert!(
            matches!(z[..], [Out::Move((525.0, 400.0)), Out::Zoom(s)] if (s - 4.1).abs() < 0.1),
            "2.5 × as far apart: about 4 rungs, about their middle {z:?}"
        );
        assert!(t.ended(2, at(t0, 120)).is_empty(), "a pinch isn't a tap");
        assert!(t.ended(1, at(t0, 130)).is_empty());
        t.start(3, (400.0, 400.0), at(t0, 500), 20.0);
        t.start(4, (450.0, 400.0), at(t0, 510), 20.0);
        assert_eq!(t.ended(4, at(t0, 560)), vec![Out::RightTap]);
        assert!(t.ended(3, at(t0, 570)).is_empty());
        t.start(5, (100.0, 100.0), at(t0, 900), 20.0);
        assert_eq!(t.cancelled(5), vec![Out::Cancel]);
        assert!(t.ended(5, at(t0, 950)).is_empty());
    }

    /// The system's mouse copy of a touch (same place, just after) is told apart; the mouse
    /// elsewhere, or later, isn't.
    #[test]
    fn the_systems_copy_of_a_touch_is_ignored() {
        let (t0, mut t) = (Instant::now(), Touch::new(true));
        assert!(!t.buttons_shown());
        t.start(1, (200.0, 150.0), t0, 20.0);
        assert!(t.buttons_shown(), "touch seen: the screen buttons");
        assert!(t.echo(at(t0, 100), (200.0, 150.0)));
        assert!(!t.echo(at(t0, 100), (260.0, 150.0)));
        assert!(!t.echo(at(t0, 600), (200.0, 150.0)));
        t.hide_buttons();
        assert!(!t.buttons_shown());
        let r = button_rects((640, 400));
        assert!(r.iter().all(|r| r[0] + r[2] <= 640 && r[1] >= 0 && r[1] + r[3] <= 400));
    }

    /// The screen buttons' words fit them (in the game's font 0).
    #[test]
    #[ignore]
    fn the_buttons_words_fit() {
        let Ok(dir) = std::env::var("U8_DATA") else { return };
        let f = u8data::find_file(std::path::Path::new(&dir), "STATIC/U8FONTS.FLX").unwrap();
        let fonts = u8data::font::Fonts::parse(&u8data::flex::Flex::parse(std::fs::read(f).unwrap()).unwrap());
        let w = button_rects((640, 400))[0][2] as u32;
        for (_, name) in BUTTONS {
            assert!(fonts.width(0, name) + 4 <= w, "{name}: {} of {w}", fonts.width(0, name));
        }
    }
}
