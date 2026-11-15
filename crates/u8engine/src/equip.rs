//! What people wear, wield and carry. TYPEFLAG's equip type (byte 2, low nibble) names the slot;
//! the paperdoll shows each slot at a fixed spot (the positions saved games give worn things
//! agree). Also weights, and how a wielded weapon is drawn over the Avatar.
use crate::combat::dmg;
use crate::world::{item_flag, ObjId, World, AVATAR};
use u8data::{typeflag::family, weapons::Weapon, GameData};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Shield = 1,
    Arms,
    Head,
    Body,
    Legs,
    Weapon,
}

impl Slot {
    /// The slot a shape is worn in, if any.
    pub fn of(gd: &GameData, shape: u32) -> Option<Self> {
        Some(match gd.typeflags.get(shape).equip {
            1 => Self::Shield,
            2 => Self::Arms,
            3 => Self::Head,
            4 => Self::Body,
            5 => Self::Legs,
            6 => Self::Weapon,
            _ => return None,
        })
    }

    /// Where it shows on the paperdoll (window position).
    pub fn spot(self) -> (i32, i32) {
        match self {
            Self::Shield => (24, 60),
            Self::Arms => (36, 50),
            Self::Head => (40, 26),
            Self::Body => (40, 63),
            Self::Legs => (40, 92),
            Self::Weapon => (16, 18),
        }
    }
}

/// MAGARMR2: a new game's Avatar wears it (NPCDATA lists it as body armour), but the original's
/// paperdoll neither shows it nor counts its weight (WGHT 1: just the backpack).
pub const UNSEEN_ARMOUR: u32 = 64;

/// Where `it` is listed in NPCDATA's equipment (1–6 its slot, 7 the backpack), if it's worn.
pub fn listed_as(gd: &GameData, it: &u8data::map::Item) -> Option<usize> {
    if it.flags & item_flag::EQUIPPED == 0 {
        return None;
    }
    match Slot::of(gd, it.shape) {
        Some(s) => Some(s as usize),
        None => (gd.typeflags.get(it.shape).family == family::CONTAINER).then_some(7),
    }
}

/// What `who` has on in `slot`.
pub fn worn(gd: &GameData, world: &World, who: ObjId, slot: Slot) -> Option<ObjId> {
    world.contents(who).into_iter().find(|&id| {
        world.get(id).is_some_and(|it| it.flags & item_flag::EQUIPPED != 0 && Slot::of(gd, it.shape) == Some(slot))
    })
}

/// Keyrings: what's on them weighs nothing.
const KEYRING: u32 = 79;
/// Barrels count as 30, whatever's in them (ScummVM's reading).
const BARREL: u32 = 115;

/// Weight of an item and everything in it, in tenths (the paperdoll shows a tenth of the total,
/// rounded up: castle game 34.4 → WGHT 35, new game 1): TYPEFLAG byte 6 × 10; coins and other quantities weigh that per hundred
/// (rounded up), reagents per ten. Checked against the original's WGHT.
pub fn weight(gd: &GameData, world: &World, id: ObjId) -> u32 {
    let Some(it) = world.get(id).filter(|it| it.shape != UNSEEN_ARMOUR) else { return 0 };
    let info = gd.typeflags.get(it.shape);
    let (w, q) = (info.weight as u32, it.quality as u32);
    let own = match info.family {
        family::QUANTITY => (w * q).div_ceil(10),
        family::REAGENT => w * q,
        _ if it.shape == BARREL => 300,
        _ => w * 10,
    };
    if it.shape == KEYRING {
        return own;
    }
    own + world.contents(id).into_iter().map(|c| weight(gd, world, c)).sum::<u32>()
}

/// What someone of this strength can carry, in tenths: four times strength in the patched game
/// (three before; Dino's guide).
pub fn max_weight(strength: u8) -> u32 {
    40 * strength as u32
}

/// Room a thing takes in a container (ScummVM's getVolume): TYPEFLAG byte 7; quantities per
/// hundred and reagents per ten (rounded up); a container at least 1; invisible things none.
pub fn volume(gd: &GameData, it: &u8data::map::Item) -> u32 {
    if it.flags & item_flag::INVISIBLE != 0 {
        return 0;
    }
    let info = gd.typeflags.get(it.shape);
    let (v, q) = (info.volume as u32, it.quality as u32);
    match info.family {
        family::QUANTITY => (q * v).div_ceil(100),
        family::REAGENT => (q * v).div_ceil(10),
        family::CONTAINER => v.max(1),
        _ => v,
    }
}

