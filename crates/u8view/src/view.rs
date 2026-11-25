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
    /// world, a step glides (the original follows the feet, which zoomed in jolts the screen).
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
            let cam = (sx as f32 / s as f32, sy as f32 / s as f32 - eye);
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
