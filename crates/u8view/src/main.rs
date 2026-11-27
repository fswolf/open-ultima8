//! Open Ultima8 viewer: plays a [`Game`] in a window. The engine simulates, `u8render` draws the
//! world and, on an overlay, the windows and menus into CPU buffers, and `gpu` shows the world at
//! the zoom (any, smoothly) with the chosen upscaling and the overlay at the interface's size.
//! Mouse, as in the original: hold right to move towards the pointer (short arrow: careful step,
//!   medium: walk, long: run) · both buttons: jump · left click: look · double-click: use / talk
//!   middle drag: pan (frees the camera) · wheel: zoom · Alt-H swaps the buttons
//! Keys: arrows/WASD walk too (Shift run) or scroll the free camera · Space jump · 1–9 pick a reply
//!   Esc game menu (or close pages and speech) · J diary · Backspace close pages and speech · Alt-X quit · +/- zoom · PgUp/PgDn other maps · Home recentre
//!   F1 settings · F2 upscaler · F3 cutaway · F4 roofs · F5 editor view · F6 follow/free camera
//!   F7 / F8 quick save / load · F9 stop all scripts · F10 colour fix · F11 fullscreen · F12 frame time · [ ] gamma
//!   P music on/off (Shift+P effects) · N next song (Shift = previous) · 9/0 volume
//! Controller (Xbox names; Steam Deck, ROG Ally): left stick walk (tilt: careful, walk, run) ·
//!   D-pad walk, or move the choice in menus and replies · right stick pointer · A left button
//!   (tap look, double use, hold drag; accept in menus) · X use · RT right button · LT jump
//!   B close (back in menus) · Y backpack · LB paperdoll · RB combat · Start menu · Back diary
//!   L3 recentre · R3 pointer to the Avatar
//! Touch: tap look · double tap use · press and slide drag · hold on the ground walk · second
//!   finger tap jump (walking) · pinch zoom · two finger tap right button tap · edge buttons
// A game, not a console program: no terminal window behind it on Windows (debug builds keep it,
// for their messages).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
mod art;
#[cfg(feature = "audio")]
mod audio;
mod book;
mod commands;
mod controls;
mod cover;
mod credits;
mod draw;
mod films;
mod frame;
mod gamedir;
mod gpu;
mod gumps;
mod input;
mod menu;
mod mouse;
mod pad;
mod pagefont;
mod panel;
mod presence;
mod settings;
mod slider;
mod touch;
mod ui;
mod view;
mod window;

use anyhow::{anyhow, Result};
use gamedir::find_data;
use input::{Command, Held};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use u8engine::{
    avatar::Input,
    usecode::{event, UiEvent},
    world::AVATAR,
    Game, ObjId,
};
use u8render::{project, text, upscale, Canvas, DrawOpts, Painter, SpriteCache};
use view::*;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::PhysicalKey,
    window::{Window, WindowId},
};

/// xBRZ makes sprites at most this many times bigger (beyond it the GPU scales on).
const MAX_XBRZ: u32 = 6;
/// Free camera speed, game pixels per second.
const SCROLL_SPEED: f32 = 500.0;
/// While idle, wake this often to animate scenery.
const IDLE_WAKE: Duration = Duration::from_millis(100);
const SCRIPT_WAKE: Duration = Duration::from_millis(33);
/// With a controller plugged in, look for its input this often while idle.
const PAD_POLL: Duration = Duration::from_millis(16);
/// Effects fade out this far (game pixels) from the centre of the view.
#[cfg(feature = "audio")]
const HEARING: f32 = 480.0;
const DOUBLE_CLICK: Duration = Duration::from_millis(450);
/// How long the red cross shows when something's out of reach.
const CROSS_FLASH: Duration = Duration::from_millis(500);

/// The tombstone comes this long after the Avatar's death animation has played (the DOS game at
/// its proper speed, timed: 9.8 s; ScummVM waits 5).
const DEATH_PAUSE: Duration = Duration::from_secs(10);
/// The tombstone: its gump (U8GUMPS 27, read in font 11) and words, as AvatarDeathProcess.
const TOMBSTONE: (u16, &str) = (27, "HERE LIES*THE AVATAR*REST IN PEACE");

