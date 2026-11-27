//! How the world is shown: the camera and its height, the interface's size and the world's
//! zoom (picked for the screen until the player picks), the looks, and where the view and the
//! pointer are in game pixels.
use super::*;

/// The interface's size: overlay pixels per game pixel.
pub(crate) const MAX_SCALE: u32 = 6;
/// The world's zoom (screen pixels per game pixel): a wheel notch (or +/-) moves a rung, and the
/// whole sizes, where every game pixel is the same size, are all on the way.
pub(crate) const ZOOMS: [f32; 13] = [1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 3.5, 4.0, 5.0, 6.0, 7.0, 8.0];
/// Zoomed out, the world shows at most this much (game pixels; the original showed 320×200).
pub(crate) const MOST_SEEN: (f32, f32) = (1920.0, 1200.0);
/// How quickly the zoom eases to where it's going (per second).
pub(crate) const ZOOM_EASE: f32 = 14.0;

/// The original shows the world 2 pixels lower than centring on the camera's point would (the
/// Avatar's tunic at rows 82–95 of 200 in DOS, 80–93 centred).
pub(crate) const VIEW_DROP: f32 = 2.0;

/// The interface size to start at when none is saved: on a small screen the game fills (a
/// handheld's: the Deck 1280×800, the Ally 1920×1080), a size that reads and taps well there;
/// elsewhere 2 to a point (`dpi` pixels each: a Mac's Retina screen 2, Windows at 150% 1.5).
pub(crate) fn first_scale(window: (u32, u32), monitor: (u32, u32), dpi: f64) -> u32 {
    let full = window.0 >= monitor.0 && window.1 >= monitor.1;
    let base = ((2.0 * dpi).round() as u32).clamp(2, MAX_SCALE);
    if full && monitor.1 <= 1200 {
        (monitor.1 / 266).max(base)
    } else {
        base
    }
}

/// The world's zoom to start at when none is saved: 2 to a point, on the zoom's rungs.
pub(crate) fn first_zoom(dpi: f64) -> f32 {
    let want = (2.0 * dpi) as f32;
    ZOOMS.into_iter().min_by(|a, b| (a - want).abs().total_cmp(&(b - want).abs())).unwrap_or(2.0)
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Mode {
    Nearest,
    Smooth,
    XbrzSprites,
    /// xBR and the CRT look are the GPU's, per screen pixel.
    Xbr,
    Crt,
}

impl Mode {
    pub(crate) const ALL: [Mode; 5] = [Mode::Nearest, Mode::Smooth, Mode::XbrzSprites, Mode::Xbr, Mode::Crt];

    pub(crate) fn next(self) -> Self {
        self.step(1)
    }
    /// `d` looks on (or back), round the list.
    pub(crate) fn step(self, d: i32) -> Self {
        let i = Self::ALL.iter().position(|&m| m == self).unwrap_or(0) as i32;
        Self::ALL[(i + d).rem_euclid(Self::ALL.len() as i32) as usize]
    }
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Nearest => "nearest",
            Self::Smooth => "smooth",
            Self::XbrzSprites => "xBRZ sprites",
            Self::Xbr => "xBR",
            Self::Crt => "CRT",
        }
    }
}

/// The smooth camera: how fast its height eases (seconds to about two thirds of the way); a
/// change too big to ease (a teleport); how far over its footing the Avatar can be and still be
/// taken for on it (a jump's arc; higher, it's climbing, hanging or falling, and followed); and
/// close enough to be there (so the view stops redrawing).
pub(crate) const EYE_EASE: f32 = 0.08;
pub(crate) const EYE_SNAP: f32 = 160.0;
pub(crate) const EYE_LIFT: i32 = 32;
pub(crate) const EYE_THERE: f32 = 0.01;

/// The smooth camera along the ground keeps a pace of its own (the Avatar's frames carry him
/// unevenly, as the original's do; following his every frame surges the world): its speed eases
/// to the steady pace of what he's doing (seconds to two thirds of the way, speeding up and
/// slowing), and it's put right by how far he's been from the middle over his last whole cycle
/// (which cancels his stride's surge), taken up slowly. With no pace (standing, turning,
/// jumping...) it eases straight to him. A jump too big to glide snaps (game pixels: a teleport,
/// a new place).
pub(crate) const PACE_UP: f32 = 0.12;
pub(crate) const PACE_DOWN: f32 = 0.05;
pub(crate) const PUT_RIGHT: f32 = 0.5;
pub(crate) const CATCH_UP: f32 = 0.10;
pub(crate) const PAN_SNAP: f32 = 120.0;

