//! The isometric painter: a screen-space index over a [`World`], the per-frame depth sort of
//! what's on screen, drawing, and picking items under a point.
//!
//! The index follows the world incrementally through [`World::take_dirty`]; a new world (map
//! change) is detected by its serial and indexed from scratch.
use crate::{overlap, project, Canvas, SpriteCache};
use std::collections::HashMap;
use u8data::{map::Item, GameData};
use u8engine::world::{item_flag::FLIPPED, Entry, ObjId, Overlay, World};

/// Screen-space culling cell and sort-neighbourhood cell (game pixels).
const CELL: i32 = 128;
const SORT_CELL: i32 = 64;
const NO_RECT: [i32; 4] = [0; 4];

/// What to show.
#[derive(Clone, Copy, Debug)]
pub struct DrawOpts {
    pub roofs: bool,
    pub editor: bool,
    /// Cutaway: hide everything whose base is above this z (to see inside buildings).
    pub max_z: Option<i32>,
}

impl Default for DrawOpts {
    fn default() -> Self {
        Self { roofs: true, editor: false, max_z: None }
    }
}

/// The Avatar flagged unseen (Fade from Sight, Flash): the original still draws him, as a
/// recolouring of what's behind him (CAMERA.C, DLITEM.C); anyone else so flagged isn't drawn.
fn unseen(world: &World, i: usize) -> bool {
    let e = world.entry(i);
    world.id(i) == u8engine::world::AVATAR && e.present() && e.it.flags & u8engine::world::item_flag::INVISIBLE != 0
}

fn cells(r: [i32; 4], c: i32) -> impl Iterator<Item = (i32, i32)> {
    let (x0, y0, x1, y1) =
        (r[0].div_euclid(c), r[1].div_euclid(c), (r[0] + r[2]).div_euclid(c), (r[1] + r[3]).div_euclid(c));
    (x0..=x1).flat_map(move |x| (y0..=y1).map(move |y| (x, y)))
}

/// Screen rect [x, y, w, h] of an item's sprite; with `all_frames`, covering every frame (so
/// animation never escapes the sort's overlap test).
fn sprite_rect(gd: &GameData, it: &Item, all_frames: bool) -> Option<[i32; 4]> {
    let (sx, sy) = project(it.x, it.y, it.z);
    let shape = gd.shapes.get(it.shape);
    let flip = it.flags & FLIPPED != 0;
    let mut r = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
    let frames: Box<dyn Iterator<Item = _>> =
        if all_frames { Box::new(shape.iter()) } else { Box::new(shape.get(it.frame as usize).into_iter()) };
    for f in frames.filter(|f| f.width > 0) {
        let l = f.left(flip);
        r = [r[0].min(l), r[1].min(-f.yoff), r[2].max(l + f.width as i32), r[3].max(f.height as i32 - f.yoff)];
    }
    (r[0] <= r[2]).then(|| [sx + r[0], sy + r[1], r[2] - r[0], r[3] - r[1]])
}

/// Is screen point (sx, sy) on an opaque pixel of the item's sprite?
fn hit(gd: &GameData, it: &Item, sx: i32, sy: i32) -> bool {
    let (px, py) = project(it.x, it.y, it.z);
    let flip = it.flags & FLIPPED != 0;
    gd.shapes.frame(it.shape, it.frame).is_some_and(|f| {
        let lx = sx - px - f.left(flip);
        f.hit(if flip { f.width as i32 - 1 - lx } else { lx }, sy - py + f.yoff)
    })
}

/// True if `a` must be painted before `b`. Port of ScummVM's SortItem::below.
/// What the sort knows of an item (ScummVM's SortItem): its box in the world (x, y, z the
/// near-bottom corner; x_left, y_far, z_top the far ones) and that box's outline on screen.
struct Sorted {
    x: i32,
    y: i32,
    z: i32,
    x_left: i32,
    y_far: i32,
    z_top: i32,
    /// Screen: the box's left and right extremes, its top (far) and bottom (near) corners.
    sx_left: i32,
    sx_right: i32,
    sx_top: i32,
    sy_top: i32,
    sx_bot: i32,
    sy_bot: i32,
    flat: bool,
    big_square: bool,
    land: bool,
    roof: bool,
    draw: bool,
    solid: bool,
    occl: bool,
    trans: bool,
    anim: bool,
    shape: u32,
    frame: u32,
}

