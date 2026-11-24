//! Eggs: invisible triggers that fire when the Avatar walks into their range — usecode eggs run
//! their class's `hatch` event, teleport eggs move the Avatar (often to another map). Monster
//! eggs are hatched by scripts, not by proximity.
use crate::world::{ObjId, World, AVATAR};
use u8data::{map::Item, typeflag::family};

/// Height band around an egg that counts as in range.
const Z_RANGE: i32 = 48;

/// Something an egg wants done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hatch {
    /// Run the egg's usecode `hatch` event.
    Usecode(ObjId),
    /// Move the Avatar to the destination egg with this id on `map`.
    Teleport { map: u16, id: u8 },
}

/// Teleport eggs come in pairs: teleporters (frame ≠ 1) send you to the destination (frame 1)
/// with the same id. The id is the low byte of the quality; the target map is the egg's map byte.
pub fn is_teleporter(it: &Item) -> bool {
    it.frame != 1
}

/// Is the Avatar, its footprint running from (x0, y0) (open) to its anchor (x, y) at height z,
/// in the egg's trigger box? Ranges are in 32-unit cells either side of the egg's anchor (x from
/// the npc byte's high nibble, y low); the footprint has only to reach into the box, ends
/// included, and the feet be within 48 up or down (EGG.C EggHatcher::process). A range of 0 is
/// the egg's own line: stood across, it fires.
pub fn in_range(egg: &Item, (x0, y0): (i32, i32), (x, y, z): (i32, i32, i32)) -> bool {
    let (rx, ry) = ((egg.npcnum >> 4) as i32 * 32, (egg.npcnum & 0xF) as i32 * 32);
    x >= egg.x - rx && x0 < egg.x + rx && y >= egg.y - ry && y0 < egg.y + ry && (z - egg.z).abs() <= Z_RANGE
}

/// Watches the Avatar against every egg on the map. Each egg fires once per visit to its range.
#[derive(Default)]
pub struct Hatcher {
    /// Just arrived through a teleporter: don't bounce straight back through a nearby one.
    just_teleported: bool,
}

impl Hatcher {
    pub fn arrived_by_teleport(&mut self) {
        self.just_teleported = true;
    }

    pub fn check(&mut self, world: &mut World) -> Vec<Hatch> {
        let Some(av) = world.index(AVATAR).map(|i| *world.entry(i)) else { return vec![] };
        let (low, at) = ((av.x0, av.y0), (av.it.x, av.it.y, av.it.z));
        let (mut fired, mut near_teleporter) = (vec![], false);
        for k in 0..world.eggs().len() {
            let idx = world.eggs()[k];
            let e = *world.entry(idx);
            if !e.present() || !in_range(&e.it, low, at) {
                world.set_hatched(idx, false);
                continue;
            }
            let teleporter = e.traits.family == family::TELEPORTEGG && is_teleporter(&e.it);
            near_teleporter |= teleporter;
            if e.hatched || (teleporter && self.just_teleported) {
                continue;
            }
            world.set_hatched(idx, true);
            match e.traits.family {
                family::TELEPORTEGG if teleporter => {
                    fired.push(Hatch::Teleport { map: e.it.mapnum as u16, id: (e.it.quality & 0xFF) as u8 })
                }
                family::UNKEGG => fired.push(Hatch::Usecode(world.id(idx))),
                _ => {} // destinations do nothing; monster eggs wait for scripts
            }
        }
        if !near_teleporter {
            self.just_teleported = false;
        }
        fired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_box() {
        let egg = Item { x: 1000, y: 2000, z: 48, npcnum: 0x21, ..Default::default() };
        // (A 64-square footprint, anchored at its far corner.)
        let at = |x: i32, y: i32, z: i32| in_range(&egg, (x - 64, y - 64), (x, y, z));
        assert!(at(1000, 2000, 48));
        assert!(at(936, 1968, 48), "just reaching the near edges");
        assert!(!at(935, 2000, 48) && !at(1000, 1967, 48));
        assert!(at(1127, 2000, 48) && !at(1128, 2000, 48), "the far edge, from beyond");
        assert!(at(1000, 2000, 96) && !at(1000, 2000, 97) && !at(1000, 2000, -1), "48 up or down");
        let line = Item { npcnum: 0, ..egg };
        assert!(in_range(&line, (960, 1960), (1024, 2024, 48)), "no range: its own point");
        assert!(!in_range(&line, (1000, 1960), (1064, 2024, 48)));
    }
}
