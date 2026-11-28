//! Playing the Avatar's actions (ANIMPRIM.C): each frame shown for the action's pace, carrying
//! him 4 × its forward amount (a jump at the pointer: its share of the way there) and its rise;
//! what happens when the way is blocked or runs out; catching a ledge in a fall and hanging
//! from it (GRAVITY.C).
use super::*;
use crate::world::WALK_STEP;

/// Shifts aside tried for a blocked walk, run or careful step: 16, 32 and 48, left first. (The
/// original tries a walk or run turned a way before the 48s; a turned run veers off for a whole
/// stride, up to 160 off his line, so here every shift comes first.)
const SHIFTS: [(i32, bool); 6] = [(16, false), (16, true), (32, false), (32, true), (48, false), (48, true)];
/// A shift aside is eased over at most this many frames (see `Avatar::drift`).
const DRIFT_FRAMES: usize = 2;
/// Moves added to some frames of an action: (place in its frames, (dx, dy)).
pub(super) type Drift = Vec<(usize, (i32, i32))>;

/// Where a frame's move takes him.
enum Way {
    To((i32, i32, i32)),
    /// Off an edge, more than a step down: there, to fall.
    Over((i32, i32, i32)),
    Blocked,
}

/// Actions in the air (the rest keep to the ground, but for climbs, whose way was checked).
fn airborne(action: usize) -> bool {
    matches!(action, action::JUMP | action::RUNNING_JUMP | action::JUMP_UP)
}

/// Not standing on anything (up off the floor, if only a little): he falls.
fn unsupported(world: &World) -> bool {
    world.get(AVATAR).is_some_and(|it| world.floor_under(AVATAR).map_or(it.z > 0, |z| z < it.z))
}

fn climb(action: usize) -> bool {
    (action::CLIMB_16..=action::CLIMB_72).contains(&action) || action == action::CLIMB_UP
}

impl Avatar {
    /// Begin the next queued action, if nothing is playing.
    pub(super) fn next(&mut self, gd: &GameData, world: &mut World) {
        while self.act.is_none() {
            let Some((action, dir)) = self.then.pop_front() else { return };
            let mut dir = dir.unwrap_or(self.dir) % 8;
            let (mut action, mut drift) = (action, vec![]);
            self.slip = None;
            if matches!(action, action::WALK | action::RUN | action::STEP) && !(action == action::STEP && self.over) {
                let was = world.get(AVATAR).map(|it| (it.x, it.y, it.z));
                (action, dir, drift) = self.choose_move(gd, world, action, dir);
                // (Shifted aside at once: drawn going there over the first frame, not hopping.)
                self.slip = was.filter(|&w| world.get(AVATAR).is_some_and(|it| (it.x, it.y, it.z) != w));
            }
            let Some((it, a)) = world.get(AVATAR).and_then(|it| Some((it, gd.anims.get(it.shape, action)?))) else {
                continue;
            };
            let Some(size) = a.frames.get(dir).map(Vec::len).filter(|&n| n > 0) else { continue };
            self.dir = dir;
            let leap = (action == action::JUMP).then(|| self.aimed.take()).flatten().map(|to| Leap {
                from: (it.x, it.y),
                to: (to.0, to.1),
                rise: to.2 - it.z,
                total: a.frames[dir].iter().map(|f| 4 * f.ddir as i32).sum(),
            });
            self.act = Some(Act {
                action,
                dir,
                flags: a.flags,
                frames: anim_range(a.flags, size, self.first, self.last.0 == action),
                step: 0,
                wait: 0.0,
                pace: (a.repeat as f32 + 1.0) / 30.0,
                struck: false,
                stopped: false,
                over: action == action::STEP && std::mem::take(&mut self.over),
                leap,
                drift,
            });
            self.last = (action, dir);
            self.failed &= action == action::STAND;
            // Exertion: swings and kicks; climbs by their height (a pull-up as 40).
            self.effort = match action {
                action::ATTACK | action::KICK => Some(Effort::Strike),
                action::CLIMB_UP => Some(Effort::Climb(3)),
                a if climb(a) => Some(Effort::Climb((a - action::CLIMB_16) as u32)),
                _ => self.effort,
            };
        }
    }

    /// How `action` facing `dir` would go from `at` (ANIMPRIM.C, run as a test): Some(false) it
    /// plays out, Some(true) it goes over an edge (which a test counts as failing too), None
    /// it's blocked.
    fn try_act(&self, gd: &GameData, world: &World, action: usize, dir: usize, at: (i32, i32, i32)) -> Option<bool> {
        self.trial(gd, world, action, dir, at, &[]).ok()
    }

