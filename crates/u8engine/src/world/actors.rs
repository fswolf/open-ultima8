//! People's state the world keeps: who's in combat and after whom, ANIM.DAT actions run for
//! scripts, the last action started, and weapons drawn over them.
use super::*;

/// An actor going through an ANIM.DAT action, each frame shown for the action's frame repeat + 1
/// game ticks (ScummVM: a run every other 60 Hz tick, a frame every repeat + 1 runs); the last
/// frame stays.
#[derive(Clone, Debug)]
pub(super) struct Acting {
    pub(super) id: ObjId,
    pub(super) action: usize,
    pub(super) dir: usize,
    pub(super) step: usize,
    /// The frames it plays, in order (see [`World::frames_to_play`]).
    pub(super) frames: Vec<usize>,
    pub(super) wait: u32,
    /// Ticks each frame shows.
    pub(super) per: u32,
    /// Played last frame first (and standing still): getting up, run back, is falling down.
    pub(super) backwards: bool,
    /// A two-step action (walking): finishing it switches which step comes next.
    pub(super) two_step: bool,
}

/// The frames an action of `size` frames plays (ScummVM's AnimAction::getAnimRange and
/// AnimationTracker::getNextFrame): a two-step action (walking, running) plays one step — the
/// first half when `first_step`, else the second — and a looping one never shows its frame 0
/// once under way; doing the same thing the same way `again` skips the first frame.
pub fn anim_range(flags: u16, size: usize, first_step: bool, again: bool) -> Vec<usize> {
    let looping = flags & LOOPING != 0;
    let (start, end) = if flags & TWO_STEP != 0 {
        match first_step {
            true => (if looping { size - 1 } else { 0 }, size / 2),
            false => (size / 2, if looping { size - 1 } else { size }),
        }
    } else if again && size > 1 {
        (1, size)
    } else {
        (0, size)
    };
    let next = |f: usize| match f + 1 {
        n if n == end => end,
        n if n >= size => looping as usize,
        n => n,
    };
    let mut v = vec![];
    let mut f = start;
    while f != end && v.len() < 2 * size {
        v.push(f);
        f = next(f);
    }
    if v.is_empty() {
        v.push(start.min(size.saturating_sub(1)));
    }
    v
}

/// Does a two-step action switch feet when done?
pub fn two_step(flags: u16) -> bool {
    flags & TWO_STEP != 0
}

/// ANIM.DAT action flags (ScummVM's AnimAction).
const TWO_STEP: u16 = 0x0001;
const LOOPING: u16 = 0x0004;

/// Whom someone in combat is after: `now`, and `fixed` — the first one a script (or a blow)
/// named, whom they go back to while still about (0: anyone hostile).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Target {
    pub now: ObjId,
    pub fixed: ObjId,
}

/// Ticks an action shows each frame.
fn per_frame(a: &u8data::anim::Action) -> u32 {
    a.repeat as u32 + 1
}

/// A held weapon drawn over an actor's sprite (WPNOVLAY.DAT): the weapon's overlay shape and
/// frame, its offset in screen pixels from the actor's origin, and whether it's mirrored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Overlay {
    pub shape: u32,
    pub frame: u32,
    pub dx: i32,
    pub dy: i32,
    pub flip: bool,
}

impl World {
    pub fn in_combat(&self, id: ObjId) -> bool {
        self.fighting.contains_key(&id)
    }

    pub fn set_in_combat(&mut self, id: ObjId, on: bool) {
        if on {
            self.fighting.entry(id).or_default();
        } else {
            self.fighting.remove(&id);
        }
    }

    /// Everyone in combat stance.
    pub fn fighters(&self) -> Vec<ObjId> {
        let mut v: Vec<ObjId> = self.fighting.keys().copied().collect();
        v.sort_unstable();
        v
    }

    /// Back in combat after `t`, exactly as saved.
    pub fn restore_target(&mut self, id: ObjId, t: Target) {
        self.fighting.insert(id, t);
    }

    pub fn target(&self, id: ObjId) -> Target {
        self.fighting.get(&id).copied().unwrap_or_default()
    }

    /// Send `id` (into combat, if it isn't) after `t`; the first one named stays its quarry.
    pub fn set_target(&mut self, id: ObjId, t: ObjId) {
        let tg = self.fighting.entry(id).or_default();
        tg.now = t;
        if tg.fixed == 0 {
            tg.fixed = t;
        }
    }

