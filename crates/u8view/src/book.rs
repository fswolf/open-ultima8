//! The game menu (the original's book) as the app drives it: opening pages, acting on choices,
//! the options and the settings file they're kept in, and the credits and quotes.
use super::*;

impl App {
    /// Put saved settings into effect (fullscreen waits for the window).
    pub(super) fn apply_settings(&mut self, s: settings::Settings) {
        let v = &mut self.view;
        // (The CPU's full-frame xBRZ gave way to the GPU's xBR.)
        let name = if s.upscaler == "xBRZ full-frame" { Mode::Xbr.name() } else { &s.upscaler };
        if let Some(m) = Mode::ALL.into_iter().find(|m| m.name() == name) {
            v.mode = m;
        }
        v.scale = s.scale.clamp(1, MAX_SCALE);
        v.zoom = s.zoom.clamp(ZOOMS[0], ZOOMS[ZOOMS.len() - 1]);
        v.zoom_to = v.zoom;
        v.cutaway = s.cutaway;
        v.smooth = s.smooth_camera;
        v.gamma = s.gamma;
        v.colour_fix |= s.colour_fix;
        self.swapped = s.left_handed;
        self.cheats = s.cheats;
        self.apply_cheats();
        self.discord = s.discord;
        (self.pad.on, self.pad.speed, self.pad.deadzone, self.pad.swap) =
            (s.pad, s.pad_speed, s.pad_deadzone, s.swap_sticks);
        if let Some(m) = &mut self.menu {
            m.finished = s.finished;
        }
        if let Some(ui) = &mut self.ui {
            ui.style = if s.classic_text { ui::Style::Classic } else { ui::Style::Modern };
        }
        #[cfg(feature = "audio")]
        if let Some(a) = &mut self.audio {
            a.change_volume(s.volume - a.volume);
            a.set_ambience(s.ambience);
            a.sfx_on = s.effects;
            if !s.music {
                a.music_off();
            }
        }
        self.settings = s;
    }

    pub(super) fn current_settings(&self) -> settings::Settings {
        let v = &self.view;
        #[allow(unused_mut)]
        let mut s = settings::Settings {
            upscaler: v.mode.name().to_string(),
            scale: v.scale,
            zoom: (v.zoom_to * 100.0).round() / 100.0,
            classic_text: self.ui.as_ref().is_some_and(|u| u.style == ui::Style::Classic),
            cutaway: v.cutaway,
            smooth_camera: v.smooth,
            fullscreen: self.fullscreen,
            left_handed: self.swapped,
            cheats: self.cheats,
            discord: self.discord,
            pad: self.pad.on,
            pad_speed: self.pad.speed,
            pad_deadzone: self.pad.deadzone,
            swap_sticks: self.pad.swap,
            gamma: v.gamma,
            // U8_COLOUR_FIX only applies to this run.
            colour_fix: if std::env::var_os("U8_COLOUR_FIX").is_some() {
                self.settings.colour_fix
            } else {
                v.colour_fix
            },
            ..self.settings.clone()
        };
        #[cfg(feature = "audio")]
        if let Some(a) = &self.audio {
            (s.music, s.effects, s.volume, s.ambience) = (a.music_on(), a.sfx_on, a.volume, a.ambience);
        }
        s
    }

    /// Write the settings file if anything changed.
    /// The game's scripts know whether cheats are on (a loaded game is told again).
    pub(super) fn apply_cheats(&mut self) {
        if let Some(s) = &mut self.game.scripts {
            s.vm.cheats = self.cheats;
        }
    }

    pub(super) fn keep_settings(&mut self) {
        let s = self.current_settings();
        if s != self.settings {
            s.save();
            self.settings = s;
        }
    }

    pub(super) fn menu_open(&self) -> bool {
        self.menu.as_ref().is_some_and(|m| m.is_open())
            || self.roll.is_some()
            || self.film.is_some()
            || self.slider.is_some()
    }

    /// Roll the credits or the quotes (from the main menu), to their own music.
    pub(super) fn start_roll(&mut self, r: credits::Roll) {
        let file = u8data::find_file(&self.root, r.file()).ok().and_then(|p| std::fs::read(p).ok());
        let (Some(d), Some(m)) = (file, &mut self.menu) else {
            self.notice("The credits (STATIC/ECREDITS.DAT, QUOTES.DAT) are missing.");
            return;
        };
        let (fonts, pal) = m.fonts();
        let scroller = credits::Scroller::new(r, &u8data::credits::decode(&d), fonts, pal);
        m.close();
        #[allow(unused_mut)]
        let mut back = 0;
        #[cfg(feature = "audio")]
        if let Some(a) = &mut self.audio {
            back = a.wanted_track();
            a.play_track(r.song() as usize);
        }
        self.roll = Some((scroller, back));
        self.dirty = true;
    }

    /// The credits are over (or the player had enough): back to the game and its music.
    pub(super) fn end_roll(&mut self) {
        #[cfg_attr(not(feature = "audio"), allow(unused_variables))]
        if let Some((_, back)) = self.roll.take() {
            #[cfg(feature = "audio")]
            if let Some(a) = &mut self.audio {
                a.play_track(back);
            }
            self.open_menu(menu::Page::Main); // back where it was rolled from
        }
        self.dirty = true;
    }

    pub(super) fn open_menu(&mut self, page: menu::Page) {
        match (&mut self.menu, &self.art) {
            (Some(m), Some(_)) => m.open(page),
            _ => self.notice("The menu art (STATIC/U8GUMPS.FLX) is missing."),
        }
        // Nothing carries on underneath: no held keys or buttons, no walking.
        self.held.clear();
        self.pad.stop();
        (self.press, self.pan, self.left, self.right, self.dragging, self.step_until) =
            (None, None, false, false, false, None);
        self.dirty = true;
    }