impl Sorted {
    fn of(e: &Entry) -> Self {
        let (x, y, z) = (e.it.x, e.it.y, e.it.z);
        let (x_left, y_far, z_top) = (e.x0, e.y0, e.z1);
        let t = &e.traits;
        // (Each term divided on its own, as ScummVM does: its rounding is part of the result.)
        Sorted {
            x,
            y,
            z,
            x_left,
            y_far,
            z_top,
            sx_left: x_left / 4 - y / 4,
            sx_right: x / 4 - y_far / 4,
            sx_top: x_left / 4 - y_far / 4,
            sy_top: x_left / 8 + y_far / 8 - z_top,
            sx_bot: x / 4 - y / 4,
            sy_bot: x / 8 + y / 8 - z,
            flat: z_top == z,
            big_square: x - x_left == y - y_far && x - x_left >= 128,
            land: t.land,
            roof: t.roof,
            draw: t.draw,
            solid: t.solid,
            occl: t.occl,
            trans: t.trans,
            anim: t.anim,
            shape: e.it.shape,
            frame: e.it.frame,
        }
    }

    /// Do the two boxes' outlines on screen overlap (given that the sprites do)? ScummVM tests
    /// the far and near corners against the outlines' slanted sides.
    fn overlaps(&self, o: &Sorted) -> bool {
        let top = (self.sx_top - o.sx_bot, self.sy_top - o.sy_bot);
        let bot = (self.sx_bot - o.sx_top, self.sy_bot - o.sy_top);
        let clear = self.sx_right <= o.sx_left
            || self.sx_left >= o.sx_right
            || top.0 + top.1 * 2 >= 0
            || -top.0 + top.1 * 2 >= 0
            || bot.0 - bot.1 * 2 >= 0
            || -bot.0 - bot.1 * 2 >= 0;
        !clear
    }
}

/// Is `a` drawn before (behind) `b`, the two overlapping on screen? ScummVM's SortItem::below
/// for U8: clearly apart in z, y or x (boxes flat in y or x compared to the 32-unit footpad,
/// z with 8 units' slack), then flats, roofs and the rest by kind and position.
fn below(a: &Sorted, b: &Sorted) -> bool {
    // Clearly in z.
    if a.flat && b.flat {
        if a.z != b.z {
            return a.z < b.z;
        }
    } else {
        if a.z_top <= b.z {
            return true;
        }
        if a.z >= b.z_top {
            return false;
        }
    }
    // Clearly in y.
    let (y_flat_a, y_flat_b) = (a.y_far == a.y, b.y_far == b.y);
    if y_flat_a && y_flat_b {
        if a.y / 32 != b.y / 32 {
            return a.y < b.y;
        }
    } else {
        if a.y <= b.y_far {
            return true;
        }
        if a.y_far >= b.y {
            return false;
        }
    }
    // Clearly in x.
    let (x_flat_a, x_flat_b) = (a.x_left == a.x, b.x_left == b.x);
    if x_flat_a && x_flat_b {
        if a.x / 32 != b.x / 32 {
            return a.x < b.x;
        }
    } else {
        if a.x <= b.x_left {
            return true;
        }
        if a.x_left >= b.x {
            return false;
        }
    }
    // In z, with slack.
    if a.z_top - 8 <= b.z && a.z < b.z_top - 8 {
        return true;
    }
    if a.z >= b.z_top - 8 && a.z_top - 8 > b.z {
        return false;
    }
    // Flat in y against not.
    if y_flat_a != y_flat_b {
        if a.y / 32 <= b.y_far / 32 {
            return true;
        }
        if a.y_far / 32 >= b.y / 32 {
            return false;
        }
        let (ca, cb) = ((a.y_far / 32 + a.y / 32) / 2, (b.y_far / 32 + b.y / 32) / 2);
        if ca != cb {
            return ca < cb;
        }
    }
    // Flat in x against not.
    if x_flat_a != x_flat_b {
        if a.x / 32 <= b.x_left / 32 {
            return true;
        }
        if a.x_left / 32 >= b.x / 32 {
            return false;
        }
        let (ca, cb) = ((a.x_left / 32 + a.x / 32) / 2, (b.x_left / 32 + b.x / 32) / 2);
        if ca != cb {
            return ca < cb;
        }
    }
    // Flat things (in z).
    if a.flat || b.flat {
        if a.z != b.z {
            return a.z < b.z;
        }
        // First: flats, then the opaque, still, drawn, solid, occluding, big squares.
        for (p, q) in [
            (a.flat, b.flat),
            (!a.trans, !b.trans),
            (!a.anim, !b.anim),
            (a.draw, b.draw),
            (a.solid, b.solid),
            (a.occl, b.occl),
            (a.big_square, b.big_square),
        ] {
            if p != q {
                return p;
            }
        }
    }
    if a.x == b.x && a.y == b.y && a.trans != b.trans {
        return !a.trans;
    }
    // Land before roof; roofs first.
    if a.land && b.land && a.roof != b.roof {
        return !a.roof;
    }
    if a.roof != b.roof {
        return a.roof;
    }
    if a.z != b.z {
        return a.z < b.z;
    }
    // (ScummVM's next rule puts what's further right on screen first. Two things flat in y on
    // the same plane (pictures side by side on a wall) DOS draws nearer, larger x, last: the
    // tapestries on map 41 at x 18751 and 18815. Those go on to the x + y order below.)
    let same_plane = y_flat_a && y_flat_b && a.y == b.y;
    if !same_plane && (x_flat_a || x_flat_b || y_flat_a || y_flat_b) {
        // Further right on screen first; then higher on screen.
        if a.sx_left != b.sx_left {
            return a.sx_left > b.sx_left;
        }
        if a.sy_bot != b.sy_bot {
            return a.sy_bot < b.sy_bot;
        }
    }
    (a.x + a.y, a.x_left + a.y_far, a.y, a.x, a.shape, a.frame)
        < (b.x + b.y, b.x_left + b.y_far, b.y, b.x, b.shape, b.frame)
}

