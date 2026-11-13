//! Where the view looks: following an item (normally the Avatar), or held still and gliding where
//! a cutscene puts it (usecode's `Camera::setCenterOn` and `Camera::scrollTo`). Hosts centre their
//! view on [`Camera::focus`] while they follow the game.
use crate::usecode::Pid;
use crate::world::{ObjId, World, AVATAR};

/// Ticks a scripted glide takes (a little under a second; the original's exact pace isn't known).
pub const SCROLL_TICKS: u32 = 25;
/// How far above an item's base the camera aims (scripts add the same to the Avatar's z).
pub const EYE: i32 = 20;

#[derive(Clone, Debug)]
struct Glide {
    pid: Pid,
    from: (i32, i32, i32),
    to: (i32, i32, i32),
    tick: u32,
}

#[derive(Clone, Debug)]
pub struct Camera {
    follow: Option<ObjId>,
    /// Where it looks when not following (world coordinates, eye height included).
    at: (i32, i32, i32),
    glide: Option<Glide>,
}

impl Default for Camera {
    fn default() -> Self {
        Self { follow: Some(AVATAR), at: (0, 0, 0), glide: None }
    }
}

impl Camera {
    /// The point to centre on (None: following something that isn't on this map).
    pub fn focus(&self, world: &World) -> Option<(i32, i32, i32)> {
        match (self.follow, &self.glide) {
            (Some(id), None) => world.get(id).map(|it| (it.x, it.y, it.z + EYE)),
            _ => Some(self.at),
        }
    }

    /// Follow `id`, or (None) stay where the view is now. Returns a glide that was cut short.
    pub fn set_center_on(&mut self, world: &World, id: Option<ObjId>) -> Option<Pid> {
        if let Some(p) = self.focus(world) {
            self.at = p;
        }
        self.follow = id;
        self.glide.take().map(|g| g.pid)
    }

    /// Glide to `to`; process `pid` finishes on arrival. Returns a glide this one replaces.
    pub fn scroll_to(&mut self, world: &World, pid: Pid, to: (i32, i32, i32)) -> Option<Pid> {
        let from = self.focus(world).unwrap_or(to);
        self.follow = None;
        self.glide.replace(Glide { pid, from, to, tick: 0 }).map(|g| g.pid)
    }

    /// Look at `to` at once, and stay there. Returns a glide this cuts short.
    pub fn move_to(&mut self, to: (i32, i32, i32)) -> Option<Pid> {
        (self.follow, self.at) = (None, to);
        self.glide.take().map(|g| g.pid)
    }

    /// One game tick. Returns the process of a glide that just arrived.
    pub fn tick(&mut self) -> Option<Pid> {
        let g = self.glide.as_mut()?;
        g.tick += 1;
        let t = g.tick.min(SCROLL_TICKS) as f32 / SCROLL_TICKS as f32;
        let t = t * t * (3.0 - 2.0 * t); // ease in and out
        let lerp = |a: i32, b: i32| a + ((b - a) as f32 * t).round() as i32;
        self.at = (lerp(g.from.0, g.to.0), lerp(g.from.1, g.to.1), lerp(g.from.2, g.to.2));
        if g.tick < SCROLL_TICKS {
            return None;
        }
        self.glide.take().map(|g| g.pid)
    }

    /// Back to following the Avatar (a new map, a loaded game).
    pub fn reset(&mut self) -> Option<Pid> {
        self.follow = Some(AVATAR);
        self.glide.take().map(|g| g.pid)
    }
}
