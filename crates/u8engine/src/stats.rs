//! People's numbers from NPCDATA.DAT — strength, dexterity, intelligence, hit points, mana — and
//! whether they're alive, as play changes them. Saving writes them back into NPCDATA.
use crate::world::ObjId;
use u8data::map::actor_flag;

/// After faking death and getting up, ticks the faker is still taken for dead (30 seconds).
pub const FEIGN_TICKS: u16 = 900;

/// NPCs are numbered 1–255 (0 unused); their records follow in NPCDATA.
const NPCS: usize = 256;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Npc {
    pub str: u8,
    pub dex: u8,
    pub int: u8,
    pub hp: i16,
    pub mana: i16,
    pub dead: bool,
    /// Hurt, but never killed (key characters from the start; Actor::setImmortal).
    pub immortal: bool,
    /// Not even hurt.
    pub invincible: bool,
    /// Walks on air (a spell).
    pub airwalk: bool,
    /// In combat when saved (the world keeps it while playing).
    pub in_combat: bool,
    /// Survives one deathblow, healed instead (Actor::setWithstandDeath).
    pub withstand_death: bool,
    /// Faking death (Actor::setFeignDeath): ticks till it's over. It falls, gets up when
    /// [`FEIGN_TICKS`] are left, and is taken for dead by the undead till 0.
    pub feigning: u16,
    /// Stunned by a kick (ITEM.C UnstunNpc): ticks till it can act again. Meanwhile every blow
    /// lands and armour does half.
    pub stunned: u16,
    /// Which sides this person is on, and whose enemy (4 bits each: NPCDATA byte 0x0B's low and
    /// high nibbles). The Avatar is 2; townsfolk 1, enemies 2; monsters 4 or 8, enemies 2 and more.
    pub alignment: u8,
    pub enemies: u8,
    /// NPCDATA's flag bytes as loaded (bits we don't model are kept as they were).
    pub flags: [u8; 3],
}

impl Npc {
    pub fn max_hp(&self) -> i16 {
        2 * self.str as i16
    }
    pub fn max_mana(&self) -> i16 {
        2 * self.int as i16
    }
    /// Would this person fight `other` (given the chance)?
    pub fn is_enemy(&self, other: &Npc) -> bool {
        self.enemies & other.alignment != 0
    }
}

#[derive(Clone, Debug)]
pub struct Stats {
    npcs: Vec<Npc>,
}

impl Stats {
    /// From NPCDATA.DAT entry 0 (everyone at 0 without it).
    pub fn load(npcdata: Option<&[u8]>) -> Self {
        let npcs = (0..NPCS)
            .map(|n| {
                let Some(s) = npcdata.and_then(|d| u8data::map::npc_stats(d, n)) else { return Npc::default() };
                let has = |(k, bit): (usize, u8)| s.flags[k] & bit != 0;
                Npc {
                    str: s.strength,
                    dex: s.dexterity,
                    int: s.intelligence,
                    hp: s.hit_points as i16,
                    mana: s.mana as i16,
                    dead: has(actor_flag::DEAD),
                    immortal: has(actor_flag::IMMORTAL),
                    invincible: has(actor_flag::INVINCIBLE),
                    airwalk: has(actor_flag::AIRWALK),
                    in_combat: has(actor_flag::IN_COMBAT),
                    withstand_death: has(actor_flag::WITHSTAND_DEATH),
                    feigning: if has(actor_flag::FEIGN_DEATH) { FEIGN_TICKS } else { 0 },
                    stunned: 0,
                    alignment: s.alignment & 0xF,
                    enemies: s.alignment >> 4,
                    flags: s.flags,
                }
            })
            .collect();
        Self { npcs }
    }

    pub fn get(&self, id: ObjId) -> Option<Npc> {
        self.npcs.get(id as usize).copied()
    }

    pub fn get_mut(&mut self, id: ObjId) -> Option<&mut Npc> {
        self.npcs.get_mut(id as usize)
    }

    /// Write the numbers and flags back into NPCDATA.DAT entry 0.
    pub fn store(&self, npcdata: &mut [u8]) {
        for (n, s) in self.npcs.iter().enumerate().skip(1) {
            let mut flags = s.flags;
            let mut put =
                |(k, bit): (usize, u8), on: bool| flags[k] = if on { flags[k] | bit } else { flags[k] & !bit };
            put(actor_flag::DEAD, s.dead);
            put(actor_flag::IMMORTAL, s.immortal);
            put(actor_flag::INVINCIBLE, s.invincible);
            put(actor_flag::AIRWALK, s.airwalk);
            put(actor_flag::IN_COMBAT, s.in_combat);
            put(actor_flag::WITHSTAND_DEATH, s.withstand_death);
            put(actor_flag::FEIGN_DEATH, s.feigning > 0);
            u8data::map::store_npc_stats(
                npcdata,
                n,
                u8data::map::Stats {
                    strength: s.str,
                    dexterity: s.dex,
                    intelligence: s.int,
                    hit_points: s.hp.clamp(0, 255) as u8,
                    mana: s.mana.clamp(0, i16::MAX) as u16,
                    alignment: (s.alignment & 0xF) | s.enemies << 4,
                    flags,
                },
            );
        }
    }
}
