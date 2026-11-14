//! Open Ultima8 viewer: plays a [`Game`] in a window. The engine simulates, `u8render` draws the
//! world and, on an overlay, the windows and menus into CPU buffers, and `gpu` shows the world at
//! the zoom (any, smoothly) with the chosen upscaling and the overlay at the interface's size.
//! Mouse, as in the original: hold right to move towards the pointer (short arrow: careful step,
//!   medium: walk, long: run) · both buttons: jump · left click: look · double-click: use / talk
//!   middle drag: pan (frees the camera) · wheel: zoom · Alt-H swaps the buttons
//! Keys: arrows/WASD walk too (Shift run) or scroll the free camera · Space jump · 1–9 pick a reply
//!   Esc game menu (or close pages and speech) · J diary · Backspace close pages and speech · Alt-X quit · +/- zoom · PgUp/PgDn other maps · Home recentre
//!   F1 text style · F2 upscaler · F3 cutaway · F4 roofs · F5 editor view · F6 follow/free camera
//!   F7 / F8 quick save / load · F9 stop all scripts · F10 colour fix · F11 fullscreen · F12 frame time · [ ] gamma
//!   P music on/off (Shift+P effects) · N next song (Shift = previous) · 9/0 volume
mod art;
#[cfg(feature = "audio")]
mod audio;
mod book;
mod commands;
mod controls;
mod credits;
mod films;
mod frame;
mod gpu;
mod gumps;
mod input;
mod menu;
mod mouse;
mod pagefont;
mod presence;
mod settings;
mod slider;
mod ui;
mod window;

use anyhow::{anyhow, Result};
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
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::PhysicalKey,
    window::{Window, WindowId},
};

/// The interface's size: overlay pixels per game pixel.
const MAX_SCALE: u32 = 6;
/// The world's zoom (screen pixels per game pixel): a wheel notch (or +/-) moves a rung, and the
/// whole sizes, where every game pixel is the same size, are all on the way.
const ZOOMS: [f32; 13] = [1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 3.5, 4.0, 5.0, 6.0, 7.0, 8.0];
/// Zoomed out, the world shows at most this much (game pixels; the original showed 320×200).
const MOST_SEEN: (f32, f32) = (1920.0, 1200.0);
/// How quickly the zoom eases to where it's going (per second).
const ZOOM_EASE: f32 = 14.0;
/// xBRZ makes sprites at most this many times bigger (beyond it the GPU scales on).
const MAX_XBRZ: u32 = 6;
/// Free camera speed, game pixels per second.
const SCROLL_SPEED: f32 = 500.0;
/// While idle, wake this often to animate scenery.
const IDLE_WAKE: Duration = Duration::from_millis(100);
const SCRIPT_WAKE: Duration = Duration::from_millis(33);
/// Effects fade out this far (game pixels) from the centre of the view.
#[cfg(feature = "audio")]
const HEARING: f32 = 480.0;
const DOUBLE_CLICK: Duration = Duration::from_millis(450);

/// The tombstone comes this long after the Avatar dies, however it died (the DOS game, timed:
/// 12 s after a deadly fall, 13 after a splash into deep water, whose drowning takes a moment).
const DEATH_PAUSE: Duration = Duration::from_secs(12);
/// The tombstone: its gump (U8GUMPS 27, read in font 11) and words, as AvatarDeathProcess.
const TOMBSTONE: (u16, &str) = (27, "HERE LIES*THE AVATAR*REST IN PEACE");

/// The Avatar's death, step by step: dead (since when); the tombstone up; the menu.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Death {
    Alive,
    Dead(Instant),
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

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Nearest,
    Smooth,
    XbrzSprites,
    /// xBR and the CRT look are the GPU's, per screen pixel.
    Xbr,
    Crt,
}

impl Mode {
    const ALL: [Mode; 5] = [Mode::Nearest, Mode::Smooth, Mode::XbrzSprites, Mode::Xbr, Mode::Crt];

    fn next(self) -> Self {
        match self {
            Self::Nearest => Self::Smooth,
            Self::Smooth => Self::XbrzSprites,
            Self::XbrzSprites => Self::Xbr,
            Self::Xbr => Self::Crt,
            Self::Crt => Self::Nearest,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Nearest => "nearest",
            Self::Smooth => "smooth",
            Self::XbrzSprites => "xBRZ sprites",
            Self::Xbr => "xBR",
            Self::Crt => "CRT",
        }
    }
}

