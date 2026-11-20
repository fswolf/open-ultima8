//! Game controllers: one generic Xbox-style pad (the Steam Deck's and ROG Ally's built-in ones
//! show up as one on Linux; their back paddles aren't reported, so nothing uses them). The pad
//! drives what's already there: buttons are key [`Command`]s or the mouse's own buttons, the left
//! stick walks like the pointer's arrow (small tilt careful, middle walk, full run), the right
//! stick moves the pointer, which the mouse and the pad share (whichever moved it last owns it).
//! Every binding is in [`BINDS`].
use crate::input::{facing_of, Command};
use crate::{mouse, project, AVATAR};
use std::time::Instant;
use winit::{event_loop::ActiveEventLoop, keyboard::KeyCode};

/// The pad's buttons (Xbox names).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Btn {
    A,
    B,
    X,
    Y,
    LB,
    RB,
    LT,
    RT,
    Start,
    Back,
    L3,
    R3,
    DUp,
    DDown,
    DLeft,
    DRight,
}

/// What a button does in play.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Act {
    Key(Command),
    /// The mouse's left and right buttons, held as long as the pad's (tap: look, double tap:
    /// use, hold: drag; right: walk towards the pointer); both at once (a jump).
    MouseLeft,
    MouseRight,
    MouseBoth,
    /// Use what's under the pointer, in one press (a double click).
    Use,
    /// The pointer back onto the Avatar.
    Home,
    /// Walk this way on screen, like the arrow keys.
    Walk(i32, i32),
}

/// What a button does in the menus, the slider and pages: accept, go back, move the choice.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Nav {
    Accept,
    Back,
    Step(i32),
}

/// Every binding: in play, and in menus.
pub const BINDS: [(Btn, Act, Option<Nav>); 16] = {
    use {Act::*, Btn::*, Command as C};
    [
        (A, MouseLeft, Some(Nav::Accept)),
        (B, Key(C::Close), Some(Nav::Back)),
        (X, Use, None),
        (Y, Key(C::Backpack), None),
        (LB, Key(C::Paperdoll), None),
        (RB, Key(C::Combat), None),
        (LT, MouseBoth, None),
        (RT, MouseRight, None),
        (Start, Key(C::Menu), Some(Nav::Back)),
        (Back, Key(C::Diary), Some(Nav::Back)),
        (L3, Key(C::Recentre), None),
        (R3, Home, None),
        (DUp, Walk(0, -1), Some(Nav::Step(-1))),
        (DDown, Walk(0, 1), Some(Nav::Step(1))),
        (DLeft, Walk(-1, 0), Some(Nav::Step(-1))),
        (DRight, Walk(1, 0), Some(Nav::Step(1))),
    ]
};

pub fn bind(b: Btn) -> (Act, Option<Nav>) {
    BINDS.iter().find(|x| x.0 == b).map(|x| (x.1, x.2)).expect("every button is bound")
}

/// (facing, run, careful), as the pointer's arrow gives it.
pub type Walk = (usize, bool, bool);

/// A stick's tilt with a round dead zone `dz`, rescaled so it starts from nothing at its edge.
pub fn deadzone((x, y): (f32, f32), dz: f32) -> (f32, f32) {
    let m = x.hypot(y);
    if m <= dz || m == 0.0 {
        return (0.0, 0.0);
    }
    let k = ((m - dz) / (1.0 - dz)).min(1.0) / m;
    (x * k, y * k)
}

/// Tilt (past the dead zone) up to which a stick steps carefully, and walks; beyond, it runs.
const CAREFUL: f32 = 0.35;
const WALKING: f32 = 0.85;

/// Walking from a stick (screen axes, y down, dead zone taken off): eight ways, as the arrow keys.
pub fn stick_walk((x, y): (f32, f32)) -> Option<Walk> {
    let m = x.hypot(y);
    if m == 0.0 {
        return None;
    }
    let sector = ((y.atan2(x).to_degrees() + 360.0 + 22.5) / 45.0) as i32 % 8; // 0 right, clockwise
    let (ax, ay) = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)][sector as usize];
    Some((facing_of((ax, ay))?, m >= WALKING, m < CAREFUL))
}

/// Pointer speed at full tilt (window pixels a second) at the speed setting 1.
const POINTER_SPEED: f64 = 3000.0;

