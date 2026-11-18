//! `attack`: strike someone and watch what follows.
use super::*;

/// Attack one person and watch what follows.
/// attack <root> <slot> map=M npc=N hour=H [swings=6] [watch=1200] [out=dir] [kill] [move=x,y,z]
/// With out=dir the state just before the first blow is written there as a GAMEDAT folder
/// (the same start for the original).
pub(super) fn attack(root: &Path, slot: Option<u32>, args: &[String]) {
    let map: u16 = arg(args, "map").and_then(|v| v.parse().ok()).unwrap();
    let id: u16 = arg(args, "npc").and_then(|v| v.parse().ok()).unwrap();
    let hour: u32 = arg(args, "hour").and_then(|v| v.parse().ok()).unwrap_or(10);
    let swings: u32 = arg(args, "swings").and_then(|v| v.parse().ok()).unwrap_or(6);
    let watch: u32 = arg(args, "watch").and_then(|v| v.parse().ok()).unwrap_or(1200);
    let hp: i16 = arg(args, "hp").and_then(|v| v.parse().ok()).unwrap_or(200);
    let kill = args.iter().any(|a| a == "kill");
    let mut g = open(root, slot);
    g.put_avatar(map, (0, 0, 0));
    if arg(args, "hour") != Some("keep") {
        g.set_minutes(hour * 60);
    }
    g.put_avatar(map, (0, 0, 0));
    if let Some(p) = arg(args, "move") {
        // put them somewhere else first (to tell a place from a person)
        let p: Vec<i32> = p.split(',').filter_map(|x| x.parse().ok()).collect();
        g.world.move_to_map(&g.data, id, (p[0], p[1], p[2]));
    }
    let Some(n) = g.world.get(id).filter(|_| !g.world.is_void(id)) else {
        println!("map {map} npc {id}: not there");
        return;
    };
    let side = arg(args, "side").and_then(|v| {
        let p: Vec<i32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
        (p.len() == 2).then(|| (n.x + p[0], n.y + p[1], n.z))
    });
    let Some(at) = side.filter(|&a| g.world.fits(&g.data, AVATAR, a)).or_else(|| beside(&g, &n)) else {
        println!("map {map} npc {id}: no free spot beside");
        return;
    };
    g.put_avatar(map, at);
    let dir = dir_to(n.x - at.0, n.y - at.1);
    if let Some(a) = g.avatar.as_mut() {
        a.turn(&g.data, &mut g.world, dir);
    }
    if let Some(s) = g.stats.get_mut(AVATAR) {
        s.hp = hp;
    }
    let mut d = Drv { keep: true, ..Default::default() };
    let mut input = Input::default();
    d.run(&mut g, 2, &mut input);
    if let Some(o) = arg(args, "out") {
        write_dir(&g.snapshot(), &PathBuf::from(o));
        let (sx, sy) = project(n.x, n.y, n.z + 20);
        let (ax, ay) = project(at.0, at.1, at.2 + 20);
        println!("start: avatar {at:?} dir {dir} npc at {},{},{} screen offset {},{}", n.x, n.y, n.z, sx - ax, sy - ay);
    }
    let before: std::collections::HashMap<u16, (i32, i32)> = (2..256u16)
        .filter(|&i| !g.world.is_void(i))
        .filter_map(|i| g.world.get(i).map(|it| (i, (it.x, it.y))))
        .collect();
    let hp0 = g.stats.get(id).map_or(0, |s| s.hp);
    input.combat = true;
    d.run(&mut g, 1, &mut input);
    d.run(&mut g, 30, &mut Input::default());
    let (mut done, mut landed) = (0, 0);
    let mut t_end = d.t + 60 * 30;
    while d.t < t_end && !g.avatar_dead() {
        let st = g.stats.get(id).unwrap_or_default();
        // struck: hurt, or (the immortal lose no hit points) roused against the Avatar
        let hurt = st.hp < hp0 || st.dead || g.world.target(id).now == AVATAR;
        let more = if kill { !st.dead && done < 60 } else { !hurt && done < swings };
        if !more {
            break;
        }
        let mut i = Input::default();
        if g.avatar.as_ref().is_some_and(|a| !a.busy()) && !g.in_stasis() {
            // keep facing them, and follow if they walk off
            if let (Some(a), Some(nn)) = (g.world.get(AVATAR), g.world.get(id)) {
                let dd = dir_to(nn.x - a.x, nn.y - a.y);
                if (nn.x - a.x).abs().max((nn.y - a.y).abs()) > 112 {
                    i.dir = Some(dd);
                } else {
                    if let Some(av) = g.avatar.as_mut() {
                        av.turn(&g.data, &mut g.world, dd);
                    }
                    i.attack = true;
                    done += 1;
                }
            }
        }
        d.run(&mut g, 1, &mut i);
        let now = g.stats.get(id).unwrap_or_default();
        if now.hp < st.hp || (now.dead && !st.dead) || (g.world.target(id).now == AVATAR && landed == 0) {
            landed += 1;
        }
        if g.in_stasis() {
            t_end = t_end.min(d.t + 1);
        }
    }
    let t_hit = d.t;
    if args.iter().any(|a| a == "cast") && g.stats.get(id).is_some_and(|s| !s.dead) && landed > 0 {
        // what the original does on a blow: the struck one's going-into-combat script
        g.trigger_with(id, 17, &[0, 0]);
    }
    let mut died_at = None;
    for _ in 0..watch {
        d.run(&mut g, 1, &mut Input::default());
        if g.avatar_dead() && died_at.is_none() {
            died_at = Some(d.t - t_hit);
        }
    }
    let a = g.world.get(AVATAR).unwrap();
    let st = g.stats.get(id).unwrap_or_default();
    let av = g.stats.get(AVATAR).unwrap_or_default();
    let mut after: Vec<String> = vec![];
    for i in 2..256u16 {
        if g.world.is_void(i) {
            continue;
        }
        let Some(it) = g.world.get(i) else { continue };
        let dist = (it.x - a.x).abs().max((it.y - a.y).abs());
        let was = before.get(&i);
        let moved = was.map_or(9999, |w| (it.x - w.0).abs().max((it.y - w.1).abs()));
        let s = g.stats.get(i).unwrap_or_default();
        if i == id || g.world.in_combat(i) || (dist < 700 && (was.is_none() || moved > 300)) {
            after.push(format!(
                "{i}(s{} d{dist}{}{}{}{})",
                it.shape,
                if was.is_none() { " NEW" } else { "" },
                if moved > 300 && was.is_some() { " came" } else { "" },
                if g.world.in_combat(i) { " fights" } else { "" },
                if s.dead { " dead" } else { "" }
            ));
        }
    }
    let vm = g.scripts.as_ref().map(|s| &s.vm).unwrap();
    if args.iter().any(|a| a == "procs") {
        vm.describe().iter().filter(|l| !l.contains("native")).for_each(|l| println!("  {l}"));
        for i in [AVATAR, id, 32] {
            if let (Some(it), Some(s)) = (g.world.get(i), g.stats.get(i)) {
                println!(
                    "  {i}: at {},{},{} hp {} combat {} target {:?}",
                    it.x,
                    it.y,
                    it.z,
                    s.hp,
                    g.world.in_combat(i),
                    g.world.target(i).now
                );
            }
        }
    }
    println!(
        "map {map} npc {id} shape {}: swings {done} landed {landed} npc hp {hp0}->{} dead {} | avatar hp {hp}->{} dead {} (after {:?} ticks) stasis {} map {} | {} | errors {} | said: {}",
        n.shape,
        st.hp,
        st.dead,
        av.hp,
        g.avatar_dead(),
        died_at,
        g.in_stasis(),
        g.map(),
        after.join(" "),
        vm.errors,
        d.script.iter().map(|l| l.trim().to_string()).collect::<Vec<_>>().join(" / ")
    );
}
