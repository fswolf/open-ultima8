//! `steal`: take things near someone and watch what follows.
use super::*;

/// Take things near someone and watch what follows.
/// steal <root> <slot> map=M npc=N [hour=H] [list] [shape=S,..] [item=ID,..] [times=K] [how=take|floor]
pub(super) fn steal(root: &Path, slot: Option<u32>, args: &[String]) {
    let num = |k: &str, d: i32| arg(args, k).and_then(|v| v.parse().ok()).unwrap_or(d);
    let list = |k: &str| -> Vec<i32> {
        arg(args, k).map_or(vec![], |v| v.split(',').filter_map(|x| x.parse().ok()).collect())
    };
    let map = num("map", 0) as u16;
    let id = num("npc", 0) as u16;
    let (r, times, gap, watch) = (num("r", 600), num("times", 1), num("gap", 300) as u32, num("watch", 1500) as u32);
    let floor = arg(args, "how") == Some("floor");
    let mut g = open(root, slot);
    g.put_avatar(map, (0, 0, 0));
    if arg(args, "hour") != Some("keep") {
        g.set_minutes(num("hour", 10) as u32 * 60);
    }
    g.put_avatar(map, (0, 0, 0));
    let mv = list("move");
    if mv.len() == 3 {
        g.world.move_to_map(&g.data, id, (mv[0], mv[1], mv[2]));
    }
    let br = list("bring");
    if br.len() == 4 {
        // put a thing somewhere else first (to have one of the kinds a script looks for at hand)
        g.world.move_to_map(&g.data, br[0] as u16, (br[1], br[2], br[3]));
    }
    if args.iter().any(|a| a == "room") {
        // make room in the pack: the pack carried inside it goes, with what's in it
        if let Some(pack) = g.backpack() {
            for o in g.world.contents(pack) {
                if g.world.get(o).is_some_and(|it| it.shape == 529) {
                    g.world.take_contents(o);
                    g.world.remove(o);
                }
            }
        }
    }
    let Some(n) = g.world.get(id).filter(|_| !g.world.is_void(id)) else {
        println!("map {map} npc {id}: not there");
        return;
    };
    let dist = |g: &Game, o: u16| g.world.get(o).map_or(9999, |it| (it.x - n.x).abs().max((it.y - n.y).abs()));
    let mut near: Vec<u16> = g.world.area(n.x, n.y, r).into_iter().filter(|&o| loose(&g, o)).collect();
    near.sort_by_key(|&o| dist(&g, o));
    if args.iter().any(|a| a == "list") {
        println!(
            "map {map} npc {id} shape {} at {},{},{}: {} loose things within {r}",
            n.shape,
            n.x,
            n.y,
            n.z,
            near.len()
        );
        for &o in &near {
            let it = g.world.get(o).unwrap();
            let tf = g.data.typeflags.get(it.shape);
            println!(
                "  item {o:5} shape {:4} f{:3} q{:5} at {:5},{:5},{:3} dist {:4} family {} weight {} fl {:04x}",
                it.shape,
                it.frame,
                it.quality,
                it.x,
                it.y,
                it.z,
                dist(&g, o),
                tf.family,
                tf.weight,
                it.flags
            );
        }
        return;
    }
    let (shapes, ids) = (list("shape"), list("item"));
    let mut picks: Vec<u16> = if !ids.is_empty() {
        ids.iter().map(|&i| i as u16).collect()
    } else {
        near.iter()
            .copied()
            .filter(|&o| shapes.is_empty() || shapes.contains(&(g.world.get(o).unwrap().shape as i32)))
            .collect()
    };
    if picks.is_empty() {
        println!("map {map} npc {id}: nothing to take");
        return;
    }
    // Somewhere to stand from which the first thing can be handled (and as many more as possible).
    let first = g.world.get(picks[0]).unwrap();
    let st = list("stand");
    let mut spots: Vec<(i32, i32, i32)> = vec![];
    if st.len() == 3 {
        spots.push((st[0], st[1], st[2]));
    } else {
        for z in [n.z, first.z] {
            for rr in [64, 80, 96, 112, 128, 144, 48].into_iter().filter(|&r| r <= num("ring", 144)) {
                for k in 0..16 {
                    let a = k as f32 * std::f32::consts::PI / 8.0;
                    let at = (first.x + (a.cos() * rr as f32) as i32, first.y + (a.sin() * rr as f32) as i32, z);
                    if g.world.fits(&g.data, AVATAR, at) {
                        spots.push(at);
                    }
                }
            }
        }
        // (a little way off the person, so neither hides the other on screen)
        let off = num("off", 150);
        spots.sort_by_key(|s| ((s.0 - n.x).abs().max((s.1 - n.y).abs()) - off).abs());
    }
    let mut stand = None;
    let mut second = None;
    // (a thing that can't be picked up from anywhere: stand by it anyway and try)
    let fallback = spots.first().copied();
    for at in spots {
        g.put_avatar(map, at);
        if g.can_pick_up(picks[0]).is_ok() {
            second = second.or(Some(at));
            if aim(&mut g, picks[0]).is_some() && aim(&mut g, AVATAR).is_some() {
                stand = Some(at);
                break;
            }
        }
    }
    let stand = stand.or(second).or(fallback);
    if let Some(at) = stand {
        g.put_avatar(map, at);
    }
    let Some(at) = stand else {
        println!("map {map} npc {id}: nowhere to stand by item {} at {},{},{}", picks[0], first.x, first.y, first.z);
        return;
    };
    if ids.is_empty() {
        picks.retain(|&o| g.can_pick_up(o).is_ok());
        if floor {
            picks.truncate(1);
        }
    }
    let dir = dir_to(first.x - at.0, first.y - at.1);
    if let Some(a) = g.avatar.as_mut() {
        a.turn(&g.data, &mut g.world, dir);
    }
    if let Some(s) = g.stats.get_mut(AVATAR) {
        s.hp = num("hp", 200) as i16;
    }
    let mut d = Drv { keep: true, stamp: true, silent: arg(args, "answer").is_none(), ..Default::default() };
    d.answer = arg(args, "answer").and_then(|v| v.parse().ok());
    d.run(&mut g, 2, &mut Input::default());
    let npc_d = (n.x - at.0).abs().max((n.y - at.1).abs());
    println!(
        "map {map} npc {id} shape {}: avatar {at:?} dir {dir}, npc at {},{},{} ({npc_d} away), pack {:?}",
        n.shape,
        n.x,
        n.y,
        n.z,
        g.backpack()
    );
    for &o in picks.iter().take(times.max(1) as usize) {
        let it = g.world.get(o).unwrap();
        let (off, to) = (offset(&g, o), aim(&mut g, o));
        println!(
            "  thing {o} shape {} f{} at {},{},{} screen offset from the feet {off:?} point at {to:?}",
            it.shape, it.frame, it.x, it.y, it.z
        );
    }
    println!("  avatar: point at {:?}; {} things in reach", aim(&mut g, AVATAR), picks.len());
    // (floor moves: its place, and one a little way off that is free)
    let home0 = (first.x, first.y, first.z);
    let mut away = None;
    if floor {
        let mut ways =
            [(40, -40), (-40, 40), (48, 48), (-48, -48), (64, 0), (0, 64), (-64, 0), (0, -64), (32, 32), (-32, -32)];
        ways.sort_by_key(|w| (home0.0 + w.0 - at.0).abs().max((home0.1 + w.1 - at.1).abs()));
        for (dx, dy) in ways {
            let p = (home0.0 + dx, home0.1 + dy, home0.2);
            if g.world.fits(&g.data, picks[0], p) {
                let at = aim(&mut g, picks[0]).map(|a| (a.0 + (dx - dy) / 4, a.1 + (dx + dy) / 8));
                // (not onto the Avatar's own picture: that would put it in the pack)
                if at.is_some_and(|a| (138..182).contains(&a.0) && (66..128).contains(&a.1)) {
                    continue;
                }
                away = Some(p);
                println!("  floor move to {p:?}: drag to {at:?}");
                break;
            }
        }
    }
    if let Some(o) = arg(args, "out") {
        write_dir(&g.snapshot(), &PathBuf::from(o));
    }
    if let Some(p) = arg(args, "shot") {
        shot(&mut g, &mut Painter::new(), &mut SpriteCache::unscaled(), p);
    }
    let before: std::collections::HashMap<u16, (i32, i32)> = (2..256u16)
        .filter(|&i| !g.world.is_void(i))
        .filter_map(|i| g.world.get(i).map(|it| (i, (it.x, it.y))))
        .collect();
    let globals0 = g.scripts.as_ref().map(|s| s.vm.globals.clone()).unwrap_or_default();
    d.t = 0;
    // (shots=PREFIX,EVERY: a picture every EVERY ticks; frames=ID,..: each one's look as it changes)
    let shots =
        arg(args, "shots").and_then(|v| v.rsplit_once(',')).map(|(p, e)| (p.to_string(), e.parse().unwrap_or(0)));
    let frames: Vec<u16> = list("frames").into_iter().map(|f| f as u16).collect();
    let (mut painter, mut cache, mut seen) =
        (Painter::new(), SpriteCache::unscaled(), std::collections::HashMap::new());
    for k in 0..times.max(1) as usize {
        let o = picks[k % picks.len()];
        let it = g.world.get(o).unwrap();
        let res = if floor {
            // along the floor: to and fro between its place and one a little way off
            let to = if k % 2 == 0 { away } else { Some(home0) };
            match to {
                Some(p) => g.move_item(o, Place::Map(p)).map(|_| format!("moved to {p:?}")),
                None => Err("no room to move it"),
            }
        } else {
            g.stow(o).map(|_| "into the pack".to_string())
        };
        d.script.push(format!(
            "  [{:.1}s] TAKE {} (shape {}): {}",
            d.t as f32 / 30.0,
            o,
            it.shape,
            res.unwrap_or_else(|e| format!("REFUSED: {e}"))
        ));
        let wait = if k + 1 == times.max(1) as usize { watch } else { gap };
        for _ in 0..wait {
            d.run(&mut g, 1, &mut Input::default());
            if let Some((pre, _)) = shots.as_ref().filter(|s| s.1 > 0 && d.t.is_multiple_of(s.1)) {
                shot(&mut g, &mut painter, &mut cache, &format!("{pre}{:05}.png", d.t));
            }
            for &w in &frames {
                let now = g
                    .world
                    .get(w)
                    .filter(|_| !g.world.is_void(w))
                    .map(|it| (it.shape, it.frame, it.flags, (it.x, it.y, it.z)));
                if seen.get(&w) != Some(&now) {
                    let act = g.world.acting(w);
                    let shown = g.world.index(w).is_some_and(|i| g.world.entry(i).shown());
                    println!(
                        "  [t{}] {w}: {now:?} shown {shown} acting {act:?} last {} camera {:?}",
                        d.t,
                        g.world.last_anim(w),
                        g.camera()
                    );
                    seen.insert(w, now);
                }
            }
            if g.avatar_dead() {
                break;
            }
        }
        if g.avatar_dead() {
            d.script.push(format!("  [{:.1}s] the Avatar is dead", d.t as f32 / 30.0));
            d.run(&mut g, 60, &mut Input::default());
            break;
        }
    }
    let a = g.world.get(AVATAR).unwrap();
    let av = g.stats.get(AVATAR).unwrap_or_default();
    let mut after: Vec<String> = vec![];
    for i in 2..256u16 {
        if g.world.is_void(i) {
            continue;
        }
        let Some(it) = g.world.get(i) else { continue };
        let dd = (it.x - a.x).abs().max((it.y - a.y).abs());
        let was = before.get(&i);
        let moved = was.map_or(9999, |w| (it.x - w.0).abs().max((it.y - w.1).abs()));
        let s = g.stats.get(i).unwrap_or_default();
        if i == id || g.world.in_combat(i) || (dd < 700 && (was.is_none() || moved > 100)) {
            after.push(format!(
                "{i}(s{} d{dd}{}{}{}{})",
                it.shape,
                if was.is_none() { " NEW" } else { "" },
                if moved > 100 && was.is_some() { " moved" } else { "" },
                if g.world.in_combat(i) { " fights" } else { "" },
                if s.dead { " dead" } else { "" }
            ));
        }
    }
    let vm = g.scripts.as_ref().map(|s| &s.vm).unwrap();
    let changed: Vec<String> = vm
        .globals
        .iter()
        .zip(globals0.iter())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, (a, b))| format!("{i:#x}:{b:02x}->{a:02x}"))
        .collect();
    println!(
        "  end: avatar hp {} dead {} at {},{},{} map {} stasis {} | people: {} | globals {} | errors {}",
        av.hp,
        g.avatar_dead(),
        a.x,
        a.y,
        a.z,
        g.map(),
        g.in_stasis(),
        after.join(" "),
        changed.join(" "),
        vm.errors
    );
    for l in &d.script {
        println!("{l}");
    }
}