/// How the world is shown: camera, zoom, upscaler and what to hide.
struct View {
    /// Centre of the view, unscaled screen coordinates.
    cam: (f32, f32),
    /// The interface's size (windows, menus, speech): screen pixels per game pixel.
    scale: u32,
    /// The world's zoom, and the zoom it's easing to.
    zoom: f32,
    zoom_to: f32,
    mode: Mode,
    opts: DrawOpts,
    cutaway: bool,
    follow: bool,
    gamma: f32,
    colour_fix: bool,
    stats: bool,
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
    shake_seed: u32,
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
    /// The original's cheats (Options): the Avatar's developers' menu.
    cheats: bool,
    /// Discord Rich Presence (Options), and the link to Discord.
    discord: bool,
    presence: presence::Presence,
    mods: input::Mods,
    step_until: Option<Instant>,
    cursors: Option<mouse::Cursors>,
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
    /// The book menus (Esc, J); while one is open the game waits.
    menu: Option<menu::Menu>,
    /// Settings as last saved (written again whenever they change).
    settings: settings::Settings,
    mouse: (f64, f64),
    last_click: Option<(ObjId, Instant)>,
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
    frame_ms: f32,
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
            shake_seed: 0x2545_F491,
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
            discord: true,
            presence: presence::Presence::start(),
            mods: input::Mods::default(),
            step_until: None,
            cursors,
            menu: None,
            windows: gumps::Windows::default(),
            art: None,
            win_drag: None,
            stats_pressed: false,
            stats_seen: None,
            last_win_click: None,
            press_item: None,
            item_drag: None,
            settings: settings::Settings::default(),
            mouse: (0.0, 0.0),
            last_click: None,
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
            frame_ms: 0.0,
            #[cfg(feature = "audio")]
            audio: audio::Audio::new(data).map_err(|e| eprintln!("audio disabled: {e:#}")).ok(),
            window: None,
            gpu: None,
            error: None,
        };
        app.menu = menu::Menu::load(data, &app.game.data.palette);
        app.art = art::GumpArt::load(data, &app.game.data.palette);
        app.apply_settings(settings::Settings::load());
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
        self.apply_cheats();
        if let Some(ui) = &mut self.ui {
            ui.clear();
        }
        self.windows.close_all(); // item ids are per map
        (self.item_drag, self.press_item, self.win_drag) = (None, None, None);
        self.painter.sync(&self.game.data, &mut self.game.world, [0; 4]);
        self.view.follow = self.game.avatar.is_some();
        self.recentre();
        #[cfg(feature = "audio")]
        if let Some(m) = &mut self.audio {
            m.hush(); // the old map's scripts (and their sounds) are gone; its music objects pick the song
        }
    }

    /// Centre on the game's camera (the Avatar, or where a cutscene looks), else the whole map.
    fn recentre(&mut self) {
        let c = self.game.camera().map_or_else(|| self.painter.centre(), |(x, y, z)| project(x, y, z));
        self.view.cam = (c.0 as f32, c.1 as f32);
        self.dirty = true;
    }

    /// Follow the game's camera if it moved.
    /// Keep what the camera follows in the middle, to the world frame's fineness (people are
    /// drawn as finely: the one followed stays put while the world glides by).
    fn follow_camera(&mut self) {
        if let Some((x, y, z)) = self.game.camera() {
            let s = self.world_ss() as i32;
            let (sx, sy) = u8render::project_fine(x, y, z, s);
            let cam = (sx as f32 / s as f32, sy as f32 / s as f32);
            if cam != self.view.cam {
                self.view.cam = cam;
                self.dirty = true;
            }
        }
    }

    fn controlling(&self) -> bool {
        self.view.follow && self.game.avatar.is_some()
    }

    /// A question or a page is open: the player must deal with it first.
    fn ui_blocking(&self) -> bool {
        self.ui.as_ref().is_some_and(|u| u.blocking())
    }

    fn window_size(&self) -> (f32, f32) {
        let sz = self.window.as_ref().map_or(winit::dpi::PhysicalSize::new(1, 1), |w| w.inner_size());
        (sz.width.max(1) as f32, sz.height.max(1) as f32)
    }

    /// The overlay's size in game pixels at the interface's scale (rounded up to cover the window).
    fn view_size(&self) -> (u32, u32) {
        let (w, h) = self.window_size();
        let s = self.view.scale as f32;
        ((w / s).ceil() as u32, (h / s).ceil() as u32)
    }

    /// The world seen at this zoom: its top-left in screen coordinates (game pixels, fractional:
    /// the GPU places the frame to a fraction of a pixel) and how much shows.
    fn world_view(&self) -> ((f32, f32), (f32, f32)) {
        let (w, h) = self.window_size();
        let (w, h) = (w / self.view.zoom, h / self.view.zoom);
        let (qx, qy) = self.shake;
        ((self.view.cam.0 - w / 2.0 + qx as f32, self.view.cam.1 - h / 2.0 + qy as f32), (w, h))
    }

    /// The world's frame: top-left (whole game pixels) and size, a little bigger than what shows
    /// (and in steps, so easing the zoom doesn't make a new texture every frame).
    fn view_origin(&self) -> (i32, i32) {
        let ((x, y), _) = self.world_view();
        (x.floor() as i32, y.floor() as i32)
    }
    fn world_size(&self) -> (u32, u32) {
        let (_, (w, h)) = self.world_view();
        let up = |v: f32| (v.ceil() as u32 + 2).next_multiple_of(32);
        (up(w), up(h))
    }

    fn view_rect(&self) -> [i32; 4] {
        let ((x, y), (w, h)) = (self.view_origin(), self.world_size());
        [x, y, w as i32, h as i32]
    }

    /// Mouse position on the overlay (game pixels at the interface's scale).
    fn mouse_game(&self) -> (i32, i32) {
        let s = self.view.scale as f64;
        ((self.mouse.0 / s) as i32, (self.mouse.1 / s) as i32)
    }

    /// Mouse position in the world (screen coordinates, game pixels).
    fn mouse_world(&self) -> (i32, i32) {
        let ((x, y), _) = self.world_view();
        let z = self.view.zoom as f64;
        ((x as f64 + self.mouse.0 / z).floor() as i32, (y as f64 + self.mouse.1 / z).floor() as i32)
    }

    /// The least zoom for this window (a big screen doesn't show the whole map at once).
    fn min_zoom(&self) -> f32 {
        let (w, h) = self.window_size();
        (w / MOST_SEEN.0).max(h / MOST_SEEN.1).max(ZOOMS[0])
    }

    /// Zoom by `steps` notches (negative: out; a touchpad's part-notches add up), about the
    /// pointer when the camera is free (the spot under it stays put), else about the centre.
    fn zoom_by(&mut self, steps: f32) {
        self.wheel += steps;
        let (notches, min) = (self.wheel.trunc(), self.min_zoom());
        self.wheel -= notches;
        let to = rung(self.view.zoom_to, notches as i32).clamp(min, ZOOMS[ZOOMS.len() - 1]);
        if to != self.view.zoom_to {
            self.view.zoom_to = to;
            self.dirty = true;
        }
    }

    /// Ease the zoom along by `dt` seconds (true while it moves).
    fn ease_zoom(&mut self, dt: f32) -> bool {
        // (A window made bigger can need a closer zoom.)
        self.view.zoom_to = self.view.zoom_to.max(self.min_zoom());
        let (from, to) = (self.view.zoom, self.view.zoom_to);
        if from == to {
            return false;
        }
        let z = if (to - from).abs() < 0.002 { to } else { from + (to - from) * (1.0 - (-dt * ZOOM_EASE).exp()) };
        if z == to {
            self.keep_settings(); // (remembered once it settles)
        }
        if !self.view.follow {
            // The spot under the pointer stays where it is.
            let (w, h) = self.window_size();
            let (mx, my) = (self.mouse.0 as f32, self.mouse.1 as f32);
            let ((ox, oy), _) = self.world_view();
            let (px, py) = (ox + mx / from, oy + my / from);
            self.view.cam = (px - mx / z + w / z / 2.0, py - my / z + h / z / 2.0);
        }
        self.view.zoom = z;
        self.dirty = true;
        true
    }
}