    /// [`try_act`](Self::try_act) with `drift` added to some frames' moves (see [`Act::drift`]):
    /// Ok(over an edge?) if it plays out, else the place in its frames of the one that's blocked.
    fn trial(
        &self,
        gd: &GameData,
        world: &World,
        action: usize,
        dir: usize,
        mut at: (i32, i32, i32),
        drift: &[(usize, (i32, i32))],
    ) -> Result<bool, usize> {
        let a = world.get(AVATAR).and_then(|it| gd.anims.get(it.shape, action)).ok_or(0usize)?;
        let frames = a.frames.get(dir).ok_or(0usize)?;
        let again = self.last.0 == action;
        for (n, k) in anim_range(a.flags, frames.len(), self.first, again).into_iter().enumerate() {
            let fwd = 4 * frames.get(k).map_or(0, |f| f.ddir as i32);
            let side = drift.iter().find(|d| d.0 == n).map_or((0, 0), |d| d.1);
            let (dx, dy) = (DIRS[dir].0 * fwd + side.0, DIRS[dir].1 * fwd + side.1);
            if (dx, dy) != (0, 0) {
                let to = world.step_from(AVATAR, at, dx, dy, true, (WALK_STEP, i32::MAX)).ok_or(n)?;
                if to.2 < at.2 - STEP {
                    return Ok(true);
                }
                at = to;
            }
        }
        Ok(false)
    }

    /// A shift aside of `by` drawn out over the last frames that carry him before the one that
    /// would be blocked (at most [`DRIFT_FRAMES`]), so he eases round what's in the way as he
    /// reaches it rather than hopping aside a stride early. The original makes the shift at
    /// once, before the step; that's what's done when he's blocked straight away, or when the
    /// eased way doesn't play out.
    fn drift(
        &self,
        gd: &GameData,
        world: &World,
        action: usize,
        dir: usize,
        start: (i32, i32, i32),
        by: (i32, i32),
    ) -> Option<Vec<(usize, (i32, i32))>> {
        let blocked = self.trial(gd, world, action, dir, start, &[]).err()?;
        let a = gd.anims.get(world.get(AVATAR)?.shape, action)?;
        let frames = a.frames.get(dir)?;
        let again = self.last.0 == action;
        let moving: Vec<usize> = anim_range(a.flags, frames.len(), self.first, again)
            .into_iter()
            .enumerate()
            .take(blocked)
            .filter(|&(_, k)| frames.get(k).is_some_and(|f| f.ddir != 0))
            .map(|(n, _)| n)
            .collect();
        let ease = &moving[moving.len().saturating_sub(DRIFT_FRAMES)..];
        let n = ease.len() as i32;
        if n == 0 {
            return None;
        }
        let share = |i: i32, v: i32| v * (i + 1) / n - v * i / n;
        let drift: Vec<_> =
            ease.iter().enumerate().map(|(i, &f)| (f, (share(i as i32, by.0), share(i as i32, by.1)))).collect();
        (self.trial(gd, world, action, dir, start, &drift) == Ok(false)).then_some(drift)
    }

