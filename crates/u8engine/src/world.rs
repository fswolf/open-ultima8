//! The live map: every item with its world box, addressable by object id and filed in a world-space
//! grid for area queries and collision. Changes are recorded so views (the renderer) can resync
//! incrementally — see [`World::take_dirty`].
use crate::geom::Bounds;
use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use u8data::{
    map::{self, Item},
    typeflag::{family, flag},
    GameData,
};

mod actors;
mod collision;
mod contents;
pub use collision::exist;
pub use contents::POP_AREA;
mod scenery;
mod search;

use actors::Acting;
pub use actors::{anim_range, two_step, Overlay, Target};
use scenery::anim_info;

/// Object id: the NPC number for actors (1 = Avatar), 256+ for everything else.
pub type ObjId = u16;
pub const AVATAR: ObjId = 1;

/// Item status flags (ScummVM item.h numbering).
pub mod item_flag {
    /// The Avatar's own (it has carried it): taking it again isn't stealing.
    pub const OWNED: u16 = 0x0004;
    /// Thrown away on leaving the map (hatched monsters).
    pub const DISPOSABLE: u16 = 0x0002;
    pub const CONTAINED: u16 = 0x0008;
    pub const INVISIBLE: u16 = 0x0010;
    pub const FLIPPED: u16 = 0x0020;
    pub const IN_NPC_LIST: u16 = 0x0040;
    /// Thrown away on leaving the fast area (hatched monsters).
    pub const FAST_ONLY: u16 = 0x0080;
    /// Hangs in the air (doesn't fall).
    pub const HANGING: u16 = 0x1000;
    /// Its window is open (scripts toggle on it).
    pub const GUMP_OPEN: u16 = 0x0100;
    /// Worn or held by the person carrying it.
    pub const EQUIPPED: u16 = 0x0200;
    /// In the ethereal void. Saves mark people put away so (a hatched monster gone again: its
    /// NPC slot is free for the next).
    pub const ETHEREAL: u16 = 0x0800;
}

/// Grid cell size in world units.
const CELL: i32 = 256;
/// Highest ledge an actor steps up onto without jumping.
pub const STEP: i32 = 16;
/// Highest step up or down for someone walking (ScummVM's U8 position search: ±8), so people
/// take stairs but don't climb onto tables and walls or walk off ledges.
pub const WALK_STEP: i32 = 8;

/// What the world (and the painter's sort) needs to know about an item's shape.
#[derive(Clone, Copy, Debug, Default)]
pub struct Traits {
    pub flat: bool,
    pub land: bool,
    pub roof: bool,
    pub draw: bool,
    pub solid: bool,
    pub occl: bool,
    pub trans: bool,
    /// Cycles through frames on its own (scenery animation).
    pub anim: bool,
    /// Square footprint of at least a floor tile.
    pub bigsq: bool,
    /// Editor-only marker, never shown in game.
    pub editor: bool,
    /// An NPC (or the Avatar).
    pub actor: bool,
    /// TYPEFLAG family (see `u8data::typeflag::family`).
    pub family: u8,
    /// An egg: an invisible trigger (usecode, monster spawn or teleport).
    pub egg: bool,
    /// The shape has a non-empty frame to show.
    pub pixels: bool,
}

/// Where an item came from, which decides how it's saved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Origin {
    /// FIXED.DAT scenery: never saved (it can't change in the original either).
    Fixed,
    /// NONFIXED.DAT, or made by a script: saved with the map.
    #[default]
    Movable,
    /// An NPC: saved in the item cache.
    Actor,
}

