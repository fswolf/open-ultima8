//! The VM's view of the game. Scripts only touch the world through [`Host`], so the VM can be
//! tested with a stub and the world can change shape without touching the interpreter.
use crate::avatar;
use crate::stats::{Npc, Stats};
use crate::world::{ObjId, World};
use u8data::{map::Item, GameData};

pub trait Host {
    fn item(&self, id: ObjId) -> Option<Item>;
    fn set_item(&mut self, id: ObjId, it: Item);
    fn destroy(&mut self, id: ObjId);
    /// Add a new item (placed on the map if `on_map`); returns its id.
    fn create(&mut self, it: Item, on_map: bool) -> ObjId;
    /// Raw 8-byte TYPEFLAG entry for a shape.
    fn typeflag(&self, shape: u32) -> [u8; 8];
    fn frame_count(&self, shape: u32) -> u32;
    /// Items whose footprint lies within `range` world units of (x, y).
    fn area(&self, x: i32, y: i32, range: i32) -> Vec<ObjId>;
    /// `loop`'s area search (ScummVM's areaSearch): anchors within `range` of `at`, in map-chunk
    /// order; with `recurse`, what's inside each straight after it.
    fn search_area(&self, at: (i32, i32), range: i32, _recurse: bool) -> Vec<ObjId> {
        self.area(at.0, at.1, range)
    }
    /// `loop`'s surface search: what sits on `id` (`above`) or holds it up (`below`).
    fn search_surface(&self, _id: ObjId, _above: bool, _below: bool) -> Vec<ObjId> {
        vec![]
    }
    /// Item::ascend: rise or sink by `dz` with what rests on it; (all the way, what to drop).
    fn ascend(&mut self, _id: ObjId, _dz: i32) -> (bool, Vec<ObjId>) {
        (false, vec![])
    }
    /// getSurfaceWeight: what everything resting on `id` weighs (tenths).
    fn surface_weight(&self, _id: ObjId) -> u32 {
        0
    }
    /// What rests on `id`, all the way up (grab makes it fall).
    fn grab(&mut self, _id: ObjId) -> Vec<ObjId> {
        vec![]
    }
    /// Items in a container (or carried by a person).
    fn contents(&self, _container: ObjId) -> Vec<ObjId> {
        vec![]
    }
    /// The container an item is in.
    fn container(&self, _id: ObjId) -> Option<ObjId> {
        None
    }
    fn map_num(&self) -> u16;
    /// Is `id` in this map's world (on it, or in its void)? People elsewhere are still known to
    /// [`item`](Self::item), as the original keeps everyone in one world.
    fn here(&self, _id: ObjId) -> bool {
        true
    }
    /// Move an item into (true) or back out of (false) the ethereal void (temporarily off the map).
    fn set_ethereal(&mut self, id: ObjId, on: bool);
    /// Facing (0–7) of an actor.
    fn dir(&self, _id: ObjId) -> u8 {
        4
    }
    /// Frame, mirror flag and length of an actor animation step, from ANIM.DAT.
    fn anim_frame(&self, _shape: u32, _action: usize, _dir: usize, _step: usize) -> Option<(u32, bool, usize)> {
        None
    }
    /// Is an actor in combat stance?
    fn in_combat(&self, _id: ObjId) -> bool {
        false
    }
    fn set_in_combat(&mut self, _id: ObjId, _on: bool) {}
    /// Whom someone in combat is after (0: nobody yet).
    fn target(&self, _id: ObjId) -> ObjId {
        0
    }
    /// Send someone (into combat, if need be) after `t`.
    fn set_target(&mut self, _id: ObjId, _t: ObjId) {}
    /// Is anyone else in combat (alive, not playing dead) within `range` of `id`?
    fn fighting_near(&self, _id: ObjId, _range: i32) -> bool {
        false
    }
    /// Is an actor in the middle of an animation?
    fn busy(&self, _id: ObjId) -> bool {
        false
    }
    /// Push into the void, on the ethereal stack.
    fn push_ethereal(&mut self, id: ObjId) {
        self.set_ethereal(id, true);
    }
    /// The top of the ethereal stack.
    fn ethereal_top(&mut self) -> Option<ObjId> {
        None
    }
    /// Take the top of the ethereal stack out of the void: back where it was, to a point, or
    /// into a container (onto the map at a thing that isn't one). Its id, if there was one.
    fn pop_ethereal(&mut self, _to: Pop) -> Option<ObjId> {
        None
    }
    /// A new actor of `shape` in NPC slot `id`, in the void on the ethereal stack, with its kind's
    /// numbers (Actor::createActor). False if the slot is taken here after all.
    fn create_actor(&mut self, _id: ObjId, _shape: u32, _frame: u32) -> bool {
        false
    }
    /// Could something of `shape` stand at (x, y, z) (nothing solid in the way)?
    fn can_exist_at(&self, _shape: u32, _at: (i32, i32, i32)) -> bool {
        true
    }
    /// The original's canExistAt: room for `shape` at (x, y, z), `mover` left out, and (by
    /// `flags`, see [`exist`](crate::world::exist)) standing on something it may stand on.
    fn exists_at(&self, shape: u32, at: (i32, i32, i32), _flags: u8, _mover: Option<ObjId>) -> bool {
        self.can_exist_at(shape, at)
    }
    /// The ANIM.DAT action an actor last started.
    fn last_anim(&self, _id: ObjId) -> usize {
        2
    }
    /// A person's numbers (NPCDATA), if `id` is a person.
    fn stats(&self, _id: ObjId) -> Option<Npc> {
        None
    }
    fn set_stats(&mut self, _id: ObjId, _s: Npc) {}
    /// Weight in tenths with everything in it (the paperdoll's reckoning).
    fn weight(&self, _id: ObjId) -> u32 {
        0
    }
    /// Is NPC `id` dead, wherever it is (None: there's no such person)?
    fn npc_dead(&self, _id: ObjId) -> Option<bool> {
        None
    }
    /// legalMoveToPoint's move (ScummVM's collideMove): towards `to`, stopping short of anything
    /// solid. (Got there unblocked, what it touched before, what it touches now); None if it
    /// was blocked and `abort` kept it where it was.
    fn collide_move(&mut self, id: ObjId, to: (i32, i32, i32), abort: bool) -> Option<(bool, Vec<ObjId>, Vec<ObjId>)> {
        let _ = abort;
        self.move_to(id, to);
        Some((true, vec![], vec![]))
    }
    /// Into container (or person) `into` (ScummVM's moveToContainer); with `check`, only if it
    /// fits and the Avatar can carry it. Whether it went.
    fn move_into(&mut self, _id: ObjId, _into: ObjId, _check: bool) -> bool {
        false
    }
    /// ScummVM's canReach: within `range` (box to box) and in sight.
    fn can_reach(&self, id: ObjId, other: ObjId, range: i32) -> bool {
        match (self.item(id), self.item(other)) {
            (Some(a), Some(b)) => (a.x - b.x).abs().max((a.y - b.y).abs()) <= range,
            _ => false,
        }
    }
    /// ScummVM's getRange: the gap between the two boxes.
    fn range_between(&self, id: ObjId, other: ObjId) -> Option<i32> {
        let (a, b) = (self.item(id)?, self.item(other)?);
        Some((a.x - b.x).abs().max((a.y - b.y).abs()))
    }
    /// Item::enterFastArea from a script: None if it's in already; else it is now, and whether
    /// its event runs (not for the dead). Someone (not the Avatar) stops fighting and stands.
    fn enter_fast(&mut self, _id: ObjId) -> Option<bool> {
        Some(true)
    }
    /// Actor::setEquip: `who` wears `id` (if it's wearable and the slot is free).
    fn set_equip(&mut self, _who: ObjId, _id: ObjId) -> bool {
        false
    }
    /// Onto the map at (x, y, z), out of any container or the void (Item::move).
    fn move_to(&mut self, id: ObjId, (x, y, z): (i32, i32, i32)) {
        if let Some(mut it) = self.item(id) {
            (it.x, it.y, it.z) = (x, y, z);
            self.set_item(id, it);
        }
    }
    /// Play an actor animation; returns how many ticks it takes.
    fn play(&mut self, _id: ObjId, _action: usize, _dir: usize) -> Option<u32> {
        None
    }
}