/// `n` rungs of [`ZOOMS`] from `z` (towards the next rung first, if `z` is between two).
fn rung(z: f32, n: i32) -> f32 {
    let mut z = z;
    for _ in 0..n.abs() {
        z = match n > 0 {
            true => ZOOMS.iter().copied().find(|&r| r > z + 1e-3),
            false => ZOOMS.iter().rev().copied().find(|&r| r < z - 1e-3),
        }
        .unwrap_or(z);
    }
    z
}

/// `arg`, else ./U8, else U8 next to the executable (or up from target/<profile>/).
/// `dir` if the game's files are in it, else its U8 folder (any case) if they're in that.
fn game_root(dir: &Path) -> Option<PathBuf> {
    let has_game = |p: &Path| u8data::find_file(p, "STATIC/U8SHAPES.FLX").is_ok();
    [Some(dir.to_path_buf()), u8data::find_file(dir, "U8").ok()].into_iter().flatten().find(|p| has_game(p))
}

/// Where the game's files are: the folder given on the command line; the one picked last time;
/// the game folder itself (the client copied into it) or a U8 folder in it — where it's started
/// from, the program's own folder and up to three above; or, failing those, the player picks the
/// folder (remembered for next time).
fn find_data(arg: Option<&String>) -> Result<PathBuf> {
    if let Some(a) = arg {
        return Ok(PathBuf::from(a));
    }
    let mut s = settings::Settings::load();
    if let Some(p) = Some(&s.data).filter(|d| !d.is_empty()).and_then(|d| game_root(Path::new(d))) {
        return Ok(p);
    }
    let mut dirs = vec![PathBuf::from(".")];
    if let Ok(exe) = std::env::current_exe() {
        dirs.extend(exe.ancestors().skip(1).take(4).map(PathBuf::from));
    }
    if let Some(p) = dirs.iter().find_map(|d| game_root(d)) {
        return Ok(p);
    }
    let found = pick_data().ok_or_else(|| {
        anyhow!("couldn't find Ultima 8's files (STATIC/U8SHAPES.FLX); pass the game folder, e.g. u8view ~/Games/U8")
    })?;
    s.data = found.display().to_string();
    s.save();
    Ok(found)
}

