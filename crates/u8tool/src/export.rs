//! Image exports: shape frames, contact sheets, actor animations, fonts, mouse cursors, gumps.
use crate::cli::{paste, save, upscaler, Args};
use anyhow::{anyhow, ensure, Result};
use std::path::{Path, PathBuf};
use u8data::{anim::AnimFrame, flex::Flex, font::Fonts, GameData};
use u8render::SpriteCache;

pub fn run(a: &Args, gd: &GameData) -> Result<()> {
    match a.get(1, "").as_str() {
        "shape" => shape(a, gd),
        "sheet" => sheet(a, gd),
        "anim" => anim(a, gd),
        "cursors" => cursors(a, gd),
        "gumps" => gumps(a, gd),
        "film" => film(a, gd),
        _ => fonts(a, gd),
    }
}

/// Stills of a film (with the subtitle, as the viewer shows it; fades left out) at the given seconds.
fn film(a: &Args, gd: &GameData) -> Result<()> {
    let fonts = Fonts::parse(&Flex::parse(std::fs::read(u8data::find_file(a.data(), "STATIC/U8FONTS.FLX")?)?)?);
    use u8engine::film::{Film, HEIGHT, WIDTH};
    let name = if a.get(2, "intro") == "endgame" { "STATIC/ENDGAME.SKF" } else { "STATIC/{}INTRO.SKF" };
    let skf = u8data::skf::Skf::parse(std::fs::read(u8data::lang::file(a.data(), name)?)?)?;
    let mut film = Film::new(skf);
    let times: Vec<f32> =
        a.get(3, "5,20,40,60,80").split(',').map(|t| crate::cli::num(t, "seconds")).collect::<Result<_>>()?;
    let prefix = a.get(4, "film");
    let mut now = 0.0;
    for t in times {
        while now < t && !film.done() {
            film.tick(0.05);
            now += 0.05;
        }
        let pal = film.palette();
        let mut rgba: Vec<u8> = film.screen().iter().flat_map(|&p| pal.0[p as usize]).collect();
        if let Some((y, t)) = film.subtitle() {
            let im = fonts.render(6, &fonts.wrap(6, t, 200), &gd.palette, None);
            paste(
                &mut rgba,
                WIDTH as u32,
                (&im.rgba, im.w, im.h),
                (60 + (200 - im.w.min(200)) / 2, *y as u32),
                u32::MAX,
            );
        }
        let out = format!("{prefix}_{t}.png");
        save(Path::new(&out), WIDTH as u32, HEIGHT as u32, rgba)?;
        say!("{out}{}", film.subtitle().map_or(String::new(), |s| format!("  \"{}\"", s.1)));
    }
    Ok(())
}

fn shape(a: &Args, gd: &GameData) -> Result<()> {
    let n: u32 = a.num(2, "0", "shape")?;
    ensure!((n as usize) < gd.shapes.count(), "shape {n} out of range (0–{})", gd.shapes.count() - 1);
    let (s, up) = upscaler(&a.get(3, "1"))?;
    let dir = PathBuf::from(a.get(4, "out"));
    std::fs::create_dir_all(&dir)?;
    let mut sc = SpriteCache::new(s, up);
    for f in 0..gd.shapes.get(n).len() as u32 {
        if let Some(sp) = sc.get(gd, n, f, false) {
            save(&dir.join(format!("shape{n:04}_{f:02}.png")), sp.w, sp.h, sp.rgba.clone())?;
        }
    }
    Ok(())
}