/// Where the top of the ethereal stack goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pop {
    /// Back where it was.
    Back,
    At((i32, i32, i32)),
    /// Into this container (at a random spot in its window).
    Into(ObjId),
}

/// [`Host`] over a live [`World`].
pub struct WorldHost<'a> {
    pub world: &'a mut World,
    pub data: &'a GameData,
    pub stats: &'a mut Stats,
    /// The other maps' worlds (people there are still the scripts' to read and change).
    pub others: Option<&'a mut std::collections::HashMap<u16, World>>,
}

impl WorldHost<'_> {
    /// The other map where person `id` is (on it rather than in its void, if both).
    fn elsewhere(&self, id: ObjId) -> Option<u16> {
        let others = self.others.as_ref().filter(|_| (2..256).contains(&id))?;
        others.iter().filter(|(_, w)| w.get(id).is_some()).min_by_key(|(_, w)| w.is_void(id)).map(|(&m, _)| m)
    }

    /// Into container (or person) `c` — if it won't go, it stays in the void; onto the map where
    /// `c` is if it holds nothing; gone if `c` is (ScummVM's popToContainer).
    fn pop_into(&mut self, id: ObjId, c: ObjId) {
        match self.world.get(c) {
            Some(_) if c < 256 || crate::equip::holds(self.data, self.world, c) => _ = self.move_into(id, c, false),
            Some(it) => self.world.move_to_map(self.data, id, (it.x, it.y, it.z)),
            None => self.world.remove(id), // nowhere to go
        }
    }
}

