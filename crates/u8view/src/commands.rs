//! Keyboard commands, saving and loading, and closing the game.
use super::*;

impl App {
    pub(super) fn notice(&mut self, text: &str) {
        if let Some(ui) = &mut self.ui {
            ui.notice(text, &self.game.data.palette);
        }
        self.dirty = true;
    }

    /// Save to `slot`; false (with a notice saying why) if it couldn't.
    pub(super) fn save_to(&mut self, slot: u32, desc: &str) -> bool {
        if self.game.in_stasis() || self.ui_blocking() {
            self.notice("Not now."); // mid-scene: the scripts' state isn't saved yet
            return false;
        }
        match self.game.save(&self.root, slot, desc) {
            Ok(_) => {
                self.notice("Game saved.");
                true
            }
            Err(e) => {
                eprintln!("{e}");
                self.notice("Couldn't save the game.");
                false
            }
        }
    }

    /// Replace the game with saved game `slot`.
    pub(super) fn load(&mut self, slot: u32) {
        match Game::open(&self.root, Some(slot), None) {
            Ok(g) => {
                self.game = g;
                self.death = Death::Alive;
                if let Some(m) = &mut self.menu {
                    m.stay = false;
                }
                self.painter = Painter::new();
                self.held.clear();
                self.on_map_loaded();
                self.notice("Game loaded.");
            }
            Err(e) => {
                eprintln!("{e}");
                self.notice(if slot == u8data::save::QUICK_SLOT {
                    "No quick save to load."
                } else {
                    "Couldn't load that game."
                });
            }
        }
    }

    /// Put away pages and speech (questions stay: the script is waiting for an answer).
    /// True if there was anything to put away.
    pub(super) fn close(&mut self) -> bool {
        let (Some(ui), Some(s)) = (&mut self.ui, &mut self.game.scripts) else { return false };
        let closed = ui.close(&mut s.vm);
        #[cfg(feature = "audio")]
        if let (true, Some(a)) = (closed, &mut self.audio) {
            a.stop_speech();
        }
        closed
    }

    /// A key while something is open to read (Space turns the page; any key puts away a grave).
    pub(super) fn read_key(&mut self, k: winit::keyboard::KeyCode) -> bool {
        let (Some(ui), Some(s)) = (&mut self.ui, &mut self.game.scripts) else { return false };
        let used = ui.read_key(k == winit::keyboard::KeyCode::Space, &mut s.vm);
        self.dirty |= used;
        used
    }

    pub(super) fn command(&mut self, c: Command) {
        let v = &mut self.view;
        match c {
            Command::Quit => self.open_menu(menu::Page::Quit), // "Quit the game?"
            Command::Close => {
                self.close();
                self.close_windows(); // the original's "close all displays"
            }
            Command::Backpack => self.open_backpack(),
            Command::Paperdoll => {
                self.game.trigger(AVATAR, event::USE); // the Avatar's script opens its paperdoll
            }
            Command::SwapButtons => self.swapped = !self.swapped,
            Command::QuickSave => {
                self.save_to(u8data::save::QUICK_SLOT, "Quick save");
            }
            Command::Menu if self.targeting.is_some() => self.end_targeting(0),
            Command::Menu => {
                let closed = self.close() | self.close_windows();
                if !closed {
                    self.open_menu(menu::Page::Main);
                }
            }
            Command::Diary => self.open_menu(menu::Page::Diary(menu::Diary::Write)),
            Command::QuickLoad => self.load(u8data::save::QUICK_SLOT),
            Command::Fullscreen => {
                // Tracked here: the window system may switch a moment later than we ask.
                self.fullscreen = !self.fullscreen;
                if let Some(w) = &self.window {
                    w.set_fullscreen(self.fullscreen.then_some(winit::window::Fullscreen::Borderless(None)));
                }
            }
            Command::ToggleTextStyle => {
                if let Some(ui) = &mut self.ui {
                    ui.toggle_style();
                }
            }
            Command::CycleUpscaler => v.mode = v.mode.next(),
            Command::ToggleRoofs => v.opts.roofs = !v.opts.roofs,
            Command::ToggleEditor => {
                v.opts.editor = !v.opts.editor;
                self.windows.editor = v.opts.editor;
            }
            Command::ToggleCutaway => v.cutaway = !v.cutaway,
            Command::ToggleFollow => {
                v.follow = !v.follow && self.game.avatar.is_some();
                if self.view.follow {
                    self.recentre();
                }
            }
            Command::ZoomIn => self.zoom_by(1.0),
            Command::ZoomOut => self.zoom_by(-1.0),
            Command::PrevMap => self.change_map(-1),
            Command::NextMap => self.change_map(1),
            Command::Recentre => {
                v.follow = self.game.avatar.is_some(); // back to following the Avatar
                self.recentre();
            }
            Command::StopScripts => {
                if let Some(vm) = self.game.vm() {
                    vm.kill_all();
                }
                if let Some(ui) = &mut self.ui {
                    ui.clear();
                }
                #[cfg(feature = "audio")]
                if let Some(a) = &mut self.audio {
                    a.hush();
                }
            }
            Command::Jump => self.jump = true,
            Command::Combat => self.combat = true,
            Command::ToggleColourFix => v.colour_fix = !v.colour_fix,
            Command::ToggleStats => v.stats = !v.stats,
            Command::GammaDown => v.gamma = (v.gamma - 0.1).max(0.3),
            Command::GammaUp => v.gamma = (v.gamma + 0.1).min(3.0),
            Command::Answer(n) => {
                let answered = match (&mut self.ui, &mut self.game.scripts) {
                    (Some(ui), Some(s)) => ui.key(n, &mut s.vm),
                    _ => false,
                };
                if !answered && n == 8 {
                    return self.command(Command::VolumeDown); // 9 doubles as volume down
                }
            }
            #[cfg(feature = "audio")]
            Command::MusicToggle
            | Command::SfxToggle
            | Command::MusicNext
            | Command::MusicPrev
            | Command::VolumeDown
            | Command::VolumeUp => {
                let Some(m) = &mut self.audio else { return };
                match c {
                    Command::SfxToggle => m.toggle_sfx(),
                    Command::MusicToggle => m.toggle(m.songs.iter().position(|s| s.name == "tenebrae").unwrap_or(0)),
                    Command::MusicNext => m.next(1),
                    Command::MusicPrev => m.next(-1),
                    Command::VolumeDown => m.change_volume(-0.1),
                    _ => m.change_volume(0.1),
                }
            }
            #[cfg(not(feature = "audio"))]
            Command::MusicToggle
            | Command::SfxToggle
            | Command::MusicNext
            | Command::MusicPrev
            | Command::VolumeDown
            | Command::VolumeUp => {}
        }
        self.dirty = true;
    }
}