    /// The Avatar's walk, run or careful step about to start (ANIMPRIM.C's initialize): as it
    /// is, else slower (run, walk, careful step), each also a little to either side (shifting
    /// him there first) or a way to either side; the first that plays out is taken. Nothing
    /// does: a careful step (it fails, and he stands); after one that failed already, he stands.
    fn choose_move(&self, gd: &GameData, world: &mut World, action: usize, dir: usize) -> (usize, usize, Drift) {
        let Some(mut it) = world.get(AVATAR) else { return (action, dir, vec![]) };
        let start = (it.x, it.y, it.z);
        let turns = [(dir + 1) % 8, (dir + 7) % 8];
        let fits = |m: usize, d: usize, at| self.try_act(gd, world, m, d, at) == Some(false);
        // A shift of `by` to the left (or right), 16 at a time, as long as there's room.
        let aside = |m: usize, by: &[(i32, bool)]| {
            by.iter().find_map(|&(off, right)| {
                let side = if right { (dir + 2) % 8 } else { (dir + 6) % 8 };
                let mut at = start;
                for _ in 0..off / 16 {
                    at = world.step_from(AVATAR, at, DIRS[side].0 * 16, DIRS[side].1 * 16, true, (WALK_STEP, STEP))?;
                }
                fits(m, dir, at).then_some(at)
            })
        };
        let moves = [action::RUN, action::WALK, action::STEP];
        let first = moves.iter().position(|&m| m == action).unwrap_or(2);
        let found = moves.iter().enumerate().skip(first).find_map(|(i, &m)| {
            let turn = |ok: bool| turns.into_iter().find(|&d| ok && fits(m, d, start)).map(|d| (d, None));
            let pick = fits(m, dir, start)
                .then_some((dir, None))
                .or_else(|| turn(m == action::STEP))
                .or_else(|| aside(m, &SHIFTS).map(|p| (dir, Some(p))))
                .or_else(|| turn(m != action::STEP));
            pick.map(|p| (i, m, p))
        });
        let dir = found.map_or(dir, |f| f.2 .0);
        let mut drift = vec![];
        if let Some((_, m, (_, Some(p)))) = found {
            match self.drift(gd, world, m, dir, start, (p.0 - start.0, p.1 - start.1)) {
                Some(d) => drift = d,
                None => {
                    (it.x, it.y, it.z) = p;
                    world.set(gd, AVATAR, it);
                }
            }
        }
        // (The original's loop runs out after the careful step whether that played or not.)
        if found.is_none_or(|f| f.0 == 2) && self.failed {
            if self.try_act(gd, world, action, dir, (it.x, it.y, it.z)) != Some(true) {
                return (action::STAND, dir, vec![]);
            }
            return (action, dir, vec![]);
        }
        (found.map_or(action::STEP, |f| f.1), dir, drift)
    }

    /// Play on for `dt` seconds.
    pub(super) fn update_act(&mut self, gd: &GameData, world: &mut World, dt: f32) -> bool {
        let mut changed = self.glide_by(world, dt);
        self.next(gd, world);
        if let Some(j) = &mut self.act {
            j.wait -= dt;
        }
        while self.act.as_ref().is_some_and(|j| j.wait <= 0.0) {
            changed |= self.frame(gd, world);
        }
        if std::mem::take(&mut self.stopped) && !self.hanging {
            self.drop |= unsupported(world); // (stopped up in the air, a blow mid-jump: down he comes)
        }
        changed | self.glide_by(world, 0.0)
    }

    /// The next frame of the action (or its end): its move, and if he made it, the frame shown,
    /// its sound, its strike (the original shows nothing of a frame that couldn't move).
    fn frame(&mut self, gd: &GameData, world: &mut World) -> bool {
        self.glide_by(world, f32::MAX); // (the last frame's move drawn done)
        let (Some(mut j), Some(mut it)) = (self.act.take(), world.get(AVATAR)) else { return false };
        let frames = gd.anims.get(it.shape, j.action).and_then(|a| a.frames.get(j.dir)).cloned().unwrap_or_default();
        let Some(&k) = j.frames.get(j.step).filter(|&&k| k < frames.len()) else {
            self.finish(gd, world, j);
            return true;
        };
        let f = frames[k];
        let fwd = 4 * f.ddir as i32;
        let side = j.drift.iter().find(|d| d.0 == j.step).map_or((0, 0), |d| d.1);
        let (mut dx, mut dy, mut dz) = (DIRS[j.dir].0 * fwd + side.0, DIRS[j.dir].1 * fwd + side.1, f.dz as i32);
        if let Some(l) = j.leap.filter(|l| l.total != 0) {
            // Its share of the way to the point, and of the rise after the take-off.
            (dx, dy) = ((l.to.0 - l.from.0) * fwd / l.total, (l.to.1 - l.from.1) * fwd / l.total);
            if k > 5 {
                dz += l.rise / (frames.len() as i32 - 5);
            }
        }
        dz = dz.max(-it.z);
        let here = (it.x, it.y, it.z);
        let to = match if (dx, dy, dz) == (0, 0, 0) { Way::To(here) } else { self.way(world, &mut j, &f, (dx, dy, dz)) }
        {
            Way::Blocked => {
                self.fail(world, &j);
                return true;
            }
            Way::Over(to) => {
                self.stop();
                self.drop = true;
                self.fling = (to.0 - here.0, to.1 - here.1, to.2 - here.2);
                to
            }
            Way::To(to) => {
                let from = self.slip.take().unwrap_or(here);
                self.glide = Some(Glide { from, to, t: -j.wait, pace: j.pace });
                j.step += 1;
                j.wait += j.pace;
                to
            }
        };
        (it.x, it.y, it.z) = to;
        show(&mut it, (f.frame, f.flags & AnimFrame::FLIPPED != 0));
        world.set(gd, AVATAR, it);
        self.pose = (j.action, k);
        if f.sfx != 0 {
            self.sounds.push((f.sfx as u16, None));
        }
        let draw = matches!(j.action, action::READY_WEAPON | action::UNREADY_WEAPON);
        if draw && f.flags & AnimFrame::SPECIAL != 0 && equip::worn(gd, world, AVATAR, Slot::Weapon).is_some() {
            self.sounds.push((0x51 + world.roll(2) as u16, None)); // (the blade drawn or sheathed)
        }
        let run = j.action == action::RUN;
        self.sounds.extend(world.footstep(AVATAR, j.action, f.flags).map(|s| (s, Some(run))));
        if f.attack_range() > 0 && !j.struck {
            self.swing = Some((j.dir, f.attack_range()));
        }
        if !self.drop {
            self.act = Some(j);
        }
        true
    }