/// Ask for the game's folder until one has its files (None: the player gave up).
fn pick_data() -> Option<PathBuf> {
    use rfd::{FileDialog, MessageButtons, MessageDialog, MessageLevel};
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    loop {
        let mut dialog =
            FileDialog::new().set_title("Where is Ultima 8? Pick the game's folder (the one with STATIC in it)");
        if let Some(h) = &home {
            dialog = dialog.set_directory(h);
        }
        let dir = dialog.pick_folder()?;
        if let Some(root) = game_root(&dir) {
            return Some(root);
        }
        MessageDialog::new()
            .set_level(MessageLevel::Warning)
            .set_title("Not Ultima 8")
            .set_description(format!(
                "{} doesn't have Ultima 8's files in it (STATIC, USECODE, SOUND…). Pick the folder the game is installed in.",
                dir.display()
            ))
            .set_buttons(MessageButtons::Ok)
            .show();
    }
}

fn main() -> Result<()> {
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
    let game = Game::open(&data, save, map).map_err(|e| anyhow!(e))?;
    let el = EventLoop::new()?;
    el.set_control_flow(ControlFlow::Wait);
    let mut app = App::new(game, &data);
    el.run_app(&mut app)?;
    app.error.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The game folder is found whether it's picked itself or the folder with a U8 (any case)
    /// in it; anything else isn't the game.
    #[test]
    fn the_game_folder_is_recognised() {
        let t = std::env::temp_dir().join(format!("u8-root-{}", std::process::id()));
        let game = t.join("Games/ultima8");
        std::fs::create_dir_all(game.join("static")).unwrap();
        std::fs::write(game.join("static/u8shapes.flx"), b"").unwrap();
        std::fs::create_dir_all(t.join("client/u8/STATIC")).unwrap();
        std::fs::write(t.join("client/u8/STATIC/U8SHAPES.FLX"), b"").unwrap();
        assert_eq!(game_root(&game), Some(game.clone()), "the game folder itself");
        assert_eq!(game_root(&t.join("client")), Some(t.join("client/u8")), "its U8 folder");
        assert_eq!(game_root(&t.join("Games")), None, "not the game");
        std::fs::remove_dir_all(&t).ok();
    }

    #[test]
    fn f2_goes_through_every_look() {
        for (i, m) in Mode::ALL.iter().enumerate() {
            assert!(Mode::ALL[(i + 1) % Mode::ALL.len()] == m.next(), "{}", m.name());
            assert_eq!(Mode::ALL.iter().filter(|o| o.name() == m.name()).count(), 1, "names tell them apart");
        }
    }

    #[test]
    fn zoom_steps_by_rungs() {
        assert_eq!(rung(2.0, 1), 2.5);
        assert_eq!(rung(2.0, -4), 1.0);
        assert_eq!(rung(2.0, -9), 1.0, "no further");
        assert_eq!(rung(7.0, 3), 8.0);
        assert_eq!(rung(2.2, 1), 2.5, "from between two");
        assert_eq!(rung(2.2, -1), 2.0);
        assert_eq!(rung(3.0, 0), 3.0);
    }
}
