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
const VERSION: u32 = 1;

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
                }
            }
        }
        let slots = store_belongings(&mut npcs, &carriers);
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
        let rest: Vec<Option<&[u8]>> =
            (0..cache.len()).map(|i| if i == 0 { Some(&npcs[..]) } else { cache.get(i) }).collect();
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
    if let Some(mut k) = s.get("KERNEL.DAT").map(<[u8]>::to_vec) {
        if let Some(o) = u8data::ticks_offset(&k) {
            k[o..o + 4].copy_from_slice(&ticks.to_le_bytes());
            s.set("KERNEL.DAT", k);
        }
    }
    let map = worlds.first().map_or(0, |w| w.map());
    s.set_extra(&[("version", VERSION.to_string()), ("ticks", ticks.to_string()), ("map", map.to_string())]);
    s
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
                        if next_free >= 9216 {
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