/// The pointer `dt` seconds on, pushed by a stick (dead zone taken off; faster the further it
/// leans), kept in a `w`×`h` window.
pub fn move_pointer(at: (f64, f64), (x, y): (f32, f32), dt: f32, speed: f32, (w, h): (f64, f64)) -> (f64, f64) {
    let v = POINTER_SPEED * speed as f64 * x.hypot(y) as f64 * dt as f64;
    ((at.0 + x as f64 * v).clamp(0.0, (w - 1.0).max(0.0)), (at.1 + y as f64 * v).clamp(0.0, (h - 1.0).max(0.0)))
}

/// Which way to walk: the pad's sticks or D-pad, else the pointer's arrow, but only while the
/// right button (or RT) is held, so a pointer left lying somewhere doesn't walk anyone.
pub fn steer(pad: Option<Walk>, pointer: Option<Walk>, held: bool) -> Option<Walk> {
    pad.or(pointer.filter(|_| held))
}

/// The pad's state as the viewer sees it, and its settings.
pub struct Pad {
    /// Settings: in use; pointer speed (0.1–1); dead zone; the right stick walks instead.
    pub on: bool,
    pub speed: f32,
    pub deadzone: f32,
    pub swap: bool,
    /// The sticks (screen axes, y down, raw) and the D-pad's walking keys held.
    sticks: [(f32, f32); 2],
    dpad: Vec<(i32, i32)>,
    /// Buttons whose press went to play (their release goes there too).
    pub held: Vec<Btn>,
    /// The pad moved the pointer last (the game draws its cursor; the system one is hidden).
    pub owns: bool,
    last: Option<Instant>,
    #[cfg(feature = "gamepad")]
    gilrs: Option<(gilrs::Gilrs, Option<gilrs::GamepadId>)>,
}

impl Default for Pad {
    fn default() -> Self {
        Self {
            on: true,
            speed: 0.5,
            deadzone: 0.2,
            swap: false,
            sticks: [(0.0, 0.0); 2],
            dpad: vec![],
            held: vec![],
            owns: false,
            last: None,
            #[cfg(feature = "gamepad")]
            gilrs: None,
        }
    }
}

/// What the pad did since last asked.
#[cfg_attr(not(feature = "gamepad"), allow(dead_code))]
pub enum Event {
    Press(Btn),
    Release(Btn),
    /// Unplugged: let go of everything.
    Gone,
}

impl Pad {
    /// Start listening for pads (none: the viewer goes on without).
    pub fn start(&mut self) {
        #[cfg(feature = "gamepad")]
        {
            self.gilrs = match gilrs::Gilrs::new() {
                Ok(g) => Some((g, None)),
                Err(e) => {
                    eprintln!("no game controllers: {e}");
                    None
                }
            };
        }
    }

    /// Is a pad plugged in (so the viewer looks for its input often)?
    pub fn connected(&self) -> bool {
        #[cfg(feature = "gamepad")]
        if let Some((g, _)) = &self.gilrs {
            return self.on && g.gamepads().any(|(_, p)| p.is_connected());
        }
        false
    }

    /// Button presses and plugging since last time, and the sticks as they are now; seconds since.
    pub fn poll(&mut self) -> (Vec<Event>, f32) {
        let now = Instant::now();
        let dt = self.last.map_or(0.0, |t| (now - t).as_secs_f32()).min(0.1);
        self.last = Some(now);
        #[allow(unused_mut)]
        let mut out = vec![];
        #[cfg(feature = "gamepad")]
        if let Some((g, active)) = &mut self.gilrs {
            use gilrs::{Axis, Button as B, EventType as E};
            while let Some(gilrs::Event { id, event, .. }) = g.next_event() {
                let btn = |b: B| {
                    Some(match b {
                        B::South => Btn::A,
                        B::East => Btn::B,
                        B::West => Btn::X,
                        B::North => Btn::Y,
                        B::LeftTrigger => Btn::LB,
                        B::RightTrigger => Btn::RB,
                        B::LeftTrigger2 => Btn::LT,
                        B::RightTrigger2 => Btn::RT,
                        B::Start => Btn::Start,
                        B::Select => Btn::Back,
                        B::LeftThumb => Btn::L3,
                        B::RightThumb => Btn::R3,
                        B::DPadUp => Btn::DUp,
                        B::DPadDown => Btn::DDown,
                        B::DPadLeft => Btn::DLeft,
                        B::DPadRight => Btn::DRight,
                        _ => return None,
                    })
                };
                match event {
                    E::ButtonPressed(b, _) if self.on => {
                        *active = Some(id);
                        out.extend(btn(b).map(Event::Press));
                    }
                    E::ButtonReleased(b, _) => out.extend(btn(b).map(Event::Release)),
                    E::Disconnected if *active == Some(id) => {
                        *active = None;
                        out.push(Event::Gone);
                    }
                    E::Connected | E::AxisChanged(..) if self.on && active.is_none() => *active = Some(id),
                    _ => {}
                }
            }
            self.sticks = [(0.0, 0.0); 2];
            if let Some(p) = active.filter(|_| self.on).and_then(|id| g.connected_gamepad(id)) {
                let axis = |x, y| (p.value(x), -p.value(y)); // (gilrs: up is +)
                self.sticks = [axis(Axis::LeftStickX, Axis::LeftStickY), axis(Axis::RightStickX, Axis::RightStickY)];
            }
        }
        if out.iter().any(|e| matches!(e, Event::Gone)) {
            (self.sticks, self.dpad) = ([(0.0, 0.0); 2], vec![]);
        }
        (out, dt)
    }

