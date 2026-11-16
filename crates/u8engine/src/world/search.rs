//! Scripts' `loop` searches over the map, as ScummVM's CurrentMap: an area around a point, or
//! what's on top of (or under) something. Results come chunk by chunk (512 units: rows, then
//! columns), and within a chunk in its list's order — loaded items in load order, things put
//! on the map since at the front (disposable and fast-only ones at the back) — which scripts
//! that take the first match depend on.
use super::*;

const CHUNK: i32 = 512;

pub(super) fn chunk(it: &Item) -> (i32, i32) {
    (it.y.div_euclid(CHUNK), it.x.div_euclid(CHUNK))
}

impl World {
    /// `idx` (re)joins its chunk's list: at the front, or the back if disposable or fast-only
    /// (ScummVM's addItem / addItemToEnd).
    pub(super) fn requeue(&mut self, idx: usize, it: &Item) {
        let seq = if it.flags & (item_flag::DISPOSABLE | item_flag::FAST_ONLY) != 0 {
            self.seq_back += 1;
            self.seq_back
        } else {
            self.seq_front -= 1;
            self.seq_front
        };
        self.entries[idx].seq = seq;
    }

    /// Map items (not in the void, not inside anything), in chunk-list order.
    fn ordered(&self, mut v: Vec<usize>) -> Vec<ObjId> {
        v.retain(|&i| self.entries[i].present() && self.entries[i].parent.is_none());
        v.sort_unstable();
        v.dedup();
        v.sort_by_key(|&i| (chunk(&self.entries[i].it), self.entries[i].seq));
        v.into_iter().map(|i| self.ids[i]).collect()
    }

    /// ScummVM's areaSearch: things whose anchor lies within `range` of (x, y) — reaching back
    /// over a footprint of `back` (x, y) behind it — and with `recurse`, everything inside them
    /// straight after each.
    pub fn search_area(&self, (x, y): (i32, i32), range: i32, back: (i32, i32), recurse: bool) -> Vec<ObjId> {
        let (x0, y0, x1, y1) = (x - back.0 - range, y - back.1 - range, x + range, y + range);
        let near = self.near(x0, y0, x1, y1).filter(|&i| {
            let it = &self.entries[i].it;
            (x0..=x1).contains(&it.x) && (y0..=y1).contains(&it.y)
        });
        let found = self.ordered(near.collect());
        if !recurse {
            return found;
        }
        let mut all = vec![];
        for id in found {
            all.push(id);
            self.all_inside(id, &mut all);
        }
        all
    }

    fn all_inside(&self, id: ObjId, out: &mut Vec<ObjId>) {
        for c in self.contents(id) {
            out.push(c);
            self.all_inside(c, out);
        }
    }

    /// ScummVM's surfaceSearch: things overlapping `id`'s footprint whose bottom is its top
    /// (`above`), or whose top is its bottom (`below`). `id` answers for where its outermost
    /// container is. With `recurse`, what's on (or under) those too, unless they're level with
    /// it (each straight after the thing it's on).
    pub fn search_surface(&self, id: ObjId, above: bool, below: bool, recurse: bool) -> Vec<ObjId> {
        let mut out = vec![];
        self.surface_into(id, above, below, recurse, &mut out);
        out
    }

    fn surface_into(&self, id: ObjId, above: bool, below: bool, recurse: bool, out: &mut Vec<ObjId>) {
        let (Some(i), Some(me)) = (self.index(id), self.abs_box(id)) else { return };
        let near = self.near(me.x0, me.y0, me.x1, me.y1).filter(|&k| {
            let o = self.entries[k].bounds();
            k != i && o.overlaps_xy(&me) && ((above && o.z0 == me.z1) || (below && o.z1 == me.z0))
        });
        for o in self.ordered(near.collect()) {
            let Some(ob) = self.abs_box(o) else { continue };
            if recurse && above && ob.z0 == me.z1 && ob.z1 != me.z1 {
                self.surface_into(o, true, false, true, out);
            }
            if recurse && below && ob.z1 == me.z0 && ob.z0 != me.z0 {
                self.surface_into(o, false, true, true, out);
            }
            out.push(o);
        }
    }
}

