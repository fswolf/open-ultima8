//! Movement and collision: what fits where, sweeping things through the air, stepping
//! actors over floors and up ledges. Actors block but are never stood on.
use super::*;

impl World {
    /// Would `id` fit with its base at (x, y, z): nothing solid in the way, something to rest on?
    pub fn fits(&self, gd: &GameData, id: ObjId, at: (i32, i32, i32)) -> bool {
        let Some(i) = self.index(id) else { return false };
        let (blocked, resting) = self.room(gd, self.entries[i].it.shape, at, Some(i));
        !blocked && resting
    }

    /// Could `id` play `action` facing `dir` from where it stands — every frame's move (ANIM.DAT:
    /// rise, and 4 × its forward step) clear, ending on something? Where it ends up, if so.
    /// (ScummVM's tryAnim; climbing checks each height this way.)
    pub fn anim_fits(&self, gd: &GameData, id: ObjId, action: usize, dir: usize) -> Option<(i32, i32, i32)> {
        let i = self.index(id)?;
        let it = self.entries[i].it;
        let frames = gd.anims.get(it.shape, action)?.frames.get(dir)?;
        let mut at = (it.x, it.y, it.z);
        for f in frames {
            let fwd = 4 * f.ddir as i32;
            let q = (at.0 + crate::geom::DIRS[dir].0 * fwd, at.1 + crate::geom::DIRS[dir].1 * fwd, at.2 + f.dz as i32);
            if self.room(gd, it.shape, q, Some(i)).0 {
                return None;
            }
            at = q;
        }
        let (blocked, resting) = self.room(gd, it.shape, at, Some(i));
        (!blocked && resting).then_some(at)
    }

    /// ScummVM's Item::ascend: `id` rises (or sinks) by `dz`, as far as it can, carrying what
    /// rests on it (not fixed things): each put back `dz` higher if there's room, else where it
    /// was. Whether it went all the way, and what was left where it was on the way down (to
    /// fall).
    pub fn ascend(&mut self, gd: &GameData, id: ObjId, dz: i32) -> (bool, Vec<ObjId>) {
        let Some(it) = self.get(id).filter(|_| dz != 0) else { return (dz == 0, vec![]) };
        let fixed = |w: &Self, o: ObjId| w.get(o).is_none_or(|i| gd.typeflags.get(i.shape).has(flag::FIXED));
        let riders: Vec<ObjId> =
            self.search_surface(id, true, false, false).into_iter().filter(|&o| !fixed(self, o)).collect();
        let was: Vec<(ObjId, Item)> = riders.iter().filter_map(|&o| Some((o, self.get(o)?))).collect();
        for &(o, _) in &was {
            self.set_void(o, true); // (out of the way while it moves)
        }
        let all_the_way = self.sweep(gd, id, (it.x, it.y, it.z + dz)).is_none();
        let moved = self.get(id).map_or(0, |now| now.z - it.z);
        let mut left = vec![];
        for (o, r) in was {
            let i = self.index(o).unwrap();
            let to = (r.x, r.y, r.z + moved);
            if !self.room(gd, r.shape, to, Some(i)).0 {
                self.move_to_map(gd, o, to);
            } else {
                self.move_to_map(gd, o, (r.x, r.y, r.z));
                if dz < 0 {
                    left.push(o);
                }
            }
        }
        (all_the_way, left)
    }

    /// Is there room for something of `shape` with its base at `at` (nothing solid in the
    /// way; it may hang in the air)? Only solid things need room: as ScummVM's isValidPosition,
    /// something blocks only if both are solid (a drowned Avatar's ripples spread over its body).
    pub fn clear_at(&self, gd: &GameData, shape: u32, at: (i32, i32, i32)) -> bool {
        gd.typeflags.get(shape).flags & u8data::typeflag::flag::SOLID == 0 || !self.room(gd, shape, at, None).0
    }

    /// The first solid thing in the way of something of `shape` moving to `at` from height
    /// `from_z` (the whole height it passes through counts: floors are flat), ignoring entry `skip`.
    fn blocker(
        &self,
        gd: &GameData,
        shape: u32,
        (x, y, z): (i32, i32, i32),
        from_z: i32,
        skip: usize,
        inside: &[usize],
    ) -> Option<usize> {
        let flipped =
            self.entries.get(skip).is_some_and(|e| e.it.shape == shape && e.it.flags & item_flag::FLIPPED != 0);
        let (dx, dy, dz) = gd.typeflags.get(shape).footpad(flipped);
        let to = Bounds { x0: x - dx, y0: y - dy, z0: z.min(from_z), x1: x, y1: y, z1: z.max(from_z) + dz.max(1) };
        self.near(to.x0, to.y0, to.x1, to.y1).filter(|&j| j != skip && !inside.contains(&j)).find(|&j| {
            let p = &self.entries[j];
            p.physical() && p.traits.solid && p.bounds().overlaps_xy(&to) && p.z1 > to.z0 && p.it.z < to.z1
        })
    }