    /// The walking stick and the pointer's, dead zones taken off.
    fn stick(&self, pointer: bool) -> (f32, f32) {
        deadzone(self.sticks[(pointer != self.swap) as usize], self.deadzone)
    }
    pub fn pointer_stick(&self) -> (f32, f32) {
        self.stick(true)
    }

    /// Walking from the stick, else the D-pad (at a walk).
    pub fn walk(&self) -> Option<Walk> {
        let (x, y) = self.dpad.iter().fold((0, 0), |a, d| (a.0 + d.0, a.1 + d.1));
        stick_walk(self.stick(false)).or_else(|| Some((facing_of((x.signum(), y.signum()))?, false, false)))
    }

    /// Stop walking with the D-pad (a menu opened over the game).
    pub fn stop(&mut self) {
        self.dpad.clear();
    }

    /// The D-pad walks (`down`) or stops walking this way.
    pub fn dpad(&mut self, way: (i32, i32), down: bool) {
        self.dpad.retain(|&d| d != way);
        if down {
            self.dpad.push(way);
        }
    }

    /// Is the pad doing anything that needs frame after frame (walking, moving the pointer)?
    pub fn active(&self) -> bool {
        self.walk().is_some() || self.pointer_stick() != (0.0, 0.0)
    }
}

impl crate::App {
    /// What the pad did since last frame: its buttons, and the pointer pushed by its stick.
    pub(super) fn poll_pad(&mut self, el: &ActiveEventLoop) {
        let (events, dt) = self.pad.poll();
        for e in events {
            match e {
                Event::Press(b) => self.pad_press(b, el),
                Event::Release(b) => {
                    if let Some(i) = self.pad.held.iter().position(|&h| h == b) {
                        self.pad.held.remove(i);
                        self.pad_let_go(b);
                    }
                }
                Event::Gone => {
                    std::mem::take(&mut self.pad.held).into_iter().for_each(|b| self.pad_let_go(b));
                    self.pad.owns = false; // (the system's cursor back)
                }
            }
        }
        let s = self.pad.pointer_stick();
        if s != (0.0, 0.0) {
            let (w, h) = self.window_size();
            self.pad_point(move_pointer(self.mouse, s, dt, self.pad.speed, (w as f64, h as f64)));
        }
    }

    /// The pad puts the pointer at `p` (window pixels).
    fn pad_point(&mut self, p: (f64, f64)) {
        self.pad.owns = true;
        self.moved_to(p);
        self.dirty = true;
    }

    /// A button let go whose press went to play. Over a menu since, the buttons are only let go
    /// of, as the mouse's are there.
    fn pad_let_go(&mut self, b: Btn) {
        match bind(b).0 {
            Act::MouseLeft | Act::MouseRight if self.menu_open() => (self.left, self.right) = (false, false),
            Act::MouseLeft => self.left_button(false),
            Act::MouseRight => self.right_button(false),
            Act::Walk(x, y) => self.pad.dpad((x, y), false),
            _ => {}
        }
    }