impl World {
    /// `id`'s box where its outermost holder is (ScummVM's getLocationAbsolute with its own
    /// footprint).
    pub fn abs_box(&self, id: ObjId) -> Option<Bounds> {
        let (i, r) = (self.index(id)?, self.index(self.root(id))?);
        let (b, at) = (self.entries[i].bounds(), self.entries[r].it);
        let (dx, dy, dz) = (b.x1 - b.x0, b.y1 - b.y0, b.z1 - b.z0);
        Some(Bounds { x0: at.x - dx, y0: at.y - dy, z0: at.z, x1: at.x, y1: at.y, z1: at.z + dz })
    }

    /// ScummVM's Item::getRange (U8): the gap between two boxes — with the original's odd
    /// choice of which gap to report kept.
    pub fn range_between(&self, a: ObjId, b: ObjId) -> Option<i32> {
        let (p, q) = (self.abs_box(a)?, self.abs_box(b)?);
        let mut r = 0;
        if p.x0 - q.x1 > r {
            r = p.y0 - q.y1;
        }
        if q.x0 - p.x1 > r {
            r = p.x0 - q.x1;
        }
        if p.y0 - q.y1 > r {
            r = q.x0 - p.x1;
        }
        if q.y0 - p.y1 > r {
            r = q.y0 - p.y1;
        }
        Some(r)
    }

    /// ScummVM's Item::canReach: `other` (or what holds it) within `range` of `id`'s box, and in
    /// sight — from anchor to anchor, else from eye height to its middle, else to its top.
    pub fn can_reach(&self, id: ObjId, other: ObjId, range: i32) -> bool {
        let other = self.root(other);
        let (Some(a), Some(b)) = (self.abs_box(id), self.abs_box(other)) else { return false };
        if a.x0 - b.x1 > range || b.x0 - a.x1 > range || a.y0 - b.y1 > range || b.y0 - a.y1 > range {
            return false;
        }
        let me = [id, self.root(id)];
        let (dx, dy, dz) = (a.x1 - a.x0, a.y1 - a.y0, a.z1 - a.z0);
        let (ox, oy, oz) = (b.x1 - b.x0, b.y1 - b.y0, b.z1 - b.z0);
        let mut from = (a.x1, a.y1, a.z0);
        let mut to = (b.x1, b.y1, b.z0);
        if to.2 > a.z0 && to.2 < a.z0 + dz {
            from.2 = to.2;
        }
        if self.in_sight(from, to, &me, other) {
            return true;
        }
        from = (a.x1 - dx / 2, a.y1 - dy / 2, a.z0 + if dz > 16 { dz - 8 } else { 0 });
        to = (b.x1 - ox / 2, b.y1 - oy / 2, b.z0 + oz / 2);
        if self.in_sight(from, to, &me, other) {
            return true;
        }
        to.2 = b.z0 + oz;
        self.in_sight(from, to, &me, other)
    }

    /// A 2×2×2 box swept from `from` to `to`: does it meet `other` before anything solid
    /// (ignoring `me`)? Reaching the end without meeting either counts as seen.
    fn in_sight(&self, from: (i32, i32, i32), to: (i32, i32, i32), me: &[ObjId], other: ObjId) -> bool {
        let d = (to.0 - from.0, to.1 - from.1, to.2 - from.2);
        let steps = (d.0.abs().max(d.1.abs()).max(d.2.abs()) / 2).max(1);
        for k in 0..=steps {
            let p = (from.0 + d.0 * k / steps, from.1 + d.1 * k / steps, from.2 + d.2 * k / steps);
            let probe = Bounds { x0: p.0 - 2, y0: p.1 - 2, z0: p.2, x1: p.0, y1: p.1, z1: p.2 + 2 };
            let (mut seen, mut blocked) = (false, false);
            for j in self.near(probe.x0, probe.y0, probe.x1, probe.y1) {
                let e = &self.entries[j];
                let id = self.ids[j];
                if !e.present() || me.contains(&id) || !e.bounds().overlaps(&probe) {
                    continue;
                }
                seen |= id == other;
                blocked |= e.traits.solid && !e.marker();
            }
            if seen {
                return true;
            }
            if blocked {
                return false;
            }
        }
        true
    }
}
