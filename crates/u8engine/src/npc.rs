//! NPC movement ordered by scripts (`pathfindToItem`): plan the steps with [`path::find`], take
//! them one doAnim at a time — the animation moves the walker, as ScummVM's PathfinderProcess
//! has it — planning again when a step won't go, and tell the script whether we got there.
use crate::avatar::anim_frame;
use crate::path::{self, Step};
use crate::usecode::Pid;
use crate::world::{item_flag::FLIPPED, ObjId, World};
use u8data::{anim::action, GameData};

/// Plans made again before giving up (a step that won't go: someone's in the way).
const REPLANS: u32 = 3;
/// Give up after this long whatever happens.
const MAX_TICKS: u32 = 30 * 60;
/// Arrival slack for the two footprints.
const SLACK: i32 = 48;

#[derive(Clone, Debug)]
pub struct Walk {
    pub pid: Pid,
    pub actor: ObjId,
    to: (i32, i32, i32),
    range: i32,
    route: Option<Vec<Step>>,
    next: usize,
    dir: usize,
    ticks: u32,
    replans: u32,
}

impl Walk {
    pub fn new(pid: Pid, actor: ObjId, to: (i32, i32, i32), range: i32) -> Self {
        Self { pid, actor, to, range, route: None, next: 0, dir: 0, ticks: 0, replans: 0 }
    }

    /// Which way it's walking.
    pub fn dir(&self) -> usize {
        self.dir
    }

    fn reach(&self) -> i32 {
        self.range.max(0) + SLACK
    }

    /// Walking, or in combat advancing.
    fn action(gd: &GameData, world: &World, id: ObjId) -> usize {
        let shape = world.get(id).map_or(0, |it| it.shape);
        match world.in_combat(id) && gd.anims.get(shape, action::ADVANCE).is_some() {
            true => action::ADVANCE,
            false => action::WALK,
        }
    }

    /// Advance one tick. Some(true) = arrived, Some(false) = gave up, None = still walking.
    pub fn step(&mut self, gd: &GameData, world: &mut World) -> Option<bool> {
        self.ticks += 1;
        if world.is_acting(self.actor) {
            return None; // mid-step
        }
        let it = world.get(self.actor)?;
        if (it.x - self.to.0).abs().max((it.y - self.to.1).abs()) <= self.reach() {
            self.stand(gd, world);
            return Some(true);
        }
        if self.ticks > MAX_TICKS {
            self.stand(gd, world);
            return Some(false);
        }
        let act = Self::action(gd, world, self.actor);
        if self.route.is_none() {
            // After a step wouldn't go, plan round people too.
            let goal = (self.to.0, self.to.1);
            match path::find(world, gd, self.actor, act, goal, self.reach(), self.replans > 0) {
                Some(r) => (self.route, self.next) = (Some(r.steps), 0),
                None => {
                    self.stand(gd, world);
                    return Some(false); // walled in, or already as close as it gets
                }
            }
        }
        let Some(&s) = self.route.as_ref()?.get(self.next) else {
            // The end of a route to the closest place found (the goal itself is out of reach).
            self.stand(gd, world);
            return Some(false);
        };
        if world.walk_fits(gd, self.actor, act, s.dir, s.frames).is_some() {
            self.dir = s.dir;
            world.play_part(gd, self.actor, act, s.dir, s.frames);
            self.next += 1;
        } else if self.replans >= REPLANS {
            self.stand(gd, world);
            return Some(false);
        } else {
            (self.replans, self.route) = (self.replans + 1, None);
        }
        None
    }

    /// Back to standing, facing the way it was going.
    pub fn stand(&self, gd: &GameData, world: &mut World) {
        let Some(mut it) = world.get(self.actor) else { return };
        if let Some((f, flip)) = anim_frame(gd, it.shape, action::STAND, self.dir, 0) {
            it.frame = f;
            it.flags = if flip { it.flags | FLIPPED } else { it.flags & !FLIPPED };
            world.set(gd, self.actor, it);
        }
        world.set_last_anim(self.actor, action::STAND);
    }
}