    /// Who `id` is after right now (the quarry kept).
    pub fn retarget(&mut self, id: ObjId, t: ObjId) {
        if let Some(tg) = self.fighting.get_mut(&id) {
            tg.now = t;
        }
    }

    /// Start `id` on `action` facing `dir` (instead of whatever it was playing). Returns how
    /// many ticks it takes, or None if its shape has no such action.
    pub fn play(&mut self, gd: &GameData, id: ObjId, action: usize, dir: usize) -> Option<u32> {
        self.play_part(gd, id, action, dir, 0)
    }

    /// [`play`](Self::play), stopping after `steps` frames (0: all of them). A two-step action
    /// cut short doesn't count as a step taken (the same foot leads next time).
    pub fn play_part(&mut self, gd: &GameData, id: ObjId, action: usize, dir: usize, steps: usize) -> Option<u32> {
        let mut it = self.get(id)?;
        let a = gd.anims.get(it.shape, action)?;
        let mut frames = self.frames_to_play(id, action, dir, a.flags, a.frames.get(dir)?.len());
        let cut = steps > 0 && steps < frames.len();
        if cut {
            frames.truncate(steps);
        }
        let (frame, flip) = crate::avatar::anim_frame(gd, it.shape, action, dir, *frames.first()?)?;
        it.frame = frame;
        it.flags = if flip { it.flags | item_flag::FLIPPED } else { it.flags & !item_flag::FLIPPED };
        self.set(gd, id, it);
        self.acting.retain(|a| a.id != id);
        self.act_move(gd, id, action, dir, frames[0]);
        let (per, two_step) = (per_frame(a), a.flags & TWO_STEP != 0 && !cut);
        let ticks = frames.len() as u32 * per;
        self.acting.push(Acting { id, action, dir, step: 0, frames, wait: per, per, backwards: false, two_step });
        self.last_anim.insert(id, action);
        self.last_dir.insert(id, dir);
        Some(ticks)
    }

    /// Where `id` would be after walking (`action`, facing `dir`) its next step — `steps`
    /// frames of it, 0 for all — each frame's move taken as walking takes it (up or down a
    /// step); None if something's in the way (ScummVM's tryAnim before each step of a path).
    pub fn walk_fits(
        &self,
        gd: &GameData,
        id: ObjId,
        action: usize,
        dir: usize,
        steps: usize,
    ) -> Option<(i32, i32, i32)> {
        let it = self.get(id)?;
        let a = gd.anims.get(it.shape, action)?;
        let frames = a.frames.get(dir)?;
        let mut at = (it.x, it.y, it.z);
        let ks = self.frames_to_play(id, action, dir, a.flags, frames.len());
        for k in ks.into_iter().take(if steps == 0 { usize::MAX } else { steps }) {
            let fwd = 4 * frames[k].ddir as i32;
            if fwd != 0 {
                let (dx, dy) = (crate::geom::DIRS[dir].0 * fwd, crate::geom::DIRS[dir].1 * fwd);
                at = self.step_from(id, at, dx, dy, true, (WALK_STEP, WALK_STEP))?;
            }
        }
        Some(at)
    }

    /// Which foot `id` leads with next: true for a two-step action's first half.
    pub fn first_step(&self, id: ObjId) -> bool {
        self.first_step.contains(&id)
    }

    /// The facing `id` last started an action in.
    pub fn last_dir(&self, id: ObjId) -> Option<usize> {
        self.last_dir.get(&id).copied()
    }

    fn frames_to_play(&self, id: ObjId, action: usize, dir: usize, flags: u16, size: usize) -> Vec<usize> {
        let again = self.last_anim(id) == action && self.last_dir.get(&id) == Some(&dir);
        anim_range(flags, size, self.first_step.contains(&id), again)
    }

    /// Play `action` last frame first, in place, ending on its first frame; `id`'s last action
    /// counts as `as_action` (a fall made of standing up, run backwards, is a death).
    pub fn play_backwards(
        &mut self,
        gd: &GameData,
        id: ObjId,
        action: usize,
        dir: usize,
        as_action: usize,
    ) -> Option<u32> {
        let mut it = self.get(id)?;
        let a = gd.anims.get(it.shape, action)?;
        let (len, per) = (a.frames.get(dir)?.len(), per_frame(a));
        let (frame, flip) = crate::avatar::anim_frame(gd, it.shape, action, dir, len.checked_sub(1)?)?;
        it.frame = frame;
        it.flags = if flip { it.flags | item_flag::FLIPPED } else { it.flags & !item_flag::FLIPPED };
        self.set(gd, id, it);
        self.acting.retain(|a| a.id != id);
        let frames = (0..len).rev().collect();
        self.acting.push(Acting { id, action, dir, step: 0, frames, wait: per, per, backwards: true, two_step: false });
        self.last_anim.insert(id, as_action);
        Some(len as u32 * per)
    }

