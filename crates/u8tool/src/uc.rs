//! Usecode: list classes, disassemble, run an event headless, smoke-test a whole map.
use crate::cli::{save, Args};
use anyhow::{anyhow, bail, Result};
use std::collections::BTreeMap;
use std::path::Path;
use u8data::{usecode as fmt, GameData};
use u8engine::usecode::{event_class, intrinsics, UiEvent, Usecode, Vm, WorldHost};
use u8engine::{stats::Stats, ObjId, World};
use u8render::{project, upscale, Canvas, DrawOpts, Painter, SpriteCache};

pub fn run(a: &Args, gd: &GameData) -> Result<()> {
    let (uc, flags) = u8data::load_usecode(a.data())?;
    match a.get(2, "list").as_str() {
        "list" => list(&uc),
        "dis" => dis(a, &uc),
        "run" => run_event(a, gd, &uc, flags.as_deref()),
        "smoke" => smoke(a, gd, &uc, flags.as_deref()),
        "live" => live(a),
        c => bail!("unknown uc command {c:?}"),
    }
}

fn events_of(uc: &Usecode, class: u32, at: Option<usize>) -> Vec<&'static str> {
    (0..fmt::EVENT_COUNT)
        .filter(|&e| uc.event(class, e).is_some_and(|o| at.is_none_or(|p| p == o)))
        .map(|e| fmt::EVENTS[e])
        .collect()
}

fn list(uc: &Usecode) -> Result<()> {
    for c in 0..uc.class_count() as u32 {
        if let Some(name) = uc.class_name(c) {
            say!("{c:5} {name:9} {}", events_of(uc, c, None).join(" "));
        }
    }
    Ok(())
}

fn dis(a: &Args, uc: &Usecode) -> Result<()> {
    let c: u32 = a.num(3, "0", "class")?;
    let code = uc.class(c).ok_or_else(|| anyhow!("class {c} has no code"))?;
    let which = a.get(4, "all");
    let starts: Vec<usize> = if let Some(h) = which.strip_prefix('@') {
        vec![usize::from_str_radix(h.trim_start_matches("0x"), 16)?]
    } else if which == "all" {
        // Functions follow each other: each starts right after the previous one's `end` (0x79).
        let mut v = vec![];
        let mut p = fmt::BASE + fmt::EVENT_COUNT * 4;
        while p < code.len() {
            v.push(p);
            while p < code.len() && code[p] != 0x79 {
                p += 1 + fmt::operand_len(code, p).unwrap_or(0);
            }
            p += 1;
        }
        v
    } else {
        let e = fmt::EVENTS.iter().position(|n| *n == which).or(which.parse().ok());
        vec![e.and_then(|e| uc.event(c, e)).ok_or_else(|| anyhow!("no such event {which}"))?]
    };
    say!("class {c} {}", uc.class_name(c).unwrap_or_default());
    for s0 in starts {
        say!("\n; function @{s0:#x} {}", events_of(uc, c, Some(s0)).join(", "));
        print!("{}", fmt::disasm(code, s0, intrinsics::name));
    }
    Ok(())
}

/// Answer UI events the way a player in a hurry would: skip speech, pick `answer(options)`.
/// Stand in for the player (and, without a `Game`, for the camera): speech is read at once,
/// questions get `answer`'s pick, camera glides, sprite effects and flights end at once. Walks are left alone (they need a
/// `Game`); scripts waiting on one count as still running.
fn auto_ui(vm: &mut Vm, mut answer: impl FnMut(&[String]) -> usize, mut log: impl FnMut(&UiEvent)) {
    for r in std::mem::take(&mut vm.requests) {
        use u8engine::usecode::Request;
        if let Request::CameraScroll { pid, .. } | Request::Sprite { pid, .. } | Request::Hurl { pid, .. } = r {
            vm.finish(pid, 0);
        }
    }
    while let Some(e) = vm.events.pop_front() {
        log(&e);
        match e {
            UiEvent::Bark { pid, .. } | UiEvent::Read { pid, .. } | UiEvent::Target { pid } => vm.finish(pid, 0),
            UiEvent::Slider { pid, min, .. } => vm.finish(pid, min as u16 as u32),
            UiEvent::Ask { pid, options, .. } => {
                let pick = answer(&options).min(options.len().saturating_sub(1));
                vm.answer(pid, &options, pick);
            }
            _ => {}
        }
    }
}

