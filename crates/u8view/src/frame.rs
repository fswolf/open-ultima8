//! A frame's work before drawing (that's `draw`): advancing the game with the player's input,
//! passing script events to the UI and the speakers, Discord's status, the title, dying.
use super::*;

impl App {
    /// Advance the game to now. Sets `moving` / `scripting` for the frame pacing.
    pub(super) fn update(&mut self) {
        let now = Instant::now();
        self.zooming = self.ease_zoom(self.last_update.map_or(0.0, |t| (now - t).as_secs_f32()).min(0.1));
        if self.film.is_some() {
            self.tick_film(self.last_update.map_or(0.0, |t| (now - t).as_secs_f32()).min(0.1));
        }
        if let Some((r, _)) = &mut self.roll {
            let dt = self.last_update.map_or(0.0, |t| (now - t).as_secs_f32()).min(0.1);
            self.dirty = true;
            if r.tick(dt) {
                self.end_roll();
            }
        }
        if self.menu_open() {
            // The game waits while a menu is open.
            self.last_update = Some(now);
            (self.moving, self.scripting) = (false, false);
            if let Some(ui) = &mut self.ui {
                self.dirty |= ui.expire_notices();
            }
            self.show_presence();
            return;
        }
        let dt = self.last_update.map_or(0.0, |t| (now - t).as_secs_f32()).min(0.1);
        self.last_update = Some(now);
        let controlling = self.controlling();
        let (ax, ay) = self.held.axes();
        if !controlling && (ax, ay) != (0, 0) {
            let sp = SCROLL_SPEED * dt * if self.held.shift() { 3.0 } else { 1.0 };
            self.view.cam.0 += ax as f32 * sp;
            self.view.cam.1 += ay as f32 * sp;
            self.dirty = true;
        }
        let free = controlling && !self.ui_blocking();
        let walk = self.steering().filter(|_| free);
        let mut input = match (self.held.dir(), walk) {
            (Some(d), _) => Input { dir: Some(d), run: self.held.shift(), ..Default::default() },
            (None, Some((d, run, careful))) => Input { dir: Some(d), run, careful, ..Default::default() },
            _ => Input { run: self.held.shift(), ..Default::default() },
        };
        input.jump = std::mem::take(&mut self.jump) && free;
        input.combat = std::mem::take(&mut self.combat) && free;
        input.attack = std::mem::take(&mut self.attack) && free;
        input.kick = std::mem::take(&mut self.kick) && free;
        input.block = free && self.left && self.swung.is_some_and(|t| t.elapsed() > DOUBLE_CLICK);
        // (From the pad with nothing leaning and the pointer not moved since it walked: the way
        // the Avatar faces, not where the pointer was left.)
        let facing = self.pad_aim == Some(pad::Aim::Facing);
        if (input.attack || input.kick || input.block) && input.dir.is_none() && !facing {
            // Standing, strikes and blocks turn to the pointer first.
            let aim = self.pointer().filter(|p| p.1 != mouse::Reach::Short).map(|p| mouse::facing(p.0));
            if let (Some(d), Some(a)) = (aim, self.game.avatar.as_mut()) {
                if !a.busy() && !a.blocking() {
                    a.dir = d;
                }
            }
        }
        if input.jump && std::mem::take(&mut self.mouse_jump) {
            // Short arrow: straight up; medium: forward; long while running: a running leap.
            let aim = self.pointer().filter(|p| p.1 != mouse::Reach::Short);
            input.dir = aim.map(|p| mouse::facing(p.0));
            input.run = aim.is_some_and(|p| p.1 == mouse::Reach::Long);
            input.careful = false;
            if facing {
                (input.dir, input.run) = (self.game.avatar.as_ref().map(|a| a.dir), false);
            }
        }
        if self.pending_look.is_some_and(|(_, t)| now >= t) {
            let (id, _) = self.pending_look.take().unwrap();
            if self.game.world.get(id).is_some() {
                self.game.trigger(id, u8engine::usecode::event::LOOK);
            }
        }
        let (_, seen) = self.world_view();
        self.game.set_view(seen.0, seen.1); // (the fast area covers what's on screen)
        let status = self.game.update(dt, &input);
        // (The original moves the picture right by the shake's x: the view's origin goes left.)
        let (qx, qy) = self.game.shake.at;
        let shake = (-(qx as i32), qy as i32);
        if shake != self.shake {
            (self.shake, self.dirty) = (shake, true);
        }
        #[cfg(feature = "audio")]
        if let Some(a) = &mut self.audio {
            if !self.game.avatar_dead() {
                a.set_combat(self.game.in_combat());
            }
        }
        // Into combat stance: the paperdoll goes (DOS closes it within a moment of the first swing).
        let fighting = self.game.in_combat();
        let was = std::mem::replace(&mut self.was_fighting, fighting);
        if fighting && !was && self.windows.items().contains(&AVATAR) {
            self.close_window(AVATAR);
            self.dirty = true;
        }
        if self.game.avatar_dead() {
            self.dying(now);
        }
        if status.map_changed {
            self.on_map_loaded(); // walked through a teleporter
        } else if self.view.follow {
            self.follow_camera(dt); // the Avatar walking, or a cutscene's camera
        }
        for e in self.game.take_events() {
            self.event(e);
        }
        // Windows of things gone, or left behind, close.
        for item in self.windows.items() {
            if !self.game.near_avatar(item, WINDOW_RANGE.0, WINDOW_RANGE.1) {
                self.close_window(item);
                self.dirty = true;
            }
        }
        if let (Some(ui), Some(vm)) = (&mut self.ui, self.game.scripts.as_mut().map(|s| &mut s.vm)) {
            self.dirty |= ui.expire(vm, dt);
        }
        // Cutaway: hide the roof over the Avatar, as the game keeps it (see `Game::roof`).
        let max_z = match self.view.cutaway {
            true => self.game.roof(),
            false => None,
        };
        if max_z != self.view.opts.max_z {
            self.view.opts.max_z = max_z;
            self.dirty = true;
        }
        // The Avatar's numbers (paperdoll, gauge) as last drawn.
        let st = self.game.stats.get(AVATAR).map(|s| [s.str as i16, s.int as i16, s.dex as i16, s.hp, s.mana]);
        if st != self.stats_seen {
            (self.stats_seen, self.dirty) = (st, true);
        }
        let view = self.view_rect();
        self.dirty |= self.painter.sync(&self.game.data, &mut self.game.world, view);
        #[cfg(feature = "audio")]
        if let Some(a) = &mut self.audio {
            let (world, cam) = (&self.game.world, self.view.cam);
            a.place(|id| sound_place(world, cam, id));
        }
        self.show_presence();
        let talking = self.ui.as_ref().is_some_and(|u| !u.is_empty());
        self.moving = status.moving || (ax, ay) != (0, 0) || self.right;
        self.scripting = status.scripting || talking;
    }

