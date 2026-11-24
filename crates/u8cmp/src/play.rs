//! `play`: our engine headless through scripted steps (walk, use, answer, shots, states).
use super::*;

/// What `id` holds, all the way down, indented.
fn tree(g: &Game, id: u16, depth: usize) {
    for c in g.world.contents(id) {
        if let Some(it) = g.world.get(c) {
            println!("  {}id {c:5} {}", "  ".repeat(depth), item_line(&it));
        }
        tree(g, c, depth + 1);
    }
}

/// A step's value (`key=value`).
fn v_arg(s: &str) -> &str {
    s.split_once('=').map_or("", |x| x.1)
}

/// A step's value as a path.
fn v_path(s: &str) -> PathBuf {
    PathBuf::from(s)
}

/// Play headless through `steps` (see the usage).
pub(super) fn play(root: &Path, slot: Option<u32>, steps: &[String]) {
    let mut g = open(root, slot);
    let mut painter = Painter::new();
    let mut cache = SpriteCache::unscaled();
    let mut d = Drv::default();
    let mut input = Input::default();
    for s in steps {
        let (k, v) = s.split_once('=').unwrap_or((s.as_str(), ""));
        let nums: Vec<i32> = v.split(',').filter_map(|n| n.parse().ok()).collect();
        let t = d.t;
        match k {
            "log" => d.log = true,
            "quiet" => d.log = false,
            "answer" => d.answer = v.parse().ok(),
            "answers" => d.queue = nums.iter().map(|&n| n as usize).collect(),
            "silent" => d.silent = v != "0",
            "targets" => d.targets = nums.iter().map(|&n| n as u32).collect(),
            "bark" => d.bark_ticks = v.parse().ok(),
            "wait" => d.run(&mut g, nums[0] as u32, &mut input),
            "walk" | "run" | "careful" => {
                input.dir = Some(nums[0] as usize);
                input.run = k == "run";
                input.careful = k == "careful";
                d.run(&mut g, nums[1] as u32, &mut input);
                input = Input::default();
            }
            "jump" => {
                input.dir = nums.first().map(|&n| n as usize);
                input.jump = true;
                d.run(&mut g, 1, &mut input);
                input = Input::default();
            }
            "runjump" => {
                input.dir = Some(nums[0] as usize);
                input.run = true;
                d.run(&mut g, nums[1] as u32, &mut input);
                input.jump = true;
                d.run(&mut g, 1, &mut input);
                input = Input::default();
            }
            "hold" => {
                // hold a direction while waiting (hanging: pull up or let go)
                input.dir = Some(nums[0] as usize);
                d.run(&mut g, nums[1] as u32, &mut input);
                input = Input::default();
            }
            "combat" => {
                input.combat = true;
                d.run(&mut g, 1, &mut input);
            }
            "attack" => {
                input.dir = nums.first().map(|&n| n as usize);
                input.attack = true;
                d.run(&mut g, 1, &mut input);
                input = Input::default();
            }
            "kick" => {
                input.kick = true;
                d.run(&mut g, 1, &mut input);
            }
            "face" => {
                if let Some(a) = g.avatar.as_mut() {
                    a.turn(&g.data, &mut g.world, nums[0] as usize);
                }
            }
            "at" => {
                let m = nums.get(3).map_or(g.map(), |&m| m as u16);
                g.put_avatar(m, (nums[0], nums[1], nums[2]));
            }
            "tp" => println!(
                "[t{t}] teleport map {} egg {} -> {}",
                nums[0],
                nums[1],
                g.teleport(nums[0] as u16, nums[1] as u8)
            ),
            "goto" => println!("[t{t}] go to {v} -> {}", g.go_to(v)),
            "kit" => println!("[t{t}] kit bag {:?}", g.give_test_kit()),
            "mins" => g.set_minutes(nums[0] as u32),
            "restored" => g.restored.on = v != "0",
            "flag" => {
                _ = g.vm().map(|vm| vm.set_global_bits(nums[0] as usize, 1, nums.get(1).map_or(1, |&b| b as u32)))
            }
            "use" => {
                let can = g.can_use(nums[0] as u16);
                let ok = g.use_item(nums[0] as u16);
                println!("[t{t}] use {} can {can} -> {ok}", nums[0]);
            }
            "look" => {
                g.trigger(nums[0] as u16, 0);
            }
            "trig" => {
                println!("[t{t}] trigger {} ev {} -> {}", nums[0], nums[1], g.trigger(nums[0] as u16, nums[1] as usize))
            }
            "useshape" => {
                let av = g.world.get(AVATAR).unwrap();
                let id = g
                    .world
                    .objects()
                    .filter(|&id| g.world.get(id).is_some_and(|it| it.shape == nums[0] as u32) && !g.world.is_void(id))
                    .min_by_key(|&id| {
                        let it = g.world.get(id).unwrap();
                        (it.x - av.x).abs().max((it.y - av.y).abs())
                    });
                match id {
                    Some(id) => {
                        let ok = g.use_item(id);
                        println!("[t{t}] use {id} ({:?}) -> {ok}", g.world.get(id).map(|i| (i.x, i.y, i.z, i.frame)));
                    }
                    None => println!("[t{t}] no shape {} here", nums[0]),
                }
            }
            "mv" => {
                // mv=ID,into,C[,x,y] | mv=ID,map,x,y,z
                let p: Vec<&str> = v.split(',').collect();
                let id: u16 = p[0].parse().unwrap();
                let n = |i: usize| p.get(i).and_then(|s| s.parse::<i32>().ok());
                let to = if p[1] == "into" {
                    Place::Into { container: n(2).unwrap() as u16, at: (n(3).unwrap_or(20), n(4).unwrap_or(20)) }
                } else {
                    Place::Map((n(2).unwrap(), n(3).unwrap(), n(4).unwrap()))
                };
                let pick = g.can_pick_up(id);
                let r = g.move_item(id, to);
                println!(
                    "[t{t}] move {id} {to:?}: pick {pick:?} -> {r:?} now {}",
                    g.world.get(id).map(|i| item_line(&i)).unwrap_or_default()
                );
            }
            "equip" => println!("[t{t}] equip {} -> {:?}", nums[0], g.equip(nums[0] as u16)),
            "stow" => println!("[t{t}] stow {} -> {:?}", nums[0], g.stow(nums[0] as u16)),
            "split" => {
                let p: Vec<&str> = v.split(',').collect();
                let n = |i: usize| p.get(i).and_then(|s| s.parse::<i32>().ok());
                let (id, cnt) = (n(0).unwrap() as u16, n(1).unwrap() as u16);
                g.let_go(id); // (as the client does when the slider opens)
                let r = if p[2] == "onto" {
                    g.split_onto(id, cnt, n(3).unwrap() as u16)
                } else if p[2] == "into" {
                    g.split_to(
                        id,
                        cnt,
                        Place::Into { container: n(3).unwrap() as u16, at: (n(4).unwrap_or(20), n(5).unwrap_or(20)) },
                    )
                } else {
                    g.split_to(id, cnt, Place::Map((n(3).unwrap(), n(4).unwrap(), n(5).unwrap())))
                };
                println!("[t{t}] split {id} x{cnt} -> {r:?}");
            }
            "merge" => println!(
                "[t{t}] merge {} onto {} can {} -> {:?}",
                nums[0],
                nums[1],
                g.can_merge(nums[0] as u16, nums[1] as u16),
                g.merge(nums[0] as u16, nums[1] as u16)
            ),
            "inv" => {
                let st = g.stats.get(AVATAR).unwrap_or_default();
                let (w, max) = g.load();
                println!(
                    "[t{t}] avatar str {} dex {} int {} hp {} mana {} armour {} weight {w}/{max} dead {} combat {} pack {:?}",
                    st.str, st.dex, st.int, st.hp, st.mana, g.armour_class(), g.avatar_dead(), g.in_combat(), g.backpack()
                );
                tree(&g, AVATAR, 0);
            }
            "tree" => {
                println!(
                    "[t{t}] {} {}",
                    nums[0],
                    g.world.get(nums[0] as u16).map(|i| item_line(&i)).unwrap_or("gone".into())
                );
                tree(&g, nums[0] as u16, 0);
            }
            "item" => println!(
                "[t{t}] item {} {} void {} in {:?} reach {}",
                nums[0],
                g.world.get(nums[0] as u16).map(|i| item_line(&i)).unwrap_or("gone".into()),
                g.world.is_void(nums[0] as u16),
                g.world.container(nums[0] as u16),
                g.within_reach(nums[0] as u16)
            ),
            "find" => {
                // every thing of a shape on this map (also inside containers)
                for id in g.world.objects() {
                    if let Some(it) = g.world.get(id) {
                        if it.shape == nums[0] as u32 && !g.world.is_void(id) {
                            println!("  id {id:5} {} in {:?}", item_line(&it), g.world.container(id));
                        }
                    }
                }
            }
            "npcs" => {
                let av = g.world.get(AVATAR).unwrap();
                let r = nums.first().copied().unwrap_or(1024);
                for id in 2..256u16 {
                    let Some(it) = g.world.get(id) else { continue };
                    if g.world.is_void(id) || (it.x - av.x).abs().max((it.y - av.y).abs()) > r {
                        continue;
                    }
                    let st = g.stats.get(id).unwrap_or_default();
                    println!(
                        "  npc {id:3} s{:4} f{:3} at {:5},{:5},{:3} d{:4} hp {:3} dead {} combat {} target {:?} acting {:?} align {:x}/{:x}",
                        it.shape,
                        it.frame,
                        it.x,
                        it.y,
                        it.z,
                        (it.x - av.x).abs().max((it.y - av.y).abs()),
                        st.hp,
                        st.dead,
                        g.world.in_combat(id),
                        g.world.target(id),
                        g.world.acting(id),
                        st.alignment,
                        st.enemies
                    );
                }
            }
            "sethp" => {
                if let Some(s) = g.stats.get_mut(nums[0] as u16) {
                    s.hp = nums[1] as i16;
                }
            }
            "setstat" => {
                if let Some(s) = g.stats.get_mut(AVATAR) {
                    (s.str, s.dex, s.int) = (nums[0] as u8, nums[1] as u8, nums[2] as u8);
                    (s.hp, s.mana) = (s.max_hp(), s.max_mana());
                }
            }
            "count" => {
                let n = g.world.objects().count();
                let void = g.world.objects().filter(|&i| g.world.is_void(i)).count();
                let procs = g.scripts.as_ref().map_or(0, |s| s.vm.running());
                let errs = g.scripts.as_ref().map_or(0, |s| s.vm.errors);
                println!(
                    "[t{t}] map {} objects {n} void {void} procs {procs} errors {errs} fighters {} rss {} kB barks {} asks {} time day {} {:02}:{:02}",
                    g.map(),
                    g.world.fighters().len(),
                    rss_kb(),
                    d.said,
                    d.asks,
                    g.minutes() / 1440 + 1,
                    g.minutes() / 60 % 24,
                    g.minutes() % 60
                );
            }
            "save" => {
                let r = g.save(root, nums[0] as u32, "playtest");
                let size = r.as_ref().ok().and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len());
                println!("[t{t}] save {} -> {r:?} {size:?} bytes", nums[0]);
            }
            "reopen" => {
                g = open(root, Some(nums[0] as u32));
                d.barks.clear();
                d.open_ask = None;
                println!("[t{t}] reopened slot {}", nums[0]);
            }
            "objs" => {
                // every object of the world as text (no ids: they change over a load), sorted
                let mut v: Vec<String> = g
                    .world
                    .objects()
                    .filter_map(|id| g.world.get(id).map(|it| (id, it)))
                    .map(|(id, it)| {
                        let c = g
                            .world
                            .container(id)
                            .and_then(|c| g.world.get(c))
                            .map(|c| format!("in s{} at {},{},{}", c.shape, c.x, c.y, c.z))
                            .unwrap_or_default();
                        format!("{} void {} {c}", item_line(&it), g.world.is_void(id))
                    })
                    .collect();
                v.sort();
                std::fs::write(v_path(v_arg(s)), v.join("\n") + "\n").unwrap();
            }
            "under" => {
                // what overlaps the Avatar's footprint (padded by N), by height
                let pad = nums.first().copied().unwrap_or(0);
                let a = g.world.get(AVATAR).unwrap();
                let (ax0, ay0) = (a.x - 64 - pad, a.y - 64 - pad);
                let mut v: Vec<(i32, String)> = vec![];
                for i in 0..g.world.len() {
                    let e = g.world.entry(i);
                    if e.gone || e.void || e.parent.is_some() || g.world.id(i) == AVATAR {
                        continue;
                    }
                    if e.x0 < a.x + pad && ax0 < e.it.x && e.y0 < a.y + pad && ay0 < e.it.y {
                        let t = e.traits;
                        v.push((
                            e.it.z,
                            format!(
                                "  id {:5} s{:4} f{:3} box x {}..{} y {}..{} z {}..{} {}{}{}{}{}{}",
                                g.world.id(i),
                                e.it.shape,
                                e.it.frame,
                                e.x0,
                                e.it.x,
                                e.y0,
                                e.it.y,
                                e.it.z,
                                e.z1,
                                if t.solid { "solid " } else { "" },
                                if t.land { "land " } else { "" },
                                if t.roof { "roof " } else { "" },
                                if t.actor { "actor " } else { "" },
                                if t.egg { "egg " } else { "" },
                                if t.editor { "editor " } else { "" }
                            ),
                        ));
                    }
                }
                v.sort();
                println!("[t{t}] under avatar at {},{},{}:", a.x, a.y, a.z);
                for (_, l) in v {
                    println!("{l}");
                }
            }
            "edsolid" => {
                // solid shapes flagged editor-only on this map: count by shape, nearest to the Avatar
                let a = g.world.get(AVATAR).unwrap();
                #[allow(clippy::type_complexity)]
                let mut by: BTreeMap<u32, (u32, i32, (i32, i32, i32))> = BTreeMap::new();
                for i in 0..g.world.len() {
                    let e = g.world.entry(i);
                    if e.gone || e.void || e.parent.is_some() || !(e.traits.editor && e.traits.solid) || e.traits.egg {
                        continue;
                    }
                    let d = (e.it.x - a.x).abs().max((e.it.y - a.y).abs());
                    let b = by.entry(e.it.shape).or_insert((0, i32::MAX, (0, 0, 0)));
                    b.0 += 1;
                    if d < b.1 {
                        (b.1, b.2) = (d, (e.it.x, e.it.y, e.it.z));
                    }
                }
                println!("[t{t}] map {} editor+solid: {by:?}", g.map());
            }
            "setq" => {
                if let Some(mut it) = g.world.get(nums[0] as u16) {
                    it.quality = nums[1] as u16;
                    g.world.set(&g.data, nums[0] as u16, it);
                }
            }
            "drop" => {
                // put a thing on the floor at the first spot near the Avatar that takes it
                let a = g.world.get(AVATAR).unwrap();
                let id = nums[0] as u16;
                let mut done = None;
                'ring: for r in [96, 128, 64, 144] {
                    for (dx, dy) in [(1, 0), (0, 1), (-1, 0), (0, -1), (1, 1), (-1, 1), (1, -1), (-1, -1)] {
                        let at = (a.x + dx * r, a.y + dy * r, a.z);
                        if g.move_item(id, Place::Map(at)).is_ok() {
                            done = Some(at);
                            break 'ring;
                        }
                    }
                }
                println!(
                    "[t{t}] drop {id} -> {done:?} now {}",
                    g.world.get(id).map(|i| item_line(&i)).unwrap_or_default()
                );
            }
            "stat" => {
                let st = g.stats.get(AVATAR).unwrap_or_default();
                let (w, max) = g.load();
                let worn: Vec<String> = g
                    .world
                    .contents(AVATAR)
                    .iter()
                    .filter_map(|&c| g.world.get(c))
                    .map(|i| format!("s{}/z{}", i.shape, i.z))
                    .collect();
                println!("[t{t}] str {} dex {} int {} hp {} mana {} armour {} weight {w}/{max} dead {} combat {} worn {worn:?}", st.str, st.dex, st.int, st.hp, st.mana, g.armour_class(), g.avatar_dead(), g.in_combat());
            }
            "fight" => {
                // fight whoever is nearest and alive for N ticks: face them, close in, swing
                let mut swings = 0;
                for _ in 0..nums[0] {
                    let a = g.world.get(AVATAR).unwrap();
                    let foe = (2..256u16)
                        .filter(|&id| !g.world.is_void(id) && g.stats.get(id).is_some_and(|s| !s.dead))
                        .filter_map(|id| g.world.get(id).map(|it| (id, it)))
                        .filter(|(id, it)| {
                            (g.world.in_combat(*id) || nums.get(1).is_some_and(|&n| n as u16 == *id))
                                && (it.z - a.z).abs() < 48
                        })
                        .min_by_key(|(_, it)| (it.x - a.x).abs().max((it.y - a.y).abs()));
                    let mut input = Input::default();
                    if let Some((_, it)) = foe {
                        let (dx, dy) = (it.x - a.x, it.y - a.y);
                        let ang = (dy as f32).atan2(dx as f32); // 0 = +x
                        let dir = (((ang / std::f32::consts::FRAC_PI_4).round() as i32 + 2).rem_euclid(8)) as usize;
                        let dist = dx.abs().max(dy.abs());
                        if !g.in_combat() {
                            input.combat = true;
                        } else if dist > 120 {
                            input.dir = Some(dir);
                        } else if g.avatar.as_ref().is_some_and(|av| !av.busy()) {
                            if let Some(av) = g.avatar.as_mut() {
                                av.turn(&g.data, &mut g.world, dir);
                            }
                            input.attack = true;
                            swings += 1;
                        }
                    }
                    d.run(&mut g, 1, &mut input);
                    if g.avatar_dead() {
                        break;
                    }
                }
                println!("[t{}] fought: {swings} swings", d.t);
            }
            "shot" => shot(&mut g, &mut painter, &mut cache, v),
            "state" => {
                let s = g.snapshot();
                let gd = GameData::load_save(root, Some(&s)).expect("snapshot loads");
                std::fs::write(v, dump::dump(&gd)).unwrap();
            }
            "gamedat" => write_dir(&g.snapshot(), &PathBuf::from(v)),
            "where" => {
                let it = g.world.get(AVATAR);
                let a = g.avatar.as_ref();
                println!(
                    "[t{t}] map {} avatar {:?} dir {:?} act {:?} hanging {:?} busy {:?} floor {:?} hp {:?} day {} {:02}:{:02}",
                    g.map(),
                    it.map(|i| (i.x, i.y, i.z, i.frame)),
                    a.map(|a| a.dir),
                    a.map(|a| a.action()),
                    a.map(|a| a.hanging()),
                    a.map(|a| a.busy()),
                    g.world.floor_under(AVATAR),
                    g.stats.get(AVATAR).map(|s| s.hp),
                    g.minutes() / 1440 + 1,
                    g.minutes() / 60 % 24,
                    g.minutes() % 60
                );
            }
            "near" => {
                let av = g.world.get(AVATAR).unwrap();
                let r = nums.first().copied().unwrap_or(256);
                let mut v: Vec<(i32, u16, map::Item)> = g
                    .world
                    .objects()
                    .filter_map(|id| g.world.get(id).map(|it| (id, it)))
                    .filter(|(id, it)| {
                        *id != AVATAR && !g.world.is_void(*id) && (it.x - av.x).abs().max((it.y - av.y).abs()) <= r
                    })
                    .filter(|(id, _)| g.world.container(*id).is_none())
                    .map(|(id, it)| ((it.x - av.x).abs().max((it.y - av.y).abs()), id, it))
                    .collect();
                v.sort_by_key(|e| e.0);
                for (d, id, it) in v {
                    let (sx, sy) = project(it.x, it.y, it.z);
                    let (ax, ay) = project(av.x, av.y, av.z + 20);
                    println!("  id {id:5} d{d:4} screen ({:4},{:4}) {}", sx - ax + 160, sy - ay + 100, item_line(&it));
                }
            }
            _ => eprintln!("unknown step {s}"),
        }
    }
    if let Some(vm) = g.scripts.as_ref().map(|s| &s.vm) {
        println!("end: script errors {} missing intrinsics {:?} running {}", vm.errors, vm.missing, vm.running());
    }
}