/// An item on the map. `it.x/y/z` is the box's bottom front corner, `x0/y0` its back corner, `z1` its top.
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub it: Item,
    pub x0: i32,
    pub y0: i32,
    pub z1: i32,
    pub traits: Traits,
    /// Destroyed. Kept so indices stay stable; its id no longer resolves.
    pub gone: bool,
    /// In usecode's "ethereal void": still addressable, but not on the map (not drawn, not solid).
    pub void: bool,
    /// Eggs: already triggered while the Avatar has been in range.
    pub hatched: bool,
    pub origin: Origin,
    /// Inside this container (or carried by this person); never on the map itself.
    pub parent: Option<ObjId>,
    /// Its place in its map chunk's list (see [`search`](self::search)): scripts' searches find
    /// things in that order.
    pub(crate) seq: i32,
    /// In the fast area (ScummVM's FLG_FASTAREA): it has had its enterFastArea.
    pub fast: bool,
    /// What a glob egg expands to. The original only expands them in the part of the map it
    /// holds (round the Avatar), so elsewhere scripts' searches don't find them.
    pub glob: bool,
}

impl Entry {
    pub fn new(gd: &GameData, it: Item) -> Self {
        let si = gd.typeflags.get(it.shape);
        let anim = anim_info(gd, it.shape).is_some();
        let (dx, dy, dz) = si.footpad(it.flags & item_flag::FLIPPED != 0);
        Entry {
            it,
            x0: it.x - dx,
            y0: it.y - dy,
            z1: it.z + dz,
            traits: Traits {
                flat: dz == 0,
                land: si.has(flag::LAND),
                roof: si.has(flag::ROOF),
                draw: si.has(flag::DRAW),
                solid: si.has(flag::SOLID),
                occl: si.has(flag::OCCL),
                trans: si.has(flag::TRANSL),
                anim,
                bigsq: dx == dy && dx >= 128,
                editor: si.has(flag::EDITOR),
                actor: it.npc != 0,
                family: si.family,
                egg: family::is_egg(si.family),
                pixels: has_pixels(gd, &it, anim),
            },
            gone: false,
            void: false,
            hatched: false,
            origin: if it.npc != 0 { Origin::Actor } else { Origin::Movable },
            parent: None,
            seq: 0,
            fast: false,
            glob: false,
        }
    }

    pub fn bounds(&self) -> Bounds {
        Bounds { x0: self.x0, y0: self.y0, z0: self.it.z, x1: self.it.x, y1: self.it.y, z1: self.z1 }
    }
    /// Present on the map (not destroyed, not in the void).
    pub fn present(&self) -> bool {
        !self.gone && !self.void
    }
    /// Seen in game: present, and not an egg, editor marker or invisible item.
    pub fn shown(&self) -> bool {
        self.present()
            && self.traits.pixels
            && !self.traits.egg
            && !self.traits.editor
            && self.it.flags & item_flag::INVISIBLE == 0
    }
    /// An egg, or an editor-only marker that isn't solid (schedule and path markers): never in
    /// the way. Solid editor shapes are the original's invisible walls, lids and ceilings (497,
    /// 499, 704, 846…): undrawn, but they block as anything solid does (ScummVM makes no
    /// exception for them either). The flat ones (MIRROR 636, ENDPLATE 686) are trigger plates
    /// that need the touch.
    pub(crate) fn marker(&self) -> bool {
        self.traits.egg || (self.traits.editor && !self.traits.solid)
    }
    /// Something to stand on, if it's in the way underfoot: not people, but the creatures the
    /// original flags as land (skellots and spiders, 806 and 807, 8 high, which the clue book
    /// says "can be squished underfoot"): the Avatar steps onto one and the touch runs its
    /// gotHit, which finds it on top and squashes it. The insect (824, 16 high, not land) is too
    /// tall to step onto (the clue book doesn't list it); one landed on is squashed the same way.
    pub(crate) fn underfoot(&self) -> bool {
        !self.traits.actor || self.traits.land
    }
    /// Something you could stand on or bump into.
    fn physical(&self) -> bool {
        self.present() && !self.marker() && (self.traits.solid || self.traits.land)
    }
}

/// Does the item (any of its frames if animated) have a non-empty frame to show?
fn has_pixels(gd: &GameData, it: &Item, all_frames: bool) -> bool {
    let shape = gd.shapes.get(it.shape);
    if all_frames {
        shape.iter().any(|f| f.width > 0)
    } else {
        shape.get(it.frame as usize).is_some_and(|f| f.width > 0)
    }
}