/// Room inside a container (byte 7; 0 means 32).
pub fn capacity(gd: &GameData, it: &u8data::map::Item) -> u32 {
    match gd.typeflags.get(it.shape).volume {
        0 => 32,
        v => v as u32,
    }
}

/// Barrels and chests: the Avatar can carry one in its backpack (ScummVM: room 500 for them).
const HAULED: [u32; 3] = [BARREL, 78, 117];

/// Could `id` go into `into` (ScummVM's CanAddItem)? Never a person, nor a bag into itself or
/// something in it, and on the Avatar itself only what it wears and a pack. With `check` (scripts'
/// legalMoveToContainer) it must also fit by volume ([`has_room`], which the player's moves
/// check too) and, going into the Avatar's belongings, not load it past what `strength` carries.
pub fn can_add(
    gd: &GameData,
    world: &World,
    id: ObjId,
    into: ObjId,
    check: bool,
    strength: u8,
) -> Result<(), &'static str> {
    if world.container(id) == Some(into) {
        return Ok(());
    }
    if id < 256 || id == into || world.is_inside(into, id) {
        return Err("It won't go in there.");
    }
    if into == AVATAR && !slot_free(gd, world, AVATAR, id) {
        return Err("You can't wear that."); // (on the Avatar itself: one of each, and a pack)
    }
    if !check {
        return Ok(());
    }
    has_room(gd, world, id, into)?;
    if world.root(into) == AVATAR && world.root(id) != AVATAR {
        let carried: u32 = world.contents(AVATAR).into_iter().map(|k| weight(gd, world, k)).sum();
        if carried + weight(gd, world, id) > max_weight(strength) {
            return Err("That's too heavy.");
        }
    }
    Ok(())
}

/// Is there room in container `into` for `id` by volume (ScummVM's CanAddItem; DOS refuses the
/// player the same: a bedroll won't go into the castle save's backpack, 188 of 175 full with a
/// bag inside, until that bag comes out)? People hold things by their slots, not volume.
pub fn has_room(gd: &GameData, world: &World, id: ObjId, into: ObjId) -> Result<(), &'static str> {
    if into < 256 || world.container(id) == Some(into) {
        return Ok(());
    }
    let (Some(it), Some(c)) = (world.get(id), world.get(into)) else { return Err("It's gone.") };
    let pack = world.contents(AVATAR).into_iter().find(|&k| holds(gd, world, k));
    let room = if HAULED.contains(&it.shape) && pack == Some(into) { 500 } else { capacity(gd, &c) };
    if used_room(gd, world, into) + volume(gd, &it) > room {
        return Err("There's no room for that.");
    }
    Ok(())
}

/// How much of container `c`'s room its contents take.
pub fn used_room(gd: &GameData, world: &World, c: ObjId) -> u32 {
    world.contents(c).into_iter().filter_map(|k| world.get(k)).map(|k| volume(gd, &k)).sum()
}

/// The backpack (the one bag a person wears).
pub const BACKPACK: u32 = 529;

/// Could `who` wear `id` (ScummVM's setEquip / MainActor::CanAddItem)? Something worn or a
/// backpack, with nothing else of its kind on already.
pub fn slot_free(gd: &GameData, world: &World, who: ObjId, id: ObjId) -> bool {
    let Some(it) = world.get(id) else { return false };
    let kind = |shape: u32| (gd.typeflags.get(shape).equip, shape == BACKPACK);
    let (equip, pack) = kind(it.shape);
    if equip == 0 && !pack {
        return false;
    }
    let others = world.contents(who).into_iter().filter(|&o| o != id).filter_map(|o| world.get(o));
    others.map(|o| kind(o.shape)).all(|(e, p)| e != equip && !(p && pack))
}

/// Does `id` hold things (a bag, a chest, a person)?
pub fn holds(gd: &GameData, world: &World, id: ObjId) -> bool {
    world.get(id).is_some_and(|it| gd.typeflags.get(it.shape).family == family::CONTAINER)
}

/// A worn backpack's position, as the original saves it (the paperdoll doesn't show it: the
/// Avatar's figure has one drawn on).
pub const BACKPACK_SPOT: (i32, i32) = (40, 63);

