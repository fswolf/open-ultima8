//! The mouse, as the original uses it: where the pointer aims, clicks that look and use, both
//! buttons to jump, dragging things between windows and the map, and the target cursor.
use super::*;
use crate::slider::{Answer, Dest, Slider};

impl App {
    /// Where the pointer aims from the Avatar, while the player controls it.
    pub(super) fn pointer(&self) -> Option<(usize, mouse::Reach)> {
        if !self.controlling() || self.menu_open() {
            return None;
        }
        // From the Avatar's feet, as the original measures it (ScummVM: 14 below the view's
        // centre, which is the camera's point 20 up); from mid-body the aim was off.
        let a = self.game.world.get(AVATAR)?;
        let (fx, fy) = u8render::project(a.x, a.y, a.z + mouse::FEET_UP);
        let ((mx, my), (_, (w, h))) = (self.mouse_world(), self.world_view());
        Some(mouse::aim(mx - fx, my - fy, (w as u32, h as u32)))
    }

    /// Movement: (facing, run, careful) from the pad's sticks or D-pad, else from the pointer's
    /// arrow while the right button (or the pad's RT) walks.
    pub(super) fn steering(&mut self) -> Option<pad::Walk> {
        let tap = self.step_until.is_some_and(|t| Instant::now() < t);
        if !tap {
            self.step_until = None;
        }
        let aim = self.pointer().map(|(s, r)| (mouse::facing(s), r == mouse::Reach::Long, r == mouse::Reach::Short));
        pad::steer(self.pad.walk(), aim, self.right || tap || self.touch.walking())
    }

    /// Single click: look at the item under the mouse; double click: use it (doors, talking…).
    pub(super) fn click(&mut self) {
        let ((gx, gy), (wx, wy), opts) = (self.mouse_game(), self.mouse_world(), self.view.opts);
        let (Some(ui), Some(scripts)) = (&mut self.ui, &mut self.game.scripts) else { return };
        if ui.click(gx, gy, &mut scripts.vm) {
            self.dirty = true;
            return;
        }
        let in_window = self.art.as_ref().and_then(|a| self.windows.hit(&self.game.data, a, &self.game.world, gx, gy));
        let picked = match in_window {
            Some(gumps::Hit::Item(id)) => Some(id),
            Some(gumps::Hit::Window(_) | gumps::Hit::Gauge) => return,
            None => self.painter.pick(&self.game.data, &mut self.game.world, wx, wy, opts),
        };
        let Some(id) = picked else {
            let skipped = ui.skip_barks(&mut scripts.vm);
            self.dirty |= skipped;
            #[cfg(feature = "audio")]
            if let (true, Some(a)) = (skipped, &mut self.audio) {
                a.stop_speech();
            }
            return;
        };
        let now = Instant::now();
        let double = self.last_click.is_some_and(|(i, t)| i == id && now - t < DOUBLE_CLICK);
        // A single click during speech just skips it (but still counts towards a double click).
        if !double && ui.skip_barks(&mut scripts.vm) {
            #[cfg(feature = "audio")]
            if let Some(a) = &mut self.audio {
                a.stop_speech();
            }
            self.last_click = Some((id, now));
            self.dirty = true;
            return;
        }
        if double {
            self.use_thing(id);
        } else {
            self.pending_look = Some((id, now + DOUBLE_CLICK));
        }
        self.last_click = if double { None } else { Some((id, now)) };
        self.dirty = true;
    }

    /// Use `id` (a double click, the pad's X): open, talk, search a body; out of reach, the red
    /// cross for a moment.
    pub(super) fn use_thing(&mut self, id: ObjId) {
        self.pending_look = None;
        if self.game.can_use(id) {
            self.game.use_item(id);
        } else {
            self.cross_until = Some(Instant::now() + CROSS_FLASH);
        }
        self.dirty = true;
    }