fn cells(x0: i32, y0: i32, x1: i32, y1: i32) -> impl Iterator<Item = (i32, i32)> {
    let (a, b, c, d) = (x0.div_euclid(CELL), y0.div_euclid(CELL), x1.div_euclid(CELL), y1.div_euclid(CELL));
    (a..=c).flat_map(move |x| (b..=d).map(move |y| (x, y)))
}

fn footprint_cells(e: &Entry) -> impl Iterator<Item = (i32, i32)> {
    cells(e.x0, e.y0, e.it.x, e.it.y)
}

static SERIAL: AtomicU64 = AtomicU64::new(1);

pub struct World {
    map: u16,
    entries: Vec<Entry>,
    ids: Vec<ObjId>,
    by_id: HashMap<ObjId, usize>,
    next_id: ObjId,
    grid: HashMap<(i32, i32), Vec<usize>>,
    /// Entries whose shape animates by itself (indices; checked against the shape each time).
    anims: BTreeSet<usize>,
    eggs: Vec<usize>,
    rng: u32,
    dirty: Vec<usize>,
    marked: Vec<bool>,
    serial: u64,
    /// People in combat stance, and whom they're after.
    fighting: HashMap<ObjId, Target>,
    /// Weapons drawn over people (see [`Overlay`]).
    overlays: HashMap<ObjId, Overlay>,
    /// People playing an animation (scripts' doAnim, being hurt, dying).
    acting: Vec<Acting>,
    /// Someone drawn this far (world units) from where they are: the Avatar going there.
    lag: Option<(ObjId, (i32, i32, i32))>,
    /// Sounds of animation frames shown since last asked (the sound, who made it).
    sounds: Vec<(u16, ObjId, Option<bool>)>,
    /// The action each actor last started (walking, standing, an animation).
    last_anim: HashMap<ObjId, usize>,
    /// The way they faced for it.
    last_dir: HashMap<ObjId, usize>,
    /// Two-step actions: who's on their first step next (ScummVM's ACT_FIRSTSTEP).
    first_step: std::collections::HashSet<ObjId>,
    /// Usecode's ethereal stack: things pushed into the void (or made there) for scripts to pop
    /// out again, last first.
    ethereal: Vec<ObjId>,
    /// Next places at the front and the back of a chunk's list (see `Entry::seq`).
    seq_front: i32,
    seq_back: i32,
    /// How far the fast area reaches round the Avatar (regions in x + y and x - y; see
    /// [`fast_area`](crate::fast_area)), as of its last update.
    pub(crate) fast_reach: (i32, i32),
}