fn missing_summary(m: impl IntoIterator<Item = (u16, u32)>) -> String {
    m.into_iter().map(|(k, v)| format!("{k:#05x}×{v}")).collect::<Vec<_>>().join(" ")
}

fn run_event(a: &Args, gd: &GameData, uc: &Usecode, flags: Option<&[u8]>) -> Result<()> {
    let m: u16 = a.num(3, "40", "map")?;
    let mode = a.get(4, "");
    let ev = match a.get(6, "look").as_str() {
        "look" => fmt::event::LOOK,
        "use" => fmt::event::USE,
        "hatch" => fmt::event::HATCH,
        e => crate::cli::num(e, "event")?,
    };
    let max_z = a.cut(7)?;
    let mut answers: Vec<usize> = a.rest(7).iter().filter_map(|s| s.parse().ok()).collect();
    answers.reverse();
    let mut world = World::build(gd, m);
    let mut painter = Painter::new();
    let id: ObjId = match mode.as_str() {
        "id" => a.num(5, "0", "id")?,
        "s" => {
            // The instance of that shape nearest the Avatar.
            let shape: u32 = a.num(5, "0", "shape")?;
            let av = world.get(1).map_or((0, 0), |a| (a.x, a.y));
            world
                .area(av.0, av.1, 4096)
                .into_iter()
                .filter(|&i| world.get(i).is_some_and(|it| it.shape == shape))
                .min_by_key(|&i| world.get(i).map_or(i32::MAX, |it| (it.x - av.0).abs() + (it.y - av.1).abs()))
                .ok_or_else(|| anyhow!("no shape {shape} near the Avatar"))?
        }
        _ => {
            let (sx, sy): (i32, i32) = (a.num(4, "0", "x")?, a.num(5, "0", "y")?);
            painter
                .pick(gd, &mut world, sx, sy, DrawOpts { max_z, ..Default::default() })
                .ok_or_else(|| anyhow!("nothing at ({sx}, {sy})"))?
        }
    };
    let it = world.get(id).ok_or_else(|| anyhow!("no item {id}"))?;
    say!(
        "item {id}: shape {} ({}) frame {} at ({}, {}, {})",
        it.shape,
        uc.class_name(it.shape).unwrap_or_default(),
        it.frame,
        it.x,
        it.y,
        it.z
    );
    let mut vm = Vm::new(uc, flags);
    let mut stats = Stats::load(gd.npcdata.as_deref());
    let Some(pid) =
        vm.item_event(uc, &WorldHost { world: &mut world, data: gd, stats: &mut stats, others: None }, id, ev)
    else {
        bail!("no handler for event {ev}")
    };
    say!("started pid {pid}");
    for t in 0..3000 {
        vm.tick(uc, &mut WorldHost { world: &mut world, data: gd, stats: &mut stats, others: None });
        auto_ui(
            &mut vm,
            |options| {
                let pick = answers.pop().unwrap_or(0);
                say!("[t{t}] ask {options:?} -> {:?}", options.get(pick.min(options.len().saturating_sub(1))));
                pick
            },
            |e| match e {
                UiEvent::Bark { item, text, .. } => say!("[t{t}] {item} says: {text}"),
                UiEvent::Read { kind, text, .. } => say!("[t{t}] read {kind:?}: {text}"),
                UiEvent::Music(n) => say!("[t{t}] music {n}"),
                UiEvent::Sfx { sfx, item, looping } => {
                    say!("[t{t}] sfx {sfx} at {item}{}", if *looping { " (loop)" } else { "" })
                }
                UiEvent::Step { .. } => {} // (footsteps: too many to list)
                UiEvent::StopSfx { sfx, item } => say!("[t{t}] stop sfx {sfx} at {item:?}"),
                UiEvent::SfxVolume { sfx, volume } => say!("[t{t}] sfx {sfx} volume {volume}"),
                UiEvent::OpenGump { item, gump } => say!("[t{t}] open window {gump} for {item}"),
                UiEvent::CloseGump { item } => say!("[t{t}] close window of {item}"),
                UiEvent::CloseItemGumps => say!("[t{t}] close every item window"),
                UiEvent::Slider { min, .. } => say!("[t{t}] slider: {min}"),
                UiEvent::Target { .. } => say!("[t{t}] target (declined)"),
                UiEvent::Endgame => say!("[t{t}] the ending film"),
                UiEvent::Victory => say!("[t{t}] victory fanfare"),
                UiEvent::Ask { .. } => {}
            },
        );
        if vm.running() == 0 {
            say!("all processes done after {t} ticks");
            break;
        }
    }
    if let Some(png) = a.flag(7, "png") {
        let (px, py) = project(it.x, it.y, it.z);
        let mut buf = vec![0u8; 640 * 480 * 4];
        let mut sc = SpriteCache::new(2, Box::new(upscale::nearest));
        let opts = DrawOpts { max_z, ..Default::default() };
        painter.draw(gd, &mut world, &mut sc, (px - 160, py - 140), &mut Canvas::new(&mut buf, 640, 480), opts);
        save(Path::new(png), 640, 480, buf)?;
    }
    let after = world.get(id).map(|x| (x.shape, x.frame));
    if after != Some((it.shape, it.frame)) {
        say!("item {id} is now {after:?}");
    }
    if !vm.missing.is_empty() {
        let miss: BTreeMap<u16, u32> = vm.missing.iter().map(|(k, v)| (*k, *v)).collect();
        say!("unimplemented intrinsics: {}", missing_summary(miss));
    }
    Ok(())
}

