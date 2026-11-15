//! Map items: FIXED.DAT (static), GLOB.FLX (prefab groups), NONFIXED.DAT (movable items) and ITEMCACH.DAT (NPCs).
use crate::{
    flex::Flex,
    typeflag::{family, TypeFlags},
    u16le,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Item {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub shape: u32,
    pub frame: u32,
    pub flags: u16,
    pub quality: u16,
    /// NPC number for actors (1 = Avatar), 0 otherwise.
    pub npc: u16,
    /// The record's raw "npc" and "map" bytes. Eggs use them: npcnum = trigger range
    /// (x << 4 | y, in 32-unit cells), mapnum = egg id / teleport target map / monster info.
    pub npcnum: u8,
    pub mapnum: u8,
    /// Record bytes 14–15: the original's object number for the item (kept so a save made here
    /// still matches the kernel's references to it).
    pub objid: u16,
}

impl Item {
    /// The 16-byte FIXED/NONFIXED record (`next` = 0). Coordinates are clamped to the format's range.
    pub fn record(&self) -> [u8; 16] {
        let mut r = [0u8; 16];
        r[0..2].copy_from_slice(&(self.x.clamp(0, 0xFFFF) as u16).to_le_bytes());
        r[2..4].copy_from_slice(&(self.y.clamp(0, 0xFFFF) as u16).to_le_bytes());
        r[4] = self.z.clamp(0, 255) as u8;
        r[5..7].copy_from_slice(&(self.shape as u16).to_le_bytes());
        r[7] = self.frame as u8;
        r[8..10].copy_from_slice(&self.flags.to_le_bytes());
        r[10..12].copy_from_slice(&self.quality.to_le_bytes());
        r[12] = self.npcnum;
        r[13] = self.mapnum;
        r[14..16].copy_from_slice(&self.objid.to_le_bytes());
        r
    }

    /// The record of an item inside a container (NONFIXED): `x` = nesting depth (1 = directly
    /// inside), `y` = window position. The item's own x, y are that window position.
    pub fn contained_record(&self, depth: usize) -> [u8; 16] {
        Item { x: depth as i32, y: pack_window_pos(self.x, self.y), ..*self }.record()
    }

    fn parse(r: &[u8]) -> Self {
        Item {
            x: u16le(r, 0) as i32,
            y: u16le(r, 2) as i32,
            z: r[4] as i32,
            shape: u16le(r, 5) as u32,
            frame: r[7] as u32,
            flags: u16le(r, 8),
            quality: u16le(r, 10),
            npc: 0,
            npcnum: r[12],
            mapnum: r[13],
            objid: u16le(r, 14),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct GlobItem {
    pub x: u8,
    pub y: u8,
    pub z: u8,
    pub shape: u16,
    pub frame: u8,
}

pub struct Globs(pub Vec<Vec<GlobItem>>);

impl Globs {
    pub fn parse(f: &Flex) -> Self {
        Self(
            (0..f.len())
                .map(|i| {
                    let Some(d) = f.get(i) else { return vec![] };
                    let n = if d.len() >= 2 { u16le(d, 0) as usize } else { 0 };
                    d[2..]
                        .as_chunks::<6>()
                        .0
                        .iter()
                        .take(n)
                        .map(|b| GlobItem { x: b[0], y: b[1], z: b[2], shape: u16le(b, 3), frame: b[5] })
                        .collect()
                })
                .collect(),
        )
    }
}

/// 16-byte records: u16 x, y; u8 z; u16 shape; u8 frame; u16 flags, quality; u8 npc, map; u16 next.
/// (Crusader stores coords halved; U8 does not.) What's on the map, glob eggs expanded; what
/// fixed chests hold is left out (see [`load_fixed_with_contents`]).
pub fn load_fixed(d: &[u8], globs: &Globs, tf: &TypeFlags) -> Vec<Item> {
    load_fixed_with_contents(d, globs, tf).into_iter().map(|(it, _)| it).collect()
}

/// FIXED.DAT map entry like NONFIXED (containers followed by their contents, nested by depth),
/// with glob eggs expanded after each egg.
pub fn load_fixed_with_contents(d: &[u8], globs: &Globs, tf: &TypeFlags) -> Vec<(Item, Vec<u8>)> {
    let mut out = Vec::with_capacity(d.len() / 16);
    for (it, inside) in load_nonfixed_with_contents(d, tf) {
        if tf.get(it.shape).family == family::GLOBEGG {
            for g in globs.0.get(it.quality as usize).into_iter().flatten() {
                // (ScummVM's GlobEgg: the egg's 512-chunk, 2 units a step, plus 1; without it
                // glob walls sit a unit off and overlap doors and lifts beside them.)
                let g = Item {
                    x: (it.x & !0x1FF) + 2 * g.x as i32 + 1,
                    y: (it.y & !0x1FF) + 2 * g.y as i32 + 1,
                    z: it.z + g.z as i32,
                    shape: g.shape as u32,
                    frame: g.frame as u32,
                    ..Default::default()
                };
                out.push((g, vec![]));
            }
        }
        out.push((it, inside));
    }
    out
}

/// NONFIXED.DAT map entry: same records as FIXED, but containers are followed by their contents,
/// whose `x` holds the nesting depth. Only top-level (on-map) items are returned.
pub fn load_nonfixed(d: &[u8], tf: &TypeFlags) -> Vec<Item> {
    load_nonfixed_with_contents(d, tf).into_iter().map(|(it, _)| it).collect()
}

/// Top-level items, each with the raw records of what it contains (written back as they were).
pub fn load_nonfixed_with_contents(d: &[u8], tf: &TypeFlags) -> Vec<(Item, Vec<u8>)> {
    let mut out: Vec<(Item, Vec<u8>)> = Vec::new();
    let mut depth = 0;
    for r in d.as_chunks::<16>().0.iter() {
        let it = Item::parse(r);
        while depth > 0 && it.x != depth {
            depth -= 1;
        }
        match out.last_mut() {
            Some(last) if depth > 0 => last.1.extend_from_slice(r),
            _ => out.push((it, vec![])),
        }
        if tf.get(it.shape).family == family::CONTAINER {
            depth += 1;
        }
    }
    out
}

/// ITEMCACH.DAT entry 0: column-major arrays for the first 256 objects, which are the NPCs.
/// Returns the actors standing on `map`.
pub fn load_actors(d: &[u8], map: usize) -> Vec<Item> {
    (1..256).filter_map(|i| load_actor(d, i)).filter(|(m, _)| *m == map).map(|(_, a)| a).collect()
}

/// ITEMCACH.DAT entry 0 columns: x, y (u16), z (u8), shape (u16), frame (u8), flags, quality (u16),
/// npcnum, map (u8), next (u16).
const OFF: [usize; 10] = [0x0, 0x4800, 0x9000, 0xB400, 0xFC00, 0x12000, 0x16800, 0x1B000, 0x1D400, 0x1F800];

/// Store NPC `npc` on `map` into ITEMCACH.DAT entry 0 (the inverse of [`load_actor`]).
pub fn store_actor(d: &mut [u8], npc: usize, map: usize, it: &Item) {
    if npc >= 256 || d.len() < OFF[9] {
        return;
    }
    let mut w = |k: usize, v: u16| d[OFF[k] + npc * 2..OFF[k] + npc * 2 + 2].copy_from_slice(&v.to_le_bytes());
    w(0, it.x.clamp(0, 0xFFFF) as u16);
    w(1, it.y.clamp(0, 0xFFFF) as u16);
    w(3, it.shape as u16);
    w(5, it.flags);
    w(6, it.quality);
    let mut b = |k: usize, v: u8| d[OFF[k] + npc] = v;
    b(2, it.z.clamp(0, 255) as u8);
    b(4, it.frame as u8);
    b(7, it.npcnum);
    b(8, map as u8);
}

/// An item inside a container or carried by someone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contained {
    /// The item-cache object number (contents refer to their container by it; NPCs are 1–255).
    pub id: u16,
    pub parent: u16,
    /// `x`, `y`: where it sits in its container's window (game pixels in the gump's item area).
    pub item: Item,
}

/// A window position as the formats store it: low byte x, high byte y.
fn pack_window_pos(x: i32, y: i32) -> i32 {
    (y.clamp(0, 255) << 8) | x.clamp(0, 255)
}

/// Contained items keep their container in `x` and their window position in `y` (low byte x,
/// high byte y). Turn a record read that way into a [`Contained`] item.
fn contained(id: u16, mut it: Item) -> Contained {
    let (parent, pos) = (it.x as u16, it.y as u16);
    (it.x, it.y) = ((pos & 0xFF) as i32, (pos >> 8) as i32);
    Contained { id, parent, item: it }
}

/// Everything inside something in ITEMCACH.DAT entry 0 (objects 256 on: the game's live object
/// table): what people carry and what's in their bags.
pub fn load_cache_contents(d: &[u8]) -> Vec<Contained> {
    const CONTAINED: u16 = 0x0008;
    if d.len() < OFF[9] {
        return vec![];
    }
    let n = (OFF[1] / 2).min((d.len() - OFF[9]) / 2);
    (256..n)
        .filter_map(|i| {
            let w = |k: usize| u16le(d, OFF[k] + i * 2);
            let b = |k: usize| d[OFF[k] + i];
            (w(3) != 0 && w(5) & CONTAINED != 0).then(|| {
                let it = Item {
                    x: w(0) as i32,
                    y: w(1) as i32,
                    z: b(2) as i32,
                    shape: w(3) as u32,
                    frame: b(4) as u32,
                    flags: w(5),
                    quality: w(6),
                    npc: 0,
                    npcnum: b(7),
                    mapnum: b(8),
                    objid: i as u16,
                };
                contained(i as u16, it)
            })
        })
        .collect()
}

/// The area the DOS game had in memory when it saved. ITEMCACH.DAT entry 1 (u16 words) holds the
/// map (word 0), a window (words 1-4) and two 64x64 grids by chunk (512 x 512 units, index
/// (x / 512) * 64 + y / 512): list heads (words 5-4100), each a chain of object numbers through
/// entry 0's `next` column (people included), and a second grid (words 4101-8196), non-zero for
/// every chunk held. The objects are the live items of those chunks, fixed ones included;
/// NONFIXED.DAT still has those chunks as they were when the game last cached them.
pub struct CachedArea {
    pub map: usize,
    /// The chunks held, as (x / 512, y / 512).
    pub chunks: std::collections::HashSet<(i32, i32)>,
    /// Their items (not people): object number and item, without the in-memory flag (0x2000).
    pub items: Vec<(u16, Item)>,
}

pub fn cached_area(e0: &[u8], e1: &[u8]) -> Option<CachedArea> {
    if e0.len() < OFF[9] + OFF[1] || e1.len() < 2 * 4101 {
        return None;
    }
    let n = OFF[1] / 2;
    let word = |i: usize| u16le(e1, 2 * i);
    let chunks = (0..4096)
        .filter(|&c| word(5 + c) != 0 || word(4101 + c) != 0)
        .map(|c| ((c / 64) as i32, (c % 64) as i32))
        .collect();
    let mut items = vec![];
    let mut seen = std::collections::HashSet::new();
    for head in (5..4101).map(word) {
        let mut o = head as usize;
        while (1..n).contains(&o) && seen.insert(o) {
            let w = |k: usize| u16le(e0, OFF[k] + o * 2);
            let b = |k: usize| e0[OFF[k] + o];
            if o >= 256 && w(3) != 0 {
                let it = Item {
                    x: w(0) as i32,
                    y: w(1) as i32,
                    z: b(2) as i32,
                    shape: w(3) as u32,
                    frame: b(4) as u32,
                    flags: w(5) & !IN_CACHE,
                    quality: w(6),
                    npc: 0,
                    npcnum: b(7),
                    mapnum: b(8),
                    objid: 0,
                };
                items.push((o as u16, it));
            }
            o = w(9) as usize;
        }
    }
    (!items.is_empty()).then(|| CachedArea { map: word(0) as usize, chunks, items })
}

/// The in-memory flag the DOS game sets on what it holds of the map.
pub const IN_CACHE: u16 = 0x2000;
/// Flags the DOS game gives the items a glob egg expands to near the Avatar: thrown away again
/// when they leave (FAST_ONLY | DISPOSABLE), held (IN_CACHE).
pub const GLOB_ITEM: u16 = IN_CACHE | 0x0082;
/// Object slots in the item cache (people 1–255, then items).
pub const OBJECTS: usize = OFF[1] / 2;
const COL_BYTES: [usize; 10] = [2, 2, 1, 2, 1, 2, 2, 1, 1, 2];

fn put_col(d: &mut [u8], k: usize, o: usize, v: u32) {
    let n = COL_BYTES[k];
    d[OFF[k] + o * n..OFF[k] + (o + 1) * n].copy_from_slice(&v.to_le_bytes()[..n]);
}

/// The cache's list heads (the map area, see [`cached_area`]) and its other lists (entry 1's last
/// words): which objects they hold. Objects the map area holds that aren't people.
pub fn cached_map_objects(e0: &[u8], e1: &[u8]) -> Vec<u16> {
    if e0.len() < OFF[9] + OFF[1] || e1.len() < 2 * 8197 {
        return vec![];
    }
    let mut out = vec![];
    let mut seen = std::collections::HashSet::new();
    for c in 0..4096 {
        let mut o = u16le(e1, 2 * (5 + c)) as usize;
        while (1..OBJECTS).contains(&o) && seen.insert(o) {
            if o >= 256 {
                out.push(o as u16);
            }
            o = u16le(e0, OFF[9] + o * 2) as usize;
        }
    }
    out
}

/// Empty object `slot` of the cache (every column).
pub fn free_cache_object(d: &mut [u8], slot: usize) {
    if (256..OBJECTS).contains(&slot) && d.len() >= OFF[9] + OFF[1] {
        for k in 0..10 {
            put_col(d, k, slot, 0);
        }
    }
}

/// Object `slot` of the cache: an item on the map (flags as given), `next` in its chunk's list.
pub fn store_cache_object(d: &mut [u8], slot: usize, it: &Item, next: u16) {
    if (256..OBJECTS).contains(&slot) && d.len() >= OFF[9] + OFF[1] {
        let v = [
            it.x as u32,
            it.y as u32,
            it.z.clamp(0, 255) as u32,
            it.shape,
            it.frame,
            it.flags as u32,
            it.quality as u32,
        ];
        for (k, v) in v.into_iter().enumerate() {
            put_col(d, k, slot, v & 0xFFFF);
        }
        put_col(d, 7, slot, it.npcnum as u32);
        put_col(d, 8, slot, it.mapnum as u32);
        put_col(d, 9, slot, next as u32);
    }
}

/// Object `o`'s `next` (in its chunk's list, or the free list).
pub fn cached_next(d: &[u8], o: usize) -> usize {
    if o < OBJECTS && d.len() >= OFF[9] + OFF[1] {
        u16le(d, OFF[9] + o * 2) as usize
    } else {
        0
    }
}

/// Set object `o`'s `next` (a person's too).
pub fn set_cache_next(d: &mut [u8], o: usize, next: u16) {
    if o < OBJECTS && d.len() >= OFF[9] + OFF[1] {
        put_col(d, 9, o, next as u32);
    }
}

/// Write entry 1's map area: `map`, the window around the Avatar's chunk `(cx, cy)` and the list
/// heads by chunk (0: not held). Its last words (the other lists) are kept.
pub fn store_cached_area(e1: &mut [u8], map: usize, (cx, cy): (i32, i32), heads: &[u16]) {
    if e1.len() < 2 * 8197 || heads.len() != 4096 {
        return;
    }
    let (s, d) = (cx + cy, cx - cy);
    let mut put = |i: usize, v: u16| e1[2 * i..2 * i + 2].copy_from_slice(&v.to_le_bytes());
    put(0, map as u16);
    for (i, v) in [s - 7, s + 7, d - 3, d + 3].into_iter().enumerate() {
        put(1 + i, v as u16);
    }
    for (c, &h) in heads.iter().enumerate() {
        put(5 + c, h);
        put(4101 + c, (h != 0) as u16);
    }
}

/// Hold nothing (after its objects were freed): every list empty.
pub fn clear_cached_area(e1: &mut [u8]) {
    if e1.len() >= 2 * 8197 {
        e1[2 * 5..2 * 8197].fill(0);
    }
}

/// Is chunk `(cx, cy)` in the window the DOS game expands glob eggs in, around chunk `around`?
pub fn in_glob_window((cx, cy): (i32, i32), around: (i32, i32)) -> bool {
    ((cx + cy) - (around.0 + around.1)).abs() <= 7 && ((cx - cy) - (around.0 - around.1)).abs() <= 3
}

/// What's inside object `parent` of the item cache, as NONFIXED-style records nested by depth.
pub fn cache_contents_records(all: &[Contained], parent: u16) -> Vec<u8> {
    fn walk(all: &[Contained], p: u16, depth: usize, out: &mut Vec<u8>, seen: &mut Vec<u16>) {
        for c in all.iter().filter(|c| c.parent == p) {
            if seen.contains(&c.id) {
                continue;
            }
            seen.push(c.id);
            out.extend_from_slice(&c.item.contained_record(depth));
            walk(all, c.id, depth + 1, out, seen);
        }
    }
    let mut out = vec![];
    walk(all, parent, 1, &mut out, &mut vec![parent]);
    out
}

/// Put a carried item into ITEMCACH.DAT entry 0 at object `slot`, inside object `parent`.
pub fn store_cache_item(d: &mut [u8], slot: usize, parent: u16, it: &Item) {
    if !(256..OFF[1] / 2).contains(&slot) || d.len() < OFF[9] + (slot + 1) * 2 {
        return;
    }
    let rec = Item { x: parent as i32, y: pack_window_pos(it.x, it.y), npc: 0, ..*it };
    let mut w = |k: usize, v: u16| d[OFF[k] + slot * 2..OFF[k] + slot * 2 + 2].copy_from_slice(&v.to_le_bytes());
    w(0, rec.x as u16);
    w(1, rec.y as u16);
    w(3, rec.shape as u16);
    w(5, rec.flags);
    w(6, rec.quality);
    let mut b = |k: usize, v: u8| d[OFF[k] + slot] = v;
    b(2, rec.z.clamp(0, 255) as u8);
    b(4, rec.frame as u8);
    b(7, rec.npcnum);
    b(8, rec.mapnum);
}

/// Free object `slot` in ITEMCACH.DAT entry 0.
pub fn clear_cache_item(d: &mut [u8], slot: usize) {
    if (256..OFF[1] / 2).contains(&slot) && d.len() >= OFF[9] + (slot + 1) * 2 {
        d[OFF[3] + slot * 2..OFF[3] + slot * 2 + 2].fill(0);
        d[OFF[5] + slot * 2..OFF[5] + slot * 2 + 2].fill(0);
    }
}

/// ITEMCACH.DAT entry 1's last words: the original's free lists, chained through `next`. Objects:
/// count at 8197, head 8199, tail 8201 (always 9215). People: count 8198, head 8200, tail 8202
/// (always 255); a person's slot is free when empty or put away (ETHEREAL | FAST_ONLY). Every
/// object slot is on the free list, in a chunk's list, or inside something (all DOS saves).
const FREE_WORDS: usize = 8203;
const PUT_AWAY: u16 = 0x0880;

/// Empty the object slots on the free list (they keep stale records, some flagged CONTAINED, which
/// would otherwise pass for belongings), so that free = no shape, as everywhere else here.
pub fn drop_free_objects(e0: &mut [u8], e1: &[u8]) {
    if e1.len() < 2 * FREE_WORDS || e0.len() < OFF[9] + OFF[1] {
        return;
    }
    let mut o = u16le(e1, 2 * 8199) as usize;
    let mut seen = std::collections::HashSet::new();
    while (256..OBJECTS).contains(&o) && seen.insert(o) {
        let next = u16le(e0, OFF[9] + o * 2) as usize;
        free_cache_object(e0, o);
        o = next;
    }
}

/// Write both free lists afresh from what's free now (lowest first, the fixed tails last).
pub fn store_free_lists(e0: &mut [u8], e1: &mut [u8]) {
    if e1.len() < 2 * FREE_WORDS || e0.len() < OFF[9] + OFF[1] {
        return;
    }
    let people: Vec<usize> = (1..255)
        .filter(|&n| u16le(e0, OFF[3] + n * 2) == 0 || u16le(e0, OFF[5] + n * 2) & PUT_AWAY == PUT_AWAY)
        .chain([255])
        .collect();
    let objects: Vec<usize> = (256..OBJECTS).filter(|&o| cache_slot_free(e0, o)).chain([OBJECTS - 1]).collect();
    for (list, (count, head, tail)) in [(people, (8198, 8200, 8202)), (objects, (8197, 8199, 8201))] {
        for (i, &o) in list.iter().enumerate() {
            put_col(e0, 9, o, list.get(i + 1).map_or(0, |&n| n as u32));
        }
        for (w, v) in [(count, list.len()), (head, list[0]), (tail, *list.last().unwrap())] {
            e1[2 * w..2 * w + 2].copy_from_slice(&(v as u16).to_le_bytes());
        }
    }
}

/// Is object `slot` free to use (no shape; the last slot is the free list's tail, never used)?
pub fn cache_slot_free(d: &[u8], slot: usize) -> bool {
    (256..OBJECTS - 1).contains(&slot) && d.len() >= OFF[9] && u16le(d, OFF[3] + slot * 2) == 0
}

/// The raw NONFIXED records that follow a container (see [`load_nonfixed_with_contents`]): each
/// `x` is the nesting depth (1 = directly inside), `y` the window position. Returns (depth, item).
pub fn parse_contents(raw: &[u8]) -> Vec<(usize, Item)> {
    raw.as_chunks::<16>()
        .0
        .iter()
        .map(|r| {
            let it = Item::parse(r);
            let c = contained(0, it);
            (it.x.max(1) as usize, c.item)
        })
        .collect()
}

/// NPCDATA.DAT entry 0: 0x31 bytes per NPC (strength, dexterity, intelligence, hit points, …);
/// byte 7 is the high byte of the NPC's frame (ITEMCACH only has room for the low byte).
const NPC_RECORD: usize = 0x31;

/// What NPCDATA says about a person (the parts we know).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub strength: u8,
    pub dexterity: u8,
    pub intelligence: u8,
    pub hit_points: u8,
    pub mana: u16,
    /// Byte 0x0B: own alignment bits in the low nibble, enemies' in the high one.
    pub alignment: u8,
    /// Bytes 0x1B, 0x2F, 0x30: state flags (see [`actor_flag`]).
    pub flags: [u8; 3],
}

/// NPCDATA state flags as (index into [`Stats::flags`], bit), as ScummVM reads them.
pub mod actor_flag {
    pub const INVINCIBLE: (usize, u8) = (0, 0x01);
    pub const IN_COMBAT: (usize, u8) = (1, 0x08);
    pub const DEAD: (usize, u8) = (1, 0x10);
    /// Key characters start with it (Mordea, Devon…): hurt, but never killed.
    pub const IMMORTAL: (usize, u8) = (2, 0x04);
    pub const WITHSTAND_DEATH: (usize, u8) = (2, 0x08);
    pub const FEIGN_DEATH: (usize, u8) = (2, 0x10);
    pub const AIRWALK: (usize, u8) = (2, 0x01);
}

/// Where NPCDATA keeps mana (u16), alignment and the state flags.
const MANA: usize = 0x2D;
const ALIGNMENT: usize = 0x0B;
const FLAGS: [usize; 3] = [0x1B, 0x2F, 0x30];

/// NPC `npc`'s stats: bytes 0–3 strength, dexterity, intelligence, hit points; 0x0B alignment;
/// 0x2D mana; flags at 0x1B, 0x2F, 0x30.
pub fn npc_stats(npcdata: &[u8], npc: usize) -> Option<Stats> {
    let r = npcdata.get(npc * NPC_RECORD..(npc + 1) * NPC_RECORD)?;
    let (strength, dexterity, intelligence, hit_points) = (r[0], r[1], r[2], r[3]);
    let (mana, alignment, flags) = (u16le(r, MANA), r[ALIGNMENT], FLAGS.map(|o| r[o]));
    Some(Stats { strength, dexterity, intelligence, hit_points, mana, alignment, flags })
}

pub fn store_npc_stats(npcdata: &mut [u8], npc: usize, s: Stats) {
    if let Some(r) = npcdata.get_mut(npc * NPC_RECORD..(npc + 1) * NPC_RECORD) {
        r[..4].copy_from_slice(&[s.strength, s.dexterity, s.intelligence, s.hit_points]);
        r[ALIGNMENT] = s.alignment;
        FLAGS.iter().zip(s.flags).for_each(|(&o, f)| r[o] = f);
        r[MANA..MANA + 2].copy_from_slice(&s.mana.to_le_bytes());
    }
}

/// Where NPCDATA lists what an NPC wears: object numbers (u16) for slots 1–7 — shield, arms,
/// head, body, legs, weapon, backpack — from byte 0x1D (0 = nothing).
const EQUIPMENT: usize = 0x1D;
pub const EQUIP_SLOTS: usize = 7;

pub fn npc_equipment(npcdata: &[u8], npc: usize) -> Option<[u16; EQUIP_SLOTS]> {
    let r = npcdata.get(npc * NPC_RECORD..(npc + 1) * NPC_RECORD)?;
    Some(std::array::from_fn(|k| u16le(r, EQUIPMENT + 2 * k)))
}

pub fn store_npc_equipment(npcdata: &mut [u8], npc: usize, worn: [u16; EQUIP_SLOTS]) {
    if let Some(r) = npcdata.get_mut(npc * NPC_RECORD..(npc + 1) * NPC_RECORD) {
        for (k, id) in worn.iter().enumerate() {
            r[EQUIPMENT + 2 * k..EQUIPMENT + 2 * k + 2].copy_from_slice(&id.to_le_bytes());
        }
    }
}

/// The high byte of NPC `npc`'s frame (frames past 255: most of the people's poses).
pub fn npc_frame_high(npcdata: &[u8], npc: usize) -> u32 {
    npcdata.get(npc * NPC_RECORD + 7).map_or(0, |&b| (b as u32) << 8)
}

pub fn store_npc_frame_high(npcdata: &mut [u8], npc: usize, frame: u32) {
    if let Some(b) = npcdata.get_mut(npc * NPC_RECORD + 7) {
        *b = (frame >> 8) as u8;
    }
}

/// NPCDATA byte 4: the way NPC `npc` faces (0 north, clockwise; the anims' direction order).
pub fn npc_dir(npcdata: &[u8], npc: usize) -> Option<usize> {
    npcdata.get(npc * NPC_RECORD + 4).map(|&d| d as usize & 7)
}

pub fn store_npc_dir(npcdata: &mut [u8], npc: usize, dir: usize) {
    if let Some(b) = npcdata.get_mut(npc * NPC_RECORD + 4) {
        *b = dir as u8;
    }
}

/// One NPC from ITEMCACH.DAT entry 0: `(map, item)`, or None if unused (or parked at 0, 0, as
/// a couple of placeholders are). The frame's high byte is in NPCDATA ([`npc_frame_high`]).
pub fn load_actor(d: &[u8], npc: usize) -> Option<(usize, Item)> {
    actor_row(d, npc).filter(|(_, it)| (it.x, it.y) != (0, 0))
}

/// NPC slot `npc`'s row as saved, whatever it holds (None: shape 0, a free slot).
pub fn actor_row(d: &[u8], npc: usize) -> Option<(usize, Item)> {
    if npc >= 256 || d.len() < OFF[9] {
        return None;
    }
    let w = |k: usize| u16le(d, OFF[k] + npc * 2) as i32;
    let b = |k: usize| d[OFF[k] + npc] as i32;
    if w(3) == 0 {
        return None;
    }
    let it = Item {
        x: w(0),
        y: w(1),
        z: b(2),
        shape: w(3) as u32,
        frame: b(4) as u32,
        flags: w(5) as u16,
        quality: w(6) as u16,
        npc: npc as u16,
        npcnum: b(7) as u8,
        mapnum: b(8) as u8,
        objid: w(9) as u16,
    };
    Some((b(8) as usize, it))
}