    fn pad_press(&mut self, b: Btn, el: &ActiveEventLoop) {
        let (act, nav) = bind(b);
        self.dirty = true;
        if self.film.is_some() {
            return self.end_film(); // skipped
        }
        if self.roll.is_some() {
            if nav == Some(Nav::Back) {
                self.end_roll();
            }
            return;
        }
        if let Some(s) = &mut self.slider {
            let k = match nav {
                Some(Nav::Accept) => KeyCode::Enter,
                Some(Nav::Back) => KeyCode::Escape,
                Some(Nav::Step(d)) if d < 0 => KeyCode::ArrowLeft,
                Some(Nav::Step(_)) => KeyCode::ArrowRight,
                None => return,
            };
            if let Some(a) = s.key(k) {
                self.answer_slider(a);
            }
            return;
        }
        if self.menu_open() {
            if let Some(n) = nav {
                self.pad_menu(n, el);
            }
            return;
        }
        if let Act::Walk(x, y) = act {
            // Pages turn; replies are pointed at; else it walks.
            if self.ui.as_ref().is_some_and(|u| u.reading()) {
                if x != 0 {
                    self.read_key(if x < 0 { KeyCode::ArrowLeft } else { KeyCode::ArrowRight });
                }
                return;
            }
            if let Some(r) = self.ui.as_ref().filter(|u| u.asking()).and_then(|u| u.answer_step(x + y)) {
                let s = self.view.scale as f64;
                return self
                    .pad_point(((r[0] + r[2] / 2) as f64 * s + s / 2.0, (r[1] + r[3] / 2) as f64 * s + s / 2.0));
            }
            self.pad.dpad((x, y), true);
        }
        self.pad.held.push(b);
        match act {
            Act::Key(c) => {
                self.command(c);
                self.keep_settings();
            }
            Act::MouseLeft => self.left_button(true),
            Act::MouseRight => self.right_button(true),
            Act::MouseBoth if self.controlling() && !self.ui_blocking() => {
                self.both_buttons();
                self.mouse_jump = self.pad.walk().is_none(); // (walking: jump the way it walks)
            }
            Act::Use if !self.ui_blocking() => {
                let id = self.thing_under_mouse();
                match (self.targeting.is_some(), id) {
                    (true, _) => self.end_targeting(id.unwrap_or(0)),
                    (false, Some(id)) => self.use_thing(id),
                    _ => {}
                }
            }
            Act::Home => {
                let Some(a) = self.game.world.get(AVATAR) else { return };
                let (fx, fy) = project(a.x, a.y, a.z + mouse::FEET_UP);
                let (((ox, oy), _), z) = (self.world_view(), self.view.zoom);
                self.pad_point((((fx as f32 - ox) * z) as f64, ((fy as f32 - oy) * z) as f64));
            }
            _ => {}
        }
    }