    pub(super) fn left_button(&mut self, down: bool) {
        self.left = down;
        if !down {
            (self.win_drag, self.swung) = (None, None);
            self.dirty |= std::mem::take(&mut self.stats_pressed);
        }
        if down && self.targeting.is_some() {
            let id = self.thing_under_mouse();
            self.end_targeting(id.unwrap_or(0));
            self.left_jumped = true; // (the release isn't a click)
            return;
        }
        if down && self.window_press() {
            return;
        }
        if down {
            if self.right && self.controlling() {
                self.both_buttons();
            } else if self.controlling() && self.game.in_combat() && !self.over_window() {
                self.attack = true; // in combat stance a left click swings (held: blocks)
                self.swung = Some(Instant::now());
                self.left_jumped = true; // and isn't a click
            } else {
                self.press = Some(self.mouse);
                self.press_item = self.item_under_mouse();
                self.dragging = false;
            }
            return;
        }
        if let Some(id) = self.item_drag.take() {
            self.drop_item(id);
        } else if !std::mem::take(&mut self.left_jumped) && self.press.is_some() && !self.dragging {
            self.click();
        }
        self.left_jumped = false;
        (self.press, self.press_item, self.dragging) = (None, None, false);
    }

    /// What's under the pointer: in a window, else on the map.
    pub(super) fn thing_under_mouse(&mut self) -> Option<ObjId> {
        let ((gx, gy), (wx, wy), opts) = (self.mouse_game(), self.mouse_world(), self.view.opts);
        let art = self.art.as_ref()?;
        match self.windows.hit(&self.game.data, art, &self.game.world, gx, gy) {
            Some(gumps::Hit::Item(id)) => Some(id),
            Some(gumps::Hit::Window(_) | gumps::Hit::Gauge) => None,
            None => self.painter.pick(&self.game.data, &mut self.game.world, wx, wy, opts),
        }
    }

    /// The player picked `id` with the targeting cursor (0: declined).
    pub(super) fn end_targeting(&mut self, id: ObjId) {
        if let (Some(pid), Some(s)) = (self.targeting.take(), &mut self.game.scripts) {
            s.vm.finish(pid, id as u32);
        }
        self.dirty = true;
    }

    /// Is the pointer over an open window (so a click there is about the window, not the map)?
    pub(super) fn over_window(&self) -> bool {
        let (gx, gy) = self.mouse_game();
        self.art.as_ref().is_some_and(|art| self.windows.hit(&self.game.data, art, &self.game.world, gx, gy).is_some())
    }

    /// The item a left press would pick up: one in an open window, or on the map while the
    /// player controls the Avatar.
    pub(super) fn item_under_mouse(&mut self) -> Option<ObjId> {
        let (gx, gy) = self.mouse_game();
        let art = self.art.as_ref()?;
        if let Some(gumps::Hit::Item(id)) = self.windows.hit(&self.game.data, art, &self.game.world, gx, gy) {
            return Some(id);
        }
        if !self.controlling() {
            return None;
        }
        let ((wx, wy), opts) = (self.mouse_world(), self.view.opts);
        let id = self.painter.pick(&self.game.data, &mut self.game.world, wx, wy, opts)?;
        let e = self.game.world.entry(self.game.world.index(id)?);
        (!e.traits.actor).then_some(id)
    }

    /// Put down the item carried by the mouse: into the window (or the bag in a window) under it,
    /// else on whatever's under it on the map.
    pub(super) fn drop_item(&mut self, id: ObjId) {
        self.dirty = true;
        let dest = match self.pile_under_mouse(id) {
            // Onto a pile it goes with (coins on coins): one pile.
            Some(pile) => Dest::Pile(pile),
            None => {
                let (gx, gy) = self.mouse_game();
                let Some(art) = &self.art else { return };
                let (gd, world) = (&self.game.data, &self.game.world);
                let wearable = world.get(id).is_some_and(|it| u8engine::equip::Slot::of(gd, it.shape).is_some());
                match self.windows.drop_target(gd, art, world, gx, gy, id) {
                    Some(gumps::Drop::Into(container, at)) => Dest::To(u8engine::Place::Into { container, at }),
                    Some(gumps::Drop::Wear) if wearable => {
                        if let Err(why) = self.game.equip(id) {
                            self.notice(why);
                        }
                        return;
                    }
                    Some(gumps::Drop::Wear) => Dest::Stow,
                    Some(gumps::Drop::Nowhere) => return self.notice("It won't go there."),
                    None => match self.map_drop(id, self.mouse_world()) {
                        Some(to) => Dest::To(to),
                        None => return self.notice("It won't go there."),
                    },
                }
            }
        };
        // Part of a pile? The original asks how many, once who saw has been told.
        match self.game.world.get(id).filter(|_| self.game.is_pile(id)) {
            Some(it) => {
                self.game.let_go(id);
                self.slider = Some(Slider::new(id, it.quality, dest));
            }
            None => self.put(id, None, dest),
        }
    }

