//! Saving a game in the original's format. A save starts from the files the game was loaded from
//! and writes back what the engine models: every visited map's movable items, where the NPCs are,
//! what they carry and wear, their stats, the global flags and the clock. The rest (gumps, the
//! kernel's processes) is kept as it was loaded, so nothing we don't understand yet is lost.
use crate::equip;
use crate::stats::Stats;
use crate::world::{item_flag, ObjId, Origin, World};
use std::collections::{HashMap, HashSet};
use u8data::{flex, map, save::Save, GameData};

/// Save format version in our [`u8data::save::EXTRA`] file.
const VERSION: u32 = 2;

/// The game state as a save. `worlds`: the current map first, then the other maps visited.
pub fn snapshot<'a>(
    gd: &GameData,
    worlds: impl IntoIterator<Item = &'a World>,
    people: &Stats,
    globals: Option<&[u8]>,
    ticks: u32,
) -> Save {
    let worlds: Vec<&World> = worlds.into_iter().collect();
    let mut s = gd.state.clone();

    // NONFIXED.DAT: one entry per map; maps we haven't been to keep what they had.
    let old = s.get("NONFIXED.DAT").and_then(|d| flex::Flex::parse(d.to_vec()).ok());
    let n = worlds.iter().map(|w| w.map() as usize + 1).max().unwrap_or(0).max(old.as_ref().map_or(256, |f| f.len()));
    let mut maps: Vec<Option<Vec<u8>>> =
        (0..n).map(|m| old.as_ref().and_then(|f| f.get(m)).map(<[u8]>::to_vec)).collect();
    for w in &worlds {
        let r = w.movable_records();
        maps[w.map() as usize] = (!r.is_empty()).then_some(r);
    }
    let refs: Vec<Option<&[u8]>> = maps.iter().map(Option::as_deref).collect();
    s.set("NONFIXED.DAT", flex::build(old.as_ref().map(|f| f.header()), &refs));

    // ITEMCACH.DAT entry 0: NPCs. The current map knows best (including people it sent elsewhere);
    // other visited maps only for people still standing there (the Avatar too, if we're only
    // looking at another map: teleporting takes it off the old one).
    if let Some(cache) = s.get("ITEMCACH.DAT").and_then(|d| flex::Flex::parse(d.to_vec()).ok()) {
        let mut npcs = cache.get(0).map(<[u8]>::to_vec).unwrap_or_default();
        let mut area = cache.get(1).map(<[u8]>::to_vec).unwrap_or_default();
        map::drop_free_objects(&mut npcs, &area);
        // The map area the game held is written afresh below: first let go of what it held.
        free_held_area(&mut npcs, &area);
        let stats = s.get("NPCDATA.DAT").and_then(|d| flex::Flex::parse(d.to_vec()).ok());
        let mut npcdata = stats.as_ref().and_then(|f| f.get(0)).map(<[u8]>::to_vec);
        let mut done = HashSet::new();
        let mut carriers: Vec<(&World, u16)> = vec![];
        for (k, w) in worlds.iter().enumerate() {
            for e in w.entries().iter().filter(|e| e.origin == Origin::Actor && !e.gone) {
                let npc = e.it.npc;
                let here = !e.void;
                if (k > 0 && !here) || !done.insert(npc) {
                    continue;
                }
                let on = if here { w.map() as usize } else { e.it.mapnum as usize };
                map::store_actor(&mut npcs, npc as usize, on, &e.it);
                carriers.push((w, npc));
                if let Some(d) = &mut npcdata {
                    map::store_npc_frame_high(d, npc as usize, e.it.frame);
                    // The way they face (the DOS game takes it from here, not from the frame).
                    if let Some(dir) = crate::avatar::facing(gd, &e.it) {
                        map::store_npc_dir(d, npc as usize, dir);
                    }
                }
            }
        }
        let slots = store_belongings(&mut npcs, &carriers);
        // The Avatar's map (only looking at another one doesn't move it there).
        if let Some(w) = worlds.iter().find(|w| w.avatar().is_some()) {
            hold_area(gd, w, &mut npcs, &mut area);
        } else {
            map::clear_cached_area(&mut area);
        }
        map::store_free_lists(&mut npcs, &mut area);
        if let Some(d) = &mut npcdata {
            people.store(d);
        }
        if let Some(d) = &mut npcdata {
            for &(w, npc) in &carriers {
                // NPCDATA's list of what they wear, by the object numbers just given out.
                let mut worn = map::npc_equipment(d, npc as usize).unwrap_or_default();
                worn.fill(0);
                for id in w.contents(npc) {
                    let k = w.get(id).and_then(|it| equip::listed_as(gd, &it));
                    if let (Some(k), Some(&s)) = (k, slots.get(&id)) {
                        worn[k - 1] = s;
                    }
                }
                map::store_npc_equipment(d, npc as usize, worn);
            }
        }
        let rest: Vec<Option<&[u8]>> = (0..cache.len())
            .map(|i| match i {
                0 => Some(&npcs[..]),
                1 if !area.is_empty() => Some(&area[..]),
                _ => cache.get(i),
            })
            .collect();
        s.set("ITEMCACH.DAT", flex::build(Some(cache.header()), &rest));
        if let (Some(f), Some(d)) = (&stats, &npcdata) {
            let entries: Vec<Option<&[u8]>> =
                (0..f.len()).map(|i| if i == 0 { Some(&d[..]) } else { f.get(i) }).collect();
            s.set("NPCDATA.DAT", flex::build(Some(f.header()), &entries));
        }
    }

    if let Some(g) = globals {
        let len = s.get("FLAG.DAT").map_or(g.len(), <[u8]>::len).min(g.len());
        s.set("FLAG.DAT", g[..len].to_vec());
    }
    // The clock (and its hour number) where the original keeps it; the 60 Hz frame counter on by
    // twice the game time played since the kernel we started from.
    if let Some(mut k) = s.get("KERNEL.DAT").map(<[u8]>::to_vec) {
        if let Some(o) = u8data::clock_offset(&k) {
            let was = u32::from_le_bytes(k[o..o + 4].try_into().unwrap());
            if let Some(f) = u8data::frames_offset(&k) {
                let frames = u8data::saved_frames(&k).unwrap_or(0).wrapping_add(2 * ticks.saturating_sub(was));
                k[f..f + 4].copy_from_slice(&frames.to_le_bytes());
            }
            k[o..o + 4].copy_from_slice(&ticks.to_le_bytes());
            k[o + 4..o + 8].copy_from_slice(&(ticks / 27000).to_le_bytes());
            s.set("KERNEL.DAT", k);
        }
    }
    let map = worlds.first().map_or(0, |w| w.map());
    s.set_extra(&[("version", VERSION.to_string()), ("clock", ticks.to_string()), ("map", map.to_string())]);
    s
}

