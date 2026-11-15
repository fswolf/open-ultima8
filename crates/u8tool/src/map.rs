//! Map inspection: archive stats, map renders, items under a point, the saved Avatar.
use crate::cli::{save, upscaler, Args};
use anyhow::{ensure, Context, Result};
use std::path::Path;
use u8data::typeflag::family;
use u8data::GameData;
use u8engine::{eggs, World};
use u8render::{project, Canvas, DrawOpts, Painter, SpriteCache};

pub fn run(a: &Args, gd: &GameData) -> Result<()> {
    match a.get(1, "").as_str() {
        "info" => info(gd),
        "map" => map(a, gd),
        "items" => items(a, gd),
        "eggs" => eggs(a, gd),
        "saves" => saves(a),
        "kit" => kit(a),
        "weapons" => weapons(gd),
        _ => avatar(gd),
    }
}

/// U8.EXE's weapon table.
fn weapons(gd: &GameData) -> Result<()> {
    let w = &gd.weapons;
    if w.list.is_empty() {
        say!("no weapon table (U8.EXE isn't in the install, or isn't one we know)");
        return Ok(());
    }
    say!("overlay shapes by hold type: {:?}", w.overlay_shapes);
    say!("shape  hold  damage      dex+atk dex+def armour  type");
    for x in &w.list {
        say!(
            "{:5}  {:4}  {:2}+0..{:<2}    {:7} {:7} {:6}  {:#04x}",
            x.shape,
            x.hold,
            x.base,
            x.modifier,
            x.dex_attack,
            x.dex_defend,
            x.armour,
            x.damage_type
        );
    }
    Ok(())
}

fn info(gd: &GameData) -> Result<()> {
    let n = gd.shapes.count();
    let frames: usize = (0..n as u32).map(|i| gd.shapes.get(i).len()).sum();
    say!("shapes: {n}, frames: {frames}, maps: {}, globs: {}", gd.fixed.len(), gd.globs.0.len());
    for m in 0..gd.fixed.len() {
        let it = gd.map_items(m);
        if !it.is_empty() {
            say!("  map {m}: {} items", it.len());
        }
    }
    Ok(())
}

fn map(a: &Args, gd: &GameData) -> Result<()> {
    let m: u16 = a.num(2, "3", "map")?;
    let t = std::time::Instant::now();
    let mut world = World::build(gd, m);
    ensure!(!world.is_empty(), "map {m} has nothing to draw");
    let mut painter = Painter::new();
    painter.sync(gd, &mut world, [0; 4]);
    let b = painter.bounds();
    say!("map {m}: {} items, built in {:?}, bounds {b:?}", world.len(), t.elapsed());
    // Size: a number, or `all` for the whole map's extent.
    let dim = |i: usize, default: &str, extent: i32| -> Result<i32> {
        let v = match a.get(i, default).as_str() {
            "all" => extent,
            s => crate::cli::num(s, "size")?,
        };
        ensure!(v > 0 && v <= 16384, "size must be 1–16384, got {v}");
        Ok(v)
    };
    let (w, h) = (dim(5, "640", b[2] - b[0])?, dim(6, "480", b[3] - b[1])?);
    let pos = |i: usize, lo: i32, hi: i32, size: i32, all: bool| -> Result<i32> {
        Ok(match a.get(i, "c").as_str() {
            _ if all => lo,
            "c" => (lo + hi) / 2 - size / 2,
            s => crate::cli::num(s, "position")?,
        })
    };
    let cam = (pos(3, b[0], b[2], w, a.get(5, "") == "all")?, pos(4, b[1], b[3], h, a.get(6, "") == "all")?);
    let (s, up) = upscaler(&a.get(7, "1"))?;
    let (bw, bh) = (w as u32 * s, h as u32 * s);
    ensure!((bw as u64) * (bh as u64) <= 400_000_000, "output {bw}x{bh} is too large; lower the scale or area");
    let mut buf = vec![0u8; (bw * bh * 4) as usize];
    let opts = DrawOpts { editor: a.rest(9).iter().any(|f| f == "editor"), max_z: a.cut(9)?, ..Default::default() };
    painter.draw(gd, &mut world, &mut SpriteCache::new(s, up), cam, &mut Canvas::new(&mut buf, bw, bh), opts);
    save(Path::new(&a.get(8, "map.png")), bw, bh, buf)
}

fn items(a: &Args, gd: &GameData) -> Result<()> {
    let m: u16 = a.num(2, "3", "map")?;
    let (x, y): (i32, i32) = (a.num(3, "0", "x")?, a.num(4, "0", "y")?);
    let mut world = World::build(gd, m);
    let opts = DrawOpts { editor: true, ..Default::default() };
    for id in Painter::new().pick_all(gd, &mut world, x, y, true, opts) {
        let Some(it) = world.get(id) else { continue };
        let si = gd.typeflags.get(it.shape);
        say!(
            "shape {:4} frame {:2} @ ({}, {}, {}) family {:2} flags {:#06x} item-flags {:#06x} q {}",
            it.shape,
            it.frame,
            it.x,
            it.y,
            it.z,
            si.family,
            si.flags,
            it.flags,
            it.quality
        );
    }
    Ok(())
}