/// Where the smooth camera centres along the ground (screen, game pixels), how fast it's going
/// (game pixels a second), and how far he's been from it, frame by frame, over his last cycle
/// (each frame's time and the offset).
#[derive(Clone, Debug, Default)]
pub(crate) struct Pan {
    pub(crate) at: (f32, f32),
    vel: (f32, f32),
    seen: std::collections::VecDeque<(f32, (f32, f32))>,
}

impl Pan {
    pub(crate) fn new(at: (f32, f32)) -> Self {
        Self { at, ..Self::default() }
    }

    /// `dt` seconds on, following `to` (where he's drawn), at his steady `pace` (game pixels a
    /// second, and his cycle's length in seconds), if he has one.
    pub(crate) fn step(&mut self, to: (f32, f32), pace: Option<((f32, f32), f32)>, dt: f32) {
        let off = |a: (f32, f32)| (to.0 - a.0, to.1 - a.1);
        let far = |d: (f32, f32)| d.0.abs().max(d.1.abs());
        if far(off(self.at)) >= PAN_SNAP {
            *self = Self::new(to);
            return;
        }
        let k = |t: f32| 1.0 - (-dt / t).exp();
        let mv = |a: (f32, f32), d: (f32, f32), f: f32| (a.0 + d.0 * f, a.1 + d.1 * f);
        match pace {
            Some((p, cycle)) => {
                // (Only the speed's aim changes with a change of pace: no jolt.)
                self.vel = mv(self.vel, (p.0 - self.vel.0, p.1 - self.vel.1), k(ease(self.vel, p)));
                self.at = mv(self.at, self.vel, dt);
                self.seen.push_back((dt, off(self.at)));
                let mut total: f32 = self.seen.iter().map(|s| s.0).sum();
                while self.seen.len() > 1 && total - self.seen[0].0 >= cycle {
                    total -= self.seen.pop_front().map_or(0.0, |s| s.0);
                }
                // (The oldest frame counts only for the part of it inside the cycle.)
                let first = (cycle - (total - self.seen[0].0)).clamp(0.0, self.seen[0].0);
                let span = total - self.seen[0].0 + first;
                let sum = self.seen.iter().enumerate().fold((0.0, 0.0), |m, (i, &(t, d))| {
                    let w = if i == 0 { first } else { t };
                    (m.0 + d.0 * w, m.1 + d.1 * w)
                });
                if span > 0.0 {
                    self.at = mv(self.at, (sum.0 / span, sum.1 / span), k(PUT_RIGHT));
                }
            }
            None => {
                self.seen.clear();
                let before = off(self.at);
                let want = (before.0 / CATCH_UP, before.1 / CATCH_UP);
                self.vel = mv(self.vel, (want.0 - self.vel.0, want.1 - self.vel.1), k(ease(self.vel, want)));
                self.at = mv(self.at, self.vel, dt);
                let after = off(self.at);
                // (Never past him: crossing over, it's on him and stops.)
                if before.0 * after.0 + before.1 * after.1 < 0.0 || far(after) <= EYE_THERE {
                    (self.at, self.vel) = (to, (0.0, 0.0));
                }
            }
        }
    }
}

/// How fast a speed `v` eases to `to`: speeding up the slower way; slowing, or turned against
/// the way it goes (he's turned about), the quicker, so the view doesn't run on past him.
fn ease(v: (f32, f32), to: (f32, f32)) -> f32 {
    let up = to.0.hypot(to.1) >= v.0.hypot(v.1) && v.0 * to.0 + v.1 * to.1 >= 0.0;
    if up {
        PACE_UP
    } else {
        PACE_DOWN
    }
}

/// The Avatar's steady pace (see [`u8engine::avatar::Avatar::pace`]) along the screen, in game
/// pixels a second, and his cycle's length.
pub(crate) fn screen_pace(game: &u8engine::Game) -> Option<((f32, f32), f32)> {
    let ((vx, vy), cycle) = game.avatar.as_ref()?.pace(&game.data, &game.world)?;
    Some((((vx - vy) / 4.0, (vx + vy) / 8.0), cycle))
}

