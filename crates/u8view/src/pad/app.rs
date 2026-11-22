//! The pad in the game: its buttons and sticks reaching the same places as the mouse and keys
//! (menus, the slider, books, replies, the settings, walking and the pointer).
use super::*;

impl crate::App {
    /// What the pad did since last frame: its buttons, and the pointer pushed by its stick.
    pub(crate) fn poll_pad(&mut self, el: &ActiveEventLoop) {
        let (events, dt) = self.pad.poll(self.focused);
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
                    self.pad.stop();
                    if self.owner == crate::Owner::Pad {
                        self.owner = crate::Owner::Mouse; // (the system's cursor back)
                    }
                }
            }
        }
        if self.pad.walk().is_some() {
            self.pad.pointed = false; // (walking: what the pointer was left at doesn't aim)
        }
        let s = self.pad.pointer_stick();
        if s != (0.0, 0.0) {
            let (w, h) = self.window_size();
            self.pad_point(move_pointer(self.mouse, s, dt, self.pad.speed, (w as f64, h as f64)));
            self.pad.pointed = true;
        }
    }

    /// The pad puts the pointer at `p` (window pixels).
    fn pad_point(&mut self, p: (f64, f64)) {
        self.owner = crate::Owner::Pad;
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
        self.touch.hide_buttons();
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
        if let Some(p) = &mut self.panel {
            // The settings: the D-pad moves and changes, A goes on round, the shoulders switch
            // tabs, B, Start and Back put it away.
            let k = match b {
                Btn::DUp => KeyCode::ArrowUp,
                Btn::DDown => KeyCode::ArrowDown,
                Btn::DLeft => KeyCode::ArrowLeft,
                Btn::DRight => KeyCode::ArrowRight,
                Btn::A => KeyCode::Enter,
                Btn::LB => KeyCode::PageUp,
                Btn::RB => KeyCode::PageDown,
                Btn::B | Btn::Start | Btn::Back => KeyCode::Escape,
                _ => return,
            };
            let out = p.key(k, false);
            return self.panel_out(out);
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
            Act::MouseLeft | Act::MouseRight | Act::MouseBoth => {
                // (What a swing, kick or jump from the pad aims by: see `aim`.)
                self.pad_aim = Some(aim(self.pad.walk().is_some(), self.pad.pointed));
                match act {
                    Act::MouseLeft => self.left_button(true),
                    Act::MouseRight => self.right_button(true),
                    _ if self.controlling() && !self.ui_blocking() => {
                        self.both_buttons();
                        self.mouse_jump = self.pad_aim != Some(Aim::Walk); // (walking: the way it walks)
                    }
                    _ => {}
                }
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
