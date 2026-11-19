//! Comparison harness: get game states in and out of the form the DOS original reads, play our
//! engine headless through scripted steps, and dump states as text that can be diffed against
//! what the original saved after the same steps. One file per command (`play`, `fuzz` with
//! `soak`, `talk`, `people`, `attack`, `steal`, `magic`, `dump`); `drive` plays the engine for
//! all of them, `scene` sets up scenes as in the original; the helpers they share are here.
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

mod attack;
mod drive;
mod dump;
mod fuzz;
mod magic;
mod people;
mod play;
mod scene;
mod steal;
mod talk;

use drive::Drv;
use dump::item_line;
use scene::{aim, beside, dir_to, loose, offset};

const USAGE: &str = "usage: u8cmp <cmd> …
  unpack <root> <slot> <dir>            write a save's files into dir (a GAMEDAT folder)
  dump <root> <slot|g> [out.txt]        text dump of a stored game (g = GAMEDAT)
  play <root> <slot|g> steps…           play headless. steps:
       wait=T  walk=DIR,T  run=DIR,T  at=x,y,z[,map]  use=ID  useshape=SHAPE  look=ID
       answer=N|last  shot=out.png  state=out.txt  gamedat=dir  log  mins=M  jump  combat
       bark=T (ticks a line of speech stays up; default by length)
  fuzz <root> <slot> seed=N steps=N [at=x,y,z,map] [nojump] [verbose]
                                        random play, looking for errors and stuck states
  soak <root> <slot> ticks=N seed=N [every=3000] [saveslot=30]
                                        a long session: wander, talk, use things, save and load
  talk <root> <slot> map=M policy=first|last|cycle|silent [npc=N] [out=dir]
                                        talk to everyone on a map
  magic <root> <slot> out=dir [at=x,y,z,map] [secs=10] [every=3] [only=shape[,frame[,q]]]
       [target=self|near|floor:dx,dy|ID] [trace=1] [dump=T shapes=..] [state=dir]
                                        cast the kit's spells; spell pictures against the world
  people <root> <slot> hour=H           everyone on every map at an hour
  attack <root> <slot> map=M npc=N [hour=H] [swings=N] [watch=T] [hp=N] [side=dx,dy] [move=x,y,z]
       [cast] [kill] [procs] [out=dir]  strike someone and watch what follows (procs: live processes)
  steal <root> <slot> map=M npc=N [hour=H] [list] [r=600] [shape=S,..] [item=ID,..] [times=K] [how=take|floor]
       [gap=T] [watch=T] [stand=x,y,z] [answer=N] [move=x,y,z] [out=dir] [shots=PREFIX,EVERY] [frames=ID,..]
                                        take things near someone and watch what follows";

/// A slot number, or ("g") the game in GAMEDAT.
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
            let text = dump::dump(&gd);
            match a.get(3) {
                Some(p) => std::fs::write(p, text).unwrap(),
                None => print!("{text}"),
            }
        }
        "play" => play::play(root, slot(&a[2]), &a[3..]),
        "fuzz" => fuzz::fuzz(root, slot(&a[2]), &a[3..]),
        "talk" => talk::talk(root, slot(&a[2]), &a[3..]),
        "soak" => fuzz::soak(root, slot(&a[2]), &a[3..]),
        "people" => people::people(root, slot(&a[2]), &a[3..]),
        "attack" => attack::attack(root, slot(&a[2]), &a[3..]),
        "steal" => steal::steal(root, slot(&a[2]), &a[3..]),
        "magic" => magic::magic(root, slot(&a[2]), &a[3..]),
        _ => eprintln!("{USAGE}"),
    }
}

/// A save's files as a GAMEDAT folder (what the original reads), without our side file.
fn write_dir(s: &Save, dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    for (name, d) in &s.files {
        if name == u8data::save::EXTRA {
            continue;
        }
        std::fs::write(dir.join(name), d).unwrap();
    }
}

/// The view as the original shows it (320 by 200 round the camera) as a PNG.
fn shot(g: &mut Game, painter: &mut Painter, cache: &mut SpriteCache, path: &str) {
    let (w, h) = (320u32, 200u32);
    let Some((x, y, z)) = g.camera() else { return };
    let (sx, sy) = project(x, y, z);
    let cam = (sx - w as i32 / 2, sy - h as i32 / 2);
    let max_z = g.roof();
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let mut canvas = Canvas::new(&mut buf, w, h);
    painter.draw(&g.data, &mut g.world, cache, cam, &mut canvas, DrawOpts { max_z, ..Default::default() });
    image::RgbaImage::from_raw(w, h, buf).unwrap().save(path).unwrap();
}

/// A game with its scripts, named if it's new.
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

/// This process's memory in use (kB), to catch leaks.
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

/// The value of `key=value` among the arguments.
fn arg<'a>(args: &'a [String], k: &str) -> Option<&'a str> {
    args.iter().find_map(|a| a.strip_prefix(k).and_then(|v| v.strip_prefix('=')))
}