    /// Move `id` towards `to` as far as it goes (ScummVM's collideMove, in steps of up to 8
    /// units): None if it got there; else what stopped it and on which axes (1 x, 2 y, 4 z),
    /// the item left just short of it.
    pub fn sweep(&mut self, gd: &GameData, id: ObjId, to: (i32, i32, i32)) -> Option<(ObjId, u8)> {
        let i = self.index(id)?;
        let mut it = self.entries[i].it;
        let (from, shape) = ((it.x, it.y, it.z), it.shape);
        let d = (to.0 - from.0, to.1 - from.1, to.2 - from.2);
        let steps = (d.0.abs().max(d.1.abs()).max(d.2.abs()) + 7) / 8;
        let mut at = from;
        let mut stopped = None;
        // What it starts inside doesn't stop it (ScummVM: a thing thrown from the thrower's middle).
        let b = self.entries[i].bounds();
        let inside: Vec<usize> =
            self.near(b.x0, b.y0, b.x1, b.y1).filter(|&j| j != i && self.entries[j].bounds().overlaps(&b)).collect();
        for k in 1..=steps.max(1) {
            let q = (from.0 + d.0 * k / steps.max(1), from.1 + d.1 * k / steps.max(1), from.2 + d.2 * k / steps.max(1));
            if let Some(j) = self.blocker(gd, shape, q, at.2, i, &inside) {
                // Which way(s) the move ran into it.
                let dirs = [(q.0, at.1, at.2), (at.0, q.1, at.2), (at.0, at.1, q.2)]
                    .iter()
                    .enumerate()
                    .filter(|&(_, &p)| p != at && self.blocker(gd, shape, p, at.2, i, &inside).is_some())
                    .fold(0u8, |m, (k, _)| m | 1 << k);
                let dirs = if dirs == 0 { 7 } else { dirs };
                // Coming down onto it: settle on its top, not short of it.
                let top = self.entries[j].z1;
                if dirs & 4 != 0
                    && d.2 < 0
                    && (q.2..=at.2).contains(&top)
                    && self.blocker(gd, shape, (at.0, at.1, top), top, i, &inside).is_none()
                {
                    at.2 = top;
                }
                stopped = Some((self.ids[j], dirs));
                break;
            }
            at = q;
        }
        (it.x, it.y, it.z) = at;
        self.set(gd, id, it);
        stopped
    }

    /// (Something solid in the way, something to rest on) for `shape` at `at`, ignoring entry `skip`.
    fn room(&self, gd: &GameData, shape: u32, (x, y, z): (i32, i32, i32), skip: Option<usize>) -> (bool, bool) {
        let me = skip.map(|j| &self.entries[j].it);
        let flipped = me.is_some_and(|it| it.shape == shape && it.flags & item_flag::FLIPPED != 0);
        let (dx, dy, dz) = gd.typeflags.get(shape).footpad(flipped);
        let to = Bounds { x0: x - dx, y0: y - dy, z0: z, x1: x, y1: y, z1: z + dz.max(1) };
        let near: Vec<&Entry> = self
            .near(to.x0, to.y0, to.x1, to.y1)
            .filter(|&j| Some(j) != skip)
            .map(|j| &self.entries[j])
            .filter(|p| p.physical() && p.bounds().overlaps_xy(&to))
            .collect();
        let blocked = near.iter().any(|p| p.traits.solid && p.z1 > to.z0 && p.it.z < to.z1);
        let resting = near.iter().any(|p| !p.traits.actor && p.z1 == z);
        (blocked, resting)
    }

    /// Airborne move of `id` by (dx, dy, dz), unless something solid occupies the new box.
    /// Things already overlapped (e.g. a saved position slightly inside a table) don't block.
    pub fn try_move_air(&self, id: ObjId, dx: i32, dy: i32, dz: i32) -> Option<(i32, i32, i32)> {
        let idx = self.index(id)?;
        let a = &self.entries[idx];
        let now = a.bounds();
        let z = (a.it.z + dz).max(0);
        let to = now.shifted(dx, dy, z - a.it.z);
        let blocked = self
            .near(to.x0, to.y0, to.x1, to.y1)
            .filter(|&i| i != idx)
            .map(|i| &self.entries[i])
            .filter(|p| p.present() && p.traits.solid && !p.traits.editor)
            .any(|p| p.bounds().overlaps(&to) && !p.bounds().overlaps(&now));
        (!blocked).then_some((to.x1, to.y1, z))
    }

    /// Height `id` lands at: the highest surface under it no more than a step above its feet.
    pub fn floor_under(&self, id: ObjId) -> Option<i32> {
        let idx = self.index(id)?;
        let a = self.entries[idx].bounds();
        self.near(a.x0, a.y0, a.x1, a.y1)
            .filter(|&i| i != idx)
            .map(|i| &self.entries[i])
            .filter(|p| p.physical() && !p.traits.actor && p.bounds().overlaps_xy(&a))
            .map(|p| p.z1)
            .filter(|&top| top <= a.z0 + STEP)
            .max()
    }

