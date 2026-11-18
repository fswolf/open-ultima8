//! `people`: everyone on every map at an hour.
use super::*;

/// Everyone on every map at an hour: people <root> <slot> hour=H
pub(super) fn people(root: &Path, slot: Option<u32>, args: &[String]) {
    let hour: u32 = arg(args, "hour").and_then(|v| v.parse().ok()).unwrap_or(10);
    for map in 0..256u16 {
        let mut g = open(root, slot);
        g.put_avatar(map, (0, 0, 0));
        g.set_minutes(hour * 60);
        g.put_avatar(map, (0, 0, 0));
        for id in 2..256u16 {
            let Some(it) = g.world.get(id).filter(|_| !g.world.is_void(id)) else { continue };
            let st = g.stats.get(id).unwrap_or_default();
            println!(
                "map {map:3} npc {id:3} shape {:4} at {:5},{:5},{:3} hp {:3} str {:2} dex {:2} align {:x}/{:x} dead {} immortal {} fl {:04x} spot {}",
                it.shape, it.x, it.y, it.z, st.hp, st.str, st.dex, st.alignment, st.enemies, st.dead, st.immortal, it.flags,
                beside(&g, &it).is_some()
            );
        }
    }
}
