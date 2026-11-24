//! A tester's kit: the Avatar at its best (STR, DEX, INT 25; full health and mana) carrying a bag
//! of every school's magic — Necromancy's keys, reagents and ready spells, Theurgy's foci,
//! charged Sorcery foci and their reagents, Thaumaturgy's reagents, the scrolls — and the recall
//! item, whose script takes the Avatar to the game's main places (and so do [`PLACES`] here),
//! with the story flags those need. `vm.cheats` adds the original's developers' menu.
//! `u8tool <data> kit` writes it as a save.
use crate::{game::Game, world::AVATAR};
use u8data::map::Item;

/// Where the recall item goes: (name, map, teleport egg 30 there), as its script does.
pub const PLACES: [(&str, u16); 8] = [
    ("tenebrae", 40),
    ("plateau", 8),
    ("catacombs", 50),
    ("mountainking", 31),
    ("argentrock", 37),
    ("carthax", 16),
    ("carthax-drained", 26),
    ("crag", 12),
];
const RECALL_EGG: u8 = 30;
/// Story flags (global bits) the kit's magic asks for: Vardion's teaching (Sorcery's foci work
/// only for a Sorcerer — Bane's, 0x185, would do too), and every place the recall item knows
/// (set as the Avatar first sees each).
const FLAGS: [usize; 8] = [0x177, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x4B, 0x4C];

const BAG: u32 = 637;
/// (shape, frame, quality) of everything in the bag.
fn contents() -> Vec<(u32, u32, u16)> {
    let mut v = vec![];
    // Necromancy: the Keys of the Caretaker, Scion and Necromancer (frames 0, 3, 4; 1 and 2
    // are empty pictures; the Caretaker's and the Necromancer's make spells from the reagents
    // in a bag), the reagents, and the ten spells ready made.
    v.extend([0, 3, 4].map(|f| (232, f, 0)));
    v.extend([0, 6, 8, 9, 10, 13, 14, 16].map(|f| (395, f, 10)));
    v.extend((0..10).map(|f| (746, f, 0)));
    // Theurgy: the enchanted foci (frames 8–17: Divination … Reveal).
    v.extend((8..18).map(|f| (396, f, 0)));
    // Sorcery: a focus for each spell (wand, rod, staff, talisman by power), 9 charges each
    // (quality: spell × 256 + charges), and the reagents.
    let foci = [(0, 1), (0, 2), (0, 3), (0, 4), (3, 5), (3, 6), (3, 7), (6, 8), (6, 9), (9, 10), (9, 11), (6, 12)];
    v.extend(foci.map(|(f, spell)| (397, f, spell << 8 | 9)));
    v.extend([0, 2, 6, 10, 14, 18].map(|f| (398, f, 10)));
    // Thaumaturgy's reagents; the scrolls (the quality says which: 1 dispelling magic portals,
    // 50–55 trap detection, trap destruction, healing, restoration, invisibility, restore to
    // sight; others are letters and notes);
    // the recall item.
    v.extend((0..4).map(|f| (399, f, 10)));
    v.extend([1, 50, 51, 52, 53, 54, 55].map(|q| (827, 0, q)));
    v.push((833, 0, 0));
    v
}

impl Game {
    /// Make the Avatar a tester (see the module): its stats, and the bag in its backpack (or at
    /// its feet without one). Returns the bag.
    pub fn give_test_kit(&mut self) -> Option<crate::world::ObjId> {
        if let Some(s) = self.stats.get_mut(AVATAR) {
            (s.str, s.dex, s.int) = (25, 25, 25);
            (s.hp, s.mana) = (s.max_hp(), s.max_mana());
        }
        if let Some(s) = &mut self.scripts {
            FLAGS.iter().for_each(|&f| s.vm.set_global_bits(f, 1, 1));
        }
        let holder = self.backpack().unwrap_or(AVATAR);
        let bag = self.world.add_inside(&self.data, Item { shape: BAG, x: 40, y: 40, ..Default::default() }, holder);
        for (k, (shape, frame, quality)) in contents().into_iter().enumerate() {
            let (x, y) = (12 + 9 * (k as i32 % 8), 12 + 7 * (k as i32 / 8 % 6));
            let it = Item { shape, frame, quality, x, y, ..Default::default() };
            self.world.add_inside(&self.data, it, bag);
        }
        Some(bag)
    }

    /// To one of [`PLACES`] by name, as the recall item would. False if there's no such place.
    pub fn go_to(&mut self, place: &str) -> bool {
        PLACES.iter().find(|p| p.0 == place).is_some_and(|&(_, map)| self.teleport(map, RECALL_EGG))
    }
}