fn smoke(a: &Args, gd: &GameData, uc: &Usecode, flags: Option<&[u8]>) -> Result<()> {
    // Look at and use every scripted item on a map, each in a fresh world, answering "bye" (the last option).
    const TICKS: usize = 1500;
    let m: u16 = a.num(3, "40", "map")?;
    let base = World::build(gd, m);
    let (mut runs, mut stuck, mut barks, mut errors) = (0, 0, 0, 0);
    let mut missing: BTreeMap<u16, u32> = BTreeMap::new();
    for id in base.objects() {
        let it = base.get(id).expect("listed objects exist");
        let egg = base.index(id).is_some_and(|i| base.entry(i).traits.egg);
        let events: &[usize] = if egg { &[fmt::event::HATCH] } else { &[fmt::event::LOOK, fmt::event::USE] };
        for &ev in events {
            let cls = event_class(uc, id, &it, gd.typeflags.get(it.shape).family, ev);
            if uc.event(cls, ev).is_none() {
                continue;
            }
            let mut world = World::build(gd, m);
            let mut vm = Vm::new(uc, flags);
            let mut stats = Stats::load(gd.npcdata.as_deref());
            vm.item_event(uc, &WorldHost { world: &mut world, data: gd, stats: &mut stats, others: None }, id, ev);
            runs += 1;
            let done = (0..TICKS).any(|_| {
                vm.tick(uc, &mut WorldHost { world: &mut world, data: gd, stats: &mut stats, others: None });
                auto_ui(
                    &mut vm,
                    |o| o.len().saturating_sub(1),
                    |e| barks += matches!(e, UiEvent::Bark { .. } | UiEvent::Read { .. }) as u32,
                );
                vm.running() == 0
            });
            if !done {
                stuck += 1;
                say!(
                    "still running after {TICKS} ticks: item {id} shape {} ({}) event {ev}",
                    it.shape,
                    uc.class_name(cls).unwrap_or_default()
                );
            }
            errors += vm.errors;
            for (k, v) in &vm.missing {
                *missing.entry(*k).or_default() += v;
            }
        }
    }
    say!("map {m}: {runs} runs, {stuck} still running, {barks} barks/reads");
    if errors > 0 {
        say!("script errors: {errors}");
    }
    say!("unimplemented: {}", missing_summary(missing));
    Ok(())
}