    /// What Discord shows (Settings: Discord status): where, what's going on, health and mana.
    fn show_presence(&mut self) {
        use presence::{Doing, Status};
        let st = self.game.stats.get(AVATAR);
        let status = st.filter(|_| self.discord).map(|st| {
            let ui = self.ui.as_ref();
            let doing = if self.game.avatar_dead() {
                Doing::Dead
            } else if self.film.is_some() {
                Doing::Watching
            } else if self.menu_open() {
                Doing::Menu
            } else if ui.is_some_and(|u| u.reading()) {
                Doing::Reading
            } else if ui.is_some_and(|u| u.asking()) {
                Doing::Talking
            } else if self.game.in_combat() {
                Doing::Fighting
            } else {
                Doing::Exploring
            };
            Status { map: self.game.map(), doing, hp: (st.hp, st.max_hp()), mana: (st.mana, st.max_mana()) }
        });
        self.presence.show(status);
    }

    /// Something a script wants seen or heard.
    pub(super) fn event(&mut self, e: UiEvent) {
        #[cfg(feature = "audio")]
        let spoken = self.sound(&e);
        #[cfg(not(feature = "audio"))]
        let spoken = None;
        match &e {
            UiEvent::Target { pid } => self.targeting = Some(*pid),
            UiEvent::Endgame => {
                // Finished: from now on the main menu offers the quotes and the ending.
                if let Some(m) = &mut self.menu {
                    m.finished = true;
                }
                self.settings.finished = true;
                self.settings.save();
                self.start_film(films::Which::Endgame, films::After::Menu);
            }
            UiEvent::OpenGump { item, gump } => {
                let view = self.view_size();
                if let Some(art) = &self.art {
                    let parent = self.game.world.container(*item);
                    self.windows.open(art, *item, *gump as u32, parent, view);
                }
                self.dirty = true;
            }
            UiEvent::CloseGump { item } => self.dirty |= self.windows.close(*item),
            UiEvent::CloseItemGumps => self.dirty |= self.close_windows(),
            UiEvent::Slider { pid, min, max, step } => {
                self.slider = Some(crate::slider::Slider::for_script(*pid, *min, *max, *step));
                self.dirty = true;
            }
            _ => {}
        }
        if let Some(ui) = &mut self.ui {
            let shown = matches!(e, UiEvent::Bark { .. } | UiEvent::Ask { .. } | UiEvent::Read { .. });
            ui.push(e, &self.game.data.palette, spoken, self.art.as_ref());
            self.dirty |= shown; // new speech or a menu: draw it now
        }
    }

