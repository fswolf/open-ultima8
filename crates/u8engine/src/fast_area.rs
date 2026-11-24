//! The "fast area": the neighbourhood of the Avatar where the world is live. Items get the
//! `enterFastArea` event when they come into it and `leaveFastArea` when they drop out — NPCs
//! start their routines, fires and fountains their sounds, many eggs their scripts.
use crate::world::{ObjId, World, AVATAR};

/// The original's area (ITEMCACH.C calculateNeeded): the map's 512-unit regions within 7 of the
/// Avatar's in x + y (down the screen, 64 pixels a region) and 3 in x - y (across, 128 a
/// region), the diamond's four corners left out — its 320×200 view and a margin. A bigger view
/// (zoomed out, a wide window) widens it by what more it shows, so the world is live as far as
/// can be seen and nobody appears or vanishes on screen.
const REGION: i32 = 512;
/// (In x + y and in x - y.)
pub const DOS_REACH: (i32, i32) = (7, 3);
pub const DOS_VIEW: (f32, f32) = (320.0, 200.0);

/// Which items are in it is kept on the world's entries (`Entry::fast`, ScummVM's FLG_FASTAREA),
/// so scripts' enterFastArea can bring one in early.
pub struct FastArea {
    /// How much of the world shows (game pixels).
    pub view: (f32, f32),
}

impl Default for FastArea {
    fn default() -> Self {
        Self { view: DOS_VIEW }
    }
}

/// Who came in and who went out since the last update.
#[derive(Default, Debug)]
pub struct Change {
    pub entered: Vec<ObjId>,
    pub left: Vec<ObjId>,
}

impl FastArea {
    /// Is `id` in it (as of the last update, or brought in by a script since)?
    pub fn contains(&self, world: &World, id: ObjId) -> bool {
        world.is_fast(id)
    }

    /// How far it reaches, in regions: in x + y and in x - y.
    pub fn reach(&self) -> (i32, i32) {
        let more = |seen: f32, dos: f32, per: f32| ((seen - dos).max(0.0) / 2.0 / per).ceil() as i32;
        (DOS_REACH.0 + more(self.view.1, DOS_VIEW.1, 64.0), DOS_REACH.1 + more(self.view.0, DOS_VIEW.0, 128.0))
    }

    /// Is a spot in the area around `at`?
    pub fn holds(&self, at: (i32, i32), x: i32, y: i32) -> bool {
        holds(self.reach(), at, x, y)
    }

    /// Recompute around the Avatar (nothing is fast on a map without it). A new map's items
    /// all start outside.
    pub fn update(&mut self, world: &mut World) -> Change {
        let now: Vec<ObjId> = match world.get(AVATAR) {
            Some(av) => {
                // (A square round the diamond, then the diamond.)
                let (sum, diff) = self.reach();
                world.fast_reach = (sum, diff);
                let square = ((sum + diff) / 2 + 1) * REGION;
                let holds = |id| world.get(id).is_some_and(|it| self.holds((av.x, av.y), it.x, it.y));
                world.area(av.x, av.y, square).into_iter().filter(|&id| holds(id)).collect()
            }
            None => vec![],
        };
        let mut change = Change::default();
        for &id in &now {
            if !world.is_fast(id) {
                world.set_fast(id, true);
                change.entered.push(id);
            }
        }
        let now: std::collections::HashSet<ObjId> = now.into_iter().collect();
        for i in 0..world.len() {
            let (id, e) = (world.id(i), *world.entry(i));
            if e.fast && !now.contains(&id) {
                world.set_fast_at(i, false);
                if !e.gone {
                    change.left.push(id);
                }
            }
        }
        change
    }
}

/// Is a spot in the area reaching `(sum, diff)` regions round `at` (the diamond, its corners
/// left out)?
pub fn holds((sum, diff): (i32, i32), at: (i32, i32), x: i32, y: i32) -> bool {
    let r = |v: i32| v.div_euclid(REGION);
    let (s, d) = ((r(x) + r(y)) - (r(at.0) + r(at.1)), (r(x) - r(y)) - (r(at.0) - r(at.1)));
    s.abs() <= sum && d.abs() <= diff && !(s.abs() == sum && d.abs() == diff)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// At the original's 320×200 it's the original's diamond (7 regions down, 3 across); bigger
    /// views widen it, and everything a view shows (its corners included, from wherever the
    /// Avatar stands in its region) is in it.
    #[test]
    fn the_area_covers_what_shows() {
        for (view, reach) in [((320.0, 200.0), (7, 3)), ((640.0, 400.0), (9, 5)), ((1920.0, 1200.0), (15, 10))] {
            let f = FastArea { view };
            assert_eq!(f.reach(), reach, "{view:?}");
            let (hw, hh) = (view.0 as i32 / 2, view.1 as i32 / 2);
            for at in [(20_000, 20_000), (20_511, 20_000), (20_000, 20_511), (20_511, 20_511)] {
                for (sx, sy) in [(-hw, -hh), (hw, -hh), (-hw, hh), (hw, hh), (0, 0)] {
                    // (On the floor: screen (sx, sy) from the Avatar is world (4sy + 2sx, 4sy - 2sx).)
                    let (x, y) = (at.0 + 4 * sy + 2 * sx, at.1 + 4 * sy - 2 * sx);
                    assert!(f.holds(at, x, y), "{view:?} from {at:?}: ({sx}, {sy}) on screen");
                }
            }
        }
        // And not much past it: the original's ends a few regions off.
        let f = FastArea::default();
        assert!(!f.holds((20_000, 20_000), 20_000 + 8 * 512, 20_000), "8 regions away");
    }
}