fn avatar(gd: &GameData) -> Result<()> {
    match gd.avatar() {
        Some((m, it)) => {
            let (sx, sy) = project(it.x, it.y, it.z);
            say!(
                "map {m} world ({}, {}, {}) screen ({sx}, {sy}) shape {} frame {}",
                it.x,
                it.y,
                it.z,
                it.shape,
                it.frame
            );
        }
        None => say!("no Avatar in GAMEDAT/ITEMCACH.DAT"),
    }
    Ok(())
}

/// Every triggering egg on a map: what it does and where teleporters lead.
fn eggs(a: &Args, gd: &GameData) -> Result<()> {
    let m: u16 = a.num(2, "40", "map")?;
    let world = World::build(gd, m);
    let mut other: std::collections::HashMap<u16, World> = Default::default();
    for &idx in world.eggs() {
        let (e, id) = (world.entry(idx), world.id(idx));
        let it = e.it;
        let (rx, ry) = (it.npcnum >> 4, it.npcnum & 0xF);
        let at = format!("{id:5} ({:5}, {:5}, {:3}) range {rx}×{ry}", it.x, it.y, it.z);
        match e.traits.family {
            family::TELEPORTEGG if eggs::is_teleporter(&it) => {
                let (tm, tid) = (it.mapnum as u16, (it.quality & 0xFF) as u8);
                let dest = if tm == m {
                    world.teleport_destination(tid)
                } else {
                    other.entry(tm).or_insert_with(|| World::build(gd, tm)).teleport_destination(tid)
                };
                let to = dest.map_or("MISSING".into(), |(x, y, z)| format!("({x}, {y}, {z})"));
                say!("{at} teleport → map {tm} #{tid} {to}");
            }
            family::TELEPORTEGG => say!("{at} destination #{}", it.quality & 0xFF),
            family::MONSTEREGG => {
                say!(
                    "{at} monster shape {} chance {} id {} activity {} (hatched by scripts)",
                    it.quality & 0x7FF,
                    it.quality >> 11,
                    it.mapnum >> 3,
                    it.mapnum & 7
                )
            }
            _ => say!("{at} usecode egg shape {} id {} quality {:#x}", it.shape, it.mapnum, it.quality),
        }
    }
    Ok(())
}

/// Saved games in SAVEGAME/, with their descriptions.
/// `kit <slot> [from=N] [at=place]`: a tester's save (u8engine::kit) in slot N (1–11 show in the
/// game's diary), made from saved game `from` (default: a new game), the Avatar taken to `at`.
fn kit(a: &Args) -> Result<()> {
    let slot: u32 = a.num(2, "", "slot")?;
    let opt = |k: &str| (3..a.len()).find_map(|i| a.get(i, "").strip_prefix(k).map(str::to_string));
    let from = opt("from=").map(|n| n.parse::<u32>()).transpose().context("from=N")?;
    let mut g = u8engine::Game::open(a.data(), from, None).map_err(anyhow::Error::msg)?;
    if g.needs_name() {
        g.set_avatar_name("Avatar");
    }
    if let Some(place) = opt("at=") {
        let names: Vec<_> = u8engine::kit::PLACES.iter().map(|p| p.0).collect();
        ensure!(g.go_to(&place), "no place {place:?} (one of {})", names.join(", "));
        g.update(0.1, &Default::default()); // (arrive)
    }
    g.give_test_kit().context("no Avatar")?;
    let desc = format!("Test kit{}", opt("at=").map_or(String::new(), |p| format!(" ({p})")));
    let p = g.save(a.data(), slot, &desc).map_err(anyhow::Error::msg)?;
    say!("wrote {} (map {}): STR/DEX/INT 25, a bag of every school's magic and the recall item", p.display(), g.map());
    Ok(())
}

fn saves(a: &Args) -> Result<()> {
    for (slot, text) in u8data::save::list(a.data()) {
        let save = u8data::save::Save::load(a.data(), slot)?;
        let gd_name = save.get("AVATAR.DAT").and_then(u8data::save::avatar_name).unwrap_or_default();
        let ticks = save.clock();
        // Game clock: 450 a game minute (30 a second: a game hour is 15 real minutes).
        let played = ticks.map_or("-".into(), |t| {
            let m = t / 450;
            format!("day {} {:02}:{:02}", m / 1440 + 1, m / 60 % 24, m % 60)
        });
        say!("{slot:3}  {text:30} {gd_name:10} {played}");
    }
    Ok(())
}