/// Where the view centres along the ground following `to`: there (smooth camera off), or the
/// smooth camera's [`Pan`] `dt` seconds on.
pub(crate) fn ground(
    pan: &mut Option<Pan>,
    smooth: bool,
    to: (f32, f32),
    pace: Option<((f32, f32), f32)>,
    dt: f32,
) -> (f32, f32) {
    match (smooth, pan) {
        (false, pan) => {
            *pan = None;
            to
        }
        (true, Some(pan)) => {
            pan.step(to, pace, dt);
            pan.at
        }
        (true, pan) => pan.insert(Pan::new(to)).at,
    }
}

/// The height the view looks at: the camera's own (`z`), or with the smooth camera (`smooth`),
/// eased over `dt` seconds from where it was (`at`) towards the Avatar's footing (the camera's
/// own height when it's well above it), straight there when it's new or too far.
pub(crate) fn eye_height(z: i32, footing: Option<i32>, at: Option<f32>, dt: f32, smooth: bool) -> f32 {
    let to = footing.filter(|&f| smooth && z - f <= EYE_LIFT).unwrap_or(z) as f32;
    match at {
        Some(at) if smooth && (to - at).abs() < EYE_SNAP && (to - at).abs() > EYE_THERE => {
            at + (to - at) * (1.0 - (-dt / EYE_EASE).exp())
        }
        _ => to,
    }
}

/// How the world is shown: camera, zoom, upscaler and what to hide.
pub(crate) struct View {
    /// Centre of the view, unscaled screen coordinates.
    pub(crate) cam: (f32, f32),
    /// The interface's size (windows, menus, speech): screen pixels per game pixel.
    pub(crate) scale: u32,
    /// The world's zoom, and the zoom it's easing to.
    pub(crate) zoom: f32,
    pub(crate) zoom_to: f32,
    pub(crate) mode: Mode,
    pub(crate) opts: DrawOpts,
    pub(crate) cutaway: bool,
    /// Ease up and down with the Avatar's footing (Settings: smooth camera); the height the view
    /// looks at now (world units).
    pub(crate) smooth: bool,
    pub(crate) eye: Option<f32>,
    /// With the smooth camera, where along the ground the view centres and how it's moving.
    pub(crate) pan: Option<Pan>,
    pub(crate) follow: bool,
    pub(crate) gamma: f32,
    pub(crate) colour_fix: bool,
    pub(crate) stats: bool,
}

impl App {
    /// Centre on the game's camera (the Avatar, or where a cutscene looks), else the whole map.
    pub(crate) fn recentre(&mut self) {
        let c = self.game.camera().map_or_else(|| self.painter.centre(), |(x, y, z)| project(x, y, z));
        self.view.cam = (c.0 as f32, c.1 as f32);
        self.dirty = true;
    }

    /// Follow the game's camera if it moved.
    /// Keep what the camera follows in the middle, to the world frame's fineness (people are
    /// drawn as finely: the one followed stays put while the world glides by).
    /// Centre the view on the camera, `dt` seconds on. With the smooth camera, following the
    /// Avatar, the height eases to its footing rather than its feet: a jump doesn't bounce the
    /// world, a step glides (the original follows the feet, which zoomed in jolts the screen);
    /// and along the ground it keeps a steady pace of its own ([`Pan`]) rather than his uneven steps.
    pub(crate) fn follow_camera(&mut self, dt: f32) {
        if let Some((x, y, z)) = self.game.camera() {
            let s = self.world_ss() as i32;
            // (Only while the view is on the Avatar itself: not where it was left when the
            // Avatar's gone to the map's corner, drowned.)
            let on = self.game.world.get(AVATAR).is_some_and(|a| (a.x, a.y) == (x, y));
            // (Following him as he's drawn: going on to where he is.)
            let (lx, ly, lz) = if on { self.game.world.lag(AVATAR) } else { (0, 0, 0) };
            let (x, y, z) = (x + lx, y + ly, z + lz);
            let smooth = self.view.smooth && self.game.camera_on_avatar() && on;
            let footing = self.game.world.floor_under(AVATAR).map(|f| f + u8engine::camera::EYE);
            let eye = eye_height(z, footing, self.view.eye, dt, smooth);
            self.view.eye = Some(eye);
            let (sx, sy) = u8render::project_fine(x, y, 0, s);
            let to = (sx as f32 / s as f32, sy as f32 / s as f32);
            let ground = ground(&mut self.view.pan, smooth, to, screen_pace(&self.game), dt);
            let cam = (ground.0, ground.1 - eye);
            if cam != self.view.cam {
                self.view.cam = cam;
                self.dirty = true;
            }
        }
    }

