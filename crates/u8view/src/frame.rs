//! A frame: advancing the game with the player's input, passing script events to the UI and
//! the speakers, and drawing the world, windows, pages and menus.
use super::*;

/// The paperdoll's stats panel as the original draws it: names right-aligned to the dotted line
/// (x 123, a colon every 9 rows from y 19), green numbers right-aligned after it, and the stats
/// button in the box under them.
fn paperdoll_stats(
    canvas: &mut Canvas,
    art: &mut art::GumpArt,
    ss: u32,
    ui: &ui::ScriptUi,
    gd: &u8data::GameData,
    (st, load, armour, pressed): (u8engine::stats::Npc, u32, i32, bool),
    at: (i32, i32),
) {
    let rows = [
        ("STR", st.str as u32),
        ("INT", st.int as u32),
        ("DEX", st.dex as u32),
        ("ARMR", armour.max(0) as u32),
        ("HITS", st.hp.max(0) as u32),
        ("MANA", st.mana.max(0) as u32),
        ("WGHT", load),
    ];
    for (k, (name, value)) in rows.iter().enumerate() {
        let mid = at.1 + 23 + k as i32 * 9;
        let (name, value) =
            (ui.text(STAT_NAMES, name, &gd.palette), ui.text(STAT_VALUES, &value.to_string(), &gd.palette));
        text::draw_plain(canvas, ss, &name, at.0 + 122 - name.w as i32, mid - name.h as i32 / 2);
        // Names end at the window's dotted colons (x 122); numbers centred between them and the
        // panel's right edge (124–148).
        text::draw_plain(canvas, ss, &value, at.0 + 136 - value.w as i32 / 2, mid - value.h as i32 / 2);
    }
    gumps::Windows::draw_stats_button(canvas, art, pressed, at);
}

/// Where speech goes over something on the map (screen pixels, unzoomed): centred on where it
/// stands, 16 above the top of its box — ScummVM's ItemRelativeGump, not the sprite, so a
/// halberd's swing (or a speaker hidden behind a wall) doesn't push it aside.
/// Also where its feet are (screen y), for what goes below it.
fn speech_anchor(world: &u8engine::world::World, id: ObjId) -> Option<((i32, i32), i32)> {
    let e = world.entry(world.index(world.root(id))?); // (something carried: over its carrier)
    let (sx, sy) = u8render::project(e.it.x, e.it.y, e.z1);
    Some(((sx, sy - 16), u8render::project(e.it.x, e.it.y, e.it.z).1))
}

/// How much bigger xBRZ makes things for a zoom (none below 1.5×; the GPU does the rest).
fn xbrz_factor(zoom: f32) -> u32 {
    (zoom.round() as u32).clamp(1, MAX_XBRZ)
}