/// The Avatar's death, step by step: falling; fallen (since when); the tombstone up; the menu.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Death {
    Alive,
    Falling,
    Fallen(Instant),
    Tombstone,
    Over,
}
/// How far (across, up/down) the Avatar can be from a thing whose window stays open.
const WINDOW_RANGE: (i32, i32) = (1024, 256);
/// The paperdoll's fonts: white names, green digits.
const STAT_NAMES: usize = 0;
const STAT_VALUES: usize = 12;
/// A quick right click still takes one careful step this long.
const TAP_STEP: Duration = Duration::from_millis(200);

/// Who moved the one pointer last (the mouse, the pad's stick, a finger).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Owner {
    Mouse,
    Pad,
    Touch,
}

struct App {
    game: Game,
    /// The install, for saving and loading.
    root: PathBuf,
    painter: Painter,
    cache: SpriteCache,
    ui: Option<ui::ScriptUi>,
    view: View,
    held: Held,
    jump: bool,
    /// The jump came from the mouse: aim it with the pointer.
    mouse_jump: bool,
    /// Draw or put away the weapon (C, double right click on the Avatar).
    combat: bool,
    /// Swing (left click in combat stance); kick (double right click); block (left held past a
    /// double click's time, since `swung`).
    attack: bool,
    kick: bool,
    swung: Option<Instant>,
    /// The last right press near the Avatar (half a double click).
    last_right: Option<Instant>,
    /// Earthquake: the view's shake this frame (game pixels).
    shake: (i32, i32),
    /// A script asked the player to pick something (the targeting cursor): its process.
    targeting: Option<u16>,
    /// The credits or quotes rolling (the game waits), and the song to go back to.
    roll: Option<(credits::Scroller, usize)>,
    /// A film showing (the game waits).
    film: Option<films::Showing>,
    /// "How many?" for part of a pile being dropped (the game waits).
    slider: Option<slider::Slider>,
    /// How far the Avatar's death has got (the game is over until a save is loaded).
    death: Death,
    /// Left button down at this position; becomes a drag once it moves a few pixels.
    press: Option<(f64, f64)>,
    dragging: bool,
    /// Middle button panning from this position.
    pan: Option<(f64, f64)>,
    left: bool,
    right: bool,
    /// The left press was half of a both-buttons jump, not a click.
    left_jumped: bool,
    /// Alt-H: left-handed mouse.
    swapped: bool,
    fullscreen: bool,
    /// The original's cheats (Settings): the Avatar's developers' menu.
    cheats: bool,
    /// Restored content (Settings; `u8engine::restore`).
    restored: bool,
    /// Discord Rich Presence (Settings), and the link to Discord.
    discord: bool,
    presence: presence::Presence,
    mods: input::Mods,
    step_until: Option<Instant>,
    cursors: Option<mouse::Cursors>,
    /// The game controller, and the cursor drawn for its pointer (the system one hidden); what
    /// an action it started aims by.
    pad: pad::Pad,
    pad_cursor: Option<usize>,
    cursor_hidden: bool,
    pad_aim: Option<pad::Aim>,
    /// The touch screen.
    touch: touch::Touch,
    /// Who moved the one pointer last; where the system's cursor last was; the window has the
    /// focus.
    owner: Owner,
    os_cursor: (f64, f64),
    focused: bool,
    /// No interface size saved yet: it's chosen for the screen (see `first_scale`).
    auto_scale: bool,
    /// No zoom saved yet: it's chosen for the screen (see `first_zoom`).
    auto_zoom: bool,
    /// Item windows (bags, chests, the paperdoll) and the art they're drawn with.
    windows: gumps::Windows,
    art: Option<art::GumpArt>,
    /// Dragging this item's window from this mouse position.
    win_drag: Option<(ObjId, (f64, f64))>,
    /// The left button is down on the paperdoll's stats button.
    stats_pressed: bool,
    /// The Avatar's STR, INT, DEX, hits and mana when last drawn.
    stats_seen: Option<[i16; 5]>,
    last_win_click: Option<(ObjId, Instant)>,
    /// The item under the mouse when the left button went down, and the item being carried by
    /// the mouse once that press became a drag.
    press_item: Option<ObjId>,
    item_drag: Option<ObjId>,
    /// While dragging, where it would go (its outline there) and the pointer saying it'd be
    /// thrown or won't go (see `drag_preview`).
    drag_ghost: Option<controls::Ghost>,
    drag_cursor: Option<usize>,
    /// The book menus (Esc, J); while one is open the game waits.
    menu: Option<menu::Menu>,
    /// The settings panel (F1, or the book's Options).
    panel: Option<panel::Panel>,
    /// The tab the settings were left on.
    panel_tab: usize,
    /// Settings as last saved (written again whenever they change).
    settings: settings::Settings,
    mouse: (f64, f64),
    last_click: Option<(ObjId, Instant)>,
    /// A single click's look, held until it can't be a double click any more (the original shows
    /// no name on a double click).
    pending_look: Option<(ObjId, Instant)>,
    /// Show the red cross cursor until then (something out of reach was used).
    cross_until: Option<Instant>,
    /// The Avatar was in combat stance last frame (going into it closes the paperdoll).
    was_fighting: bool,
    last_update: Option<Instant>,
    /// Next idle wake-up (scenery keeps animating while nothing else happens).
    next_wake: Instant,
    moving: bool,
    scripting: bool,
    dirty: bool,
    /// The world's frame, and the overlay (windows, speech, menus, films) with its own sprites.
    buf: Vec<u8>,
    overlay: Vec<u8>,
    ui_cache: SpriteCache,
    /// The world texture's pixels per game pixel (xBRZ makes it bigger), and the zoom easing.
    frame_ss: u32,
    /// Canvas pixels per game pixel the overlay was last drawn at.
    overlay_ss: u32,
    /// The world's sprites are xBRZ'd (else plain pixels) at `cache.scale`.
    world_xbrz: bool,
    zooming: bool,
    /// Part of a wheel notch (touchpads) not yet zoomed by; whether the overlay has anything.
    wheel: f32,
    overlay_shown: bool,
    title: String,
    /// When the title last changed (the F12 numbers change it at most twice a second).
    titled: Option<Instant>,
    frame_ms: f32,
    /// The times between updates over the last second (F12 shows them).
    gaps: frame::Gaps,
    #[cfg(feature = "audio")]
    audio: Option<audio::Audio>,
    window: Option<Arc<Window>>,
    gpu: Option<gpu::Gpu>,
    error: Option<anyhow::Error>,
}