impl World {
    /// Everything on map `map`: fixed items, the saved game's movable items and NPCs.
    pub fn build(gd: &GameData, map: u16) -> Self {
        let l = gd.map_layers(map as usize);
        let mut nested: Vec<(usize, Vec<u8>)> = vec![];
        let mut entries: Vec<Entry> = vec![];
        // (FIXED.DAT's own records; past those, what's there came out of a glob egg.)
        let key = |it: &Item| (it.x, it.y, it.z, it.shape, it.frame);
        let mut records: HashMap<_, usize> = HashMap::new();
        for it in map::load_fixed(gd.fixed.get(map as usize).unwrap_or(&[]), &map::Globs(vec![]), &gd.typeflags) {
            *records.entry(key(&it)).or_default() += 1;
        }
        for (it, inside) in l.fixed {
            if !inside.is_empty() {
                nested.push((entries.len(), inside));
            }
            let glob = match records.get_mut(&key(&it)) {
                Some(n) if *n > 0 => {
                    *n -= 1;
                    false
                }
                _ => true,
            };
            entries.push(Entry { origin: Origin::Fixed, glob, ..Entry::new(gd, it) });
        }
        for (it, inside) in l.movable {
            if !inside.is_empty() {
                nested.push((entries.len(), inside));
            }
            entries.push(Entry::new(gd, it));
        }
        // People put away in the void stay there (the saves' parked monsters).
        let away = |it: &Item| it.flags & item_flag::ETHEREAL != 0;
        entries.extend(l.actors.into_iter().map(|it| Entry { void: away(&it), ..Entry::new(gd, it) }));
        let mut next_id: ObjId = 256;
        let ids: Vec<ObjId> = entries
            .iter()
            .map(|e| {
                if e.it.npc != 0 {
                    e.it.npc
                } else {
                    next_id += 1;
                    next_id - 1
                }
            })
            .collect();
        let mut w = World {
            map,
            by_id: ids.iter().enumerate().map(|(i, &id)| (id, i)).collect(),
            ids,
            next_id,
            grid: HashMap::new(),
            anims: BTreeSet::new(),
            eggs: vec![],
            rng: 0x1234_5678 ^ map as u32,
            dirty: vec![],
            marked: vec![false; entries.len()],
            serial: SERIAL.fetch_add(1, Ordering::Relaxed),
            entries,
            fighting: HashMap::new(),
            overlays: HashMap::new(),
            acting: vec![],
            sounds: vec![],
            lag: None,
            ethereal: vec![],
            last_anim: HashMap::new(),
            last_dir: HashMap::new(),
            first_step: Default::default(),
            seq_front: -1,
            seq_back: 0,
            fast_reach: crate::fast_area::DOS_REACH,
        };
        // Loaded in order: fixed, then movable, then people (ScummVM's loadMap).
        for (i, e) in w.entries.iter_mut().enumerate() {
            e.seq = i as i32;
        }
        w.seq_back = w.entries.len() as i32;
        let actors: Vec<ObjId> = w.ids.iter().copied().filter(|&id| id != 0 && id < 256).collect();
        // What they last did, as saved (the scripts ask: "This person" or "That person").
        if let Some(d) = &gd.npcdata {
            w.last_anim = actors.iter().filter_map(|&id| Some((id, map::npc_last_anim(d, id as usize)?))).collect();
        }
        for i in 0..w.entries.len() {
            w.file(i);
            w.note_egg(i);
            if w.entries[i].traits.anim {
                w.anims.insert(i);
            }
        }
        // What's inside containers on the map (from NONFIXED, nested by depth)…
        for (idx, raw) in nested {
            let mut stack = vec![w.ids[idx]];
            for (depth, it) in map::parse_contents(&raw) {
                stack.truncate(depth.min(stack.len()));
                let Some(&parent) = stack.last() else { break };
                let id = w.add_inside(gd, it, parent);
                if gd.typeflags.get(it.shape).family == family::CONTAINER {
                    stack.push(id);
                }
            }
        }
        // …and what the people here carry (from the item cache).
        if let Some(cache) = &gd.itemcach {
            w.add_carried(gd, &map::load_cache_contents(cache), &actors);
        }
        w
    }

    pub fn map(&self) -> u16 {
        self.map
    }

