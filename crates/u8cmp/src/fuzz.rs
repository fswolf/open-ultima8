//! `fuzz` and `soak`: random play, short and long, looking for errors, stuck states and leaks.
use super::*;

/// Random walking, running, jumping and climbing from a save; prints anything that breaks the
/// rules (inside something solid, standing on nothing, stuck mid-action, odd height gains).
/// fuzz <root> <slot> seed=N steps=N [at=x,y,z,map] [nojump] [verbose]
pub(super) fn fuzz(root: &Path, slot: Option<u32>, args: &[String]) {
    let mut seed: u64 = arg(args, "seed").and_then(|v| v.parse().ok()).unwrap_or(1);
    let steps: u32 = arg(args, "steps").and_then(|v| v.parse().ok()).unwrap_or(500);
    let verbose = args.iter().any(|a| a == "verbose");
    let nojump = args.iter().any(|a| a == "nojump");
    let at: Option<Vec<i32>> = arg(args, "at").map(|v| v.split(',').filter_map(|n| n.parse().ok()).collect());
    let mut rnd = move |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    let start = |g: &mut Game| {
        if let Some(a) = &at {
            g.put_avatar(a.get(3).map_or(g.map(), |&m| m as u16), (a[0], a[1], a[2]));
        }
    };
    let mut g = open(root, slot);
    start(&mut g);
    let mut d = Drv::default();
    let (mut deaths, mut odd) = (0, 0);
    let mut still = 0u32;
    let mut last_ok = (0, 0, 0);
    let pos = |g: &Game| g.world.get(AVATAR).map(|i| (i.x, i.y, i.z)).unwrap_or((0, 0, 0));
    for n in 0..steps {
        let before = pos(&g);
        let hp0 = g.stats.get(AVATAR).map_or(0, |s| s.hp);
        let map0 = g.map();
        let hanging0 = g.avatar.as_ref().is_some_and(|a| a.hanging());
        let dir = rnd(8) as usize;
        let mut input = Input::default();
        let kind = if nojump { rnd(2) } else { rnd(8) };
        let what = match kind {
            0 => {
                input.dir = Some(dir);
                d.run(&mut g, 5 + rnd(40) as u32, &mut input);
                format!("walk {dir}")
            }
            1 => {
                input.dir = Some(dir);
                input.run = true;
                d.run(&mut g, 5 + rnd(40) as u32, &mut input);
                format!("run {dir}")
            }
            2 => {
                input.jump = true;
                d.run(&mut g, 1, &mut input);
                "jump up".into()
            }
            3 | 4 => {
                input.dir = Some(dir);
                input.jump = true;
                d.run(&mut g, 1, &mut input);
                format!("standing jump {dir}")
            }
            5 => {
                input.dir = Some(dir);
                d.run(&mut g, 3 + rnd(10) as u32, &mut input);
                input.jump = true;
                d.run(&mut g, 1, &mut input);
                format!("walking jump {dir}")
            }
            6 => {
                input.dir = Some(dir);
                input.run = true;
                d.run(&mut g, 3 + rnd(12) as u32, &mut input);
                input.jump = true;
                d.run(&mut g, 1, &mut input);
                format!("running jump {dir}")
            }
            _ => {
                input.dir = Some(dir);
                input.careful = true;
                d.run(&mut g, 5 + rnd(30) as u32, &mut input);
                format!("careful {dir}")
            }
        };
        let mut input = Input::default();
        // let the action play out
        let mut waited = 0;
        while waited < 600 && g.avatar.as_ref().is_some_and(|a| a.busy()) && !g.avatar_dead() {
            d.run(&mut g, 1, &mut input);
            waited += 1;
        }
        d.run(&mut g, 20, &mut input); // falls start and end
        while waited < 600 && g.avatar.as_ref().is_some_and(|a| a.busy()) && !g.avatar_dead() {
            d.run(&mut g, 1, &mut input); // (and the landing after)
            waited += 1;
        }
        let mut hung = String::new();
        if g.avatar.as_ref().is_some_and(|a| a.hanging()) {
            let facing = g.avatar.as_ref().map_or(0, |a| a.dir);
            let h = pos(&g);
            let choice = rnd(3);
            input.dir = Some(if choice == 0 { (facing + 4) % 8 } else { facing });
            if choice == 2 {
                input.dir = None;
                input.jump = true;
            }
            d.run(&mut g, 2, &mut input);
            let mut input = Input::default();
            let mut w = 0;
            while w < 600 && g.avatar.as_ref().is_some_and(|a| a.busy() || a.hanging()) && !g.avatar_dead() {
                d.run(&mut g, 1, &mut input);
                w += 1;
            }
            d.run(&mut g, 40, &mut input);
            hung = format!(" hung at {h:?} then {}", ["let go", "pulled up", "jumped up"][choice as usize]);
            if g.avatar.as_ref().is_some_and(|a| a.hanging()) {
                hung += " STILL HANGING";
            }
        }
        let after = pos(&g);
        let hp1 = g.stats.get(AVATAR).map_or(0, |s| s.hp);
        let a = g.avatar.as_ref();
        let busy = a.is_some_and(|a| a.busy());
        let floor = g.world.floor_under(AVATAR);
        let fits = g.world.fits(&g.data, AVATAR, after);
        let mut notes = vec![];
        if g.map() != map0 {
            notes.push(format!("map {} -> {}", map0, g.map()));
        } else {
            if busy {
                notes.push("STILL BUSY after 600 ticks".into());
            }
            if !fits && !g.avatar_dead() {
                if floor == Some(after.2) {
                    let a = g.world.get(AVATAR).unwrap();
                    let mut inside = vec![];
                    for i in 0..g.world.len() {
                        let e = g.world.entry(i);
                        if e.gone
                            || e.void
                            || e.parent.is_some()
                            || g.world.id(i) == AVATAR
                            || !e.traits.solid
                            || e.traits.editor
                            || e.traits.egg
                        {
                            continue;
                        }
                        if e.x0 < a.x
                            && a.x - 64 < e.it.x
                            && e.y0 < a.y
                            && a.y - 64 < e.it.y
                            && e.it.z < a.z + 40
                            && a.z < e.z1
                        {
                            inside.push(format!(
                                "{}s{}{}",
                                if e.traits.actor { "actor " } else { "" },
                                e.it.shape,
                                if e.traits.actor {
                                    format!(" #{}", g.world.id(i))
                                } else {
                                    format!(" z{}..{}", e.it.z, e.z1)
                                }
                            ));
                        }
                    }
                    if inside.iter().any(|s| !s.starts_with("actor")) {
                        notes.push(format!("INSIDE {inside:?}"));
                    } else if !inside.is_empty() {
                        notes.push(format!("overlaps {inside:?}"));
                    }
                } else if floor.is_none() && after.2 == 0 {
                    notes.push("VOID (z 0, nothing under)".into());
                } else if floor == Some(after.2 - 1) {
                    notes.push("in water".into());
                } else {
                    notes.push(format!("UNSUPPORTED (floor {floor:?})"));
                }
            }
            let dz = after.2 - before.2;
            if dz > 72 && !hanging0 {
                notes.push(format!("ROSE {dz}"));
            }
            if (kind == 0 || kind == 7) && dz > 8 {
                notes.push(format!("walk rose {dz}"));
            }
            if kind == 2 && (after.0, after.1) != (before.0, before.1) && hung.is_empty() {
                notes.push("jump up moved sideways".into());
            }
            let far = (after.0 - before.0).abs().max((after.1 - before.1).abs());
            if far > 700 {
                notes.push(format!("moved {far} in one action"));
            }
            if hp1 < hp0 {
                notes.push(format!("hp {hp0} -> {hp1} (dz {dz})"));
            }
        }
        if g.avatar_dead() {
            notes.push("DIED".into());
        }
        if !hung.is_empty() {
            notes.push(hung.trim().to_string());
        }
        if !notes.is_empty() || verbose {
            odd += !notes.is_empty() as u32;
            println!("#{n} map {map0} {what}: {before:?} -> {after:?} {}", notes.join("; "));
        }
        still = if after == before && !g.avatar_dead() { still + 1 } else { 0 };
        let mut reset = false;
        if still >= 12 {
            // really stuck? every walk and jump from here
            let mut free = false;
            for k in 0..17 {
                let mut i = Input::default();
                if k < 8 {
                    i.dir = Some(k);
                    d.run(&mut g, 30, &mut i);
                } else if k < 16 {
                    i.dir = Some(k - 8);
                    i.jump = true;
                    d.run(&mut g, 1, &mut i);
                    d.run(&mut g, 90, &mut Input::default());
                } else {
                    i.jump = true;
                    d.run(&mut g, 1, &mut i);
                    d.run(&mut g, 90, &mut Input::default());
                }
                if pos(&g) != after {
                    free = true;
                    break;
                }
            }
            if !free {
                odd += 1;
                println!("#{n} map {map0} STUCK at {after:?}: no walk or jump moves it (last free spot {last_ok:?})");
                reset = true;
            }
            still = 0;
        } else if after != before {
            last_ok = before;
        }
        let void = floor.is_none() && after.2 == 0 && !g.avatar_dead();
        if void {
            reset = true; // one report per way out of the world
        }
        if reset && !g.avatar_dead() {
            g = open(root, slot);
            start(&mut g);
            d = Drv::default();
        }
        if g.avatar_dead() || after == (0, 0, 0) {
            deaths += 1;
            g = open(root, slot);
            start(&mut g);
            d = Drv::default();
        }
    }
    let vm = g.scripts.as_ref().map(|s| &s.vm).unwrap();
    println!(
        "fuzz done: {steps} actions, {odd} noted, {deaths} deaths, script errors {} missing {:?}",
        vm.errors, vm.missing
    );
}