    /// The options page's lines, with current values.
    pub(super) fn option_lines(&self) -> Vec<(menu::Setting, String)> {
        use menu::Setting::*;
        let v = &self.view;
        let on = |b: bool| if b { "on" } else { "off" };
        #[allow(unused_mut)]
        let mut audio = (false, false, 0.0f32, 0.0f32);
        #[cfg(feature = "audio")]
        if let Some(a) = &self.audio {
            audio = (a.music_on(), a.sfx_on, a.volume, a.ambience);
        }
        let classic = self.ui.as_ref().is_some_and(|u| u.style == ui::Style::Classic);
        #[allow(unused_mut)]
        let mut lines = vec![
            (Upscaler, format!("Look: {}", v.mode.name())),
            (Scale, format!("Interface: x{}", v.scale)),
            (TextStyle, format!("Replies: {}", if classic { "classic" } else { "numbered" })),
            (Music, format!("Music: {}", on(audio.0))),
            (Effects, format!("Sound effects: {}", on(audio.1))),
            (Volume, format!("Music volume: {} of 10", (audio.2 * 10.0).round())), // the fonts have no % or /
            (Ambience, format!("Ambience: {} of 10", (audio.3 * 10.0).round())),   // fires, drips, fountains
            (Cutaway, format!("See inside: {}", on(v.cutaway))),
            (SmoothCamera, format!("Smooth camera: {}", on(v.smooth))),
            (Fullscreen, format!("Full screen: {}", on(self.fullscreen))),
            (Buttons, format!("Mouse: {}", if self.swapped { "left-handed" } else { "right-handed" })),
            (Cheats, format!("Cheats: {}", on(self.cheats))),
            (Discord, format!("Discord status: {}", on(self.discord))),
        ];
        #[cfg(feature = "gamepad")]
        lines.extend([
            (Pad, format!("Controller: {}", on(self.pad.on))),
            (PadSpeed, format!("Pointer speed: {} of 10", (self.pad.speed * 10.0).round())),
            (Deadzone, format!("Dead zone: {} of 10", (self.pad.deadzone * 10.0).round())),
            (SwapSticks, format!("Walk with: {} stick", if self.pad.swap { "right" } else { "left" })),
        ]);
        lines
    }

    pub(super) fn menu_action(&mut self, a: menu::Action, el: &ActiveEventLoop) {
        use menu::{Action, Setting};
        let close = |app: &mut Self| {
            if let Some(m) = &mut app.menu {
                m.close();
            }
        };
        match a {
            Action::Close => close(self),
            Action::Quit => el.exit(),
            Action::Roll(r) => self.start_roll(r),
            Action::Film(w) => _ = self.start_film(w, films::After::Menu),
            Action::Name(name) => self.game.set_avatar_name(&name),
            Action::Save { slot, desc } => {
                if self.save_to(slot, &desc) {
                    close(self);
                } else if let Some(m) = &mut self.menu {
                    m.open(menu::Page::Diary(menu::Diary::Write));
                }
            }
            Action::Load(slot) => {
                close(self);
                self.load(slot);
            }
            Action::Change(s) => {
                let c = match s {
                    Setting::Upscaler => Command::CycleUpscaler,
                    Setting::Scale => {
                        self.view.scale = self.view.scale % MAX_SCALE + 1;
                        Command::Recentre
                    }
                    Setting::TextStyle => Command::ToggleTextStyle,
                    Setting::Music => Command::MusicToggle,
                    Setting::Effects => Command::SfxToggle,
                    Setting::Volume => {
                        #[cfg(feature = "audio")]
                        if let Some(a) = &mut self.audio {
                            let v = if a.volume > 0.95 { 0.0 } else { a.volume + 0.1 };
                            a.change_volume(v - a.volume);
                        }
                        Command::Recentre
                    }
                    Setting::Ambience => {
                        #[cfg(feature = "audio")]
                        if let Some(a) = &mut self.audio {
                            a.set_ambience(if a.ambience > 0.95 { 0.0 } else { a.ambience + 0.1 });
                        }
                        Command::Recentre
                    }
                    Setting::Cutaway => Command::ToggleCutaway,
                    Setting::SmoothCamera => {
                        self.view.smooth = !self.view.smooth;
                        Command::Recentre
                    }
                    Setting::Fullscreen => Command::Fullscreen,
                    Setting::Buttons => Command::SwapButtons,
                    Setting::Cheats => {
                        self.cheats = !self.cheats;
                        self.apply_cheats();
                        self.notice(if self.cheats { "Cheats on: click the Avatar." } else { "Cheats off." });
                        Command::Recentre
                    }
                    Setting::Discord => {
                        self.discord = !self.discord;
                        Command::Recentre
                    }
                    Setting::Pad | Setting::PadSpeed | Setting::Deadzone | Setting::SwapSticks => {
                        let p = &mut self.pad;
                        let next = |v: f32, lo: f32, hi: f32| {
                            if v > hi - 0.05 {
                                lo
                            } else {
                                ((v * 10.0).round() + 1.0) / 10.0
                            }
                        };
                        match s {
                            Setting::Pad => (p.on, p.owns) = (!p.on, false),
                            Setting::PadSpeed => p.speed = next(p.speed, 0.1, 1.0),
                            Setting::Deadzone => p.deadzone = next(p.deadzone, 0.1, 0.4),
                            _ => p.swap = !p.swap,
                        }
                        Command::Recentre
                    }
                };
                self.command(c);
            }
        }
        self.keep_settings();
        self.dirty = true;
    }
}