/// Play a map headless: the Avatar stands (or walks `walk=DIR` 0–7) while eggs, the fast area and
/// scripts run. Prints what the player would see and where they end up.
fn live(a: &Args) -> Result<()> {
    use u8engine::{avatar::Input, usecode::Scripts, world::AVATAR, Game};
    let m: u16 = a.num(3, "40", "map")?;
    let ticks: u32 = a.num(4, "300", "ticks")?;
    let dir = a.flag(5, "walk").map(|d| crate::cli::num::<usize>(d, "walk")).transpose()?;
    let save = a.flag(5, "save").map(|n| crate::cli::num::<u32>(n, "save")).transpose()?;
    let save = save.map(|n| u8data::save::Save::load(a.data(), n)).transpose()?;
    let gd = GameData::load_save(a.data(), save.as_ref())?;
    let scripts = Scripts::load(a.data(), &gd)?;
    let mut g = Game::new(gd, Some(scripts), Some(m)).map_err(|e| anyhow!(e))?;
    say!(
        "starting {:02}:{:02} as {}",
        g.minutes() / 60 % 24,
        g.minutes() % 60,
        g.scripts.as_ref().map_or("?", |s| s.vm.avatar_name.as_str())
    );
    if let Some(at) = a.flag(5, "at") {
        // Start somewhere else: at=x,y,z
        let v: Vec<i32> = at.split(',').map(|n| crate::cli::num(n, "at")).collect::<Result<_>>()?;
        // (bringing the saved Avatar along if this isn't the map it's on)
        let mut it = g.world.get(AVATAR).or_else(|| g.data.avatar().map(|(_, it)| it));
        let it = it.as_mut().ok_or_else(|| anyhow!("no Avatar in the saved game"))?;
        (it.x, it.y, it.z, it.mapnum) = (v[0], *v.get(1).unwrap_or(&it.y), *v.get(2).unwrap_or(&it.z), m as u8);
        g.world.put_actor(&g.data, *it);
        g.avatar = u8engine::avatar::Avatar::new(&g.data, &g.world);
    }
    let npcs = |g: &Game| -> BTreeMap<u16, (i32, i32, i32)> {
        g.world
            .objects()
            .filter(|&id| id > 1 && id < 256)
            .filter_map(|id| g.world.get(id).map(|i| (id, (i.x, i.y, i.z))))
            .collect()
    };
    let before = npcs(&g);
    let mut last = before.clone();
    let input = Input { dir, ..Default::default() };
    let (mut peak, mut frozen) = (0, 0);
    for t in 0..ticks {
        let st = g.update(1.0 / 30.0, &input);
        if st.map_changed {
            let p = g.world.get(AVATAR).map(|i| (i.x, i.y, i.z));
            say!("[t{t}] now on map {} at {p:?}", g.map());
        }
        let vm = &mut g.scripts.as_mut().expect("loaded").vm;
        auto_ui(
            vm,
            |o| o.len().saturating_sub(1),
            |e| match e {
                UiEvent::Bark { item, text, .. } => say!("[t{t}] {item} says: {}", text.trim()),
                UiEvent::Ask { options, .. } => say!("[t{t}] ask {options:?}"),
                UiEvent::Read { kind, text, .. } => say!("[t{t}] read {kind:?}: {}", text.trim()),
                UiEvent::Music(n) => say!("[t{t}] music {n}"),
                UiEvent::Sfx { sfx, item, looping } => {
                    say!("[t{t}] {}sfx {sfx} at {item}", if *looping { "ambient " } else { "" })
                }
                UiEvent::StopSfx { sfx, item } => say!("[t{t}] stop sfx {sfx} at {item:?}"),
                _ => {}
            },
        );
        peak = peak.max(vm.running());
        if t % 1500 == 1499 {
            // Who moved in the last 50 seconds (game time in brackets)?
            let now = npcs(&g);
            for (id, p) in &now {
                if last.get(id) != Some(p) {
                    say!("[t{t} {:02}:{:02}] NPC {id} now at {p:?}", g.minutes() / 60 % 24, g.minutes() % 60);
                }
            }
            last = now;
        }
        let vm = &mut g.scripts.as_mut().expect("loaded").vm;
        frozen += vm.stasis as u32;
    }
    for (id, now) in npcs(&g) {
        if before.get(&id) != Some(&now) {
            say!("NPC {id} moved {:?} → {now:?}", before.get(&id));
        }
    }
    say!("game time {:02}:{:02}", g.minutes() / 60 % 24, g.minutes() % 60);
    let p = g.world.get(AVATAR).map(|i| (i.x, i.y, i.z));
    let vm = &g.scripts.as_ref().expect("loaded").vm;
    say!(
        "after {ticks} ticks: map {} avatar {p:?}, {} processes (peak {peak}), stasis {frozen} ticks, {} errors, heap {:?}",
        g.map(),
        vm.running(),
        vm.errors,
        vm.heap_size()
    );
    let miss: BTreeMap<u16, u32> = vm.missing.iter().map(|(k, v)| (*k, *v)).collect();
    say!("unimplemented: {}", missing_summary(miss));
    if let Some(slot) = a.flag(5, "write").map(|n| crate::cli::num::<u32>(n, "write")).transpose()? {
        let path = g.save(a.data(), slot, "u8tool live").map_err(|e| anyhow!(e))?;
        say!("saved to {}", path.display());
    }
    Ok(())
}