    /// The pad in the book: move the choice, take it, go back. Naming needs typing, so the pad
    /// gets a name ready (the place and the day, for a save) that A then takes.
    fn pad_menu(&mut self, n: Nav, el: &ActiveEventLoop) {
        let save = format!("{}, day {}", crate::presence::place(self.game.map()), self.game.minutes() / 1440 + 1);
        let Some(m) = self.menu.as_mut() else { return };
        let action = match n {
            Nav::Step(d) => {
                m.step(d);
                None
            }
            Nav::Back => m.key(KeyCode::Escape, None),
            Nav::Accept if m.typing() => match m.suggest(&save) {
                true => None,
                false => m.key(KeyCode::Enter, None),
            },
            Nav::Accept => {
                let a = m.accept();
                m.suggest(&save);
                a
            }
        };
        if let Some(a) = action {
            self.menu_action(a, el);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each way on screen walks as the arrow keys do; the further the tilt, the faster.
    #[test]
    fn sticks_walk_eight_ways_at_three_paces() {
        let ways = [
            ((0, -1), 7),
            ((1, -1), 0),
            ((1, 0), 1),
            ((1, 1), 2),
            ((0, 1), 3),
            ((-1, 1), 4),
            ((-1, 0), 5),
            ((-1, -1), 6),
        ];
        for ((ax, ay), facing) in ways {
            assert_eq!(facing_of((ax, ay)), Some(facing), "the arrow keys agree");
            let (x, y) = (ax as f32, ay as f32);
            for (tilt, run, careful) in [(0.2, false, true), (0.6, false, false), (1.0, true, false)] {
                let k = tilt / x.hypot(y);
                assert_eq!(stick_walk((x * k, y * k)), Some((facing, run, careful)), "{ax},{ay} at {tilt}");
            }
        }
        // (A little off the axis still goes along it.)
        assert_eq!(stick_walk((0.95, 0.2)).map(|w| w.0), Some(1));
        assert_eq!(stick_walk((0.0, 0.0)), None);
    }

    #[test]
    fn the_dead_zone_is_round_and_starts_from_nothing() {
        assert_eq!(deadzone((0.15, 0.1), 0.2), (0.0, 0.0), "inside: nothing");
        assert_eq!(deadzone((0.14, 0.14), 0.2), (0.0, 0.0), "round, not square (0.198 out)");
        let (x, y) = deadzone((0.21, 0.0), 0.2);
        assert!(x > 0.0 && x < 0.02 && y == 0.0, "just past it: barely ({x})");
        assert_eq!(deadzone((1.0, 0.0), 0.2), (1.0, 0.0), "all the way: all of it");
        let (x, y) = deadzone((0.6, 0.6), 0.2);
        assert!((x - y).abs() < 1e-6 && x.hypot(y) > 0.6, "keeps its direction ({x}, {y})");
        assert!(deadzone((1.0, 1.0), 0.2).0.hypot(deadzone((1.0, 1.0), 0.2).1) <= 1.0001, "never past full");
    }

    #[test]
    fn the_pointer_moves_faster_the_further_and_stays_in_the_window() {
        let w = (1280.0, 800.0);
        let (a, _) = move_pointer((640.0, 400.0), (0.3, 0.0), 0.1, 0.5, w);
        let (b, _) = move_pointer((640.0, 400.0), (0.6, 0.0), 0.1, 0.5, w);
        assert!(a > 640.0 && b - 640.0 > 2.0 * (a - 640.0), "more than twice as far for twice the tilt");
        assert_eq!(move_pointer((640.0, 400.0), (0.0, 0.0), 0.1, 0.5, w), (640.0, 400.0), "still");
        assert_eq!(move_pointer((10.0, 790.0), (-1.0, 1.0), 1.0, 1.0, w), (0.0, 799.0), "clamped");
        assert_eq!(move_pointer((1270.0, 5.0), (1.0, -1.0), 1.0, 1.0, w), (1279.0, 0.0));
    }

    /// Every button does what the layout says, in play and in menus.
    #[test]
    fn every_button_is_bound() {
        use {Act::*, Btn::*, Command as C};
        let play = [
            (A, MouseLeft),
            (B, Key(C::Close)),
            (X, Use),
            (Y, Key(C::Backpack)),
            (LB, Key(C::Paperdoll)),
            (RB, Key(C::Combat)),
            (LT, MouseBoth),
            (RT, MouseRight),
            (Start, Key(C::Menu)),
            (Back, Key(C::Diary)),
            (L3, Key(C::Recentre)),
            (R3, Home),
            (DUp, Walk(0, -1)),
            (DDown, Walk(0, 1)),
            (DLeft, Walk(-1, 0)),
            (DRight, Walk(1, 0)),
        ];
        for (b, a) in play {
            assert_eq!(bind(b).0, a, "{b:?}");
        }
        assert_eq!(bind(A).1, Some(Nav::Accept));
        for b in [B, Start, Back] {
            assert_eq!(bind(b).1, Some(Nav::Back), "{b:?}");
        }
        assert_eq!((bind(DUp).1, bind(DDown).1), (Some(Nav::Step(-1)), Some(Nav::Step(1))));
        let mut bs: Vec<_> = BINDS.iter().map(|b| format!("{:?}", b.0)).collect();
        bs.dedup();
        assert_eq!(bs.len(), BINDS.len(), "each button once");
    }

    /// A pointer the pad left lying about doesn't walk the Avatar; RT held, it does; the sticks
    /// and the D-pad walk whatever the pointer does.
    #[test]
    fn a_parked_pointer_doesnt_walk() {
        let aim = Some((3, true, false));
        assert_eq!(steer(None, aim, false), None);
        assert_eq!(steer(None, aim, true), aim);
        let mut pad = Pad::default();
        assert_eq!(pad.walk(), None);
        assert_eq!(steer(pad.walk(), aim, false), None, "nothing held, nothing tilted");
        pad.dpad((1, 0), true);
        pad.dpad((0, -1), true);
        assert_eq!(steer(pad.walk(), aim, true), Some((0, false, false)), "up and right: north-east, at a walk");
        pad.dpad((1, 0), false);
        assert_eq!(pad.walk(), Some((7, false, false)));
        pad.sticks = [(0.0, 0.5), (0.0, 0.0)];
        assert_eq!(pad.walk().map(|w| w.0), Some(3), "the stick before the D-pad");
        pad.swap = true;
        assert_eq!(pad.walk().map(|w| w.0), Some(7), "swapped: the right stick walks");
        assert_ne!(pad.pointer_stick(), (0.0, 0.0), "and the left moves the pointer");
    }
}