fn sheet(a: &Args, gd: &GameData) -> Result<()> {
    let (first, count): (u32, u32) = (a.num(2, "0", "first")?, a.num(3, "64", "count")?);
    ensure!(count > 0, "count must be > 0");
    let (s, up) = upscaler(&a.get(4, "1"))?;
    let mut sc = SpriteCache::new(s, up);
    let sprites: Vec<_> = (first..first + count).map(|i| sc.get(gd, i, 0, false)).collect();
    // Cells fit the largest sprite (capped so one huge shape can't blow up the sheet).
    let cell = sprites.iter().flatten().map(|sp| sp.w.max(sp.h)).max().unwrap_or(1).min(256 * s) + 4;
    let cols = 8.min(count);
    let (w, h) = (cols * cell, count.div_ceil(cols) * cell);
    let mut buf = [40u8, 40, 40, 255].repeat((w * h) as usize);
    for (i, sp) in sprites.iter().enumerate() {
        if let Some(sp) = sp {
            let (ox, oy) = ((i as u32 % cols) * cell + 2, (i as u32 / cols) * cell + 2);
            paste(&mut buf, w, (&sp.rgba, sp.w, sp.h), (ox, oy), cell - 4);
        }
    }
    save(Path::new(&a.get(5, "sheet.png")), w, h, buf)
}

fn anim(a: &Args, gd: &GameData) -> Result<()> {
    let (shape, act): (u32, usize) = (a.num(2, "1", "shape")?, a.num(3, "0", "action")?);
    let action = gd.anims.get(shape, act).ok_or_else(|| anyhow!("shape {shape} has no action {act}"))?;
    let (s, up) = upscaler(&a.get(4, "1"))?;
    let mut sc = SpriteCache::new(s, up);
    // Optionally a held weapon: its overlay shape and how it's held (WPNOVLAY.DAT type).
    let weapon: Option<(u32, usize)> = match (a.get(6, ""), a.get(7, "0")) {
        (s, k) if !s.is_empty() => Some((s.parse()?, k.parse()?)),
        _ => None,
    };
    let cols = action.frames[0].len().max(1) as u32;
    let cell = 128 * s;
    let (w, h) = (cols * cell, 8 * cell);
    let mut buf = [40u8, 40, 40, 255].repeat((w * h) as usize);
    for (dir, frames) in action.frames.iter().enumerate() {
        for (i, af) in frames.iter().enumerate() {
            let flip = af.flags & AnimFrame::FLIPPED != 0;
            let Some(sp) = sc.get(gd, shape, af.frame, flip) else { continue };
            // Anchor every frame at its hotspot so the motion lines up.
            let (ax, ay) = (i as i32 * cell as i32 + cell as i32 / 2, dir as i32 * cell as i32 + cell as i32 * 3 / 4);
            let held = weapon.and_then(|(ws, kind)| {
                let o = gd.overlays.get(act, kind, dir, i)?;
                let wsp = sc.get(gd, ws, o.frame, flip)?;
                let dx = if flip { -o.dx } else { o.dx };
                Some((wsp, ax + dx * s as i32, ay + o.dy * s as i32))
            });
            for (sp, ax, ay) in std::iter::once((sp, ax, ay)).chain(held) {
                for y in 0..sp.h as i32 {
                    for x in 0..sp.w as i32 {
                        let si = ((y * sp.w as i32 + x) * 4) as usize;
                        let (dx, dy) = (ax - sp.xoff + x, ay - sp.yoff + y);
                        if sp.rgba[si + 3] < 128 || dx < 0 || dy < 0 || dx >= w as i32 || dy >= h as i32 {
                            continue;
                        }
                        let di = ((dy as u32 * w + dx as u32) * 4) as usize;
                        buf[di..di + 4].copy_from_slice(&sp.rgba[si..si + 4]);
                    }
                }
            }
        }
    }
    say!("shape {shape} action {act}: {cols} frames × 8 dirs, repeat {}, flags {:#06x}", action.repeat, action.flags);
    save(Path::new(&a.get(5, "anim.png")), w, h, buf)
}

