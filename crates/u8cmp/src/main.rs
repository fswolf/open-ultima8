//! Comparison harness: get game states in and out of the form the DOS original reads, play our
//! engine headless through scripted steps, and dump states as text that can be diffed against
//! what the original saved after the same steps.
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use u8data::{map, save::Save, GameData};
use u8engine::Place;
use u8engine::{
    avatar::Input,
    usecode::{Scripts, UiEvent},
    world::AVATAR,
    Game,
};
use u8render::{project, Canvas, DrawOpts, Painter, SpriteCache};

const USAGE: &str = "usage: u8cmp <cmd> …
  unpack <root> <slot> <dir>            write a save's files into dir (a GAMEDAT folder)
  dump <root> <slot|g> [out.txt]        text dump of a stored game (g = GAMEDAT)
  play <root> <slot|g> steps…           play headless. steps:
       wait=T  walk=DIR,T  run=DIR,T  at=x,y,z[,map]  use=ID  useshape=SHAPE  look=ID
       answer=N|last  shot=out.png  state=out.txt  gamedat=dir  log  mins=M  jump  combat
       bark=T (ticks a line of speech stays up; default by length)";

fn slot(s: &str) -> Option<u32> {
    (s != "g").then(|| s.parse().expect("slot number or g"))
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 3 {
        eprintln!("{USAGE}");
        std::process::exit(2);
    }
    let root = Path::new(&a[1]);
    match a[0].as_str() {
        "unpack" => {
            let s = Save::load(root, a[2].parse().unwrap()).expect("save");
            write_dir(&s, Path::new(&a[3]));
        }
        "dump" => {
            let save = slot(&a[2]).map(|n| Save::load(root, n).expect("save"));
            let gd = GameData::load_save(root, save.as_ref()).expect("data");
            let text = dump(&gd);
            match a.get(3) {
                Some(p) => std::fs::write(p, text).unwrap(),
                None => print!("{text}"),
            }
        }
        "play" => play(root, slot(&a[2]), &a[3..]),
        "fuzz" => fuzz(root, slot(&a[2]), &a[3..]),
        "talk" => talk(root, slot(&a[2]), &a[3..]),
        "soak" => soak(root, slot(&a[2]), &a[3..]),
        _ => eprintln!("{USAGE}"),
    }
}

fn write_dir(s: &Save, dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    for (name, d) in &s.files {
        if name == u8data::save::EXTRA {
            continue;
        }
        std::fs::write(dir.join(name), d).unwrap();
    }
}

/// Flags that say how the game holds a thing just now, not what it is.
const TRANSIENT: u16 = 0x2000 | 0x0100 | 0x4000 | 0x8000;

fn item_line(it: &map::Item) -> String {
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
fn dump(gd: &GameData) -> String {
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

fn shot(g: &mut Game, painter: &mut Painter, cache: &mut SpriteCache, path: &str) {
    let (w, h) = (320u32, 200u32);
    let Some((x, y, z)) = g.camera() else { return };
    let (sx, sy) = project(x, y, z);
    let cam = (sx - w as i32 / 2, sy - h as i32 / 2);
    let max_z = g.world.ceiling(AVATAR).map(|z| z - 1);
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let mut canvas = Canvas::new(&mut buf, w, h);
    painter.draw(&g.data, &mut g.world, cache, cam, &mut canvas, DrawOpts { max_z, ..Default::default() });
    image::RgbaImage::from_raw(w, h, buf).unwrap().save(path).unwrap();
}

fn open(root: &Path, slot: Option<u32>) -> Game {
    let mut g = Game::open(root, slot, None).expect("game");
    if g.scripts.is_none() {
        let s = Scripts::load(root, &g.data).expect("scripts");
        g.scripts = Some(s);
    }
    if g.needs_name() {
        g.set_avatar_name("Ryan");
    }
    g
}

#[derive(Default)]
struct Drv {
    answer: Option<usize>,
    queue: std::collections::VecDeque<usize>,
    targets: std::collections::VecDeque<u32>,
    silent: bool,
    bark_ticks: Option<u32>,
    log: bool,
    t: u32,
    barks: Vec<(u32, u32)>,
    asks: u32,
    said: u32,
    open_ask: Option<(u32, u32)>,
    cycle: Option<usize>,
    cap: u32,
    script: Vec<String>,
    keep: bool,
}

impl Drv {
    fn run(&mut self, g: &mut Game, n: u32, input: &mut Input) {
        for _ in 0..n {
            let st = g.update(1.0 / 30.0, input);
            (input.jump, input.combat, input.attack, input.kick) = (false, false, false, false);
            self.t += 1;
            let t = self.t;
            if st.map_changed && self.log {
                println!("[t{t}] now on map {}", g.map());
            }
            for e in g.take_events() {
                match e {
                    UiEvent::Bark { pid, item, text } => {
                        self.said += 1;
                        if self.keep {
                            self.script.push(format!("  {item}: {}", text.trim()));
                        }
                        if self.log {
                            println!("[t{t}] {item} says: {}", text.trim());
                        }
                        let ticks = self.bark_ticks.unwrap_or(60 + 2 * text.len() as u32);
                        self.barks.push((pid as u32, ticks));
                    }
                    UiEvent::Ask { pid, options, item } => {
                        self.asks += 1;
                        if self.silent {
                            if self.log {
                                println!("[t{t}] {item} asks {options:?} -> (no answer, waits 924 ticks)");
                            }
                            self.open_ask = Some((pid as u32, 924));
                            if self.keep {
                                self.script.push(format!("  ? {options:?} -> (silence)"));
                            }
                            continue;
                        }
                        let last = options.len().saturating_sub(1);
                        let mut pick = self.queue.pop_front().or(self.answer).unwrap_or(last).min(last);
                        if let Some(c) = self.cycle.as_mut() {
                            if self.asks <= self.cap {
                                pick = *c % options.len().max(1);
                                *c += 1;
                            } else {
                                pick = last;
                            }
                        }
                        if self.keep {
                            self.script
                                .push(format!("  ? {options:?} -> {}", options.get(pick).cloned().unwrap_or_default()));
                        }
                        if self.log {
                            println!("[t{t}] {item} asks {options:?} -> {pick}");
                        }
                        g.vm().unwrap().answer(pid, &options, pick);
                    }
                    UiEvent::Read { pid, text, .. } => {
                        if self.log {
                            println!("[t{t}] read: {}", text.trim().replace('\n', " / "));
                        }
                        g.vm().unwrap().finish(pid, 0);
                    }
                    UiEvent::Target { pid } => {
                        let pick = self.targets.pop_front().unwrap_or(0);
                        if self.log {
                            println!("[t{t}] target asked -> {pick}");
                        }
                        g.vm().unwrap().finish(pid, pick)
                    }
                    UiEvent::Slider { pid, min, .. } => g.vm().unwrap().finish(pid, min as u16 as u32),
                    UiEvent::Music(n) if self.log => println!("[t{t}] music {n}"),
                    UiEvent::OpenGump { item, gump } if self.log => println!("[t{t}] open gump {gump} for {item}"),
                    UiEvent::CloseGump { item } if self.log => println!("[t{t}] close gump for {item}"),
                    UiEvent::Endgame if self.log => println!("[t{t}] endgame"),
                    _ => {}
                }
            }
            if let Some((pid, left)) = self.open_ask {
                if left <= 1 {
                    g.vm().unwrap().answer_nothing(pid as _);
                    self.open_ask = None;
                    if self.log {
                        println!("[t{t}] question timed out");
                    }
                } else {
                    self.open_ask = Some((pid, left - 1));
                }
            }
            let mut done = vec![];
            for b in self.barks.iter_mut() {
                b.1 = b.1.saturating_sub(1);
                if b.1 == 0 {
                    done.push(b.0);
                }
            }
            self.barks.retain(|b| b.1 > 0);
            for pid in done {
                g.vm().unwrap().finish(pid as _, 0);
            }
        }
    }
}

fn tree(g: &Game, id: u16, depth: usize) {
    for c in g.world.contents(id) {
        if let Some(it) = g.world.get(c) {
            println!("  {}id {c:5} {}", "  ".repeat(depth), item_line(&it));
        }
        tree(g, c, depth + 1);
    }
}

fn v_arg(s: &str) -> &str {
    s.split_once('=').map_or("", |x| x.1)
}
fn v_path(s: &str) -> PathBuf {
    PathBuf::from(s)
}

fn rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS"))
                .and_then(|l| l.split_whitespace().nth(1).and_then(|n| n.parse().ok()))
        })
        .unwrap_or(0)
}

