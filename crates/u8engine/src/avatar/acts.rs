//! The Avatar's actions from ANIM.DAT: jumps and running leaps, climbing onto ledges, catching
//! one and hanging from it, pulling up or letting go, and being knocked flat.
use super::*;

impl Avatar {
    /// One ANIM.DAT frame per its pace, applying its height change; moves forward while airborne.
    pub(super) fn update_act(&mut self, gd: &GameData, world: &mut World, dt: f32) -> bool {
        let Some(mut j) = self.act else { return false };
        let Some(mut it) = world.get(AVATAR) else {
            self.act = None;
            return false;
        };
        j.wait -= dt;
        if j.wait > 0.0 {
            self.act = Some(j);
            return false;
        }
        let dir = self.dir;
        let mut frames = gd.anims.get(it.shape, j.action).and_then(|a| a.frames.get(dir)).cloned().unwrap_or_default();
        frames.truncate(j.to);
        frames.drain(..j.from.min(frames.len()));
        if j.step >= frames.len() {
            // Done (landed): settle onto whatever is underneath, or more than a step over it, fall.
            match world.floor_under(AVATAR) {
                Some(z) if z >= it.z - STEP => it.z = z,
                _ => self.drop = true,
            }
            if !self.then.is_empty() {
                // On to the next (getting up after a fall).
                let next = self.then.remove(0);
                self.act = Some(Act::moving(next, Moves::Ground));
                if next == action::SHAKE_HEAD {
                    self.stunned = false; // up again
                }
                world.set(gd, AVATAR, it);
                return true;
            }
            if !(j.action == action::START_BLOCK && self.blocking) {
                self.pose(gd, &mut it, self.stance(), 0); // (a block holds its last frame)
            }
            world.set(gd, AVATAR, it);
            self.act = None;
            return true;
        }
        let f = frames[j.step];
        show(&mut it, (f.frame, f.flags & AnimFrame::FLIPPED != 0));
        self.pose = (j.action, j.from + j.step);
        if f.attack_range() > 0 && !j.struck {
            self.swing = Some((dir, f.attack_range()));
        }
        world.set(gd, AVATAR, it);
        if j.moves != Moves::Fixed {
            let fwd = 4 * f.ddir as i32;
            let (dx, dy, dz) = (DIRS[dir].0 * fwd, DIRS[dir].1 * fwd, f.dz as i32);
            let to = match j.moves {
                // Climbing: the path was checked before it began.
                Moves::Checked => Some((it.x + dx, it.y + dy, it.z + dz)),
                _ if (dx, dy) == (0, 0) => None,
                // (not over an edge: knocked flat, it slides to a stop there)
                _ => world.try_move(AVATAR, dx, dy).filter(|&(_, _, z)| z >= it.z - STEP),
            };
            if let Some(p) = to {
                (it.x, it.y, it.z) = p;
                world.set(gd, AVATAR, it);
            }
            j.step += 1;
            j.wait = j.pace;
            self.act = Some(j);
            return true;
        }
        let airborne = frames[..=j.step].iter().map(|f| f.dz as i32).sum::<i32>() > 0 || f.dz != 0;
        let (dx, dy) = if airborne { (DIRS[dir].0 * j.fwd, DIRS[dir].1 * j.fwd) } else { (0, 0) };
        // Prefer the full arc; under a low ceiling keep going forward at the same height. Never
        // out over nothing at all (off the map's edge): it'd come down on nothing.
        let dz = f.dz as i32;
        let moved = [(dx, dy, dz), (dx, 0, dz), (0, dy, dz), (dx, dy, 0), (dx, 0, 0), (0, dy, 0), (0, 0, dz)]
            .iter()
            .filter(|t| **t != (0, 0, 0))
            .filter(|&&(x, y, _)| (x, y) == (0, 0) || world.ground_below(AVATAR, x, y))
            .find_map(|&(x, y, z)| world.try_move_air(AVATAR, x, y, z));
        let leap = j.action == action::JUMP || j.action == action::RUNNING_JUMP;
        // Stopped going forward (a wall, the map's edge), it goes on up and down where it is (and
        // can catch the ledge on the way down), but that hop is its last: at its end, or as soon
        // as it can't come down further, it lands or drops.
        j.stopped |= leap && (dx, dy) != (0, 0) && moved.is_none_or(|(x, y, _)| (x, y) == (it.x, it.y));
        if let Some((x, y, z)) = moved {
            (it.x, it.y, it.z) = (x, y, z);
            world.set(gd, AVATAR, it);
        }
        if j.stopped && j.step > 0 && (f.dz == 0 || (f.dz < 0 && moved.is_none())) {
            self.act = None;
            if !self.grab(gd, world) {
                match world.floor_under(AVATAR) {
                    Some(z) if z >= it.z - STEP => it.z = z,
                    _ => self.drop = true,
                }
                self.pose(gd, &mut it, self.stance(), 0);
                world.set(gd, AVATAR, it);
            }
            return true;
        }
        j.step += 1;
        j.wait = j.pace;
        self.act = Some(j);
        // Coming down past a ledge just out of reach of the landing: catch it by the hands.
        if leap && f.dz < 0 && self.grab(gd, world) {
            self.act = None;
        }
        true
    }

    /// Could the Avatar, coming down here, catch a ledge in front and hang (the pull-up would
    /// take it onto the top)? If so it's hanging, at the height that fits.
    pub(super) fn grab(&mut self, gd: &GameData, world: &mut World) -> bool {
        let Some(it) = world.get(AVATAR) else { return false };
        for dz in (-GRAB_SLACK..=GRAB_SLACK).rev() {
            let Some((_, _, z)) = world.try_move_air(AVATAR, 0, 0, dz).filter(|&(_, _, z)| z == it.z + dz) else {
                continue;
            };
            let mut at = it;
            at.z = z;
            world.set(gd, AVATAR, at);
            let footing = world.floor_under(AVATAR).is_some_and(|f| f >= z - GRAB_SLACK);
            if !footing && world.anim_fits(gd, AVATAR, action::CLIMB_UP, self.dir).is_some() {
                self.hanging = true;
                self.pose(gd, &mut at, action::HANG, 0);
                world.set(gd, AVATAR, at);
                return true;
            }
            world.set(gd, AVATAR, it);
        }
        false
    }

    /// Hanging: towards the ledge (or a jump) pulls up onto it; away lets go.
    pub(super) fn update_hanging(&mut self, gd: &GameData, world: &mut World, input: &Input) -> bool {
        if world.get(AVATAR).zip(world.floor_under(AVATAR)).is_some_and(|(it, f)| f >= it.z - GRAB_SLACK) {
            // Moved (by a script) onto something: standing again.
            self.hanging = false;
            return self.stand(gd, world);
        }
        let up = input.jump || input.attack || input.dir == Some(self.dir);
        let away = input.dir == Some((self.dir + 4) % 8);
        if up && world.anim_fits(gd, AVATAR, action::CLIMB_UP, self.dir).is_some() {
            self.hanging = false;
            self.act = Some(Act::moving(action::CLIMB_UP, Moves::Checked));
            self.effort = Some(Effort::Climb(3)); // (a pull-up of 40, as climbing that high)
            return true;
        }
        if away {
            // Let go: it falls.
            self.hanging = false;
            self.drop = true;
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