/// Empty the objects the cache's map area holds, and whatever is inside them.
fn free_held_area(cache: &mut [u8], area: &[u8]) {
    let held: HashSet<u16> = map::cached_map_objects(cache, area).into_iter().collect();
    let inside = map::load_cache_contents(cache);
    let parent: HashMap<u16, u16> = inside.iter().map(|c| (c.id, c.parent)).collect();
    for c in &inside {
        let mut root = c.parent;
        for _ in 0..64 {
            match parent.get(&root) {
                Some(&p) => root = p,
                None => break,
            }
        }
        if held.contains(&root) {
            map::free_cache_object(cache, c.id as usize);
        }
    }
    for o in held {
        map::free_cache_object(cache, o as usize);
    }
}

/// Write the current map into the cache as the DOS game keeps the area it has in memory (it
/// loads a game from there, not from NONFIXED.DAT; without it the map is empty or black):
/// every FIXED.DAT record and movable item of the map with what's inside, the people on it, and
/// near the Avatar what the glob eggs expand to, each chunk's list linked through `next`.
fn hold_area(gd: &GameData, w: &World, cache: &mut [u8], area: &mut [u8]) {
    let Some(avatar) = w.get(crate::world::AVATAR) else { return };
    let around = (avatar.x >> 9, avatar.y >> 9);
    let map_no = w.map() as usize;
    let chunk = |x: i32, y: i32| (x >> 9, y >> 9);
    // A chunk's list in entry 1 (None off the 64×64 map).
    let list = |it: &map::Item| {
        let (cx, cy) = chunk(it.x, it.y);
        ((0..64).contains(&cx) && (0..64).contains(&cy)).then_some((cx * 64 + cy) as usize)
    };
    let mut heads = vec![0u16; 4096];
    let mut free = free_slots(cache);
    // An item into the next free slot, first in its chunk's list (None: off the map, or no room
    // left; the original holds 9216 objects, and what doesn't fit is left out, not half-written).
    let mut hold = |cache: &mut [u8], it: &map::Item, heads: &mut Vec<u16>| -> Option<usize> {
        let c = list(it)?;
        let o = free.next()?;
        map::store_cache_object(cache, o, it, heads[c]);
        heads[c] = o as u16;
        Some(o)
    };
    let key = |it: &map::Item| (it.x, it.y, it.z, it.shape, it.frame);
    // What glob eggs expand to (what's FIXED.DAT's own is a record of it).
    let mut globbed: HashMap<_, usize> = HashMap::new();
    for it in gd.map_items(map_no) {
        *globbed.entry(key(&it)).or_default() += 1;
    }
    for it in map::load_fixed(gd.fixed.get(map_no).unwrap_or(&[]), &map::Globs(vec![]), &gd.typeflags) {
        if let Some(n) = globbed.get_mut(&key(&it)) {
            *n = n.saturating_sub(1);
        }
    }
    let mut held: Vec<(usize, Vec<u8>)> = vec![];
    for (i, e) in w.entries().iter().enumerate().filter(|(_, e)| e.origin == Origin::Fixed && !e.gone) {
        let glob = globbed.get_mut(&key(&e.it)).is_some_and(|n| {
            let g = *n > 0;
            *n = n.saturating_sub(1);
            g
        });
        if glob && !map::in_glob_window(chunk(e.it.x, e.it.y), around) {
            continue;
        }
        let flags = if glob { map::GLOB_ITEM } else { e.it.flags | map::IN_CACHE };
        let o = hold(cache, &map::Item { flags, objid: 0, npc: 0, ..e.it }, &mut heads);
        if let (Some(o), u8data::typeflag::family::CONTAINER) = (o, e.traits.family) {
            held.push((o, w.contents_records(w.id(i))));
        }
    }
    // Movable items.
    for (it, inside) in map::load_nonfixed_with_contents(&w.movable_records(), &gd.typeflags) {
        if let Some(o) = hold(cache, &map::Item { flags: it.flags | map::IN_CACHE, objid: 0, ..it }, &mut heads) {
            held.push((o, inside));
        }
    }
    // What's inside them (fixed chests too).
    let mut free = free_slots(cache);
    for (o, inside) in held {
        let mut stack = vec![o as u16];
        for (depth, c) in map::parse_contents(&inside) {
            stack.truncate(depth.min(stack.len()));
            let (Some(&p), Some(s)) = (stack.last(), free.next()) else { break };
            map::store_cache_item(cache, s, p, &map::Item { flags: c.flags | item_flag::CONTAINED, ..c });
            if gd.typeflags.get(c.shape).family == u8data::typeflag::family::CONTAINER {
                stack.push(s as u16);
            }
        }
    }
    // The people here.
    for e in w.entries().iter().filter(|e| e.origin == Origin::Actor && e.present() && !e.void) {
        if let Some(c) = list(&e.it) {
            map::set_cache_next(cache, e.it.npc as usize, heads[c]);
            heads[c] = e.it.npc;
        }
    }
    map::store_cached_area(area, map_no, around, &heads);
}