    /// Play the sound side of a script event. For a bark, the length of its recorded speech.
    #[cfg(feature = "audio")]
    pub(super) fn sound(&mut self, e: &UiEvent) -> Option<f32> {
        let a = self.audio.as_mut()?;
        match e {
            UiEvent::Sfx { sfx, item, looping } => {
                let at = (*item != 0).then(|| sound_place(&self.game.world, self.view.cam, *item)).flatten();
                a.sfx(*sfx, *item, *looping, at)
            }
            UiEvent::StopSfx { sfx, item } => a.stop_sfx(*sfx, *item),
            UiEvent::SfxVolume { sfx, volume } => a.set_sfx_volume(*sfx, *volume),
            UiEvent::Music(n) => a.play_track(*n as usize),
            UiEvent::Victory => a.fanfare(),
            UiEvent::Bark { item, text, .. } => {
                // Titans, statues and the Guardian have recorded lines (SOUND/E<shape>.FLX).
                let shape = match self.game.world.get(*item) {
                    Some(it) => it.shape,
                    None if *item == ui::GUARDIAN => ui::GUARDIAN as u32,
                    None => 0,
                };
                if !self.game.world.same_floor(AVATAR, *item) {
                    return None; // (not heard from another storey)
                }
                return a.speak(shape, text, *item);
            }
            _ => {}
        }
        None
    }

    /// Just the name; F12 adds the frame time.
    pub(super) fn update_title(&mut self, win: &Window) {
        let mut title = String::from("Open Ultima8");
        if self.view.stats {
            title += &format!(" ({:.1} ms/frame)", self.frame_ms);
        }
        if title != self.title {
            win.set_title(&title);
            self.title = title;
        }
    }
}

impl App {
    /// The Avatar's death, as the DOS game's: the death song as it falls; 10 seconds after the
    /// fall, the colours back to normal and the tombstone, put away only by a key (clicks don't);
    /// then the game menu, which stays open until a game is loaded or begun.
    fn dying(&mut self, now: Instant) {
        let falling = self.game.world.is_acting(AVATAR);
        self.death = match self.death {
            Death::Alive => {
                #[cfg(feature = "audio")]
                if let Some(a) = &mut self.audio {
                    a.death();
                }
                Death::Falling
            }
            Death::Falling if falling => Death::Falling,
            Death::Falling => Death::Fallen(now),
            Death::Fallen(t) if now - t >= DEATH_PAUSE && !self.menu_open() => {
                if let Some(s) = &mut self.game.scripts {
                    s.vm.fade = Default::default();
                }
                let (gump, text) = TOMBSTONE;
                let read = UiEvent::Read {
                    pid: u8engine::usecode::Pid::MAX,
                    kind: u8engine::usecode::ReadKind::Grave,
                    item: 0,
                    gump,
                    text: text.into(),
                };
                self.event(read);
                if let Some(u) = &mut self.ui {
                    u.keys_only();
                }
                self.dirty = true;
                Death::Tombstone
            }
            Death::Tombstone if !self.ui.as_ref().is_some_and(|u| u.reading()) => {
                self.open_menu(menu::Page::Main);
                if let Some(m) = &mut self.menu {
                    m.stay = true;
                }
                Death::Over
            }
            d => d,
        };
    }
}

/// Where a sound at `id` is heard from the view centred on `cam`: (volume 0–1, pan −1…1), or
/// None if it isn't anywhere. By its world position (emitters are often invisible markers),
/// what it's in counts (something carried sounds from its carrier); nothing through a floor (a
/// fire or a clock in the hall under the roof is silent up there); fading with the square of
/// the distance left, so what's at the edge of a wide view is faint.
#[cfg(feature = "audio")]
fn sound_place(world: &u8engine::world::World, cam: (f32, f32), id: ObjId) -> Option<(f32, f32)> {
    let root = world.root(id);
    let it = world.get(root)?;
    let (sx, sy) = project(it.x, it.y, it.z);
    let (dx, dy) = (sx as f32 - cam.0, sy as f32 - cam.1);
    let pan = (dx / 320.0).clamp(-1.0, 1.0);
    if !world.same_floor(AVATAR, root) {
        return Some((0.0, pan));
    }
    let near = (1.0 - dx.hypot(dy) / HEARING).clamp(0.0, 1.0);
    Some((near * near, pan))
}

#[cfg(all(test, feature = "audio"))]
mod tests {
    use super::*;

    /// Sounds are placed where they are: something the Avatar carries is heard from the Avatar
    /// (not from its spot in a bag's window), at full volume at the centre of the view and
    /// faint across a wide one; something gone isn't anywhere.
    #[test]
    #[ignore]
    fn sounds_come_from_where_things_are() {
        let Ok(dir) = std::env::var("U8_DATA") else { return };
        let gd = u8data::GameData::load_fixed(std::path::Path::new(&dir)).unwrap();
        let world = u8engine::world::World::build(&gd, 40);
        let a = world.get(AVATAR).unwrap();
        let (sx, sy) = project(a.x, a.y, a.z);
        let cam = (sx as f32, sy as f32);
        let carried = world.contents(AVATAR).into_iter().find(|&id| !world.contents(id).is_empty()).unwrap();
        let deep = world.contents(carried)[0];
        assert_eq!(sound_place(&world, cam, deep), Some((1.0, 0.0)), "from the Avatar");
        let (far, _) = sound_place(&world, (cam.0 + 300.0, cam.1), AVATAR).unwrap();
        assert!(far > 0.0 && far < 0.15, "faint across the view: {far}");
        let gone = (1..u16::MAX).rev().find(|&id| world.get(id).is_none()).unwrap();
        assert_eq!(sound_place(&world, cam, gone), None);
    }
}
