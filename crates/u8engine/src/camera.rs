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
            // Something put away to the map's origin (the Avatar drowning: its body goes to
            // 0,0,0) leaves the view where it last was, as the DOS game's.
            (Some(id), None) => match world.get(id) {
                Some(it) if (it.x, it.y, it.z) == (0, 0, 0) => Some(self.at),
                it => it.map(|it| (it.x, it.y, it.z + EYE)),
            },
            _ => Some(self.at),
        }
    }

    /// What it follows, if it isn't gliding or holding still.
    pub fn following(&self) -> Option<ObjId> {
        self.follow.filter(|_| self.glide.is_none())
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
    pub fn tick(&mut self, world: &World) -> Option<Pid> {
        if self.glide.is_none() && self.follow.is_some() {
            self.at = self.focus(world).unwrap_or(self.at); // (kept for when it goes away)
        }
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

/// The view's shake while a script's quake runs (CAMERA.C slamQuake, once a frame): each way, a
/// drift pulled back by half the offset, nudged one at random and held to the quake's strength,
/// then added to the offset. Strength 1 wanders about 4 pixels, 5 about 10. Stopping puts all
/// back to nothing, so the view is where it was.
#[derive(Clone, Copy, Debug)]
pub struct Shake {
    /// The picture's offset (game pixels: right, down).
    pub at: (i8, i8),
    drift: (i8, i8),
    seed: u32,
}

impl Default for Shake {
    fn default() -> Self {
        Self { at: (0, 0), drift: (0, 0), seed: 0x2545_F491 }
    }
}

impl Shake {
    /// One frame of a quake of `strength` (0: still, all back to nothing).
    pub fn step(&mut self, strength: u16) {
        if strength == 0 {
            (self.at, self.drift) = ((0, 0), (0, 0));
            return;
        }
        let m = strength.min(127) as i8;
        let mut axis = |at: &mut i8, d: &mut i8| {
            let s = &mut self.seed;
            *s ^= *s << 13;
            *s ^= *s >> 17;
            *s ^= *s << 5;
            *d = d.wrapping_sub(*at / 2).wrapping_add(if *s & 1 == 1 { 1 } else { -1 }).clamp(-m, m);
            *at = at.wrapping_add(*d);
        };
        let (mut a, mut d) = (self.at, self.drift);
        axis(&mut a.0, &mut d.0);
        axis(&mut a.1, &mut d.1);
        (self.at, self.drift) = (a, d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quake_wanders_by_its_strength_and_stops_where_it_started() {
        for (strength, most) in [(1, 4), (5, 10)] {
            let mut s = Shake::default();
            let mut seen = 0;
            for _ in 0..20000 {
                s.step(strength);
                seen = seen.max(s.at.0.abs()).max(s.at.1.abs());
            }
            assert_eq!(seen, most, "strength {strength}");
            s.step(0);
            assert_eq!(s.at, (0, 0));
        }
    }
}