/// The item cache's free object slots (past the people's; the last is the free list's tail).
fn free_slots(cache: &[u8]) -> std::vec::IntoIter<usize> {
    (256..map::OBJECTS).filter(|&s| map::cache_slot_free(cache, s)).collect::<Vec<_>>().into_iter()
}

/// What the people we're saving carry: their old belongings come out of the item cache, their
/// current ones go in (each in its old object slot when that's free, else in a free one).
/// Returns the object number each was given.
fn store_belongings(cache: &mut [u8], carriers: &[(&World, u16)]) -> HashMap<ObjId, u16> {
    let old = map::load_cache_contents(cache);
    let parent: HashMap<u16, u16> = old.iter().map(|c| (c.id, c.parent)).collect();
    let ours: HashSet<u16> = carriers.iter().map(|&(_, npc)| npc).collect();
    for c in &old {
        let mut root = c.parent;
        for _ in 0..64 {
            match parent.get(&root) {
                Some(&p) if root >= 256 => root = p,
                _ => break,
            }
        }
        if ours.contains(&root) {
            map::clear_cache_item(cache, c.id as usize);
        }
    }
    let mut used = HashSet::new();
    let mut given = HashMap::new();
    let mut next_free = 256;
    for &(w, npc) in carriers {
        let mut queue = vec![(npc, npc)];
        while let Some((id, slot)) = queue.pop() {
            for child in w.contents(id) {
                let Some(mut it) = w.get(child) else { continue };
                let wanted = it.objid as usize;
                let s = if map::cache_slot_free(cache, wanted) && used.insert(wanted) {
                    wanted
                } else {
                    while !(map::cache_slot_free(cache, next_free) && !used.contains(&next_free)) {
                        next_free += 1;
                        if next_free >= map::OBJECTS {
                            return given; // full (the original's limit)
                        }
                    }
                    used.insert(next_free);
                    next_free
                };
                it.flags |= item_flag::CONTAINED;
                it.objid = s as u16;
                map::store_cache_item(cache, s, slot, &it);
                given.insert(child, s as u16);
                queue.push((child, s as u16));
            }
        }
    }
    given
}
