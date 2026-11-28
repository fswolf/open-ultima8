//! The game menu (the original's book) as the app drives it: opening pages, acting on choices,
//! the settings panel and the settings file it's kept in, and the credits and quotes.
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
        // (None saved yet: 2 until the window shows how big the screen is.)
        self.auto_scale = s.scale == 0;
        v.scale = if self.auto_scale { 2 } else { s.scale.clamp(1, MAX_SCALE) };
        self.auto_zoom = s.zoom == 0.0;
        v.zoom = if self.auto_zoom { 2.0 } else { s.zoom.clamp(ZOOMS[0], ZOOMS[ZOOMS.len() - 1]) };
        v.zoom_to = v.zoom;
        v.cutaway = s.cutaway;
        v.smooth = s.smooth_camera;
        v.gamma = s.gamma;
        v.colour_fix |= s.colour_fix;
        self.swapped = s.left_handed;
        (self.cheats, self.restored) = (s.cheats, s.restored);
        self.apply_game_options();
        self.discord = s.discord;
        self.touch.buttons = s.touch_buttons;
        (self.pad.on, self.pad.speed, self.pad.deadzone, self.pad.swap) =
            (s.pad, s.pad_speed, s.pad_deadzone, s.swap_sticks);
        if let Some(m) = &mut self.menu {
            m.finished = s.finished;
        }
        if let Some(ui) = &mut self.ui {
            ui.style = if s.classic_text { ui::Style::Classic } else { ui::Style::Modern };
            ui.set_speech(s.speech_font, s.speech_size);
        }
        #[cfg(feature = "audio")]
        if let Some(a) = &mut self.audio {
            a.change_volume(s.volume - a.volume);
            a.set_ambience(s.ambience);
            (a.sfx_on, a.steps_on) = (s.effects, s.footsteps);
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
            scale: if self.auto_scale { 0 } else { v.scale },
            touch_buttons: self.touch.buttons,
            zoom: if self.auto_zoom { 0.0 } else { (v.zoom_to * 100.0).round() / 100.0 },
            classic_text: self.ui.as_ref().is_some_and(|u| u.style == ui::Style::Classic),
            speech_font: self.ui.as_ref().map_or(self.settings.speech_font, |u| u.typed()),
            speech_size: self.ui.as_ref().map_or(self.settings.speech_size, |u| u.speech_size),
            cutaway: v.cutaway,
            smooth_camera: v.smooth,
            fullscreen: self.fullscreen,
            left_handed: self.swapped,
            cheats: self.cheats,
            restored: self.restored,
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
            (s.music, s.effects, s.footsteps, s.volume, s.ambience) =
                (a.music_on(), a.sfx_on, a.steps_on, a.volume, a.ambience);
        }
        s
    }

    /// The game knows whether cheats and restored content are on (a loaded game is told again).
    pub(super) fn apply_game_options(&mut self) {
        if let Some(s) = &mut self.game.scripts {
            s.vm.cheats = self.cheats;
        }
        self.game.restored.on = self.restored;
    }

    /// Write the settings file if anything changed.
    pub(super) fn keep_settings(&mut self) {
        let s = self.current_settings();
        if s != self.settings {
            s.save();
            self.settings = s;
        }
    }

    pub(super) fn menu_open(&self) -> bool {
        self.menu.as_ref().is_some_and(|m| m.is_open())
            || self.panel.is_some()
            || self.roll.is_some()
            || self.film.is_some()
            || self.slider.is_some()
    }

    /// Roll the credits or the quotes (from the main menu), to their own music.
    pub(super) fn start_roll(&mut self, r: credits::Roll) {
        let file = u8data::lang::file(&self.root, r.file()).ok().and_then(|p| std::fs::read(p).ok());
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
        self.let_go_of_everything();
    }

    /// Nothing carries on under a menu: no held keys or buttons, no walking.
    fn let_go_of_everything(&mut self) {
        self.held.clear();
        self.pad.stop();
        (self.press, self.pan, self.left, self.right, self.dragging, self.step_until) =
            (None, None, false, false, false, None);
        self.dirty = true;
    }

    /// Open the settings panel (`from_menu`: the book's Options, which it goes back to); F1
    /// again puts it away.
    pub(super) fn open_settings(&mut self, from_menu: bool) {
        if self.panel.is_some() {
            return self.close_settings();
        }
        if let Some(m) = &mut self.menu {
            m.close();
        }
        self.let_go_of_everything();
        let mut p = panel::Panel::new(from_menu);
        p.set_tab(self.panel_tab); // (where it was last time)
        self.panel = Some(p);
        self.refresh_panel();
    }

    pub(super) fn close_settings(&mut self) {
        let Some(p) = self.panel.take() else { return };
        self.panel_tab = p.tab;
        if p.from_menu {
            self.open_menu(menu::Page::Main);
        }
        self.dirty = true;
    }

    /// What the panel did.
    pub(super) fn panel_out(&mut self, out: Option<panel::Out>) {
        match out {
            Some(panel::Out::Close) => self.close_settings(),
            Some(panel::Out::Change(s, d, wrap)) => self.change_setting(s, d, wrap),
            None => {}
        }
        self.refresh_panel();
        self.dirty = true;
    }

    /// The panel's rows for its tab, with their values now.
    pub(super) fn refresh_panel(&mut self) {
        let Some(tab) = self.panel.as_ref().map(|p| p.tab) else { return };
        let rows = panel::ON_TAB
            .get(tab)
            .map(|t| t.iter().filter_map(|&s| self.setting_row(s).map(|(l, v)| (s, l.to_string(), v))).collect())
            .unwrap_or_default();
        if let Some(p) = &mut self.panel {
            p.rows = rows;
        }
    }

    /// A setting's label and value (None: not in this build).
    fn setting_row(&self, s: panel::Setting) -> Option<(&'static str, String)> {
        use panel::Setting::*;
        let v = &self.view;
        let on = |b: bool| if b { "on" } else { "off" }.to_string();
        let tenths = |f: f32| format!("{}%", (f * 10.0).round() * 10.0);
        #[cfg(feature = "audio")]
        let audio = self.audio.as_ref();
        #[cfg(not(feature = "audio"))]
        let audio: Option<&()> = None;
        let pad = cfg!(feature = "gamepad").then_some(&self.pad);
        Some(match s {
            Upscaler => ("Look", v.mode.name().into()),
            Scale => ("Interface size", format!("x{}{}", v.scale, if self.auto_scale { " (auto)" } else { "" })),
            Zoom => (
                "Zoom",
                format!("x{}{}", (v.zoom_to * 100.0).round() / 100.0, if self.auto_zoom { " (auto)" } else { "" }),
            ),
            Fullscreen => ("Full screen", on(self.fullscreen)),
            Brightness => ("Brightness", format!("{:.1}", v.gamma)),
            ColourFix => ("Colour fix", on(v.colour_fix)),
            SmoothCamera => ("Smooth camera", on(v.smooth)),
            FrameTime => ("Frame time in the title", on(v.stats)),
            #[cfg(feature = "audio")]
            Music => {
                let a = audio?;
                ("Music", if a.music_ready { on(a.music_on()) } else { "no SoundFont".into() })
            }
            #[cfg(feature = "audio")]
            Volume => ("Music volume", tenths(audio?.volume)),
            #[cfg(feature = "audio")]
            Song => {
                let a = audio?;
                ("Song", a.current.and_then(|i| a.songs.get(i)).map_or("none".into(), |s| s.name.clone()))
            }
            #[cfg(feature = "audio")]
            Effects => ("Sound effects", on(audio?.sfx_on)),
            #[cfg(feature = "audio")]
            Footsteps => ("Footsteps", on(audio?.steps_on)),
            #[cfg(feature = "audio")]
            Ambience => ("Background sounds", tenths(audio?.ambience)),
            #[cfg(not(feature = "audio"))]
            Music | Volume | Song | Effects | Footsteps | Ambience => {
                audio?;
                return None;
            }
            Buttons => ("Mouse buttons", if self.swapped { "left-handed" } else { "right-handed" }.into()),
            Pad => ("Controller", on(pad?.on)),
            PadSpeed => ("Pointer speed", tenths(pad?.speed)),
            Deadzone => ("Stick dead zone", tenths(pad?.deadzone)),
            SwapSticks => ("Walk with", if pad?.swap { "right stick" } else { "left stick" }.into()),
            TouchButtons => ("Touch screen buttons", on(self.touch.buttons)),
            TextStyle => {
                let classic = self.ui.as_ref().is_some_and(|u| u.style == ui::Style::Classic);
                ("Replies", if classic { "classic" } else { "numbered" }.into())
            }
            SpeechFont => ("Speech type", if self.ui.as_ref()?.typed() { "readable" } else { "original" }.into()),
            SpeechSize => ("Speech size", ui::SPEECH_SIZES[self.ui.as_ref()?.speech_size].0.into()),
            Cutaway => ("See inside buildings", on(v.cutaway)),
            Roofs => ("Roofs", if v.opts.roofs { "shown" } else { "hidden" }.into()),
            Follow => ("Camera", if v.follow { "on the Avatar" } else { "free" }.into()),
            Cheats => ("Cheats", on(self.cheats)),
            Restored => ("Restored content", on(self.restored)),
            Discord => ("Discord status", on(self.discord)),
            Editor => ("Editor view", on(v.opts.editor)),
            Language => {
                let (now, next) = (u8data::lang::of(&self.root), self.settings.language.chars().next());
                let next = next.filter(|c| u8data::lang::available(&self.root).contains(c)).unwrap_or(now);
                let later = if next == now { "" } else { " (next start)" };
                ("Language", format!("{}{later}", u8data::lang::name(next)))
            }
        })
    }

    /// Change a setting by `d` steps; `wrap`: past either end goes round (else it stops there).
    pub(super) fn change_setting(&mut self, s: panel::Setting, d: i32, wrap: bool) {
        use panel::Setting::*;
        // A value stepped by `d`, kept to lo..=hi or taken round.
        let by = |v: f32, step: f32, lo: f32, hi: f32| {
            let n = ((hi - lo) / step).round() as i32 + 1;
            let i = ((v - lo) / step).round() as i32 + d;
            let i = if wrap { i.rem_euclid(n) } else { i.clamp(0, n - 1) };
            lo + i as f32 * step
        };
        let flip = |b: &mut bool| *b = !*b;
        let v = &mut self.view;
        match s {
            Upscaler => v.mode = v.mode.step(d),
            Scale => {
                v.scale = by(v.scale as f32, 1.0, 1.0, MAX_SCALE as f32) as u32;
                self.auto_scale = false; // (chosen now: kept)
            }
            Zoom => self.zoom_by(d as f32),
            Fullscreen => self.command(Command::Fullscreen),
            Brightness => v.gamma = (by(v.gamma, 0.1, 0.3, 3.0) * 10.0).round() / 10.0,
            ColourFix => flip(&mut v.colour_fix),
            SmoothCamera => flip(&mut v.smooth),
            FrameTime => flip(&mut v.stats),
            Music => self.command(Command::MusicToggle),
            Effects => self.command(Command::SfxToggle),
            Footsteps =>
            {
                #[cfg(feature = "audio")]
                if let Some(a) = &mut self.audio {
                    a.steps_on = !a.steps_on;
                }
            }
            Song => self.command(if d < 0 { Command::MusicPrev } else { Command::MusicNext }),
            Volume | Ambience =>
            {
                #[cfg(feature = "audio")]
                if let Some(a) = &mut self.audio {
                    match s {
                        Volume => a.change_volume(by(a.volume, 0.1, 0.0, 1.0) - a.volume),
                        _ => a.set_ambience(by(a.ambience, 0.1, 0.0, 1.0)),
                    }
                }
            }
            Buttons => flip(&mut self.swapped),
            Pad | PadSpeed | Deadzone | SwapSticks => {
                let p = &mut self.pad;
                match s {
                    Pad => flip(&mut p.on),
                    PadSpeed => p.speed = by(p.speed, 0.1, 0.1, 1.0),
                    Deadzone => p.deadzone = by(p.deadzone, 0.1, 0.1, 0.4),
                    _ => flip(&mut p.swap),
                }
                if self.owner == crate::Owner::Pad {
                    self.owner = crate::Owner::Mouse;
                }
            }
            TouchButtons => flip(&mut self.touch.buttons),
            TextStyle => {
                if let Some(ui) = &mut self.ui {
                    ui.toggle_style();
                }
            }
            SpeechFont | SpeechSize => {
                if let Some(ui) = &mut self.ui {
                    let (on, n) = (ui.typed(), ui.speech_size);
                    match s {
                        SpeechFont => ui.set_speech(!on, n),
                        _ => ui.set_speech(on, by(n as f32, 1.0, 0.0, (ui::SPEECH_SIZES.len() - 1) as f32) as usize),
                    }
                }
            }
            Cutaway => flip(&mut v.cutaway),
            Roofs => flip(&mut v.opts.roofs),
            Follow => self.command(Command::ToggleFollow),
            Editor => self.command(Command::ToggleEditor),
            Cheats => {
                self.cheats = !self.cheats;
                self.apply_game_options();
                self.notice(if self.cheats { "Cheats on: click the Avatar." } else { "Cheats off." });
            }
            Restored => {
                self.restored = !self.restored;
                self.apply_game_options();
                self.notice(if self.restored {
                    "Restored content on: no longer the original game."
                } else {
                    "Restored content off."
                });
            }
            Discord => flip(&mut self.discord),
            Language => {
                // (The scripts are loaded at the start: it's the next start's language.)
                let have = u8data::lang::available(&self.root);
                let now = self.settings.language.chars().next().unwrap_or(u8data::lang::of(&self.root));
                let at = have.iter().position(|&c| c == now).unwrap_or(0) as i32;
                if let Some(&c) = have.get(((at + d).rem_euclid(have.len().max(1) as i32)) as usize) {
                    self.settings.language = c.to_string();
                    self.settings.save();
                    if have.len() < 2 {
                        self.notice("Only English is installed.");
                    }
                }
            }
        }
        self.keep_settings();
        self.dirty = true;
    }

    pub(super) fn menu_action(&mut self, a: menu::Action, el: &ActiveEventLoop) {
        use menu::Action;
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
            Action::Settings => self.open_settings(true),
        }
        self.keep_settings();
        self.dirty = true;
    }
}