/// Where and how the paperdoll shows a worn thing: its slot's spot (whatever position was
/// saved) and its worn look, the frame after its ground frame (armour comes in such pairs).
pub fn paperdoll_look(gd: &GameData, it: &u8data::map::Item) -> Option<((i32, i32), u32)> {
    let slot = Slot::of(gd, it.shape).filter(|_| it.flags & item_flag::EQUIPPED != 0)?;
    let worn = gd.shapes.frame(it.shape, it.frame + 1).map_or(it.frame, |_| it.frame + 1);
    Some((slot.spot(), worn))
}

/// A weapon's numbers, from U8.EXE's table when the install has it, else rough stand-ins by
/// how it's held (no bonuses or magic).
pub fn weapon(gd: &GameData, shape: u32) -> Option<Weapon> {
    if !gd.weapons.list.is_empty() {
        return gd.weapons.get(shape).copied();
    }
    let (blade, blunt) = (dmg::BLADE as u8, dmg::BLUNT as u8);
    let (hold, modifier, base, damage_type) = match shape {
        417 | 540 | 815 | 816 => (0, 9, 4, blunt), // maces, hammer, Bone Crusher, Slayer
        420 | 541 | 542 | 817..=819 | 821 => (1, 12, 5, blade), // swords
        419 | 822 => (2, 14, 6, blade | blunt),    // axes
        418 | 820 => (3, 7, 3, blade),             // daggers
        _ => return None,
    };
    Some(Weapon { shape, hold, modifier, base, dex_attack: 0, dex_defend: 0, armour: 0, damage_type })
}

/// How a wielded weapon is drawn over the Avatar: (overlay shape, WPNOVLAY.DAT hold type). The
/// overlay shapes are named after the weapons (MACE 241, SWORD 243, AXE 244, DAGGER 245), in
/// hold-type order; U8.EXE lists them before its weapon table.
pub fn weapon_overlay(gd: &GameData, shape: u32) -> Option<(u32, usize)> {
    const SHAPES: [u32; 4] = [241, 243, 244, 245];
    let w = weapon(gd, shape)?;
    let overlay = gd.weapons.overlay_shapes.get(w.hold).or(SHAPES.get(w.hold))?;
    Some((*overlay, w.hold))
}

/// What a piece of armour adds to armour class, by shape and frame (ground/worn frames come in
/// pairs, one pair per kind). The values are ScummVM's table (u8armour.ini, reverse-engineered
/// from the original; U8.EXE works them out in code). Checked in the original: plate leggings 3
/// (Dino's guide has leggings 1/2, and plate gauntlets 3).
pub fn armour_value(shape: u32, frame: u32) -> i32 {
    let kind = frame as usize / 2;
    let by_kind: &[i32] = match shape {
        523 => &[4, 3, 2],       // breast plate, chain hauberk, cuirbolli
        530 => &[2, 2, 1, 3],    // arm guards: two kinds at 2, leather 1, plate gauntlets 3
        531 => &[4, 2, 2, 3, 3], // great helm, chain coif, open-faced, crested (two looks)
        532 => &[2, 3],          // leather, plate leggings
        539 => &[4, 3],          // large, small shield
        823 => &[1],             // clothes (any)
        841 => &[8],             // magic armour (5 before the 2.12 patch; this is 2.13)
        64 => &[6],              // magic silver armour
        842 | 843 => &[5],       // magic (Daemon) shield, magic helm
        844 | 845 => &[4],       // magic arm guards, magic leggings
        _ => return 0,
    };
    by_kind.get(kind).or(by_kind.last()).copied().unwrap_or(0)
}

/// What legwear adds to a kick (ScummVM's armour table: the magic leggings).
pub fn kick_bonus(shape: u32) -> i32 {
    if shape == 845 {
        3
    } else {
        0
    }
}

/// What a piece of armour defends against (damage type flags): the magic shield, fire.
pub fn armour_defence(shape: u32) -> u16 {
    match shape {
        842 => dmg::FIRE,
        _ => 0,
    }
}

/// Armour class: what `who` wears adds up, with its weapon's armour bonus. (The new game's
/// unseen armour doesn't count: the original shows ARMR 0 then.)
pub fn armour_class(gd: &GameData, world: &World, who: ObjId) -> i32 {
    world
        .contents(who)
        .into_iter()
        .filter_map(|id| world.get(id))
        .filter(|it| it.flags & item_flag::EQUIPPED != 0 && it.shape != UNSEEN_ARMOUR)
        .map(|it| match Slot::of(gd, it.shape) {
            Some(Slot::Weapon) => weapon(gd, it.shape).map_or(0, |w| w.armour as i32),
            Some(_) => armour_value(it.shape, it.frame),
            None => 0,
        })
        .sum()
}
