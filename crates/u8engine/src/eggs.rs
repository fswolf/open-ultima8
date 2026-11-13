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

/// Is the point (x, y, z) — the Avatar's footprint centre — inside the egg's trigger box?
/// Ranges are in 32-unit cells around the egg's anchor (x from the npc byte's high nibble, y low).
pub fn in_range(egg: &Item, (x, y, z): (i32, i32, i32)) -> bool {
    let (rx, ry) = ((egg.npcnum >> 4) as i32 * 32, (egg.npcnum & 0xF) as i32 * 32);
    egg.x - rx <= x
        && x < egg.x + rx
        && egg.y - ry <= y
        && y < egg.y + ry
        && egg.z - Z_RANGE < z
        && z <= egg.z + Z_RANGE
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
        let centre = (av.it.x - (av.it.x - av.x0) / 2, av.it.y - (av.it.y - av.y0) / 2, av.it.z);
        let (mut fired, mut near_teleporter) = (vec![], false);
        for k in 0..world.eggs().len() {
            let idx = world.eggs()[k];
            let e = *world.entry(idx);
            if !e.present() || !in_range(&e.it, centre) {
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
        assert!(in_range(&egg, (1000, 2000, 48)));
        assert!(in_range(&egg, (936, 1968, 48))); // x - 64, y - 32: inclusive
        assert!(!in_range(&egg, (1064, 2000, 48))); // x + 64: exclusive
        assert!(!in_range(&egg, (1000, 2032, 48)));
        assert!(!in_range(&egg, (1000, 2000, 0))); // 48 below
        assert!(!in_range(&Item { npcnum: 0, ..egg }, (1000, 2000, 48))); // no range: never
    }
}
