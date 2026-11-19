//! Setting up a scene as in the original: where the Avatar stands beside someone, what's loose to
//! take, and where to point at things in the original's 320 by 200 view.
use super::*;

/// A free spot beside someone, nearest first.
pub(super) fn beside(g: &Game, n: &map::Item) -> Option<(i32, i32, i32)> {
    for r in [96, 112, 128, 160, 80] {
        for (dx, dy) in [(0, 1), (1, 0), (0, -1), (-1, 0), (1, 1), (-1, -1), (1, -1), (-1, 1)] {
            let at = (n.x + dx * r, n.y + dy * r, n.z);
            if g.world.fits(&g.data, AVATAR, at) {
                return Some(at);
            }
        }
    }
    None
}

/// The way (0 north, clockwise) along (dx, dy).
pub(super) fn dir_to(dx: i32, dy: i32) -> usize {
    let ang = (dy as f32).atan2(dx as f32);
    (((ang / std::f32::consts::FRAC_PI_4).round() as i32 + 2).rem_euclid(8)) as usize
}

/// Something loose on the map that nobody owns: what the player could take.
pub(super) fn loose(g: &Game, id: u16) -> bool {
    let Some(i) = g.world.index(id) else { return false };
    let e = g.world.entry(i);
    let tf = g.data.typeflags.get(e.it.shape);
    id >= 256
        && e.parent.is_none()
        && !e.void
        && !e.gone
        && !e.traits.actor
        && !e.traits.egg
        && !e.traits.editor
        && e.origin == u8engine::world::Origin::Movable
        && !tf.has(u8data::typeflag::flag::FIXED)
        && e.it.flags & 0x4 == 0
        && tf.weight != 0
}

/// Where the middle of a thing's box is drawn, from the Avatar's feet (game pixels).
pub(super) fn offset(g: &Game, id: u16) -> (i32, i32) {
    let (Some(b), Some(a)) = (g.world.abs_box(id), g.world.get(AVATAR)) else { return (0, 0) };
    let (sx, sy) = project((b.x0 + b.x1) / 2, (b.y0 + b.y1) / 2, (b.z0 + b.z1) / 2);
    let (ax, ay) = project(a.x, a.y, a.z);
    (sx - ax, sy - ay)
}

/// Where to point at a thing in the original's 320 by 200 view: a pixel where it is the topmost
/// thing drawn, nearest the middle of all such pixels. None if it is hidden.
pub(super) fn aim(g: &mut Game, id: u16) -> Option<(i32, i32)> {
    let (x, y, z) = g.camera()?;
    let (sx, sy) = project(x, y, z);
    let cam = (sx - 160, sy - 100);
    let max_z = g.roof();
    let opts = DrawOpts { max_z, ..Default::default() };
    let mut p = Painter::new();
    p.sync(&g.data, &mut g.world, [cam.0, cam.1, 320, 200]);
    let r = p.rect(&g.world, id)?;
    let mut pts = vec![];
    for py in r[1]..r[1] + r[3] {
        for px in r[0]..r[0] + r[2] {
            let (vx, vy) = (px - cam.0, py - cam.1);
            if (2..318).contains(&vx)
                && (2..196).contains(&vy)
                && p.pick(&g.data, &mut g.world, px, py, opts) == Some(id)
            {
                pts.push((vx, vy));
            }
        }
    }
    if pts.is_empty() {
        return None;
    }
    let n = pts.len() as i32;
    let (mx, my) = (pts.iter().map(|p| p.0).sum::<i32>() / n, pts.iter().map(|p| p.1).sum::<i32>() / n);
    // (the original draws its view two pixels lower)
    pts.into_iter().min_by_key(|p| (p.0 - mx).pow(2) + (p.1 - my).pow(2)).map(|p| (p.0, p.1 + 2))
}
