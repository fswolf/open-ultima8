//! Monsters from eggs. A monster egg (family 7) holds a creature's shape in its quality (low 11
//! bits; the top 5 a chance, which nothing reads) and an id and activity in its map byte (id
//! above bit 3, activity below). Scripts hatch them: a usecode egg (MONEGG) or a fast-area
//! marker (MONFAST) runs over the monster eggs about and calls `hatch` on those with its id.
//!
//! A hatched creature takes a free NPC slot, the way the original's saves show: numbers are
//! handed out lowest first, and a creature is put away again (into the void, marked ETHEREAL,
//! its slot free) when it leaves the fast area or the Avatar leaves the map. Its hit points and
//! dexterity are rolled from its kind's ranges ([`monsters`]); strength and intelligence 0.
use crate::game::Game;
use crate::monsters;
use crate::stats::Npc;
use crate::world::{item_flag, ObjId, World, AVATAR};
use u8data::{anim::action, map::Item};

/// Put away: in the void, and marked so in saves.
const PARKED: u16 = item_flag::ETHEREAL | item_flag::FAST_ONLY;
/// Monster egg quality: the creature's shape.
const SHAPE_MASK: u16 = 0x7FF;
/// Monster egg map byte: activity (0 loiter, 1 fight, 2 stand).
const ACTIVITY_MASK: u8 = 0x07;

fn parked(it: &Item) -> bool {
    it.flags & PARKED == PARKED
}

impl Game {
    /// Hatch monster egg `e`: its creature appears there, if there's room and a free slot.
    pub fn hatch_monster(&mut self, e: Item) -> Option<ObjId> {
        let shape = (e.quality & SHAPE_MASK) as u32;
        if shape == 0 || !self.world.clear_at(&self.data, shape, (e.x, e.y, e.z)) {
            return None;
        }
        let id = self.free_npc()?;
        for w in self.visited.values_mut() {
            Self::clear_slot(w, id);
        }
        Self::clear_slot(&mut self.world, id);
        let dir = self.world.roll(8) as usize;
        let frame = crate::avatar::anim_frame(&self.data, shape, action::STAND, dir, 0).map_or(0, |f| f.0);
        let flags = item_flag::FAST_ONLY | item_flag::DISPOSABLE | item_flag::IN_NPC_LIST;
        let map = self.map() as u8;
        let (x, y, z) = (e.x, e.y, e.z);
        let it = Item { x, y, z, shape, frame, flags, npc: id, npcnum: id as u8, mapnum: map, ..Item::default() };
        self.world.put_actor(&self.data, it);
        self.world.set_void(id, false);
        let kind = monsters::get(shape);
        let roll = |w: &mut World, (lo, hi): (u16, u16)| lo + w.roll(hi.saturating_sub(lo) as u32 + 1) as u16;
        let hp = kind.map_or(1, |m| roll(&mut self.world, m.hp));
        let dex = kind.map_or(10, |m| roll(&mut self.world, (m.dex.0 as u16, m.dex.1 as u16)) as u8);
        let align = kind.map_or(0, |m| m.alignment);
        if let Some(s) = self.stats.get_mut(id) {
            *s = Npc { hp: hp as i16, dex, alignment: align & 0xF, enemies: align >> 4, ..Npc::default() };
        }
        match e.mapnum & ACTIVITY_MASK {
            0 => self.start_loitering(id),
            1 => self.world.set_in_combat(id, true),
            _ => {} // stand
        }
        Some(id)
    }

    /// The lowest NPC number nobody holds: never used, or a creature put away.
    fn free_npc(&self) -> Option<ObjId> {
        self.free_npcs().first().copied()
    }

    /// A script made actor `id` (createActor): whatever held its slot on other maps is gone.
    pub(crate) fn made_actor(&mut self, id: ObjId) {
        for w in self.visited.values_mut() {
            Self::clear_slot(w, id);
        }
    }