impl App {
    fn new(game: Game, data: &Path) -> Self {
        let ui = u8data::find_file(data, "STATIC/U8FONTS.FLX")
            .and_then(|p| u8data::flex::Flex::parse(std::fs::read(p)?))
            .map(|f| ui::ScriptUi::new(u8data::font::Fonts::parse(&f)))
            .map_err(|e| eprintln!("no fonts, scripts can't talk: {e}"))
            .ok();
        let cursors = mouse::Cursors::load(data, &game.data.palette);
        let mut app = Self {
            game,
            root: data.to_path_buf(),
            painter: Painter::new(),
            cache: SpriteCache::unscaled(),
            ui_cache: SpriteCache::unscaled(),
            ui,
            view: View {
                cam: (0.0, 0.0),
                scale: 2,
                zoom: 2.0,
                zoom_to: 2.0,
                mode: Mode::Nearest,
                opts: DrawOpts::default(),
                cutaway: true,
                smooth: true,
                eye: None,
                pan: None,
                follow: true,
                gamma: 1.0,
                colour_fix: std::env::var_os("U8_COLOUR_FIX").is_some(),
                stats: false,
            },
            held: Held::default(),
            jump: false,
            mouse_jump: false,
            combat: false,
            attack: false,
            kick: false,
            swung: None,
            last_right: None,
            shake: (0, 0),
            targeting: None,
            roll: None,
            film: None,
            slider: None,
            death: Death::Alive,
            press: None,
            dragging: false,
            pan: None,
            left: false,
            right: false,
            left_jumped: false,
            swapped: false,
            fullscreen: false,
            cheats: false,
            restored: false,
            discord: true,
            presence: presence::Presence::start(),
            mods: input::Mods::default(),
            step_until: None,
            cursors,
            pad: pad::Pad::default(),
            pad_cursor: None,
            cursor_hidden: false,
            pad_aim: None,
            touch: touch::Touch::new(true),
            owner: Owner::Mouse,
            os_cursor: (-1.0, -1.0),
            focused: true,
            auto_scale: false,
            auto_zoom: false,
            menu: None,
            panel: None,
            panel_tab: 0,
            windows: gumps::Windows::default(),
            art: None,
            win_drag: None,
            stats_pressed: false,
            stats_seen: None,
            last_win_click: None,
            press_item: None,
            item_drag: None,
            drag_ghost: None,
            drag_cursor: None,
            settings: settings::Settings::default(),
            mouse: (0.0, 0.0),
            last_click: None,
            pending_look: None,
            cross_until: None,
            was_fighting: false,
            last_update: None,
            next_wake: Instant::now(),
            moving: false,
            scripting: false,
            dirty: true,
            buf: vec![],
            overlay: vec![],
            frame_ss: 1,
            overlay_ss: 1,
            world_xbrz: false,
            zooming: false,
            wheel: 0.0,
            overlay_shown: false,
            title: String::new(),
            titled: None,
            frame_ms: 0.0,
            gaps: frame::Gaps::default(),
            #[cfg(feature = "audio")]
            audio: audio::Audio::new(data).map_err(|e| eprintln!("audio disabled: {e:#}")).ok(),
            window: None,
            gpu: None,
            error: None,
        };
        app.menu = menu::Menu::load(data, &app.game.data.palette);
        app.art = art::GumpArt::load(data, &app.game.data.palette);
        app.pad.start();
        app.apply_settings(settings::Settings::load());
        #[cfg(feature = "audio")]
        if app.audio.as_ref().is_some_and(|a| !a.music_ready) {
            app.notice("No SoundFont found, so no music: see the README's Music part.");
        }
        if app.game.needs_name() && !app.start_film(films::Which::Intro, films::After::Name) {
            app.open_menu(menu::Page::Name); // the game waits until the Avatar has a name
        }
        app.on_map_loaded();
        app
    }