impl App {
    /// The world frame's pixels per game pixel. The looks drawn from the sprites themselves
    /// (xBRZ'd, or plain pixels) are made at the zoom, rounded, so people move by less than a
    /// game pixel — smooth when zoomed in; the GPU's filters work on the original pixels (1).
    /// Kept while the zoom eases (the sprites are remade once it settles).
    pub(super) fn world_ss(&self) -> u32 {
        match self.view.mode {
            Mode::XbrzSprites | Mode::Nearest if self.zooming => self.cache.scale,
            Mode::XbrzSprites | Mode::Nearest => xbrz_factor(self.view.zoom_to),
            _ => 1,
        }
    }

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
        let status = self.game.update(dt, &input);
        let quake = self.game.scripts.as_ref().map_or(0, |s| s.vm.quake as i32);
        let shake = if quake > 0 {
            let s = &mut self.shake_seed;
            *s ^= *s << 13;
            *s ^= *s >> 17;
            *s ^= *s << 5;
            let n = 2 * quake as u32 + 1;
            ((*s % n) as i32 - quake, ((*s >> 16) % n) as i32 - quake)
        } else {
            (0, 0)
        };
        if shake != self.shake || quake > 0 {
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

    /// What Discord shows (Options: Discord status): where, what's going on, health and mana.
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

    pub(super) fn redraw(&mut self) {
        let (Some(win), Some(_)) = (self.window.clone(), &self.gpu) else { return };
        let sz = win.inner_size();
        if sz.width == 0 || sz.height == 0 {
            return; // minimised
        }
        self.update();
        let (s, zoom) = (self.view.scale, self.view.zoom);
        let xbrz = self.view.mode == Mode::XbrzSprites;
        let world_ss = self.world_ss();
        let world_xbrz = xbrz && world_ss > 1;
        // Nothing over the world (the usual while walking about): no overlay to draw or send.
        let overlay_used = !self.windows.items().is_empty()
            || self.windows.gauge.is_some()
            || self.item_drag.is_some()
            || self.ui.as_ref().is_some_and(|u| !u.is_empty())
            || self.roll.is_some()
            || self.pad_cursor.is_some()
            || self.touch_buttons_up()
            || self.menu_open();
        if self.dirty {
            let t0 = Instant::now();
            // (Pages to read are drawn at the window's resolution, so their type is crisp.)
            let reading = self.ui.as_ref().is_some_and(|u| u.reading());
            let ui_ss = if xbrz || reading { s } else { 1 };
            let up = |smooth: bool| -> u8render::Upscaler {
                if smooth {
                    Box::new(upscale::xbrz)
                } else {
                    Box::new(upscale::nearest)
                }
            };
            if (self.cache.scale, self.world_xbrz) != (world_ss, world_xbrz) {
                self.cache.set_scale(world_ss, up(world_xbrz));
                self.world_xbrz = world_xbrz;
            }
            if self.ui_cache.scale != ui_ss {
                self.ui_cache.set_scale(ui_ss, up(ui_ss > 1));
            }
            // The world.
            let (ww, wh) = self.world_size();
            let (bw, bh) = (ww * world_ss, wh * world_ss);
            self.buf.resize((bw * bh * 4) as usize, 0);
            let cam = self.view_origin();
            let mut canvas = Canvas::new(&mut self.buf, bw, bh);
            self.painter.draw(&self.game.data, &mut self.game.world, &mut self.cache, cam, &mut canvas, self.view.opts);
            // Over it, at the interface's size: windows, the carried item, speech, menus.
            let (vw, vh) = self.view_size();
            let (ow, oh) = if overlay_used { (vw * ui_ss, vh * ui_ss) } else { (1, 1) };
            self.overlay.resize((ow * oh * 4) as usize, 0);
            if self.menu_open() && self.menu.is_some() {
                let lines = self.option_lines();
                if let Some(m) = &mut self.menu {
                    m.options = lines;
                }
            }
            let (mouse, buttons_up) = (self.mouse_game(), self.touch_buttons_up());
            // Where things on the map are on the overlay (speech goes over the speaker).
            let ((ox, oy), _) = self.world_view();
            let k = zoom / s as f32;
            let mut canvas = Canvas::new(&mut self.overlay, ow, oh);
            canvas.clear();
            if let Some(art) = &mut self.art {
                art.set_style(ui_ss, xbrz);
                let (game, gd, ui, stats_pressed) = (&self.game, &self.game.data, &self.ui, self.stats_pressed);
                // The paperdoll's stats panel goes with its window (under any window above it).
                let stats = |canvas: &mut Canvas, art: &mut art::GumpArt, item: ObjId, gump: u32, at: (i32, i32)| {
                    let st = game.stats.get(item);
                    if let (Some(st), Some(ui), gumps::PAPERDOLL) = (st, ui, gump) {
                        // (Weighing walks everything carried: only when the paperdoll is open.)
                        paperdoll_stats(
                            canvas,
                            art,
                            ui_ss,
                            ui,
                            gd,
                            (st, game.load().0, game.armour_class(), stats_pressed),
                            at,
                        );
                    }
                };
                self.windows.draw(&mut canvas, art, gd, &self.game.world, &mut self.ui_cache, self.item_drag, stats);
                if let Some(st) = self.game.stats.get(AVATAR) {
                    let bars = [(st.hp, st.max_hp()), (st.mana, st.max_mana())];
                    self.windows.draw_gauge(&mut canvas, art, gd, ui_ss, bars);
                }
            }
            if let Some(it) = self.item_drag.and_then(|id| self.game.world.get(id)) {
                // The item being carried, under the pointer.
                let flip = it.flags & u8engine::world::item_flag::FLIPPED != 0;
                if let Some(sp) = self.ui_cache.get(&self.game.data, it.shape, it.frame, flip) {
                    canvas.blit(&sp, mouse.0 * ui_ss as i32 - sp.xoff, mouse.1 * ui_ss as i32 - sp.yoff);
                }
            }
            if let Some(ui) = &mut self.ui {
                let (world, windows, art, gd) = (&self.game.world, &self.windows, &self.art, &self.game.data);
                // Speech goes over the thing speaking: on the map, or in an open window.
                // (ScummVM's ItemRelativeGump.)
                let rect_of = |id| {
                    // In an open window: over it there (on the overlay).
                    if world.container(id).is_some() {
                        if let Some(r) = art.as_ref().and_then(|a| windows.item_rect(gd, a, world, id)) {
                            return Some([r[0], r[1] - 2, r[2], r[3] + 2]); // (2 clear of it)
                        }
                    }
                    // Else over what it's in, on the map (a bag on someone's back: over them).
                    let id = world.root(id);
                    if world.get(id).is_none() || world.is_void(id) {
                        // Nobody to stand over (the Guardian): low in the middle, its bottom 8 up.
                        return Some([vw as i32 / 2, vh as i32 - 8, 0, 0]);
                    }
                    if !world.same_floor(AVATAR, id) {
                        return None; // (nor seen from another storey)
                    }
                    // (Over the head, down to the feet.)
                    let ((sx, sy), feet) = speech_anchor(world, id)?;
                    let (x, y) = ((sx as f32 - ox) * k, (sy as f32 - oy) * k);
                    let down = ((feet - sy) as f32 * k).round() as i32;
                    Some([x.round() as i32, y.round() as i32, 0, down])
                };
                ui.draw(&mut canvas, ui_ss, (vw, vh), rect_of);
                ui.draw_reading(&mut canvas, self.art.as_mut(), ui_ss, (vw, vh));
            }
            if let Some((r, _)) = &self.roll {
                r.draw(&mut canvas, ui_ss, (vw, vh));
            }
            if let Some(f) = &self.film {
                f.draw(&mut canvas, ui_ss, (vw, vh));
            }
            if let (Some(sl), Some(art)) = (self.slider.as_mut(), self.art.as_mut()) {
                sl.draw(&mut canvas, art, self.menu.as_ref().map(|m| m.fonts()), ui_ss, (vw, vh));
            }
            if let (Some(m), Some(art)) = (self.menu.as_mut().filter(|m| m.is_open()), self.art.as_mut()) {
                let name = self.game.data.avatar_name.as_deref().unwrap_or("");
                if m.avatar != name {
                    m.avatar = name.to_string();
                }
                m.draw(&mut canvas, art, (vw, vh));
                if let Some(ui) = &mut self.ui {
                    ui.draw_notices(&mut canvas, ui_ss, vw); // "Not now." etc. stay readable over the book
                }
            }
            // The touch screen's buttons, down the right edge.
            if let (true, Some(ui)) = (buttons_up, &self.ui) {
                for (r, (_, name)) in touch::button_rects((vw, vh)).iter().zip(touch::BUTTONS) {
                    text::shade(&mut canvas, ui_ss, *r);
                    let img = ui.text(0, name, &self.game.data.palette);
                    let (x, y) = (r[0] + (r[2] - img.w as i32) / 2, r[1] + (r[3] - img.h as i32) / 2);
                    text::draw_plain(&mut canvas, ui_ss, &img, x, y);
                }
            }
            // The pad's pointer, over everything (the system's cursor is hidden meanwhile).
            if let Some(sp) = self.pad_cursor.and_then(|f| self.cursors.as_ref()?.sprite(f, ui_ss)) {
                canvas.blit(&sp, mouse.0 * ui_ss as i32 - sp.xoff, mouse.1 * ui_ss as i32 - sp.yoff);
            }
            let gpu = self.gpu.as_mut().unwrap();
            gpu.upload(gpu::Layer::World, &self.buf, bw, bh);
            self.frame_ss = world_ss;
            self.overlay_ss = ui_ss;
            if overlay_used {
                gpu.upload(gpu::Layer::Overlay, &self.overlay, ow, oh);
            }
            self.overlay_shown = overlay_used;
            self.dirty = false;
            let ms = t0.elapsed().as_secs_f32() * 1000.0;
            self.frame_ms = if self.frame_ms == 0.0 { ms } else { self.frame_ms * 0.9 + ms * 0.1 };
            self.update_title(&win);
        }
        // The world's frame starts at whole game pixels; the view, at a fraction of one.
        let ((x, y), _) = self.world_view();
        let (tx, ty) = self.view_origin();
        let fs = self.frame_ss as f32;
        let world = gpu::Place {
            k: fs / zoom,
            off: ((x - tx as f32) * fs, (y - ty as f32) * fs),
            filter: match self.view.mode {
                Mode::Smooth => gpu::Filter::Plain,
                Mode::Xbr => gpu::Filter::Xbr,
                Mode::Crt => gpu::Filter::Crt,
                Mode::Nearest | Mode::XbrzSprites => gpu::Filter::Sharp,
            },
        };
        // (Windows and text stay crisp whatever the world's look.)
        // (At the scale it was last drawn at: the window's while a book is open, so its type is crisp.)
        let overlay = gpu::Place { k: self.overlay_ss as f32 / s as f32, off: (0.0, 0.0), filter: gpu::Filter::Sharp };
        self.gpu.as_mut().unwrap().present(
            [Some(world), self.overlay_shown.then_some(overlay)],
            self.view.gamma,
            self.view.colour_fix,
            self.game.scripts.as_ref().map_or(u8engine::fade::Fade::default().rows(), |s| s.vm.fade.rows()),
            self.game.scripts.as_ref().map_or(0, |s| s.vm.inverter.state),
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Sounds are placed where they are: something the Avatar carries is heard from the Avatar
    /// (not from its spot in a bag's window), at full volume at the centre of the view and
    /// faint across a wide one; something gone isn't anywhere.
    #[cfg(feature = "audio")]
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

    /// The gauge comes up twice its old size in the bottom-left corner, answers clicks over all
    /// of it, and fills its bars from the bottom; the paperdoll's numbers sit centred between
    /// the dotted colons and the panel's edge. With `U8_SHOTS` set, written there as PPM.
    #[test]
    #[ignore]
    fn gauge_is_bigger_and_stats_are_centred() {
        let Ok(dir) = std::env::var("U8_DATA") else { return };
        let gd = u8data::GameData::load_fixed(std::path::Path::new(&dir)).unwrap();
        let root = std::path::PathBuf::from(&dir);
        let mut art = art::GumpArt::load(&root, &gd.palette).unwrap();
        let read = |p: &str| std::fs::read(u8data::find_file(&root, p).unwrap()).unwrap();
        let ui = ui::ScriptUi::new(u8data::font::Fonts::parse(
            &u8data::flex::Flex::parse(read("STATIC/U8FONTS.FLX")).unwrap(),
        ));
        let world = u8engine::world::World::build(&gd, 0);
        let (vw, vh) = (320u32, 200u32);
        let mut w = gumps::Windows::default();
        w.show_gauge(&art, (vw, vh));
        let (gx, gy) = w.gauge.unwrap();
        let (sw, sh) = art.size(gumps::GAUGE);
        assert_eq!((gx, gy + sh * gumps::GAUGE_SCALE), (5, vh as i32 - 5), "bottom-left, at its new size");
        let (cx, cy) = (gx + sw * gumps::GAUGE_SCALE - 4, gy + sh * gumps::GAUGE_SCALE - 4);
        assert!(matches!(w.hit(&gd, &art, &world, cx, cy), Some(gumps::Hit::Gauge)), "its far corner is part of it");
        let mut px = vec![0u8; (vw * vh * 4) as usize];
        let mut canvas = Canvas::new(&mut px, vw, vh);
        art.set_style(1, false);
        w.draw_gauge(&mut canvas, &mut art, &gd, 1, [(15, 30), (30, 30)]);
        // Ryan: without the art's black outline — its outermost pixels aren't drawn.
        let k = gumps::GAUGE_SCALE;
        let edge = [(gx, gy), (gx + sw * k - 1, gy), (gx + 4 * k, gy + sh * k - 1), (gx, gy + 6 * k)];
        assert!(edge.iter().all(|&(x, y)| canvas.px[((y as u32 * vw + x as u32) * 4 + 3) as usize] == 0));
        assert!(canvas.px[(((gy + k) as u32 * vw + (gx + 2 * k) as u32) * 4 + 3) as usize] == 255, "the rest is");
        let st = u8engine::stats::Npc { str: 15, int: 15, dex: 15, hp: 30, mana: 30, ..Default::default() };
        art.draw(&mut canvas, gumps::PAPERDOLL, 0, 100, 10);
        paperdoll_stats(&mut canvas, &mut art, 1, &ui, &gd, (st, 35, 7, false), (100, 10));
        // Values: centred about x 136 in the window.
        let ink = |x0: i32, x1: i32, y0: i32, y1: i32| {
            let cols: Vec<i32> = (x0..x1)
                .filter(|&x| (y0..y1).any(|y| px_at(&px, vw, x, y)[1] as i32 > px_at(&px, vw, x, y)[0] as i32 + 60))
                .collect();
            (cols.first().copied(), cols.last().copied())
        };
        let (l, r) = ink(100 + 124, 100 + 150, 10 + 18, 10 + 28); // STR's "15", in green
        let (l, r) = (l.unwrap() - 100, r.unwrap() - 100);
        assert!(((l + r) / 2 - 136).abs() <= 1, "15 spans {l}..{r}");
        if let Ok(out) = std::env::var("U8_SHOTS") {
            let mut ppm = format!("P6 {vw} {vh} 255\n").into_bytes();
            px.chunks(4).for_each(|p| ppm.extend(&p[..3]));
            std::fs::write(format!("{out}/gauge_stats.ppm"), ppm).unwrap();
        }
    }

    /// Speech sits over where someone stands, the same whichever frame they're on (a guard's
    /// halberd swinging wide doesn't push it aside): 16 above their head, centred on their feet.
    #[test]
    #[ignore]
    fn speech_sits_over_the_speaker_whatever_the_frame() {
        let Ok(dir) = std::env::var("U8_DATA") else { return };
        let gd = u8data::GameData::load_fixed(std::path::Path::new(&dir)).unwrap();
        let mut world = u8engine::world::World::build(&gd, 40);
        let guard = world.objects().find(|&id| world.get(id).is_some_and(|it| it.shape == 574)).expect("a guard");
        let it = world.get(guard).unwrap();
        let at = speech_anchor(&world, guard).unwrap();
        assert_eq!(at.0 .0, it.x / 4 - it.y / 4, "centred on where it stands");
        let e = world.entry(world.index(guard).unwrap());
        assert_eq!(at.0 .1, it.x / 8 + it.y / 8 - e.z1 - 16, "16 above its head");
        assert_eq!(at.1, it.x / 8 + it.y / 8 - it.z, "and its feet");
        for f in 0..gd.shapes.get(574).len() as u32 {
            world.set(&gd, guard, u8data::map::Item { frame: f, ..it });
            assert_eq!(speech_anchor(&world, guard), Some(at), "frame {f}");
        }
    }

    /// Something in a closed container speaks from over the container on the map (in a bag on
    /// someone's back: over them).
    #[test]
    #[ignore]
    fn things_inside_speak_from_their_container() {
        let Ok(dir) = std::env::var("U8_DATA") else { return };
        let gd = u8data::GameData::load_fixed(std::path::Path::new(&dir)).unwrap();
        let g = u8engine::Game::new(gd, None, Some(40)).unwrap();
        let world = &g.world;
        let held = (1..u16::MAX).find(|&id| world.container(id).is_some()).expect("something in a chest");
        let carrier = world.root(held);
        assert_ne!(carrier, held);
        assert!(speech_anchor(world, held).is_some());
        assert_eq!(speech_anchor(world, held), speech_anchor(world, carrier));
    }

    fn px_at(px: &[u8], w: u32, x: i32, y: i32) -> [u8; 4] {
        let i = ((y as u32 * w + x as u32) * 4) as usize;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    }
}