    /// NPC numbers nobody holds, lowest first.
    pub(crate) fn free_npcs(&self) -> Vec<ObjId> {
        let worlds: Vec<&World> = std::iter::once(&self.world).chain(self.visited.values()).collect();
        let loaded = |m: usize| worlds.iter().any(|w| w.map() as usize == m);
        (AVATAR + 1..255) // (255 is the original's free-list tail: never handed out)
            .filter(|&n| {
                let mut here = worlds.iter().filter_map(|w| w.index(n).map(|i| &w.entry(i).it)).peekable();
                if here.peek().is_some() {
                    return here.all(parked);
                }
                // Not in any map we hold: as the save has it (a map we hold without it: gone).
                match self.data.itemcach.as_deref().and_then(|d| u8data::map::actor_row(d, n as usize)) {
                    None => true,
                    Some((m, it)) => parked(&it) || (loaded(m) && (it.x, it.y) != (0, 0)),
                }
            })
            .collect()
    }

    /// Clear whatever held NPC slot `id` in `w` (a creature put away), with its belongings.
    fn clear_slot(w: &mut World, id: ObjId) {
        if w.index(id).is_some_and(|i| !w.entry(i).gone) {
            w.take_contents(id);
            w.remove(id);
        }
    }

    /// Put a hatched creature away: off the map, its slot free (it stays in the save as the
    /// original leaves them).
    pub(crate) fn put_away(&mut self, id: ObjId) {
        let Some(mut it) = self.world.get(id) else { return };
        it.flags |= item_flag::ETHEREAL;
        self.world.set(&self.data, id, it);
        self.world.set_void(id, true);
        self.world.set_in_combat(id, false);
        self.fights.remove(&id);
        self.loiters.remove(&id);
        self.rising.retain(|&(r, _)| r != id);
        self.stop_walking(id);
    }

    /// Is `id` something thrown away when it leaves the fast area (a hatched creature)?
    pub(crate) fn fast_only(&self, id: ObjId) -> bool {
        id != AVATAR && self.world.get(id).is_some_and(|it| it.flags & item_flag::FAST_ONLY != 0 && it.npc != 0)
    }

    /// Leaving the map: its hatched creatures go.
    pub(crate) fn put_away_all(&mut self) {
        let gone: Vec<ObjId> = (AVATAR + 1..256).filter(|&id| self.fast_only(id) && !self.world.is_void(id)).collect();
        for id in gone {
            self.put_away(id);
        }
    }
}

/// Loitering (activity 0): wander to a spot up to this many 32-unit steps away, then pause
/// 4–7 seconds (ScummVM's LoiterProcess).
const LOITER_STEPS: i32 = 10;
const LOITER_PAUSE: (u32, u32) = (4 * 30, 7 * 30);

/// Someone loitering: the walk under way, or ticks till the next.
#[derive(Default)]
pub(crate) struct Loiter {
    walk: Option<crate::npc::Walk>,
    wait: u32,
}

impl Game {
    /// Start `id` loitering (activity 0): wandering about where it is.
    pub(crate) fn start_loitering(&mut self, id: ObjId) {
        self.loiters.insert(id, Loiter::default());
    }

    /// One tick of everyone loitering near the Avatar.
    pub(crate) fn loiter(&mut self) {
        let Some(av) = self.world.get(AVATAR) else { return };
        let ids: Vec<ObjId> = self.loiters.keys().copied().collect();
        for id in ids {
            let still = self.world.get(id).filter(|_| !self.world.is_void(id) && !self.world.in_combat(id));
            let Some(it) = still.filter(|_| !self.stats.get(id).is_some_and(|s| s.dead)) else {
                self.loiters.remove(&id);
                continue;
            };
            let near = (it.x - av.x).abs().max((it.y - av.y).abs()) <= crate::fast_area::RANGE;
            if !near || self.world.is_acting(id) {
                continue;
            }
            let Some(mut l) = self.loiters.remove(&id) else { continue };
            if let Some(w) = &mut l.walk {
                if w.step(&self.data, &mut self.world).is_some() {
                    l.walk = None;
                    l.wait = LOITER_PAUSE.0 + self.world.roll(LOITER_PAUSE.1 - LOITER_PAUSE.0 + 1);
                }
            } else if l.wait > 0 {
                l.wait -= 1;
            } else {
                let mut off = || (self.world.roll(2 * LOITER_STEPS as u32 + 1) as i32 - LOITER_STEPS) * 32;
                let to = (it.x + off(), it.y + off(), it.z);
                l.walk = Some(crate::npc::Walk::new(0, id, to, 0));
            }
            self.loiters.insert(id, l);
        }
    }
}