    /// Where the frame's move (`d`) takes him: on the ground as walking does (up a step, down
    /// any way, over an edge (a careful step gets there only as the step over the edge he
    /// teetered at, or when nothing else would go: see `choose_move`); sliding along a wall met
    /// slantwise); a climb as its frames say; in the air only if the way is clear (a
    /// standing jump, blocked, goes on where it is).
    fn way(&self, world: &World, j: &mut Act, f: &AnimFrame, (dx, dy, dz): (i32, i32, i32)) -> Way {
        let Some(it) = world.get(AVATAR) else { return Way::Blocked };
        if climb(j.action) {
            return Way::To((it.x + dx, it.y + dy, it.z + dz));
        }
        if j.stopped {
            return Way::To((it.x, it.y, it.z));
        }
        if !airborne(j.action) || f.flags & AnimFrame::ON_GROUND != 0 {
            let slide = !j.over && dx != 0 && dy != 0;
            let tries = [(dx, dy), (dx, 0), (0, dy)];
            for &(x, y) in &tries[..if slide { 3 } else { 1 }] {
                let Some(p) = world.try_move(AVATAR, x, y) else { continue };
                if p.2 >= it.z - STEP {
                    return Way::To(p);
                }
                return world.try_move_air(AVATAR, x, y, 0).map_or(Way::Blocked, Way::Over);
            }
            return Way::Blocked;
        }
        let clear = ((dx, dy) == (0, 0) || world.ground_below(AVATAR, dx, dy))
            .then(|| world.try_move_air(AVATAR, dx, dy, dz))
            .flatten();
        match clear {
            Some(p) => Way::To(p),
            None if j.flags & UNSTOPPABLE != 0 => {
                j.stopped = true;
                Way::To((it.x, it.y, it.z))
            }
            None => Way::Blocked,
        }
    }

    /// The way was blocked (ANIMPRIM.C's failFunction): running or leaping into it may hurt
    /// (the game decides), and stops all he was to do; else what's queued goes on, and with
    /// nothing more to do, he stands. Up off the ground, he falls.
    fn fail(&mut self, world: &World, j: &Act) {
        (self.last.0, self.failed) = (FAILED, true);
        if j.flags & (BUMPS | HURTS) != 0 {
            self.stop(); // (everything he was to do, dropped)
            self.bump = Some(j.flags & HURTS != 0);
        } else {
            // (What was to come still comes; with nothing, he stands.)
            (self.act, self.glide, self.aimed, self.over) = (None, None, None, false);
            if self.then.is_empty() {
                self.queue(self.stance(), None);
            }
        }
        self.drop = unsupported(world);
    }

    /// The action's done: the other foot leads next (walking); down on whatever is underneath
    /// after a jump or a climb, or more than a step over it, falling; on to the next.
    fn finish(&mut self, gd: &GameData, world: &mut World, j: Act) {
        if crate::world::two_step(j.flags) {
            self.first = !self.first;
        }
        if j.action == action::STAND_UP {
            self.down = false;
        }
        if airborne(j.action) || climb(j.action) {
            let Some(mut it) = world.get(AVATAR) else { return };
            match world.floor_under(AVATAR) {
                Some(z) if z >= it.z - STEP => {
                    it.z = z;
                    world.set(gd, AVATAR, it);
                }
                _ => {
                    self.stop();
                    self.drop = true;
                    return;
                }
            }
        }
        self.next(gd, world);
        match &mut self.act {
            Some(n) => n.wait = j.wait, // (the time over, carried)
            None => self.spare = Some(j.wait),
        }
    }