    /// Unique per built world: views compare it to know when to rebuild from scratch.
    pub fn serial(&self) -> u64 {
        self.serial
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn entry(&self, idx: usize) -> &Entry {
        &self.entries[idx]
    }
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
    pub fn id(&self, idx: usize) -> ObjId {
        self.ids[idx]
    }
    pub fn index(&self, id: ObjId) -> Option<usize> {
        self.by_id.get(&id).copied().filter(|&i| !self.entries[i].gone)
    }
    pub fn get(&self, id: ObjId) -> Option<Item> {
        self.index(id).map(|i| self.entries[i].it)
    }
    /// The Avatar, if on this map.
    pub fn avatar(&self) -> Option<ObjId> {
        self.index(AVATAR).map(|_| AVATAR)
    }
    /// Ids of everything on the map, in index order.
    pub fn objects(&self) -> impl Iterator<Item = ObjId> + '_ {
        self.entries.iter().zip(&self.ids).filter(|(e, _)| e.present()).map(|(_, &id)| id)
    }
    /// Triggering eggs (usecode, monster and teleport; glob eggs are expanded at load).
    pub fn eggs(&self) -> &[usize] {
        &self.eggs
    }
    fn note_egg(&mut self, idx: usize) {
        if self.entries[idx].traits.egg && self.entries[idx].traits.family != family::GLOBEGG {
            self.eggs.push(idx);
        }
    }
    pub fn set_hatched(&mut self, idx: usize, hatched: bool) {
        self.entries[idx].hatched = hatched;
    }
    /// Where teleporters with this id lead: the matching destination egg (frame 1).
    pub fn teleport_destination(&self, id: u8) -> Option<(i32, i32, i32)> {
        self.eggs.iter().map(|&i| &self.entries[i]).find_map(|e| {
            let it = e.it;
            (e.present() && e.traits.family == family::TELEPORTEGG && it.frame == 1 && it.quality & 0xFF == id as u16)
                .then_some((it.x, it.y, it.z))
        })
    }

    /// Indices changed since the last [`take_dirty`](Self::take_dirty) (each listed once).
    pub fn dirty(&self) -> &[usize] {
        &self.dirty
    }
    pub fn take_dirty(&mut self) -> Vec<usize> {
        for &i in &self.dirty {
            self.marked[i] = false;
        }
        std::mem::take(&mut self.dirty)
    }

    fn touch(&mut self, idx: usize) {
        if !std::mem::replace(&mut self.marked[idx], true) {
            self.dirty.push(idx);
        }
    }

    fn file(&mut self, idx: usize) {
        if self.entries[idx].parent.is_some() {
            return; // inside something: not on the map
        }
        for c in footprint_cells(&self.entries[idx]) {
            self.grid.entry(c).or_default().push(idx);
        }
    }
    fn unfile(&mut self, idx: usize) {
        for c in footprint_cells(&self.entries[idx]) {
            if let Some(v) = self.grid.get_mut(&c) {
                v.retain(|&i| i != idx);
            }
        }
    }
    /// Entries filed in grid cells touching the rectangle (may repeat; callers filter).
    fn near(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> impl Iterator<Item = usize> + '_ {
        cells(x0, y0, x1, y1).flat_map(|c| self.grid.get(&c).into_iter().flatten().copied())
    }

    /// Has `id` entered the fast area (had its enterFastArea)?
    pub fn is_fast(&self, id: ObjId) -> bool {
        self.index(id).is_some_and(|i| self.entries[i].fast)
    }
    /// Would a spot be in the fast area (round where the Avatar is now)?
    pub fn in_fast_area(&self, x: i32, y: i32) -> bool {
        self.get(AVATAR).is_some_and(|a| crate::fast_area::holds(self.fast_reach, (a.x, a.y), x, y))
    }
    pub fn set_fast(&mut self, id: ObjId, on: bool) {
        if let Some(i) = self.index(id) {
            self.entries[i].fast = on;
        }
    }
    pub(crate) fn set_fast_at(&mut self, idx: usize, on: bool) {
        self.entries[idx].fast = on;
    }

    /// Replace an item (its box and traits are recomputed from the new shape).
    pub fn set(&mut self, gd: &GameData, id: ObjId, it: Item) {
        if let Some(i) = self.index(id) {
            self.update(gd, i, it);
        }
    }

    fn update(&mut self, gd: &GameData, idx: usize, it: Item) {
        let old = self.entries[idx];
        let e = Entry {
            gone: old.gone,
            void: old.void,
            hatched: old.hatched,
            origin: old.origin,
            parent: old.parent,
            seq: old.seq,
            fast: old.fast,
            glob: old.glob,
            ..Entry::new(gd, it)
        };
        let moved = (old.x0, old.y0, old.it.x, old.it.y) != (e.x0, e.y0, e.it.x, e.it.y);
        if old.parent.is_none() && search::chunk(&old.it) != search::chunk(&e.it) {
            self.requeue(idx, &e.it); // (to another chunk's list)
        }
        if moved {
            self.unfile(idx);
        }
        self.entries[idx] = e;
        if moved {
            self.file(idx);
        }
        if e.traits.anim {
            self.anims.insert(idx);
        }
        self.touch(idx);
    }