/// The window a dead body opens in (its belongings, for searching).
pub const BODY_GUMP: u16 = 12;
/// Where loot lands in a body's window: anywhere in this box (window position).
const LOOT_AREA: (i32, i32) = (60, 40);
/// Those who rise again do so after 5–30 seconds.
const RISE_AFTER: (u32, u32) = (5 * 30, 30 * 30);
/// Sorcerers' foci (wand, rod, symbol, talisman: frames for charged and spent).
const FOCUS: u32 = 397;

impl Game {
    /// A creature of a kind with loot or that rises again has died (`kind`: what killed it).
    pub(crate) fn monster_died(&mut self, id: ObjId, kind: u16) {
        let Some(m) = self.monster(id) else { return };
        if self.fast_only(id) {
            self.world.take_contents(id); // what a hatched creature picked up goes with it
        }
        for loot in m.loot {
            self.give_loot(id, loot);
        }
        if m.resurrects && kind & crate::combat::dmg::FIRE == 0 {
            let wait = RISE_AFTER.0 + self.world.roll(RISE_AFTER.1 - RISE_AFTER.0 + 1);
            self.rising.push((id, wait));
        }
    }

    fn give_loot(&mut self, id: ObjId, loot: &monsters::Loot) {
        use monsters::Loot;
        match *loot {
            Loot::Items { shape, frames, permille, count, map } => {
                if map != 0 && map as u16 != self.map() || self.world.roll(1000) >= permille as u32 {
                    return;
                }
                let n = count.0 + self.world.roll((count.1 - count.0) as u32 + 1) as u16;
                if self.data.typeflags.get(shape).family == u8data::typeflag::family::QUANTITY {
                    let pile = self.loot_item(id, shape, 0, n);
                    self.trigger(pile, crate::usecode::event::COMBINE); // its frame shows the amount
                } else {
                    for _ in 0..n {
                        let f = frames[self.world.roll(frames.len() as u32) as usize];
                        self.loot_item(id, shape, f, 0);
                    }
                }
            }
            Loot::Weapon => {
                let w = monsters::WEAPON_LOOT.iter().copied().find(|&(_, chance)| self.world.roll(100) < chance);
                if let Some((shape, _)) = w {
                    self.loot_item(id, shape, 0, 1);
                }
            }
            Loot::Focus => {
                // (chance in 10, frames charged/spent, chance in 10 of charges, charges, spells)
                for (odds, (charged, spent), live, charges, spells) in [
                    (8, (0, 15), 4, (3, 6), (1, 4)),
                    (6, (3, 16), 2, (3, 6), (1, 7)),
                    (5, (12, 19), 5, (1, 1), (1, 11)),
                    (2, (9, 18), 2, (1, 2), (10, 11)),
                ] {
                    if self.world.roll(10) >= odds {
                        continue;
                    }
                    let (frame, quality) = if self.world.roll(10) < live {
                        let spell = (spells.0 + self.world.roll(spells.1 - spells.0 + 1)) as u16;
                        // A symbol holds one charge, but several of ignite or extinguish.
                        let n = if charged == 12 && spell < 4 { (3, 6) } else { charges };
                        let c = (n.0 + self.world.roll(n.1 - n.0 + 1)) as u16;
                        (charged, spell << 8 | c)
                    } else {
                        (spent, 0)
                    };
                    self.loot_item(id, FOCUS, frame, quality);
                }
            }
        }
    }