/// Where entry `idx`'s sprite goes on screen (with any weapon drawn over it); nowhere if it's
/// inside a container.
fn place(gd: &GameData, world: &World, idx: usize) -> [i32; 4] {
    let e = world.entry(idx);
    if e.parent.is_some() {
        return NO_RECT;
    }
    let Some(r) = sprite_rect(gd, &e.it, e.traits.anim) else { return NO_RECT };
    match world.overlay(world.id(idx)).and_then(|o| overlay_rect(gd, &e.it, o)) {
        Some(w) => {
            let (x0, y0) = (r[0].min(w[0]), r[1].min(w[1]));
            [x0, y0, (r[0] + r[2]).max(w[0] + w[2]) - x0, (r[1] + r[3]).max(w[1] + w[3]) - y0]
        }
        None => r,
    }
}

/// Screen rect of a weapon drawn over `it`.
fn overlay_rect(gd: &GameData, it: &Item, o: Overlay) -> Option<[i32; 4]> {
    let (sx, sy) = project(it.x, it.y, it.z);
    let f = gd.shapes.frame(o.shape, o.frame).filter(|f| f.width > 0)?;
    Some([sx + o.dx + f.left(o.flip), sy + o.dy - f.yoff, f.width as i32, f.height as i32])
}

/// [`project`] in 1/`s` game pixels (the same at `s` 1; finer, the rounding goes).
pub fn project_fine(x: i32, y: i32, z: i32, s: i32) -> (i32, i32) {
    (x * s / 4 - y * s / 4, x * s / 8 + y * s / 8 - z * s)
}

pub struct Painter {
    serial: u64,
    rects: Vec<[i32; 4]>,
    grid: HashMap<(i32, i32), Vec<usize>>,
    bounds: [i32; 4],
}

impl Default for Painter {
    fn default() -> Self {
        Self::new()
    }
}

impl Painter {
    pub fn new() -> Self {
        Self { serial: 0, rects: vec![], grid: HashMap::new(), bounds: [0; 4] }
    }

    /// Screen extent of the map when it was indexed ([x0, y0, x1, y1], hidden items excluded).
    pub fn bounds(&self) -> [i32; 4] {
        self.bounds
    }
    pub fn centre(&self) -> (i32, i32) {
        ((self.bounds[0] + self.bounds[2]) / 2, (self.bounds[1] + self.bounds[3]) / 2)
    }

