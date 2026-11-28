//! What kinds of creature (and some kinds of people) are like in a fight, by shape: hit points
//! and dexterity for new ones, how hard they hit and with what, their armour and what they're
//! proof against, and whose side they're on. The numbers are ScummVM's reading of the original
//! (its u8monsters.ini and u8game.ini); people without an entry hit for 1 and wear no armour.
use crate::combat::dmg;

/// The ghost, and the skull it pulls off and sends after you (ANIMCALL.C).
pub const GHOST: u32 = 411;
pub const SKULL: u32 = 413;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Monster {
    pub name: &'static str,
    pub shape: u32,
    pub hp: (u16, u16),
    pub dex: (u8, u8),
    pub damage: (u8, u8),
    pub armour: u8,
    /// Own sides (low nibble) and enemies (high nibble), as NPCDATA's alignment byte.
    pub alignment: u8,
    pub damage_type: u16,
    pub defence: u16,
    /// Gets up again a while after dying (unless burnt).
    pub resurrects: bool,
    /// Changes shape while waiting (changeling).
    pub shifter: bool,
    /// Blows apart into pieces of this shape on dying (seeker: u8monsters' `explode`).
    pub bursts_into: Option<u32>,
    /// What it leaves on dying (in its body, for searching).
    pub loot: &'static [Loot],
}

/// One kind of thing a creature may carry to its death.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loot {
    /// `count` of `shape` (one pile, for things counted by quantity; else that many, each a frame
    /// picked from `frames`), with a chance in a thousand; `map` (not 0): only on that map.
    Items { shape: u32, frames: &'static [u32], permille: u16, count: (u16, u16), map: u8 },
    /// Maybe a weapon (see [`WEAPON_LOOT`]).
    Weapon,
    /// A sorcerer's foci: maybe a wand, rod, symbol, demon talisman (charged or not).
    Focus,
}

/// Weapons a creature may drop, lowest shape first, each with a chance in a hundred; the first
/// that comes up is the one (ScummVM's treasure chances).
pub const WEAPON_LOOT: [(u32, u32); 7] = [(417, 9), (418, 15), (419, 1), (420, 2), (540, 7), (541, 5), (542, 3)];

/// Damage type bit beyond the weapon table's eight (the ghost's): carried, unused.
const EXTRA: u16 = 0x200;