    /// Put a new thing in `id`'s belongings (thrown away with it, if it goes).
    fn loot_item(&mut self, id: ObjId, shape: u32, frame: u32, quality: u16) -> ObjId {
        let it = Item { shape, frame, quality, flags: item_flag::DISPOSABLE, ..Item::default() };
        let thing = self.world.add(&self.data, it, false);
        let at = (self.world.roll(LOOT_AREA.0 as u32) as i32, self.world.roll(LOOT_AREA.1 as u32) as i32);
        self.world.move_inside(&self.data, thing, id, at);
        thing
    }

    /// One tick of the dead who rise again: up, fresh, and fighting.
    pub(crate) fn rise(&mut self) {
        let mut up = vec![];
        self.rising.retain_mut(|(id, wait)| {
            *wait = wait.saturating_sub(1);
            if *wait == 0 {
                up.push(*id);
            }
            *wait > 0
        });
        for id in up {
            let dead = self.stats.get(id).is_some_and(|s| s.dead);
            let Some(it) = self.world.get(id).filter(|_| dead && !self.world.is_void(id)) else { continue };
            if it.flags & item_flag::GUMP_OPEN != 0 {
                self.close_window(id); // someone was going through its belongings
            }
            let roll = |w: &mut World, (lo, hi): (u16, u16)| lo + w.roll(hi.saturating_sub(lo) as u32 + 1) as u16;
            if let Some(m) = monsters::get(it.shape) {
                let (hp, dex) = (roll(&mut self.world, m.hp), roll(&mut self.world, (m.dex.0 as u16, m.dex.1 as u16)));
                if let Some(s) = self.stats.get_mut(id) {
                    (s.hp, s.dex, s.dead, s.withstand_death) = (hp as i16, dex as u8, false, false);
                    (s.alignment, s.enemies) = (m.alignment & 0xF, m.alignment >> 4);
                }
            }
            self.world.set_in_combat(id, true);
            let dir = crate::avatar::facing(&self.data, &it).unwrap_or(0);
            self.world.play(&self.data, id, action::STAND_UP, dir);
        }
    }

    /// Can the player use `id` (a double click)? People always (to talk, or search the dead);
    /// anything else within reach, as the original (ScummVM's GameMapGump: canReach, else the
    /// red cross).
    pub fn can_use(&self, id: ObjId) -> bool {
        let person = id < 256 || self.world.get(id).is_some_and(|it| it.npc != 0);
        person || self.within_reach(id)
    }

    /// Double-click on `id`: a dead body opens (or closes) for searching; anything else is used.
    pub fn use_item(&mut self, id: ObjId) -> bool {
        let dead = id != AVATAR
            && self.world.get(id).is_some_and(|it| it.npc != 0)
            && self.stats.get(id).is_some_and(|s| s.dead);
        if !dead {
            return self.trigger(id, crate::usecode::event::USE);
        }
        let Some(mut it) = self.world.get(id) else { return false };
        if it.flags & item_flag::GUMP_OPEN != 0 {
            self.close_window(id);
        } else {
            it.flags |= item_flag::GUMP_OPEN;
            self.world.set(&self.data, id, it);
            if let Some(s) = &mut self.scripts {
                s.vm.events.push_back(crate::usecode::UiEvent::OpenGump { item: id, gump: BODY_GUMP });
            }
        }
        true
    }

    /// Close `id`'s window (and tell the host).
    fn close_window(&mut self, id: ObjId) {
        if let Some(mut it) = self.world.get(id) {
            it.flags &= !item_flag::GUMP_OPEN;
            self.world.set(&self.data, id, it);
        }
        if let Some(s) = &mut self.scripts {
            s.vm.events.push_back(crate::usecode::UiEvent::CloseGump { item: id });
        }
    }
}