    /// Clear a status flag on every item (e.g. "window open" when no windows are).
    pub fn clear_flag(&mut self, flag: u16) {
        for i in 0..self.entries.len() {
            if self.entries[i].it.flags & flag != 0 {
                self.entries[i].it.flags &= !flag;
                self.touch(i);
            }
        }
    }

    pub fn remove(&mut self, id: ObjId) {
        self.fighting.remove(&id);
        self.overlays.remove(&id);
        self.acting.retain(|a| a.id != id);
        self.last_anim.remove(&id);
        self.last_dir.remove(&id);
        self.first_step.remove(&id);
        self.ethereal.retain(|&e| e != id);
        if let Some(i) = self.index(id) {
            self.entries[i].gone = true;
            self.touch(i);
        }
    }

    /// Add an item; `on_map = false` starts it in the void. Returns its new id.
    pub fn add(&mut self, gd: &GameData, it: Item, on_map: bool) -> ObjId {
        let id = self.new_id();
        self.insert(gd, it, id, on_map);
        id
    }

    /// A free id (256 up; after wrapping round, ids still in use are skipped).
    fn new_id(&mut self) -> ObjId {
        let mut id = self.next_id;
        while self.by_id.contains_key(&id) {
            id = id.wrapping_add(1).max(256);
        }
        self.next_id = id.wrapping_add(1).max(256);
        id
    }

    /// Put an actor (e.g. the Avatar arriving through a teleporter) on this map at `it`'s position.
    pub fn put_actor(&mut self, gd: &GameData, it: Item) {
        match self.index(it.npc) {
            Some(i) => {
                self.entries[i].gone = false; // (back after being sent off this map)
                self.set(gd, it.npc, it)
            }
            None => self.insert(gd, it, it.npc, true),
        }
    }

    fn insert(&mut self, gd: &GameData, it: Item, id: ObjId, on_map: bool) {
        let idx = self.entries.len();
        self.entries.push(Entry { void: !on_map, ..Entry::new(gd, it) });
        self.requeue(idx, &it);
        self.ids.push(id);
        self.marked.push(false);
        self.by_id.insert(id, idx);
        self.file(idx);
        self.note_egg(idx);
        if self.entries[idx].traits.anim {
            self.anims.insert(idx);
        }
        self.touch(idx);
    }

    /// Ids of items whose footprint touches the square of `range` around (x, y), in index order.
    pub fn area(&self, x: i32, y: i32, range: i32) -> Vec<ObjId> {
        let (x0, y0, x1, y1) = (x - range, y - range, x + range, y + range);
        let mut v: Vec<usize> = self
            .near(x0, y0, x1, y1)
            .filter(|&i| {
                let e = &self.entries[i];
                e.present() && e.x0 <= x1 && x0 <= e.it.x && e.y0 <= y1 && y0 <= e.it.y
            })
            .collect();
        v.sort_unstable();
        v.dedup();
        v.into_iter().map(|i| self.ids[i]).collect()
    }

    /// A random number below `n` (0 for 0).
    pub fn roll(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.random() % n
        }
    }

    /// Items on the map (not markers or eggs) whose box overlaps `b`.
    pub fn touching(&self, b: Bounds) -> Vec<ObjId> {
        let mut v: Vec<usize> = self
            .near(b.x0, b.y0, b.x1, b.y1)
            .filter(|&i| {
                let e = &self.entries[i];
                e.present() && !e.marker() && e.bounds().overlaps(&b)
            })
            .collect();
        v.sort_unstable();
        v.dedup();
        v.into_iter().map(|i| self.ids[i]).collect()
    }

    fn random(&mut self) -> u32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng
    }
}