#[rustfmt::skip]
const MONSTERS: &[Monster] = &[
    Monster { name: "ghoul", shape: 142, hp: (35, 50), dex: (14, 16), damage: (6, 12), armour: 15, alignment: 0x68, damage_type: dmg::NORMAL, defence: dmg::UNDEAD | dmg::SLAYER, resurrects: false, shifter: false, bursts_into: None, loot: &[] },
    Monster { name: "ghost", shape: 411, hp: (80, 95), dex: (15, 21), damage: (0, 0), armour: 16, alignment: 0x68, damage_type: dmg::NORMAL | EXTRA, defence: dmg::UNDEAD | dmg::MAGIC | dmg::SLAYER, resurrects: false, shifter: false, bursts_into: None, loot: &[Loot::Items { shape: 821, frames: &[0], permille: 1000, count: (1, 1), map: 54 }, Loot::Items { shape: 766, frames: &[0, 1, 2, 3, 4, 5, 6, 7], permille: 500, count: (1, 1), map: 0 }, Loot::Items { shape: 143, frames: &[0], permille: 500, count: (20, 40), map: 0 }, Loot::Items { shape: 592, frames: &[0, 4], permille: 330, count: (1, 5), map: 0 }] },
    Monster { name: "skeleton", shape: 119, hp: (35, 45), dex: (15, 20), damage: (7, 13), armour: 17, alignment: 0x68, damage_type: dmg::BLADE, defence: dmg::UNDEAD | dmg::SLAYER, resurrects: true, shifter: false, bursts_into: None, loot: &[Loot::Items { shape: 766, frames: &[0, 1, 2, 3, 4, 5, 6, 7], permille: 500, count: (1, 1), map: 0 }, Loot::Weapon, Loot::Items { shape: 143, frames: &[0], permille: 500, count: (7, 15), map: 0 }, Loot::Items { shape: 592, frames: &[0, 4], permille: 330, count: (1, 4), map: 0 }, Loot::Items { shape: 579, frames: &[0, 4], permille: 330, count: (1, 4), map: 0 }] },
    Monster { name: "torax", shape: 120, hp: (40, 55), dex: (8, 12), damage: (7, 14), armour: 28, alignment: 0x24, damage_type: dmg::NORMAL, defence: dmg::BLADE | dmg::PIERCE, resurrects: false, shifter: false, bursts_into: None, loot: &[Loot::Items { shape: 537, frames: &[2], permille: 1000, count: (1, 1), map: 0 }] },
    Monster { name: "troll", shape: 83, hp: (150, 225), dex: (7, 10), damage: (10, 28), armour: 18, alignment: 0xE8, damage_type: dmg::BLUNT, defence: dmg::PIERCE, resurrects: false, shifter: false, bursts_into: None, loot: &[Loot::Items { shape: 766, frames: &[0, 1, 2, 3, 4, 5, 6, 7], permille: 500, count: (1, 1), map: 0 }, Loot::Weapon, Loot::Items { shape: 143, frames: &[0], permille: 500, count: (30, 75), map: 0 }, Loot::Items { shape: 592, frames: &[0, 4], permille: 330, count: (1, 8), map: 0 }, Loot::Items { shape: 579, frames: &[0, 4], permille: 330, count: (1, 8), map: 0 }] },
    Monster { name: "demon", shape: 96, hp: (80, 100), dex: (14, 21), damage: (12, 18), armour: 19, alignment: 0xA4, damage_type: dmg::NORMAL | dmg::FIRE | dmg::MAGIC, defence: dmg::FIRE | dmg::PIERCE, resurrects: false, shifter: false, bursts_into: None, loot: &[Loot::Items { shape: 398, frames: &[18], permille: 800, count: (1, 1), map: 0 }, Loot::Items { shape: 398, frames: &[19], permille: 600, count: (1, 1), map: 0 }, Loot::Items { shape: 398, frames: &[20], permille: 750, count: (0, 2), map: 0 }] },
    Monster { name: "golem", shape: 214, hp: (150, 175), dex: (7, 8), damage: (15, 25), armour: 18, alignment: 0x24, damage_type: dmg::BLUNT, defence: dmg::BLADE | dmg::FIRE | dmg::PIERCE, resurrects: false, shifter: false, bursts_into: None, loot: &[Loot::Items { shape: 143, frames: &[0], permille: 500, count: (30, 75), map: 0 }, Loot::Items { shape: 592, frames: &[0, 4], permille: 330, count: (1, 8), map: 0 }] },
    Monster { name: "seeker", shape: 509, hp: (75, 100), dex: (17, 25), damage: (12, 16), armour: 14, alignment: 0x68, damage_type: dmg::NORMAL, defence: 0, resurrects: false, shifter: false, bursts_into: Some(458), loot: &[] },
    Monster { name: "fireskull", shape: 413, hp: (2, 5), dex: (12, 15), damage: (1, 3), armour: 0, alignment: 0xA4, damage_type: dmg::NORMAL | dmg::PIERCE, defence: dmg::UNDEAD, resurrects: false, shifter: false, bursts_into: None, loot: &[] },
    Monster { name: "changeling", shape: 357, hp: (30, 45), dex: (12, 16), damage: (7, 10), armour: 11, alignment: 0x24, damage_type: dmg::NORMAL, defence: 0, resurrects: false, shifter: true, bursts_into: None, loot: &[Loot::Items { shape: 143, frames: &[0], permille: 500, count: (6, 13), map: 0 }, Loot::Items { shape: 592, frames: &[0, 4], permille: 333, count: (1, 3), map: 0 }, Loot::Items { shape: 579, frames: &[0, 4], permille: 333, count: (0, 4), map: 0 }] },
    Monster { name: "guard", shape: 574, hp: (35, 65), dex: (15, 25), damage: (8, 15), armour: 25, alignment: 0xE1, damage_type: dmg::BLADE, defence: 0, resurrects: false, shifter: false, bursts_into: None, loot: &[Loot::Weapon, Loot::Items { shape: 143, frames: &[0], permille: 500, count: (10, 20), map: 0 }, Loot::Items { shape: 592, frames: &[0, 4], permille: 330, count: (1, 4), map: 0 }, Loot::Items { shape: 579, frames: &[0, 4], permille: 330, count: (1, 4), map: 0 }] },
    Monster { name: "smallspider", shape: 807, hp: (2, 6), dex: (8, 12), damage: (1, 1), armour: 0, alignment: 0x24, damage_type: dmg::BLADE, defence: 0, resurrects: false, shifter: false, bursts_into: None, loot: &[] },
    Monster { name: "smalllizard", shape: 806, hp: (2, 7), dex: (8, 12), damage: (1, 2), armour: 0, alignment: 0x24, damage_type: dmg::BLADE, defence: 0, resurrects: false, shifter: false, bursts_into: None, loot: &[] },
    Monster { name: "peasant1", shape: 707, hp: (10, 20), dex: (10, 15), damage: (2, 5), armour: 10, alignment: 0x21, damage_type: dmg::NORMAL, defence: 0, resurrects: false, shifter: false, bursts_into: None, loot: &[] },
    Monster { name: "kith", shape: 76, hp: (85, 105), dex: (14, 18), damage: (12, 16), armour: 17, alignment: 0x28, damage_type: dmg::BLUNT, defence: dmg::PIERCE, resurrects: false, shifter: false, bursts_into: None, loot: &[] },
    Monster { name: "invisible", shape: 808, hp: (15, 25), dex: (8, 12), damage: (4, 6), armour: 5, alignment: 0x24, damage_type: dmg::NORMAL, defence: 0, resurrects: false, shifter: false, bursts_into: None, loot: &[] },
    Monster { name: "smallinsect", shape: 824, hp: (18, 22), dex: (15, 23), damage: (3, 5), armour: 3, alignment: 0x24, damage_type: dmg::BLADE, defence: 0, resurrects: false, shifter: false, bursts_into: None, loot: &[] },
    Monster { name: "peasant2", shape: 835, hp: (15, 25), dex: (10, 15), damage: (2, 5), armour: 10, alignment: 0x21, damage_type: dmg::NORMAL, defence: 0, resurrects: false, shifter: false, bursts_into: None, loot: &[] },
    Monster { name: "peasant3", shape: 836, hp: (15, 25), dex: (10, 15), damage: (2, 5), armour: 10, alignment: 0x21, damage_type: dmg::NORMAL, defence: 0, resurrects: false, shifter: false, bursts_into: None, loot: &[] },
    Monster { name: "child", shape: 708, hp: (7, 14), dex: (10, 15), damage: (1, 1), armour: 5, alignment: 0x21, damage_type: dmg::NORMAL, defence: 0, resurrects: false, shifter: false, bursts_into: None, loot: &[] },
    Monster { name: "sorcerer", shape: 200, hp: (17, 24), dex: (12, 16), damage: (5, 10), armour: 12, alignment: 0x24, damage_type: dmg::NORMAL, defence: 0, resurrects: false, shifter: false, bursts_into: None, loot: &[Loot::Items { shape: 766, frames: &[0, 1, 2, 3, 4, 5, 6, 7], permille: 500, count: (1, 1), map: 0 }, Loot::Items { shape: 398, frames: &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13], permille: 1000, count: (0, 9), map: 0 }, Loot::Items { shape: 398, frames: &[14, 15, 16, 17, 18, 19, 20], permille: 1000, count: (0, 5), map: 0 }, Loot::Focus] },
    Monster { name: "malchir", shape: 187, hp: (17, 24), dex: (12, 16), damage: (5, 10), armour: 12, alignment: 0x24, damage_type: dmg::NORMAL, defence: 0, resurrects: false, shifter: false, bursts_into: None, loot: &[Loot::Items { shape: 766, frames: &[0, 1, 2, 3, 4, 5, 6, 7], permille: 500, count: (1, 1), map: 0 }, Loot::Items { shape: 398, frames: &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13], permille: 1000, count: (0, 9), map: 0 }, Loot::Items { shape: 398, frames: &[14, 15, 16, 17, 18, 19, 20], permille: 1000, count: (0, 5), map: 0 }, Loot::Focus] },
    Monster { name: "theurgist", shape: 269, hp: (17, 24), dex: (12, 16), damage: (5, 10), armour: 12, alignment: 0x24, damage_type: dmg::NORMAL, defence: 0, resurrects: false, shifter: false, bursts_into: None, loot: &[Loot::Items { shape: 766, frames: &[0, 1, 2, 3, 4, 5, 6, 7], permille: 500, count: (1, 1), map: 0 }, Loot::Items { shape: 143, frames: &[0], permille: 500, count: (5, 10), map: 0 }] },
    Monster { name: "airservant", shape: 377, hp: (40, 60), dex: (15, 22), damage: (6, 12), armour: 5, alignment: 0x24, damage_type: dmg::NORMAL, defence: dmg::MAGIC, resurrects: false, shifter: false, bursts_into: None, loot: &[] },
    Monster { name: "avatar", shape: 1, hp: (1, 1), dex: (5, 10), damage: (7, 15), armour: 15, alignment: 0x24, damage_type: dmg::NORMAL, defence: 0, resurrects: false, shifter: false, bursts_into: None, loot: &[] },
    Monster { name: "darion", shape: 401, hp: (1, 1), dex: (5, 10), damage: (6, 10), armour: 13, alignment: 0x21, damage_type: dmg::BLADE, defence: 0, resurrects: false, shifter: false, bursts_into: None, loot: &[] },
    Monster { name: "shaana", shape: 144, hp: (1, 1), dex: (5, 10), damage: (9, 15), armour: 10, alignment: 0x21, damage_type: dmg::BLADE, defence: 0, resurrects: false, shifter: false, bursts_into: None, loot: &[] },
    Monster { name: "tarna", shape: 405, hp: (1, 1), dex: (5, 10), damage: (6, 10), armour: 13, alignment: 0x21, damage_type: dmg::BLADE, defence: 0, resurrects: false, shifter: false, bursts_into: None, loot: &[] },
];

/// The fighting numbers for actors of shape `shape`, if it has any.
pub fn get(shape: u32) -> Option<&'static Monster> {
    MONSTERS.iter().find(|m| m.shape == shape)
}

#[cfg(test)]
mod tests {
    #[test]
    fn ghouls_are_undead() {
        let g = super::get(142).unwrap();
        assert_eq!((g.name, g.hp, g.armour), ("ghoul", (35, 50), 15));
        assert_ne!(g.defence & super::dmg::UNDEAD, 0);
    }
}