    /// With no interface size saved, pick it for the screen (see [`first_scale`]).
    pub(crate) fn fit_interface(&mut self) {
        let Some(w) = self.window.as_ref() else { return };
        let (sz, monitor, dpi) = (w.inner_size(), w.current_monitor().map(|m| m.size()), w.scale_factor());
        if let (Some(m), true) = (monitor, self.auto_scale) {
            let s = first_scale((sz.width, sz.height), (m.width, m.height), dpi);
            if s != self.view.scale {
                (self.view.scale, self.dirty) = (s, true);
            }
        }
        if self.auto_zoom {
            let z = first_zoom(dpi).max(self.min_zoom());
            if z != self.view.zoom_to {
                (self.view.zoom, self.view.zoom_to, self.dirty) = (z, z, true);
            }
        }
    }

    pub(crate) fn window_size(&self) -> (f32, f32) {
        let sz = self.window.as_ref().map_or(winit::dpi::PhysicalSize::new(1, 1), |w| w.inner_size());
        (sz.width.max(1) as f32, sz.height.max(1) as f32)
    }

    /// The overlay's size in game pixels at the interface's scale (rounded up to cover the window).
    pub(crate) fn view_size(&self) -> (u32, u32) {
        let (w, h) = self.window_size();
        let s = self.view.scale as f32;
        ((w / s).ceil() as u32, (h / s).ceil() as u32)
    }

    /// The world seen at this zoom: its top-left in screen coordinates (game pixels, fractional:
    /// the GPU places the frame to a fraction of a pixel) and how much shows.
    pub(crate) fn world_view(&self) -> ((f32, f32), (f32, f32)) {
        let (w, h) = self.window_size();
        let (w, h) = (w / self.view.zoom, h / self.view.zoom);
        let (qx, qy) = self.shake;
        ((self.view.cam.0 - w / 2.0 + qx as f32, self.view.cam.1 - h / 2.0 - VIEW_DROP + qy as f32), (w, h))
    }

    /// The world's frame: top-left (whole game pixels) and size, a little bigger than what shows
    /// (and in steps, so easing the zoom doesn't make a new texture every frame).
    pub(crate) fn view_origin(&self) -> (i32, i32) {
        let ((x, y), _) = self.world_view();
        (x.floor() as i32, y.floor() as i32)
    }
    pub(crate) fn world_size(&self) -> (u32, u32) {
        let (_, (w, h)) = self.world_view();
        let up = |v: f32| (v.ceil() as u32 + 2).next_multiple_of(32);
        (up(w), up(h))
    }

    pub(crate) fn view_rect(&self) -> [i32; 4] {
        let ((x, y), (w, h)) = (self.view_origin(), self.world_size());
        [x, y, w as i32, h as i32]
    }

    /// Mouse position on the overlay (game pixels at the interface's scale).
    pub(crate) fn mouse_game(&self) -> (i32, i32) {
        let s = self.view.scale as f64;
        ((self.mouse.0 / s) as i32, (self.mouse.1 / s) as i32)
    }

    /// Mouse position in the world (screen coordinates, game pixels).
    pub(crate) fn mouse_world(&self) -> (i32, i32) {
        let ((x, y), _) = self.world_view();
        let z = self.view.zoom as f64;
        ((x as f64 + self.mouse.0 / z).floor() as i32, (y as f64 + self.mouse.1 / z).floor() as i32)
    }

    /// The least zoom for this window (a big screen doesn't show the whole map at once).
    pub(crate) fn min_zoom(&self) -> f32 {
        let (w, h) = self.window_size();
        (w / MOST_SEEN.0).max(h / MOST_SEEN.1).max(ZOOMS[0])
    }

    /// Zoom by `steps` notches (negative: out; a touchpad's part-notches add up), about the
    /// pointer when the camera is free (the spot under it stays put), else about the centre.
    pub(crate) fn zoom_by(&mut self, steps: f32) {
        self.wheel += steps;
        let (notches, min) = (self.wheel.trunc(), self.min_zoom());
        self.wheel -= notches;
        let to = rung(self.view.zoom_to, notches as i32).clamp(min, ZOOMS[ZOOMS.len() - 1]);
        if to != self.view.zoom_to {
            self.view.zoom_to = to;
            self.auto_zoom = false; // (chosen now: kept)
            self.dirty = true;
        }
    }