impl Host for WorldHost<'_> {
    fn item(&self, id: ObjId) -> Option<Item> {
        self.world.get(id).or_else(|| match self.elsewhere(id) {
            Some(m) => self.others.as_ref()?.get(&m)?.get(id),
            // (on a map not built yet: as saved; not if they're gone from here or from where the
            // save had them)
            None if (2..256).contains(&id) => {
                let others = self.others.as_ref()?;
                let (m, it) = self.data.actor(id)?;
                (m as u16 != self.world.map() && !others.contains_key(&(m as u16))).then_some(it)
            }
            None => None,
        })
    }
    fn set_item(&mut self, id: ObjId, it: Item) {
        if self.world.get(id).is_some() || self.others.is_none() || !(2..256).contains(&id) {
            return self.world.set(self.data, id, it);
        }
        // Someone on another map: changed there (its world built first, if it hasn't been).
        let data = self.data;
        let here = self.world.map();
        let m = self.elsewhere(id).or_else(|| data.actor(id).map(|(m, _)| m as u16)).filter(|&m| m != here);
        if let (Some(m), Some(others)) = (m, self.others.as_mut()) {
            let w = others.entry(m).or_insert_with(|| World::build(data, m));
            if w.get(id).is_some() {
                w.set(data, id, it);
            }
        }
    }
    /// Gone for good — but never the Avatar, and what was inside spills into the container's own
    /// container or onto the map where it stood (ScummVM's Container::destroy).
    fn destroy(&mut self, id: ObjId) {
        if id != crate::world::AVATAR {
            self.world.destroy(self.data, id);
        }
    }
    fn weight(&self, id: ObjId) -> u32 {
        crate::equip::weight(self.data, self.world, id)
    }
    fn npc_dead(&self, id: ObjId) -> Option<bool> {
        (id < 256).then(|| self.stats.get(id).map(|s| s.dead)).flatten()
    }
    fn move_to(&mut self, id: ObjId, at: (i32, i32, i32)) {
        self.world.move_to_map(self.data, id, at);
    }
    fn move_into(&mut self, id: ObjId, into: ObjId, check: bool) -> bool {
        let holder = into < 256 || crate::equip::holds(self.data, self.world, into);
        let strength = self.stats.get(crate::world::AVATAR).map_or(15, |s| s.str);
        if !holder || crate::equip::can_add(self.data, self.world, id, into, check, strength).is_err() {
            return false;
        }
        self.world.put_into(self.data, id, into);
        true
    }
    fn can_reach(&self, id: ObjId, other: ObjId, range: i32) -> bool {
        self.world.can_reach(id, other, range)
    }
    fn range_between(&self, id: ObjId, other: ObjId) -> Option<i32> {
        self.world.range_between(id, other)
    }
    fn enter_fast(&mut self, id: ObjId) -> Option<bool> {
        let it = self.world.get(id).filter(|_| !self.world.is_fast(id))?;
        self.world.set_fast(id, true);
        if it.npc == 0 {
            return Some(true);
        }
        if self.stats.get(id).is_some_and(|s| s.dead) {
            return Some(false);
        }
        if id != crate::world::AVATAR {
            self.world.set_in_combat(id, false);
            let dir = crate::avatar::facing(self.data, &it).unwrap_or(0);
            self.world.play(self.data, id, u8data::anim::action::STAND, dir);
        }
        Some(true)
    }
    fn set_equip(&mut self, who: ObjId, id: ObjId) -> bool {
        use crate::equip::{self, Slot};
        let Some(it) = self.world.get(id) else { return false };
        if !equip::slot_free(self.data, self.world, who, id) || !self.move_into(id, who, false) {
            return false;
        }
        let spot = Slot::of(self.data, it.shape).map_or(equip::BACKPACK_SPOT, |s| s.spot());
        if let Some(mut it) = self.world.get(id) {
            (it.x, it.y, it.z) = (spot.0, spot.1, equip::worn_z(self.data, it.shape));
            it.flags |= crate::world::item_flag::EQUIPPED;
            self.world.set(self.data, id, it);
        }
        true
    }
    fn collide_move(&mut self, id: ObjId, to: (i32, i32, i32), abort: bool) -> Option<(bool, Vec<ObjId>, Vec<ObjId>)> {
        let w = &mut *self.world;
        let mut it = w.get(id)?;
        if w.container(id).is_some() || w.is_void(id) {
            // From a bag (or the void) only where it lands counts (ScummVM teleports it).
            w.move_to_map(self.data, id, to);
            it = w.get(id)?;
        }
        let before = w.contacts(id);
        let blocked = w.sweep(self.data, id, to).is_some();
        if blocked && abort {
            w.set(self.data, id, it);
            return None;
        }
        Some((!blocked, before, w.contacts(id)))
    }
    fn contents(&self, container: ObjId) -> Vec<ObjId> {
        self.world.contents(container)
    }
    fn container(&self, id: ObjId) -> Option<ObjId> {
        self.world.container(id)
    }
    fn create(&mut self, it: Item, on_map: bool) -> ObjId {
        let id = self.world.add(self.data, it, on_map);
        if !on_map {
            self.world.push_ethereal(id);
        }
        id
    }
    fn push_ethereal(&mut self, id: ObjId) {
        self.world.push_ethereal(id);
    }
    fn ethereal_top(&mut self) -> Option<ObjId> {
        self.world.ethereal_top()
    }
    fn pop_ethereal(&mut self, to: Pop) -> Option<ObjId> {
        let id = self.world.ethereal_top()?;
        match to {
            Pop::Back => self.world.set_void(id, false),
            Pop::At(at) => self.world.move_to_map(self.data, id, at),
            Pop::Into(c) => self.pop_into(id, c),
        }
        Some(id)
    }
    fn create_actor(&mut self, id: ObjId, shape: u32, frame: u32) -> bool {
        let Some(av) = self.world.get(crate::world::AVATAR) else { return false };
        let parked = |it: Item| it.flags & crate::world::item_flag::ETHEREAL != 0 && self.world.is_void(id);
        if self.world.get(id).is_some_and(|it| !parked(it)) {
            return false; // taken since the list was made
        }
        let flags = crate::world::item_flag::IN_NPC_LIST;
        let it = Item { shape, frame, flags, npc: id, npcnum: id as u8, mapnum: av.mapnum, ..Item::default() };
        self.world.put_actor(self.data, it);
        self.world.push_ethereal(id);
        let kind = crate::monsters::get(shape);
        let w = &mut *self.world;
        let mut roll = |(lo, hi): (u16, u16)| lo + w.roll(hi.saturating_sub(lo) as u32 + 1) as u16;
        let s = Npc {
            hp: kind.map_or(0, |m| roll(m.hp)) as i16,
            dex: kind.map_or(0, |m| roll((m.dex.0 as u16, m.dex.1 as u16))) as u8,
            alignment: kind.map_or(0, |m| m.alignment & 0xF),
            enemies: kind.map_or(0, |m| m.alignment >> 4),
            ..Npc::default()
        };
        if let Some(n) = self.stats.get_mut(id) {
            *n = s;
        }
        true
    }
    fn typeflag(&self, shape: u32) -> [u8; 8] {
        self.data.typeflags.raw(shape)
    }
    fn frame_count(&self, shape: u32) -> u32 {
        self.data.shapes.get(shape).len() as u32
    }
    fn area(&self, x: i32, y: i32, range: i32) -> Vec<ObjId> {
        self.world.area(x, y, range)
    }
    fn search_area(&self, at: (i32, i32), range: i32, recurse: bool) -> Vec<ObjId> {
        self.world.search_area(at, range, (0, 0), recurse)
    }
    fn search_surface(&self, id: ObjId, above: bool, below: bool) -> Vec<ObjId> {
        self.world.search_surface(id, above, below, false)
    }
    fn ascend(&mut self, id: ObjId, dz: i32) -> (bool, Vec<ObjId>) {
        self.world.ascend(self.data, id, dz)
    }
    fn surface_weight(&self, id: ObjId) -> u32 {
        let on = self.world.search_surface(id, true, false, true);
        on.into_iter().map(|o| crate::equip::weight(self.data, self.world, o)).sum()
    }
    fn grab(&mut self, id: ObjId) -> Vec<ObjId> {
        self.world.search_surface(id, true, false, true)
    }
    fn map_num(&self) -> u16 {
        self.world.map()
    }
    fn here(&self, id: ObjId) -> bool {
        self.world.get(id).is_some()
    }
    fn set_ethereal(&mut self, id: ObjId, on: bool) {
        self.world.set_void(id, on);
    }
    fn in_combat(&self, id: ObjId) -> bool {
        self.world.in_combat(id)
    }
    fn set_in_combat(&mut self, id: ObjId, on: bool) {
        self.world.set_in_combat(id, on);
    }
    fn target(&self, id: ObjId) -> ObjId {
        let t = self.world.target(id).now;
        if self.stats.get(t).is_some_and(|s| s.dead) {
            0
        } else {
            t
        }
    }
    fn set_target(&mut self, id: ObjId, t: ObjId) {
        self.world.set_target(id, t);
    }
    fn fighting_near(&self, id: ObjId, range: i32) -> bool {
        let Some(me) = self.world.get(id) else { return false };
        let up = |t: ObjId| self.stats.get(t).is_some_and(|s| !s.dead && s.feigning == 0);
        self.world.fighters().into_iter().any(|t| {
            t != id
                && up(t)
                && self.world.get(t).is_some_and(|it| (it.x - me.x).abs().max((it.y - me.y).abs()) <= range)
        })
    }
    fn busy(&self, id: ObjId) -> bool {
        self.world.is_acting(id)
    }
    fn last_anim(&self, id: ObjId) -> usize {
        self.world.last_anim(id)
    }
    fn can_exist_at(&self, shape: u32, at: (i32, i32, i32)) -> bool {
        self.world.clear_at(self.data, shape, at)
    }
    fn exists_at(&self, shape: u32, at: (i32, i32, i32), flags: u8, mover: Option<ObjId>) -> bool {
        self.world.can_exist_at(self.data, shape, at, flags, mover)
    }
    fn stats(&self, id: ObjId) -> Option<Npc> {
        self.stats.get(id).filter(|_| self.world.get(id).is_some_and(|it| it.npc != 0))
    }
    fn set_stats(&mut self, id: ObjId, s: Npc) {
        if let Some(n) = self.stats.get_mut(id) {
            *n = s;
        }
    }
    fn play(&mut self, id: ObjId, action: usize, dir: usize) -> Option<u32> {
        self.world.play(self.data, id, action, dir)
    }
    fn dir(&self, id: ObjId) -> u8 {
        self.world.facing(self.data, id).unwrap_or(4) as u8
    }
    fn anim_frame(&self, shape: u32, action: usize, dir: usize, step: usize) -> Option<(u32, bool, usize)> {
        let len = self.data.anims.get(shape, action)?.frames.get(dir)?.len();
        avatar::anim_frame(self.data, shape, action, dir, step).map(|(f, flip)| (f, flip, len))
    }
}