    /// Move `id` (or `n` of the pile) to `dest`, saying why not if it can't.
    pub(super) fn put(&mut self, id: ObjId, n: Option<u16>, dest: Dest) {
        let g = &mut self.game;
        let result = match (dest, n) {
            (Dest::Pile(p), None) => g.merge(id, p),
            (Dest::Pile(p), Some(n)) => g.split_onto(id, n, p),
            (Dest::To(to), None) => g.move_item(id, to),
            (Dest::To(to), Some(n)) => g.split_to(id, n, to),
            (Dest::Stow, None) => g.stow(id),
            (Dest::Stow, Some(n)) => match g.backpack() {
                Some(pack) => g.split_to(id, n, u8engine::Place::Into { container: pack, at: gumps::STOW_AT }),
                None => Err("You have no backpack."),
            },
            (Dest::Script(_), _) => Ok(()),
        };
        if let Err(why) = result {
            self.notice(why);
        }
        self.dirty = true;
    }

    /// The player answered the slider.
    pub(super) fn answer_slider(&mut self, a: Answer) {
        let Some(s) = self.slider.take() else { return };
        match (s.dest, a) {
            // (A script gets what's shown either way.)
            (Dest::Script(pid), _) => _ = self.game.vm().map(|vm| vm.finish(pid, s.amount() as i16 as u32)),
            (_, Answer::Take(n)) => self.put(s.item, Some(n), s.dest),
            _ => {}
        }
        self.dirty = true;
    }

    /// The pile `id` would be dropped on (in a window, or on the map), if it merges with it.
    fn pile_under_mouse(&mut self, id: ObjId) -> Option<ObjId> {
        let ((gx, gy), (wx, wy), opts) = (self.mouse_game(), self.mouse_world(), self.view.opts);
        let art = self.art.as_ref()?;
        let under = match self.windows.hit(&self.game.data, art, &self.game.world, gx, gy) {
            Some(gumps::Hit::Item(t)) => Some(t),
            Some(gumps::Hit::Window(_) | gumps::Hit::Gauge) => None,
            None => {
                let all = self.painter.pick_all(&self.game.data, &mut self.game.world, wx, wy, true, opts);
                all.into_iter().find(|&o| o != id)
            }
        };
        // (A pile out of reach: what's dropped there is thrown instead.)
        under.filter(|&t| self.game.can_merge(id, t) && self.game.within_reach(t))
    }

    /// Where an item dropped on the map at screen point (sx, sy) goes: into the bag or chest
    /// drawn there, else onto the topmost surface drawn there, centred under the pointer.
    pub(super) fn map_drop(&mut self, id: ObjId, (sx, sy): (i32, i32)) -> Option<u8engine::Place> {
        let opts = self.view.opts;
        let under = self.painter.pick_all(&self.game.data, &mut self.game.world, sx, sy, true, opts);
        let w = &self.game.world;
        // It lands on what's drawn under the pointer, if that's a floor, a table or the like
        // ("land"): not a wall, door or person, nor anything hidden behind one.
        let top = under.into_iter().find(|&o| o != id)?;
        if w.index(top).is_some_and(|i| !w.entry(i).traits.actor) && u8engine::equip::holds(&self.game.data, w, top) {
            return Some(u8engine::Place::Into { container: top, at: gumps::STOW_AT });
        }
        let below = Some(top).filter(|&o| w.index(o).is_some_and(|i| w.entry(i).traits.land))?;
        let z = w.entry(w.index(below)?).z1;
        // Screen to world at height z (sx = x/4 - y/4, sy = x/8 + y/8 - z; to within a unit or two).
        let (sum, diff) = (8 * (sy + z), 4 * sx);
        let it = w.get(id)?;
        let flipped = it.flags & u8engine::world::item_flag::FLIPPED != 0;
        let (dx, dy, _) = self.game.data.typeflags.get(it.shape).footpad(flipped);
        Some(u8engine::Place::Map(((sum + diff) / 2 + dx / 2, (sum - diff) / 2 + dy / 2, z)))
    }