    fn file(&mut self, idx: usize) {
        for c in cells(self.rects[idx], CELL) {
            self.grid.entry(c).or_default().push(idx);
        }
    }
    fn unfile(&mut self, idx: usize) {
        for c in cells(self.rects[idx], CELL) {
            if let Some(v) = self.grid.get_mut(&c) {
                v.retain(|&i| i != idx);
            }
        }
    }

    fn rebuild(&mut self, gd: &GameData, world: &World) {
        self.serial = world.serial();
        self.grid.clear();
        self.rects = (0..world.entries().len()).map(|i| place(gd, world, i)).collect();
        (0..self.rects.len()).for_each(|i| self.file(i));
        let mut b = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
        for (e, r) in world.entries().iter().zip(&self.rects) {
            if e.shown() && *r != NO_RECT {
                b = [b[0].min(r[0]), b[1].min(r[1]), b[2].max(r[0] + r[2]), b[3].max(r[1] + r[3])];
            }
        }
        self.bounds = if b[0] > b[2] { [0; 4] } else { b };
    }

    /// Catch up with changes in the world. Returns true if anything changed inside `view`
    /// ([x, y, w, h], unscaled screen) — i.e. whether a redraw is needed.
    pub fn sync(&mut self, gd: &GameData, world: &mut World, view: [i32; 4]) -> bool {
        if world.serial() != self.serial {
            world.take_dirty();
            self.rebuild(gd, world);
            return true;
        }
        let mut visible = false;
        for idx in world.take_dirty() {
            let new = place(gd, world, idx);
            if idx >= self.rects.len() {
                self.rects.resize(idx + 1, NO_RECT);
            }
            let old = self.rects[idx];
            visible |= overlap(&old, &view) || overlap(&new, &view);
            if old != new {
                self.unfile(idx);
                self.rects[idx] = new;
                self.file(idx);
            }
        }
        visible
    }

    /// Screen rect of an item's sprite as last synced (None inside a container: not on the map).
    pub fn rect(&self, world: &World, id: ObjId) -> Option<[i32; 4]> {
        world.index(id).and_then(|i| self.rects.get(i)).copied().filter(|r| *r != NO_RECT)
    }

    /// Indices of shown items whose rect touches `view`, in a stable order.
    fn in_view(&self, world: &World, view: [i32; 4], o: DrawOpts) -> Vec<usize> {
        let mut seen = vec![false; self.rects.len()];
        let mut v: Vec<usize> = cells(view, CELL)
            .flat_map(|c| self.grid.get(&c).into_iter().flatten().copied())
            .filter(|&i| !std::mem::replace(&mut seen[i], true))
            .filter(|&i| {
                let e = world.entry(i);
                // Editor view also shows eggs, markers and invisible items.
                (if o.editor { e.present() } else { e.shown() || unseen(world, i) })
                    && overlap(&self.rects[i], &view)
                    && !(e.traits.roof && !o.roofs)
                    && !(o.max_z.is_some_and(|z| e.it.z > z) && !e.traits.draw)
            })
            .collect();
        // (ScummVM's list order: lower first, flats before the rest at a height.)
        v.sort_unstable_by_key(|&i| {
            let e = world.entry(i);
            (e.it.z, e.z1 != e.it.z, e.it.x + e.it.y, i)
        });
        v
    }

    /// Painter's order for a set of items (DFS: everything below an item is painted first).
    fn sort(&self, world: &World, vis: &[usize]) -> Vec<usize> {
        let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (k, &i) in vis.iter().enumerate() {
            cells(self.rects[i], SORT_CELL).for_each(|c| grid.entry(c).or_default().push(k));
        }
        let boxes: Vec<Sorted> = vis.iter().map(|&i| Sorted::of(world.entry(i))).collect();
        let mut state = vec![0u8; vis.len()];
        let mut order = Vec::with_capacity(vis.len());
        for root in 0..vis.len() {
            let mut stack = vec![(root, false)];
            while let Some((k, done)) = stack.pop() {
                if done {
                    state[k] = 2;
                    order.push(vis[k]);
                    continue;
                }
                if state[k] != 0 {
                    continue;
                }
                state[k] = 1;
                stack.push((k, true));
                let (me, mr) = (&boxes[k], &self.rects[vis[k]]);
                let mut deps: Vec<usize> = cells(*mr, SORT_CELL)
                    .flat_map(|c| grid.get(&c).into_iter().flatten().copied())
                    .filter(|&j| {
                        j != k
                            && state[j] == 0
                            && overlap(mr, &self.rects[vis[j]])
                            && me.overlaps(&boxes[j])
                            && below(&boxes[j], me)
                    })
                    .collect();
                deps.sort_unstable();
                deps.dedup();
                stack.extend(deps.into_iter().rev().map(|j| (j, false)));
            }
        }
        order
    }