fn fonts(a: &Args, gd: &GameData) -> Result<()> {
    let fonts = Fonts::parse(&Flex::parse(std::fs::read(u8data::find_file(a.data(), "STATIC/U8FONTS.FLX")?)?)?);
    let text = a.get(2, "Greetings! Who are you that you dare address the Tempest?");
    let imgs: Vec<_> =
        (0..16).map(|f| fonts.render(f, &fonts.wrap(f, &format!("{f}: {text}"), 300), &gd.palette, None)).collect();
    let (w, h) = (imgs.iter().map(|i| i.w).max().unwrap_or(1), imgs.iter().map(|i| i.h + 4).sum::<u32>());
    let mut buf = [60u8, 60, 60, 255].repeat((w * h) as usize);
    let mut y = 0;
    for im in &imgs {
        paste(&mut buf, w, (&im.rgba, im.w, im.h), (0, y), u32::MAX);
        y += im.h + 4;
    }
    save(Path::new(&a.get(3, "fonts.png")), w, h, buf)
}

/// Every mouse cursor (STATIC/U8MOUSE.SHP), numbered, 8 to a row.
fn cursors(a: &Args, gd: &GameData) -> Result<()> {
    let shape = u8data::shape::parse_shape(&std::fs::read(u8data::find_file(a.data(), "STATIC/U8MOUSE.SHP")?)?)?;
    let cell = shape.iter().map(|f| f.width.max(f.height)).max().unwrap_or(1) + 4;
    let (cols, n) = (8, shape.len() as u32);
    let (w, h) = (cols * cell, n.div_ceil(cols) * cell);
    let mut buf = [60u8, 60, 60, 255].repeat((w * h) as usize);
    for (i, f) in shape.iter().enumerate() {
        let (ox, oy) = ((i as u32 % cols) * cell + 2, (i as u32 / cols) * cell + 2);
        paste(&mut buf, w, (&f.to_rgba(&gd.palette, None), f.width, f.height), (ox, oy), cell);
        println!("{i:2}: {}x{} hotspot ({}, {})", f.width, f.height, f.xoff, f.yoff);
    }
    save(Path::new(&a.get(2, "cursors.png")), w, h, buf)
}

/// Gumps (U8GUMPS.FLX: windows, books, menus): every one's first frame, or (`list` = "35,37")
/// every frame of those, numbered.
fn gumps(a: &Args, gd: &GameData) -> Result<()> {
    let flex = Flex::parse(std::fs::read(u8data::find_file(a.data(), "STATIC/U8GUMPS.FLX")?)?)?;
    let shape = |i: usize| flex.get(i).and_then(|d| u8data::shape::parse_shape(d).ok()).unwrap_or_default();
    let pick: Vec<(usize, usize)> = match a.rest(3).first() {
        Some(list) => list
            .split(',')
            .map(|n| crate::cli::num::<usize>(n, "gump"))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flat_map(|g| (0..shape(g).len()).map(move |f| (g, f)))
            .collect(),
        None => (0..flex.len()).map(|g| (g, 0)).collect(),
    };
    let frames: Vec<_> =
        pick.iter().filter_map(|&(g, f)| Some((g, f, shape(g).get(f).cloned()?))).filter(|x| x.2.width > 0).collect();
    let cell = frames.iter().map(|x| x.2.width.max(x.2.height)).max().unwrap_or(1).min(320) + 4;
    let cols = (1600 / cell).max(1);
    let n = frames.len() as u32;
    let (w, h) = (cols * cell, n.div_ceil(cols).max(1) * cell);
    let mut buf = [60u8, 60, 60, 255].repeat((w * h) as usize);
    for (i, (g, f, fr)) in frames.iter().enumerate() {
        let (ox, oy) = ((i as u32 % cols) * cell + 2, (i as u32 / cols) * cell + 2);
        paste(&mut buf, w, (&fr.to_rgba(&gd.palette, None), fr.width, fr.height), (ox, oy), cell - 4);
        println!("{i:3}: gump {g} frame {f}: {}x{} hotspot ({}, {})", fr.width, fr.height, fr.xoff, fr.yoff);
    }
    save(Path::new(&a.get(2, "gumps.png")), w, h, buf)
}