    /// A left press on a window's frame: bring it forward and drag it, or (double-click) use what
    /// it belongs to (which closes it).
    /// The paperdoll's stats button shows the health and mana gauge; the gauge drags, and a
    /// double click on it opens the paperdoll in its place.
    pub(super) fn window_press(&mut self) -> bool {
        let (gx, gy) = self.mouse_game();
        let Some(art) = &self.art else { return false };
        let item = match self.windows.hit(&self.game.data, art, &self.game.world, gx, gy) {
            Some(gumps::Hit::Window(i)) if self.windows.on_stats_button(i, gx, gy) => {
                self.windows.show_gauge(art, self.view_size());
                (self.stats_pressed, self.dirty) = (true, true);
                return true;
            }
            Some(gumps::Hit::Window(i)) => {
                let i = self.windows.raise(i);
                self.windows.item_of(i).unwrap_or(0)
            }
            Some(gumps::Hit::Gauge) => gumps::GAUGE_ID,
            _ => return false,
        };
        let now = Instant::now();
        if self.last_win_click.is_some_and(|(w, t)| w == item && now - t < DOUBLE_CLICK) {
            if item == gumps::GAUGE_ID {
                self.windows.gauge = None;
                if !self.windows.items().contains(&AVATAR) {
                    self.game.trigger(AVATAR, event::USE); // the Avatar's script opens its paperdoll
                }
            } else {
                // Its owner is used (ScummVM's ContainerGump: a double click on the window uses
                // the container), so its script closes it as it opened it: a chest's lid shuts.
                self.game.use_item(item);
            }
            self.last_win_click = None;
        } else {
            self.last_win_click = Some((item, now));
            self.win_drag = Some((item, self.mouse));
        }
        self.dirty = true;
        true
    }

    /// Close an item's window (the game is told, so its scripts see it closed).
    pub(super) fn close_window(&mut self, item: ObjId) {
        self.windows.close(item);
        self.game.window_closed(item);
    }

    pub(super) fn close_windows(&mut self) -> bool {
        let items = self.windows.close_all();
        for &item in &items {
            self.game.window_closed(item);
        }
        !items.is_empty()
    }

    /// I: the Avatar's backpack (its own script opens the window).
    pub(super) fn open_backpack(&mut self) {
        match self.game.backpack() {
            Some(id) => {
                self.game.trigger(id, event::USE);
            }
            None => self.notice("No backpack."),
        }
    }

    pub(super) fn right_button(&mut self, down: bool) {
        self.right = down;
        if down && self.targeting.is_some() {
            self.end_targeting(0); // never mind
            return;
        }
        if down && self.controlling() && !self.ui_blocking() {
            let now = Instant::now();
            // (By its feet, or anywhere on its figure: the arrow's measured from the feet.)
            let (mx, my) = self.mouse_world();
            let on_figure = self
                .painter
                .rect(&self.game.world, AVATAR)
                .is_some_and(|r| (r[0]..r[0] + r[2]).contains(&mx) && (r[1]..r[1] + r[3]).contains(&my));
            let on_avatar = on_figure || self.pointer().is_some_and(|p| p.1 == mouse::Reach::Short);
            if self.left {
                self.both_buttons();
            } else if self.last_right.is_some_and(|t| now - t < DOUBLE_CLICK) {
                // Double right click: on the Avatar, weapon out or away; elsewhere in combat, a kick.
                if on_avatar {
                    self.combat = true;
                } else {
                    self.kick = true;
                }
                self.last_right = None;
                self.step_until = None;
            } else {
                self.step_until = Some(now + TAP_STEP);
                self.last_right = (on_avatar || self.game.in_combat()).then_some(now);
            }
        }
    }

