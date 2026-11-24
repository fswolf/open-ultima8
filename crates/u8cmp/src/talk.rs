//! `talk`: talk to everyone on a map, answering by a policy, and note what was said.
use super::*;

/// Talk to everyone on a map: talk <root> <slot> map=M policy=first|last|cycle|silent [npc=N] [out=dir]
/// [flag=BIT,..] (story flags set first)
pub(super) fn talk(root: &Path, slot: Option<u32>, args: &[String]) {
    let map: u16 = arg(args, "map").and_then(|v| v.parse().ok()).unwrap_or(40);
    let policy = arg(args, "policy").unwrap_or("last").to_string();
    let only: Option<u16> = arg(args, "npc").and_then(|v| v.parse().ok());
    let hour: Option<u32> = arg(args, "hour").and_then(|v| v.parse().ok());
    let out = arg(args, "out").map(PathBuf::from);
    let mut g = open(root, slot);
    g.put_avatar(map, (0, 0, 0));
    let people: Vec<u16> = (2..256u16)
        .filter(|&id| g.world.get(id).is_some() && !g.world.is_void(id) && only.is_none_or(|o| o == id))
        .collect();
    for id in people {
        let mut g = open(root, slot);
        if let Some(h) = hour {
            g.put_avatar(map, (0, 0, 0));
            g.set_minutes(h * 60);
        }
        g.put_avatar(map, (0, 0, 0));
        let Some(n) = g.world.get(id).filter(|_| !g.world.is_void(id)) else {
            println!("npc {id:3}: not there at this hour");
            continue;
        };
        // a free spot beside them
        let mut spot = None;
        'find: for r in [96, 128, 160, 64] {
            for (dx, dy) in [(0, 1), (1, 0), (0, -1), (-1, 0), (1, 1), (-1, -1), (1, -1), (-1, 1)] {
                let at = (n.x + dx * r, n.y + dy * r, n.z);
                if g.world.fits(&g.data, AVATAR, at) {
                    spot = Some(at);
                    break 'find;
                }
            }
        }
        let Some(at) = spot else {
            println!("npc {id:3} s{:4} at {},{},{}: no free spot beside", n.shape, n.x, n.y, n.z);
            continue;
        };
        g.put_avatar(map, at);
        for f in arg(args, "flag")
            .unwrap_or("")
            .split(',')
            .filter_map(|f| usize::from_str_radix(f.trim_start_matches("0x"), 16).ok())
        {
            if let Some(vm) = g.vm() {
                vm.set_global_bits(f, 1, 1);
            }
        }
        let mut d = Drv { keep: true, bark_ticks: Some(25), cap: 40, ..Default::default() };
        match policy.as_str() {
            "first" => d.answer = Some(0),
            "cycle" => d.cycle = Some(0),
            "silent" => d.silent = true,
            _ => {}
        }
        let mut input = Input::default();
        d.run(&mut g, 30, &mut input); // arrival events settle
        let pre = d.script.len();
        let can = g.can_use(id);
        let used = g.use_item(id);
        let mut ticks = 0;
        let limit = if policy == "silent" { 9000 } else { 6000 };
        let mut quiet = 0;
        let mut seen = d.script.len();
        while ticks < limit {
            d.run(&mut g, 10, &mut input);
            ticks += 10;
            let busy = !d.barks.is_empty() || d.open_ask.is_some() || d.script.len() != seen || g.in_stasis();
            seen = d.script.len();
            quiet = if busy { 0 } else { quiet + 10 };
            if quiet >= 300 {
                break;
            }
        }
        let vm = g.scripts.as_ref().map(|s| &s.vm).unwrap();
        let (errors, missing, running) = (vm.errors, vm.missing.len(), vm.running());
        let acting = g.world.acting(id);
        let end = g.world.get(id);
        let lines = d.script.len() - pre;
        let mut flags = vec![];
        if ticks >= limit {
            flags.push("NEVER ENDS".to_string());
        }
        if errors > 0 || missing > 0 {
            flags.push(format!("script errors {errors} missing {missing}"));
        }
        if g.in_stasis() {
            flags.push("LEFT IN STASIS".into());
        }
        if g.avatar_dead() {
            flags.push("avatar dead".into());
        }
        if g.map() != map {
            flags.push(format!("now on map {}", g.map()));
        }
        if !used || !can {
            flags.push(format!("use {used} can {can}"));
        }
        println!(
            "npc {id:3} s{:4} at {},{},{}: {lines} lines, {} asks, {ticks} ticks, procs {running}, acting {acting:?}, moved {} {}",
            n.shape,
            n.x,
            n.y,
            n.z,
            d.asks,
            end.map_or(0, |e| (e.x - n.x).abs().max((e.y - n.y).abs())),
            flags.join("; ")
        );
        if let Some(o) = &out {
            std::fs::create_dir_all(o).unwrap();
            std::fs::write(o.join(format!("m{map}_npc{id}_{policy}.txt")), d.script.join("\n") + "\n").unwrap();
        }
    }
}
