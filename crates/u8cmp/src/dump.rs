//! A stored game as sorted text (clock, globals, people, what they carry, every map's things), to
//! diff against what the original saved after the same steps.
use super::*;

/// Flags that say how the game holds a thing just now, not what it is.
const TRANSIENT: u16 = 0x2000 | 0x0100 | 0x4000 | 0x8000;

/// One thing as a line: shape, frame, place, quality, flags, NPC and map numbers.
pub(super) fn item_line(it: &map::Item) -> String {
    format!(
        "s{:4} f{:3} at {:5},{:5},{:3} q{:5} fl{:04x} n{:3} m{:3}",
        it.shape,
        it.frame,
        it.x,
        it.y,
        it.z,
        it.quality,
        it.flags & !TRANSIENT,
        it.npcnum,
        it.mapnum
    )
}

/// A stored game as sorted text: clock, globals, people, what they carry, every map's things.
pub(super) fn dump(gd: &GameData) -> String {
    let mut o = String::new();
    writeln!(o, "clock {:?}", gd.clock).unwrap();
    if let Some(f) = &gd.flags {
        for (i, b) in f.iter().enumerate().filter(|(_, b)| **b != 0) {
            writeln!(o, "global {i:#05x} = {b:#04x}").unwrap();
        }
    }
    if let (Some(c), Some(n)) = (&gd.itemcach, &gd.npcdata) {
        for npc in 1..256 {
            let Some((m, it)) = map::actor_row(c, npc) else { continue };
            if it.shape == 0 {
                continue;
            }
            let st = map::npc_stats(n, npc).unwrap_or_default();
            let eq = map::npc_equipment(n, npc).unwrap_or_default();
            let worn = eq.iter().filter(|&&e| e != 0).count();
            writeln!(
                o,
                "npc {npc:3} map {m:3} {} | str {} dex {} int {} hp {} mana {} align {:#04x} flags {:02x?} worn {worn}",
                item_line(&gd_full_frame(gd, it)),
                st.strength,
                st.dexterity,
                st.intelligence,
                st.hit_points,
                st.mana,
                st.alignment,
                st.flags
            )
            .unwrap();
        }
        // What people carry, as a tree under each carrier.
        let all = map::load_cache_contents(c);
        let mut kids: BTreeMap<u16, Vec<&map::Contained>> = BTreeMap::new();
        for k in &all {
            kids.entry(k.parent).or_default().push(k);
        }
        fn tree(
            o: &mut String,
            kids: &BTreeMap<u16, Vec<&map::Contained>>,
            parent: u16,
            depth: usize,
            seen: &mut Vec<u16>,
        ) {
            let Some(list) = kids.get(&parent) else { return };
            let mut lines: Vec<(String, u16)> = list.iter().map(|k| (item_line_in(&k.item), k.id)).collect();
            lines.sort();
            for (l, id) in lines {
                if seen.contains(&id) || depth > 8 {
                    continue;
                }
                seen.push(id);
                writeln!(o, "{}{l}", "  ".repeat(depth)).unwrap();
                tree(o, kids, id, depth + 1, seen);
            }
        }
        for npc in 1..256u16 {
            if kids.contains_key(&npc) && map::actor_row(c, npc as usize).is_some_and(|(_, it)| it.shape != 0) {
                writeln!(o, "carried by npc {npc}:").unwrap();
                tree(&mut o, &kids, npc, 1, &mut vec![]);
            }
        }
    }
    for m in 0..256 {
        let l = gd.map_layers(m);
        if l.movable.is_empty() {
            continue;
        }
        let mut lines: Vec<String> = vec![];
        for (it, raw) in &l.movable {
            let mut s = item_line(it);
            let inside = map::parse_contents(raw);
            let mut sub: Vec<String> =
                inside.iter().map(|(d, c)| format!("\n      {}{}", "  ".repeat(*d), item_line_in(c))).collect();
            sub.sort();
            s.push_str(&sub.concat());
            lines.push(s);
        }
        lines.sort();
        writeln!(o, "map {m}: {} things", lines.len()).unwrap();
        for l in lines {
            writeln!(o, "  {l}").unwrap();
        }
    }
    o
}

/// An NPC with its whole frame (the high byte is in NPCDATA).
fn gd_full_frame(gd: &GameData, mut it: map::Item) -> map::Item {
    if let Some(d) = &gd.npcdata {
        it.frame |= map::npc_frame_high(d, it.npc as usize);
    }
    it
}

/// (An item in a bag: its place in the window instead of on the map.)
fn item_line_in(it: &map::Item) -> String {
    format!("s{:4} f{:3} q{:5} fl{:04x} win {},{}", it.shape, it.frame, it.quality, it.flags & !TRANSIENT, it.x, it.y)
}