    /// PgUp/PgDn: look around other maps (the Avatar stays put). The next update reports the change.
    fn change_map(&mut self, step: isize) {
        self.game.step_map(step);
    }

    fn on_map_loaded(&mut self) {
        (self.view.eye, self.view.pan) = (None, None); // (a new place: the view starts on the Avatar)
        self.apply_game_options();
        if let Some(ui) = &mut self.ui {
            ui.clear();
        }
        self.windows.close_all(); // item ids are per map
        (self.item_drag, self.press_item, self.win_drag, self.pending_look) = (None, None, None, None);
        (self.drag_ghost, self.drag_cursor) = (None, None);
        self.painter.sync(&self.game.data, &mut self.game.world, [0; 4]);
        self.view.follow = self.game.avatar.is_some();
        self.recentre();
        #[cfg(feature = "audio")]
        if let Some(m) = &mut self.audio {
            m.hush(); // the old map's scripts (and their sounds) are gone; its music objects pick the song
        }
    }

    fn controlling(&self) -> bool {
        self.view.follow && self.game.avatar.is_some()
    }

    /// A question or a page is open: the player must deal with it first.
    fn ui_blocking(&self) -> bool {
        self.ui.as_ref().is_some_and(|u| u.blocking())
    }
}

fn main() -> Result<()> {
    // With no terminal (Windows), a failure to start is shown in a message box instead.
    start().inspect_err(|e| {
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("Open Ultima8")
            .set_description(format!("{e:#}"))
            .show();
    })
}

fn start() -> Result<()> {
    let mut a: Vec<String> = std::env::args().skip(1).collect();
    if a.iter().any(|s| s == "-h" || s == "--help") {
        println!("usage: u8view [U8 data dir] [map] [--save N]");
        println!("  map: default is wherever the Avatar is in the game being played");
        println!("  --save N: play saved game N from SAVEGAME/ (0 = new game; `u8tool U8 saves` lists them)");
        return Ok(());
    }
    let save = match a.iter().position(|s| s == "--save") {
        Some(i) => {
            let n = a.get(i + 1).ok_or_else(|| anyhow!("--save needs a slot number"))?;
            let n: u32 = n.parse().map_err(|_| anyhow!("save slot must be a number, got {n:?}"))?;
            a.drain(i..i + 2);
            Some(n)
        }
        None => None,
    };
    let data = find_data(a.first())?;
    let map = a.get(1).map(|s| s.parse().map_err(|_| anyhow!("map must be a number, got {s:?}"))).transpose()?;
    u8data::lang::prefer(settings::Settings::load().language.chars().next());
    let game = Game::open(&data, save, map).map_err(|e| anyhow!(e))?;
    let el = EventLoop::new()?;
    el.set_control_flow(ControlFlow::Wait);
    let mut app = App::new(game, &data);
    el.run_app(&mut app)?;
    app.error.map_or(Ok(()), Err)
}