    /// The pointer moved to `p` (window pixels), by the mouse or the pad's stick: a window being
    /// dragged follows, a press becomes a drag, highlights follow.
    pub(super) fn moved_to(&mut self, p: (f64, f64)) {
        if let Some((item, from)) = self.win_drag {
            let s = self.view.scale as f64;
            let (dx, dy) = (((p.0 - from.0) / s) as i32, ((p.1 - from.1) / s) as i32);
            if (dx, dy) != (0, 0) {
                self.windows.move_by(item, dx, dy);
                self.win_drag = Some((item, (from.0 + dx as f64 * s, from.1 + dy as f64 * s)));
                self.dirty = true;
            }
            self.mouse = p;
            return;
        }
        if let Some(d) = self.press {
            // A drag is never a click; dragging an item picks it up.
            self.dragging |= (p.0 - d.0).abs() + (p.1 - d.1).abs() > 6.0;
            if self.dragging && self.item_drag.is_none() {
                // Pick it up only if the Avatar can (else say why, once).
                if let Some(id) = self.press_item.take() {
                    match self.game.can_pick_up(id) {
                        Ok(()) => self.item_drag = Some(id),
                        Err(why) => self.notice(why),
                    }
                }
            }
        }
        if self.item_drag.is_some() {
            self.dirty = true; // the carried item follows the pointer
        }
        // Middle drag pans; with a free camera, so does left drag.
        let left_pan = self.press.filter(|_| self.dragging && !self.controlling() && self.item_drag.is_none());
        if let Some(d) = self.pan.or(left_pan) {
            self.view.follow = false; // panning detaches the camera
            self.view.cam.0 -= ((p.0 - d.0) / self.view.zoom as f64) as f32;
            self.view.cam.1 -= ((p.1 - d.1) / self.view.zoom as f64) as f32;
            if self.pan.is_some() {
                self.pan = Some(p);
            } else {
                self.press = Some(p);
            }
            self.dirty = true;
        }
        self.mouse = p;
        let (gx, gy) = self.mouse_game();
        if let Some(s) = &mut self.slider {
            self.dirty |= s.drag(gx);
        }
        if let Some(ui) = &mut self.ui {
            self.dirty |= ui.hover(gx, gy);
        }
        if let Some(m) = self.menu.as_mut().filter(|m| m.is_open()) {
            self.dirty |= m.hover(gx, gy);
        }
    }

    /// A mouse button (or a finger, or the pad, as one), wherever it goes: the slider, a film, the
    /// book, or the game.
    pub(super) fn mouse_button(&mut self, button: MouseButton, down: bool, el: &ActiveEventLoop) {
        if self.slider.is_some() {
            let left = matches!((button, self.swapped), (MouseButton::Left, false) | (MouseButton::Right, true));
            let at = self.mouse_game();
            let (Some(s), Some(art)) = (&mut self.slider, &self.art) else { return };
            let answer = match (left, down) {
                (true, true) => s.press(art, at),
                (false, true) => Some(crate::slider::Answer::Cancel),
                _ => {
                    s.release();
                    None
                }
            };
            if let Some(a) = answer {
                self.answer_slider(a);
            }
            if !down {
                (self.left, self.right) = (false, false);
            }
            self.dirty = true;
            return;
        }
        if self.film.is_some() {
            if down {
                self.end_film(); // skipped
            }
            return;
        }
        if self.menu_open() {
            if !down {
                (self.left, self.right) = (false, false); // pressed before the menu opened
            }
            let (gx, gy) = self.mouse_game();
            let Some(m) = self.menu.as_mut() else { return };
            let action = match (button, self.swapped, down) {
                (MouseButton::Left, false, true) | (MouseButton::Right, true, true) => m.click(gx, gy),
                (MouseButton::Right, false, true) | (MouseButton::Left, true, true) => {
                    m.back();
                    None
                }
                _ => None,
            };
            if let Some(a) = action {
                self.menu_action(a, el);
            }
            self.dirty = true;
            return;
        }
        match (button, self.swapped) {
            (MouseButton::Left, false) | (MouseButton::Right, true) => self.left_button(down),
            (MouseButton::Right, false) | (MouseButton::Left, true) => self.right_button(down),
            (MouseButton::Middle, _) => self.pan = down.then_some(self.mouse),
            _ => {}
        }
    }

    /// Let go of the left button without a click or a drop (a touch taken back, a finger that
    /// walks or pinches instead).
    pub(super) fn cancel_press(&mut self) {
        (self.press, self.press_item, self.item_drag, self.win_drag) = (None, None, None, None);
        (self.left, self.left_jumped, self.dragging, self.swung) = (false, false, false, None);
    }

    /// Both buttons: jump towards the pointer (and a left press held isn't a click).
    pub(super) fn both_buttons(&mut self) {
        self.jump = true;
        self.mouse_jump = true;
        self.left_jumped = self.left;
        self.press = None;
    }
}