    /// Draw the world with `cam` (unscaled screen coords) at the canvas's top-left. The canvas is
    /// in sprite-cache pixels (`cache.scale` per game pixel).
    pub fn draw(
        &mut self,
        gd: &GameData,
        world: &mut World,
        cache: &mut SpriteCache,
        cam: (i32, i32),
        canvas: &mut Canvas,
        opts: DrawOpts,
    ) {
        let s = cache.scale as i32;
        let view = [cam.0, cam.1, canvas.w.div_ceil(s as u32) as i32, canvas.h.div_ceil(s as u32) as i32];
        self.sync(gd, world, view);
        cache.begin_frame();
        canvas.fill([0, 0, 0, 255]);
        for i in self.sort(world, &self.in_view(world, view, opts)) {
            let e = world.entry(i);
            let it = &e.it;
            let Some(sp) = cache.get(gd, it.shape, it.frame, it.flags & FLIPPED != 0) else { continue };
            // People to the canvas's pixel (they walk by less than a game pixel); the scenery
            // to whole game pixels, as the original lays it out.
            let (px, py) = if e.traits.actor {
                let (lx, ly, lz) = world.lag(world.id(i));
                project_fine(it.x + lx, it.y + ly, it.z + lz, s)
            } else {
                let (x, y) = project(it.x, it.y, it.z);
                (x * s, y * s)
            };
            let (px, py) = (px - cam.0 * s, py - cam.1 * s);
            let ghost = unseen(world, i) && !opts.editor;
            let draw = |canvas: &mut Canvas, sp: &crate::Sprite, x, y| match ghost {
                true => canvas.recolour(sp, x, y, |c| gd.xform.unseen(&gd.palette, c)),
                false => canvas.blit(sp, x, y),
            };
            draw(canvas, &sp, px - sp.xoff, py - sp.yoff);
            if let Some(o) = world.overlay(world.id(i)) {
                if let Some(wp) = cache.get(gd, o.shape, o.frame, o.flip) {
                    draw(canvas, &wp, px + o.dx * s - wp.xoff, py + o.dy * s - wp.yoff);
                }
            }
        }
    }

    /// The order things in `view` (screen rect) are drawn in, first to last (for checking the
    /// sort).
    pub fn order(&mut self, gd: &GameData, world: &mut World, view: [i32; 4], opts: DrawOpts) -> Vec<ObjId> {
        self.sync(gd, world, view);
        self.sort(world, &self.in_view(world, view, opts)).into_iter().map(|i| world.id(i)).collect()
    }

    /// Items under screen point (sx, sy), topmost first. `exact`: only where the sprite is opaque.
    pub fn pick_all(
        &mut self,
        gd: &GameData,
        world: &mut World,
        sx: i32,
        sy: i32,
        exact: bool,
        opts: DrawOpts,
    ) -> Vec<ObjId> {
        let view = [sx, sy, 1, 1];
        self.sync(gd, world, view);
        self.sort(world, &self.in_view(world, view, opts))
            .into_iter()
            .rev()
            .filter(|&i| !exact || hit(gd, &world.entry(i).it, sx, sy))
            .map(|i| world.id(i))
            .collect()
    }

    /// The topmost item drawn at screen point (sx, sy).
    pub fn pick(&mut self, gd: &GameData, world: &mut World, sx: i32, sy: i32, opts: DrawOpts) -> Option<ObjId> {
        self.pick_all(gd, world, sx, sy, true, opts).into_iter().next()
    }
}