/// A long session: wander, talk, use things, save and load. soak <root> <slot> ticks=N seed=N [every=3000] [saveslot=30]
pub(super) fn soak(root: &Path, slot: Option<u32>, args: &[String]) {
    let total: u32 = arg(args, "ticks").and_then(|v| v.parse().ok()).unwrap_or(27000);
    let every: u32 = arg(args, "every").and_then(|v| v.parse().ok()).unwrap_or(3000);
    let save_slot: u32 = arg(args, "saveslot").and_then(|v| v.parse().ok()).unwrap_or(30);
    let mut seed: u64 = arg(args, "seed").and_then(|v| v.parse().ok()).unwrap_or(1);
    let mut rnd = move |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    let mut g = open(root, slot);
    if let Some(h) = arg(args, "hour").and_then(|v| v.parse::<u32>().ok()) {
        g.set_minutes(h * 60);
    }
    let mut d = Drv { bark_ticks: Some(40), ..Default::default() };
    let (mut next, mut loads, mut errs_total) = (every, 0u32, 0u32);
    let mut clock = std::time::Instant::now();
    let mut still: std::collections::HashMap<u16, ((i32, i32, i32), u32)> = Default::default();
    println!("start: map {} minutes {}", g.map(), g.minutes());
    while d.t < total {
        let mut input = Input::default();
        match rnd(10) {
            0..=5 => {
                input.dir = Some(rnd(8) as usize);
                input.run = rnd(3) == 0;
                d.run(&mut g, 10 + rnd(60) as u32, &mut input);
            }
            6 => d.run(&mut g, 30 + rnd(90) as u32, &mut input),
            7 | 8 => {
                // use the nearest person or scripted thing in reach
                let ids: Vec<u16> = g
                    .world
                    .objects()
                    .filter(|&id| {
                        id != AVATAR
                            && !g.world.is_void(id)
                            && g.world.container(id).is_none()
                            && g.near_avatar(id, 200, 64)
                    })
                    .collect();
                if !ids.is_empty() {
                    let id = ids[rnd(ids.len() as u64) as usize];
                    if g.can_use(id) {
                        g.use_item(id);
                    }
                }
                d.run(&mut g, 60, &mut input);
            }
            _ => {
                input.combat = rnd(6) == 0;
                d.run(&mut g, 20, &mut input);
            }
        }
        if g.avatar_dead() {
            println!("[t{}] the Avatar died; loading the last save", d.t);
            g = open(root, if loads > 0 { Some(save_slot) } else { slot });
            d.barks.clear();
            d.open_ask = None;
        }
        if d.t >= next {
            next += every;
            let ms = clock.elapsed().as_millis();
            clock = std::time::Instant::now();
            let vm = g.scripts.as_ref().map(|s| &s.vm).unwrap();
            let (procs, errs) = (vm.running(), vm.errors);
            // people who haven't moved a unit while acting (looping on the spot)
            let mut loopers = vec![];
            for id in 2..256u16 {
                if let Some(it) = g.world.get(id).filter(|_| !g.world.is_void(id)) {
                    let acting = g.world.is_acting(id);
                    let e = still.entry(id).or_insert(((it.x, it.y, it.z), 0));
                    if e.0 == (it.x, it.y, it.z) && acting {
                        e.1 += 1;
                        if e.1 >= 3 {
                            loopers.push(id);
                        }
                    } else {
                        *e = ((it.x, it.y, it.z), 0);
                    }
                }
            }
            let r = if g.in_stasis() || g.scripting() && d.open_ask.is_some() {
                Err("in a scene".to_string())
            } else {
                g.save(root, save_slot, "soak").map(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
            };
            let a = g.world.get(AVATAR).unwrap();
            println!(
                "[t{}] {} ms per {every} ticks, map {} at {},{},{} day {} {:02}:{:02} objects {} procs {procs} errors {errs} fighters {} rss {} kB stasis {} save {:?} acting-in-place {:?}",
                d.t,
                ms,
                g.map(),
                a.x,
                a.y,
                a.z,
                g.minutes() / 1440 + 1,
                g.minutes() / 60 % 24,
                g.minutes() % 60,
                g.world.objects().count(),
                g.world.fighters().len(),
                rss_kb(),
                g.in_stasis(),
                r,
                loopers
            );
            errs_total += errs;
            if r.is_ok() && (next / every).is_multiple_of(2) {
                g = open(root, Some(save_slot));
                d.barks.clear();
                d.open_ask = None;
                loads += 1;
                still.clear();
            }
        }
    }
    println!("soak done: {} ticks, {loads} loads, script errors seen {errs_total}", d.t);
}