    /// Where the view is cut away over `id` (ScummVM's CameraProcess::findRoof): the base of the
    /// lowest roof over any part of a 64×64 footprint at its feet, from 36 up (a camera box 20
    /// up, 16 tall). Only roofs count — not trees, walls or what's at head height.
    pub fn ceiling(&self, id: ObjId) -> Option<i32> {
        let it = self.get(id)?;
        let (x0, y0, z) = (it.x - 64, it.y - 64, it.z + 36);
        self.near(x0, y0, it.x, it.y)
            .map(|i| &self.entries[i])
            .filter(|p| {
                p.present()
                    && p.traits.roof
                    && !p.traits.actor
                    && p.it.z >= z
                    && p.x0 < it.x
                    && x0 < p.it.x
                    && p.y0 < it.y
                    && y0 < p.it.y
            })
            .map(|p| p.it.z)
            .min()
    }

    /// Are `a` and `b` (or what they're in) on the same floor — no roof between them (one of
    /// [`ceiling`](Self::ceiling)'s, over the lower, at or below the higher's feet)? Speech and
    /// sounds from another storey (the hall under a roof the Avatar is on) are kept there.
    pub fn same_floor(&self, a: ObjId, b: ObjId) -> bool {
        let (a, b) = (self.root(a), self.root(b));
        let (Some(p), Some(q)) = (self.get(a), self.get(b)) else { return true };
        let (low, high) = if p.z <= q.z { (a, q.z) } else { (b, p.z) };
        !self.ceiling(low).is_some_and(|roof| roof <= high)
    }

    /// Where `id` ends up stepping by (dx, dy): climbs up to [`WALK_STEP`] onto surfaces (ScummVM's
    /// ±8 position search — stairs, not tables), drops onto lower ones, and is blocked by solid
    /// things in the way. None = blocked or nothing to stand on.
    pub fn try_move(&self, id: ObjId, dx: i32, dy: i32) -> Option<(i32, i32, i32)> {
        let it = self.get(id)?;
        self.step_from(id, (it.x, it.y, it.z), dx, dy, true, (WALK_STEP, i32::MAX))
    }

    /// A walking step: up or down at most [`WALK_STEP`] (stairs, not tables, walls or ledges).
    pub fn try_walk(&self, id: ObjId, dx: i32, dy: i32) -> Option<(i32, i32, i32)> {
        let it = self.get(id)?;
        self.step_from(id, (it.x, it.y, it.z), dx, dy, true, (WALK_STEP, WALK_STEP))
    }

    /// [`try_move`](Self::try_move) as if `id` stood at `from`, climbing at most `up` and
    /// dropping at most `down`. With `actors` false, other people don't block (planning a
    /// route: they'll have moved by the time we get there).
    pub fn step_from(
        &self,
        id: ObjId,
        from: (i32, i32, i32),
        dx: i32,
        dy: i32,
        actors: bool,
        (up, down): (i32, i32),
    ) -> Option<(i32, i32, i32)> {
        let idx = self.index(id)?;
        let a = &self.entries[idx];
        let h = a.z1 - a.it.z;
        let start = a.bounds().shifted(from.0 - a.it.x, from.1 - a.it.y, from.2 - a.it.z);
        let to = start.shifted(dx, dy, 0);
        let near: Vec<&Entry> = self
            .near(to.x0, to.y0, to.x1, to.y1)
            .filter(|&i| i != idx)
            .map(|i| &self.entries[i])
            .filter(|p| p.physical() && (actors || !p.traits.actor) && p.bounds().overlaps_xy(&to))
            .collect();
        let floor = near.iter().filter(|p| !p.traits.actor).map(|p| p.z1).filter(|&top| top <= from.2 + up).max()?;
        let blocked = floor < from.2.saturating_sub(down)
            || near.iter().any(|p| {
                // What it's already inside (a door swung into it) doesn't hold it there (ScummVM).
                p.traits.solid && p.z1 > floor && p.it.z < floor + h.max(8) && !p.bounds().overlaps(&start)
            });
        (!blocked).then_some((to.x1, to.y1, floor))
    }

    /// What `id` is in contact with, as ScummVM's sweep counts it for events: what it overlaps,
    /// and what it stands on (a top at its feet). Sorted; eggs and markers aren't counted.
    pub fn contacts(&self, id: ObjId) -> Vec<ObjId> {
        let Some(i) = self.index(id).filter(|&i| self.entries[i].present() && self.entries[i].parent.is_none()) else {
            return vec![];
        };
        let b = self.entries[i].bounds();
        let mut v: Vec<usize> = self
            .near(b.x0, b.y0, b.x1, b.y1)
            .filter(|&k| k != i)
            .filter(|&k| {
                let e = &self.entries[k];
                let o = e.bounds();
                e.present()
                    && !e.traits.editor
                    && !e.traits.egg
                    && o.overlaps_xy(&b)
                    && (o.overlaps(&b) || o.z1 == b.z0)
            })
            .collect();
        v.sort_unstable();
        v.dedup();
        v.into_iter().map(|k| self.ids[k]).collect()
    }
}