    /// Ease the zoom along by `dt` seconds (true while it moves).
    pub(crate) fn ease_zoom(&mut self, dt: f32) -> bool {
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
            self.view.cam = (px - mx / z + w / z / 2.0, py - my / z + h / z / 2.0 + VIEW_DROP);
        }
        self.view.zoom = z;
        self.dirty = true;
        true
    }
}

/// `n` rungs of [`ZOOMS`] from `z` (towards the next rung first, if `z` is between two).
pub(crate) fn rung(z: f32, n: i32) -> f32 {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The smooth camera eases a step up over a fraction of a second, ignores a jump's arc (the
    /// footing doesn't change), snaps to a teleport, and off it follows the camera's own height.
    #[test]
    fn the_smooth_camera_eases_up_and_down() {
        let frame = 1.0 / 60.0;
        assert_eq!(eye_height(92, Some(68), Some(48.0), frame, false), 92.0, "off: the camera's height");
        assert_eq!(eye_height(92, Some(68), Some(68.0), frame, true), 68.0, "mid-jump: still the footing");
        let mut at = 68.0;
        for _ in 0..3 {
            at = eye_height(76, Some(76), Some(at), frame, true);
        }
        assert!(at > 68.5 && at < 75.0, "a step up eases: {at}");
        for _ in 0..60 {
            at = eye_height(76, Some(76), Some(at), frame, true);
        }
        assert_eq!(at, 76.0, "and gets there (the view stops changing) within a second");
        assert_eq!(eye_height(500, Some(500), Some(68.0), frame, true), 500.0, "a teleport snaps");
        // Falling (or climbing, hanging): well above its footing, the Avatar itself is followed.
        let falling = eye_height(160, Some(68), Some(150.0), frame, true);
        assert!(falling > 150.0 && falling < 160.0, "{falling}");
    }

    /// Turned about at a run, the view's speed turns the quick way: it runs on no more than a
    /// pixel or so the old way (eased at the slow speeding-up rate, it ran on about 5).
    #[test]
    fn turned_about_the_view_doesnt_run_on() {
        let (dt, west, east) = (1.0 / 60.0, ((-180.0, 0.0), 0.6), ((180.0, 0.0), 0.6));
        let (mut pan, mut to) = (Pan::new((0.0, 0.0)), 0.0);
        for _ in 0..120 {
            to -= 3.0;
            pan.step((to, 0.0), Some(west), dt);
        }
        let turned = pan.at.0;
        let mut furthest = turned;
        for _ in 0..60 {
            to += 3.0;
            pan.step((to, 0.0), Some(east), dt);
            furthest = furthest.min(pan.at.0);
        }
        assert!(turned - furthest < 2.5, "ran on {}", turned - furthest);
    }

    #[test]
    fn handhelds_start_with_a_bigger_interface() {
        assert_eq!(first_scale((1280, 800), (1280, 800), 1.0), 3, "Steam Deck");
        assert_eq!(first_scale((1920, 1080), (1920, 1080), 1.0), 4, "ROG Ally");
        assert_eq!(first_scale((1280, 800), (2560, 1440), 1.0), 2, "a window on a big monitor");
        assert_eq!(first_scale((2560, 1440), (2560, 1440), 1.0), 2, "a big monitor");
        // Screens with more pixels to a point: as big to the eye as on an ordinary one.
        assert_eq!(first_scale((2560, 1600), (2880, 1800), 2.0), 4, "a Mac's Retina screen");
        assert_eq!(first_scale((1920, 1200), (2880, 1800), 1.5), 3, "Windows at 150%");
        assert_eq!((first_zoom(1.0), first_zoom(2.0), first_zoom(1.5), first_zoom(1.25)), (2.0, 4.0, 3.0, 2.5));
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

#[cfg(test)]
mod ride {
    use super::*;
    use u8engine::avatar::Input;

    /// Each frame: where he's drawn and where the view centres (screen, game pixels), and his pace.
    pub(crate) struct Frame {
        pub to: (f32, f32),
        pub at: (f32, f32),
        pub pace: bool,
    }

    pub(crate) fn open(map: u16, at: (i32, i32, i32)) -> Option<u8engine::Game> {
        let dir = std::path::PathBuf::from(std::env::var("U8_DATA").ok()?);
        let mut g = u8engine::Game::open(&dir, Some(2), None).ok()?;
        assert!(g.put_avatar(map, at));
        Some(g)
    }

    /// Play `secs` at `fps` with `input(t)`, following as the viewer does (zoom 3).
    pub(crate) fn ride(
        g: &mut u8engine::Game,
        pan: &mut Option<Pan>,
        fps: f32,
        secs: f32,
        smooth: bool,
        input: impl Fn(f32) -> Input,
    ) -> Vec<Frame> {
        let (dt, s) = (1.0 / fps, 3);
        (0..(fps * secs) as usize)
            .map(|i| {
                g.update(dt, &input(i as f32 * dt));
                g.take_events();
                let (a, l) = (g.world.get(AVATAR).unwrap(), g.world.lag(AVATAR));
                let (sx, sy) = u8render::project_fine(a.x + l.0, a.y + l.1, 0, s);
                let to = (sx as f32 / s as f32, sy as f32 / s as f32);
                let pace = screen_pace(g);
                let at = ground(pan, smooth, to, pace, dt);
                Frame { to, at, pace: pace.is_some() }
            })
            .collect()
    }

    pub(crate) fn run(dir: usize, run: bool) -> impl Fn(f32) -> Input {
        move |_| Input { dir: Some(dir), run, ..Default::default() }
    }

    /// How far the view moved each frame.
    pub(crate) fn moves(f: &[Frame]) -> Vec<f32> {
        f.windows(2).map(|w| (w[1].at.0 - w[0].at.0).hypot(w[1].at.1 - w[0].at.1)).collect()
    }
    pub(crate) fn off(f: &Frame) -> f32 {
        (f.to.0 - f.at.0).hypot(f.to.1 - f.at.1)
    }

    /// The biggest change from one frame's move to the next (game pixels): a jolt.
    pub(crate) fn jolt(f: &[Frame]) -> f32 {
        let d = |w: &[Frame], i: usize| (w[i + 1].at.0 - w[i].at.0, w[i + 1].at.1 - w[i].at.1);
        f.windows(3).map(|w| (d(w, 1).0 - d(w, 0).0).hypot(d(w, 1).1 - d(w, 0).1)).fold(0.0, f32::max)
    }

    /// A run and a walk on the East Road (west, and a diagonal south-west) at 60, 90 and 144
    /// frames a second: once steady the view moves within 4 percent of its mean each frame, and
    /// he stays within 10 game pixels of the middle (a little more on the faster diagonal);
    /// setting off he gets no further than 17 (50 screen pixels at zoom 3).
    #[test]
    #[ignore]
    fn the_view_keeps_an_even_pace_on_real_moves() {
        let cases = [
            (6, true, 60.0, (9000, 12000), 10.0),
            (6, true, 90.0, (9000, 12000), 10.0),
            (6, true, 144.0, (9000, 12000), 10.0),
        ];
        let walks = [
            (6, false, 60.0, (9000, 12000), 10.0),
            (6, false, 144.0, (9000, 12000), 10.0),
            (5, true, 60.0, (8000, 12000), 11.0),
        ];
        for (dir, runs, fps, (x, y), most) in cases.into_iter().chain(walks) {
            let Some(mut g) = open(5, (x, y, 48)) else { return };
            let f = ride(&mut g, &mut None, fps, 5.0, true, run(dir, runs));
            let steady = &f[(fps * 2.5) as usize..];
            assert!(steady.iter().all(|x| x.pace), "{dir} {runs} {fps}: held on all the way");
            let m = moves(steady);
            let mean = m.iter().sum::<f32>() / m.len() as f32;
            assert!(m.iter().all(|v| (v / mean - 1.0).abs() < 0.04), "{dir} {runs} {fps}: {m:?}");
            let o = steady.iter().map(off).fold(0f32, f32::max);
            assert!(o <= most, "{dir} {runs} {fps}: he's {o} off the middle");
            let first = f[..fps as usize].iter().map(off).fold(0f32, f32::max);
            assert!(first <= 17.0, "{dir} {runs} {fps}: setting off he's {first} off");
        }
    }

    /// Letting go of a run: the view comes to rest on him without ever passing him, within
    /// 0.8 s; a teleport snaps; with the smooth camera off it's on him every frame.
    #[test]
    #[ignore]
    fn stopping_snapping_and_off() {
        for fps in [60.0, 144.0] {
            let Some(mut g) = open(5, (9000, 12000, 48)) else { return };
            let f = ride(&mut g, &mut None, fps, 4.0, true, |t| Input {
                dir: (t < 2.5).then_some(6),
                run: true,
                ..Default::default()
            });
            let go = (fps * 2.5) as usize;
            let last = f[f.len() - 1].to;
            let way = (last.0 - f[go].to.0, last.1 - f[go].to.1);
            for x in &f[go..] {
                assert!((x.to.0 - x.at.0) * way.0 + (x.to.1 - x.at.1) * way.1 >= -0.01, "{fps}: past him");
            }
            let rest = go + (fps * 0.8) as usize;
            assert!(f[rest..].iter().all(|x| x.at == x.to), "{fps}: at rest on him 0.8 s after letting go");
        }
        let Some(mut g) = open(5, (9000, 12000, 48)) else { return };
        let mut pan = None;
        ride(&mut g, &mut pan, 60.0, 2.0, true, run(6, true));
        assert!(g.put_avatar(5, (9000, 12000, 48)));
        let f = ride(&mut g, &mut pan, 60.0, 0.1, true, |_| Input::default());
        assert_eq!(f[0].at, f[0].to, "a teleport snaps");
        let f = ride(&mut g, &mut pan, 60.0, 2.0, false, run(6, true));
        assert!(f.iter().all(|x| x.at == x.to), "off: on him");
    }

    /// No jolts: a turn mid-run, a sidestep round something in the way (the view's speed only
    /// eases to the new pace), a run into a wall and a leap from a run (it comes to rest on
    /// him); a jolt here is the change of a frame's move from the last's, in game pixels.
    #[test]
    #[ignore]
    fn turns_sidesteps_walls_and_leaps_dont_jolt() {
        use u8data::typeflag::flag;
        let Some(mut g) = open(5, (9000, 12000, 48)) else { return };
        let f = ride(&mut g, &mut None, 60.0, 5.0, true, |t| Input {
            dir: Some(if t < 2.5 { 6 } else { 5 }),
            run: true,
            ..Default::default()
        });
        assert!(jolt(&f[100..]) < 0.4, "turn: {}", jolt(&f[100..]));
        // Something in the way, a little off his line: he steps round it.
        let Some(mut g) = open(5, (9000, 12000, 48)) else { return };
        let small = |g: &u8engine::Game, id: u16| {
            let it = g.world.get(id).unwrap();
            let si = g.data.typeflags.get(it.shape);
            let (x, y, z) = si.dims();
            si.has(flag::SOLID) && !si.has(flag::FIXED) && (32..=64).contains(&x) && (32..=64).contains(&y) && z >= 16
        };
        let thing = g.world.objects().find(|&id| id > 255 && small(&g, id)).expect("something to put in the way");
        let mut it = g.world.get(thing).unwrap();
        (it.x, it.y, it.z) = (8600, 12024, 48);
        g.world.set(&g.data, thing, it);
        let f = ride(&mut g, &mut None, 60.0, 3.0, true, run(6, true));
        assert!(g.world.get(AVATAR).unwrap().y < 12000, "he stepped round it");
        assert!(jolt(&f[30..]) < 1.0, "sidestep: {}", jolt(&f[30..]));
        // Into the castle's wall, and a leap from a run.
        let Some(mut g) = open(40, (12774, 9010, 48)) else { return };
        let f = ride(&mut g, &mut None, 60.0, 3.0, true, run(4, true));
        assert!(jolt(&f) < 0.6 && f[f.len() - 1].at == f[f.len() - 1].to, "wall: {}", jolt(&f));
        let Some(mut g) = open(5, (9000, 12000, 48)) else { return };
        let f = ride(&mut g, &mut None, 60.0, 4.0, true, |t| Input {
            dir: Some(6),
            run: true,
            jump: (t - 2.0).abs() < 0.01,
            ..Default::default()
        });
        assert!(f.iter().filter(|x| !x.pace).count() > 20, "leapt");
        assert!(
            jolt(&f[100..]) < 1.5 && f[100..].iter().map(off).fold(0f32, f32::max) < PAN_SNAP / 2.0,
            "leap: {}",
            jolt(&f[100..])
        );
    }
}