    /// Draw the frame's move `dt` further on (none once something else has moved him).
    pub(super) fn glide_by(&mut self, world: &mut World, dt: f32) -> bool {
        let was = world.lag(AVATAR);
        let Some(mut g) = self.glide else {
            world.set_lag(AVATAR, (0, 0, 0)); // (stopped mid-frame: drawn where he is)
            return was != (0, 0, 0);
        };
        g.t = (g.t + dt).min(g.pace);
        let k = if g.pace > 0.0 { g.t.max(0.0) / g.pace } else { 1.0 };
        let there = world.get(AVATAR).is_some_and(|it| (it.x, it.y, it.z) == g.to);
        self.glide = (k < 1.0 && there).then_some(g);
        let back = |a: i32, b: i32| ((a - b) as f32 * (1.0 - k)).round() as i32;
        // (Up at once, down at the end: never drawn part way into a step.)
        let lag = (back(g.from.0, g.to.0), back(g.from.1, g.to.1), (g.from.2.max(g.to.2) - g.to.2) * (k < 1.0) as i32);
        world.set_lag(AVATAR, if self.glide.is_some() { lag } else { (0, 0, 0) });
        world.lag(AVATAR) != was
    }

    /// Would a careful step `dir` take him over an edge?
    pub(super) fn step_falls(&self, gd: &GameData, world: &World, dir: usize) -> bool {
        let Some(it) = world.get(AVATAR) else { return false };
        let Some(frames) = gd.anims.get(it.shape, action::STEP).and_then(|a| a.frames.get(dir)) else { return false };
        let mut at = (it.x, it.y, it.z);
        for f in frames.iter().filter(|f| f.ddir != 0) {
            let fwd = 4 * f.ddir as i32;
            let (dx, dy) = (DIRS[dir].0 * fwd, DIRS[dir].1 * fwd);
            // (In the way: no step at all.)
            let Some(to) = world.step_from(AVATAR, at, dx, dy, true, (crate::world::WALK_STEP, i32::MAX)) else {
                return false;
            };
            if to.2 < it.z - STEP {
                return true;
            }
            at = to;
        }
        false
    }

    /// Falling with the move button held (GRAVITY.C's clinging fall): a ledge just ahead, one
    /// a 40-high climb would take him onto, caught by the hands. True if he's hanging now.
    pub fn cling(&mut self, gd: &GameData, world: &mut World) -> bool {
        let Some(it) = world.get(AVATAR).filter(|_| self.held && !self.hanging) else { return false };
        let (z, (ex, ey)) = (it.z & !7, DIRS[self.dir]);
        for k in [64, 32, 16, 0] {
            let at = Item { x: it.x + ex * k, y: it.y + ey * k, z, ..it };
            world.set(gd, AVATAR, at);
            let free = world.room_for(gd, AVATAR, (at.x, at.y, at.z))
                && world.floor_under(AVATAR).is_none_or(|f| f < z - GRAB_SLACK);
            if free && world.anim_fits(gd, AVATAR, CLIMB_40, self.dir).is_some() {
                self.hang(gd, world);
                return true;
            }
        }
        world.set(gd, AVATAR, it);
        false
    }

    /// Hanging by the hands where he is.
    pub(super) fn hang(&mut self, gd: &GameData, world: &mut World) {
        self.stop();
        self.hanging = true;
        self.last = (action::HANG, self.dir);
        self.hold_frame(gd, world);
    }

    /// Hanging: both buttons pull up onto the ledge; letting go of the move button drops.
    pub(super) fn update_hanging(&mut self, gd: &GameData, world: &mut World, input: &Input, released: bool) -> bool {
        if world.get(AVATAR).zip(world.floor_under(AVATAR)).is_some_and(|(it, f)| f >= it.z - GRAB_SLACK) {
            // Moved (by a script) onto something: standing again.
            self.hanging = false;
            return self.stand(gd, world);
        }
        if input.jump && world.anim_fits(gd, AVATAR, action::CLIMB_UP, self.dir).is_some() {
            self.hanging = false;
            self.queue(action::CLIMB_UP, None);
            self.next(gd, world);
            return true;
        }
        if released {
            self.hanging = false;
            self.drop = true;
            self.last = (action::STAND, self.dir);
            return self.stand(gd, world);
        }
        false
    }

    /// Keep showing the hanging frame (scripts may have set another).
    pub(super) fn hold_frame(&mut self, gd: &GameData, world: &mut World) -> bool {
        let Some(mut it) = world.get(AVATAR) else { return false };
        let was = (it.frame, it.flags);
        self.pose(gd, &mut it, action::HANG, 0);
        world.set(gd, AVATAR, it);
        was != (it.frame, it.flags)
    }
}