    /// The step a frame of an action takes (ScummVM's AnimationTracker): 4 × its forward
    /// amount along the facing, and its rise; a frame on the ground climbs and drops as walking
    /// does. Something in the way: no step (the action plays on in place).
    fn act_move(&mut self, gd: &GameData, id: ObjId, action: usize, dir: usize, step: usize) {
        let Some(it) = self.get(id) else { return };
        let Some(f) = gd.anims.get(it.shape, action).and_then(|a| a.frames.get(dir)?.get(step).copied()) else {
            return;
        };
        let fwd = 4 * f.ddir as i32;
        let (dx, dy, dz) = (crate::geom::DIRS[dir].0 * fwd, crate::geom::DIRS[dir].1 * fwd, f.dz as i32);
        let ground = f.flags & u8data::anim::AnimFrame::ON_GROUND != 0;
        let to = match (dx, dy, dz) {
            (0, 0, 0) => return,
            _ if ground => self.try_walk(id, dx, dy),
            _ => self.try_move_air(id, dx, dy, dz),
        };
        if let Some((x, y, z)) = to {
            let mut it = it;
            (it.x, it.y, it.z) = (x, y, z);
            self.set(gd, id, it);
        }
    }

    /// The action `id` last started (standing if nothing's known).
    pub fn last_anim(&self, id: ObjId) -> usize {
        self.last_anim.get(&id).copied().unwrap_or(u8data::anim::action::STAND)
    }

    pub fn set_last_anim(&mut self, id: ObjId, action: usize) {
        self.last_anim.insert(id, action);
    }

    /// Is `id` playing an animation?
    pub fn is_acting(&self, id: ObjId) -> bool {
        self.acting.iter().any(|a| a.id == id)
    }

    /// The action, facing and frame `id` is on, if it's playing one.
    pub fn acting(&self, id: ObjId) -> Option<(usize, usize, usize)> {
        self.acting.iter().find(|a| a.id == id).map(|a| (a.action, a.dir, a.frames[a.step]))
    }

    /// One game tick of everyone's animations. Returns who finished one in mid-air (a jump off
    /// a ledge): they fall, as ScummVM's ActorAnimProcess has them.
    pub fn tick_acting(&mut self, gd: &GameData) -> Vec<ObjId> {
        let mut playing = std::mem::take(&mut self.acting);
        let mut aloft = vec![];
        playing.retain_mut(|a| {
            a.wait -= 1;
            if a.wait > 0 {
                return true;
            }
            a.step += 1;
            a.wait = a.per;
            if a.step >= a.frames.len() {
                if a.two_step && !self.first_step.remove(&a.id) {
                    self.first_step.insert(a.id); // (the other foot next time)
                }
                let z = self.get(a.id).map_or(0, |it| it.z);
                if !a.backwards && self.floor_under(a.id).is_none_or(|f| f < z) {
                    aloft.push(a.id);
                }
                return false;
            }
            let Some(mut it) = self.get(a.id) else { return false };
            let frame_no = a.frames[a.step];
            if let Some((frame, flip)) = crate::avatar::anim_frame(gd, it.shape, a.action, a.dir, frame_no) {
                it.frame = frame;
                it.flags = if flip { it.flags | item_flag::FLIPPED } else { it.flags & !item_flag::FLIPPED };
                self.set(gd, a.id, it);
            }
            if !a.backwards {
                self.act_move(gd, a.id, a.action, a.dir, frame_no);
            }
            true
        });
        playing.append(&mut self.acting); // any started meanwhile
        self.acting = playing;
        aloft
    }

    pub fn overlay(&self, id: ObjId) -> Option<Overlay> {
        self.overlays.get(&id).copied()
    }

    /// Draw (or stop drawing) a weapon over `id`.
    pub fn set_overlay(&mut self, id: ObjId, o: Option<Overlay>) {
        if self.overlay(id) == o {
            return;
        }
        match o {
            Some(o) => self.overlays.insert(id, o),
            None => self.overlays.remove(&id),
        };
        if let Some(i) = self.index(id) {
            self.touch(i);
        }
    }
}