fn play(root: &Path, slot: Option<u32>, steps: &[String]) {
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
                std::fs::write(v, dump(&gd)).unwrap();
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

/// Random walking, running, jumping and climbing from a save; prints anything that breaks the
/// rules (inside something solid, standing on nothing, stuck mid-action, odd height gains).
/// fuzz <root> <slot> seed=N steps=N [at=x,y,z,map] [nojump] [verbose]
fn fuzz(root: &Path, slot: Option<u32>, args: &[String]) {
    let get = |k: &str| args.iter().find_map(|a| a.strip_prefix(k).and_then(|v| v.strip_prefix('=')));
    let mut seed: u64 = get("seed").and_then(|v| v.parse().ok()).unwrap_or(1);
    let steps: u32 = get("steps").and_then(|v| v.parse().ok()).unwrap_or(500);
    let verbose = args.iter().any(|a| a == "verbose");
    let nojump = args.iter().any(|a| a == "nojump");
    let at: Option<Vec<i32>> = get("at").map(|v| v.split(',').filter_map(|n| n.parse().ok()).collect());
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

/// Talk to everyone on a map: talk <root> <slot> map=M policy=first|last|cycle|silent [npc=N] [out=dir]
fn talk(root: &Path, slot: Option<u32>, args: &[String]) {
    let get = |k: &str| args.iter().find_map(|a| a.strip_prefix(k).and_then(|v| v.strip_prefix('=')));
    let map: u16 = get("map").and_then(|v| v.parse().ok()).unwrap_or(40);
    let policy = get("policy").unwrap_or("last").to_string();
    let only: Option<u16> = get("npc").and_then(|v| v.parse().ok());
    let hour: Option<u32> = get("hour").and_then(|v| v.parse().ok());
    let out = get("out").map(PathBuf::from);
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

/// A long session: wander, talk, use things, save and load. soak <root> <slot> ticks=N seed=N [every=3000] [saveslot=30]
fn soak(root: &Path, slot: Option<u32>, args: &[String]) {
    let get = |k: &str| args.iter().find_map(|a| a.strip_prefix(k).and_then(|v| v.strip_prefix('=')));
    let total: u32 = get("ticks").and_then(|v| v.parse().ok()).unwrap_or(27000);
    let every: u32 = get("every").and_then(|v| v.parse().ok()).unwrap_or(3000);
    let save_slot: u32 = get("saveslot").and_then(|v| v.parse().ok()).unwrap_or(30);
    let mut seed: u64 = get("seed").and_then(|v| v.parse().ok()).unwrap_or(1);
    let mut rnd = move |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    let mut g = open(root, slot);
    if let Some(h) = get("hour").and_then(|v| v.parse::<u32>().ok()) {
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
