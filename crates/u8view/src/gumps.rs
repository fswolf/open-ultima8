//! Item windows ("gumps"): a container's contents or the Avatar's paperdoll, opened by the game's
//! scripts (`Item::openGump`). Items sit where the save says, inside the window's item area.
use crate::art::GumpArt;
use u8data::GameData;
use u8engine::{
    equip::{holds, paperdoll_look},
    world::World,
    ObjId,
};
use u8render::{Canvas, SpriteCache};

/// The Avatar's paperdoll window (U8GUMPS.FLX 1).
pub const PAPERDOLL: u32 = 1;
/// The pack drawn on the paperdoll figure's back ([x, y, w, h] in the window): stands for the
/// backpack (pick it up, open it, drop things on it).
const PACK: [i32; 4] = [49, 25, 10, 25];
/// The paperdoll's stats button ([x, y, w, h] in the window; U8GUMPS 38, frame 1 pressed): it
/// opens the little health and mana gauge (ScummVM's PaperdollGump).
pub const STATS_BUTTON: [i32; 4] = [81, 84, 70, 15];
const STATS_BUTTON_SHAPE: u32 = 38;
/// The health and mana gauge (U8GUMPS 33; ScummVM's MiniStatsGump): bars 3 wide from x 6 (hits)
/// and 13 (mana), bottom at y 19, 14 high when full, in these palette colours (left to right).
pub const GAUGE: u32 = 33;
const GAUGE_BARS: [(i32, [u8; 3]); 2] = [(6, [41, 39, 37]), (13, [138, 139, 141])];
const GAUGE_BOTTOM: i32 = 19;
const GAUGE_HEIGHT: i32 = 14;
/// Room left round the gauge when it's put in the view's bottom-left corner.
const GAUGE_MARGIN: i32 = 5;
/// The gauge is drawn this many times its original size (22×26 is small on today's screens).
pub const GAUGE_SCALE: i32 = 2;
/// Stands for the gauge where windows are named by their item (dragging, double clicks).
pub const GAUGE_ID: ObjId = ObjId::MAX;
/// Where a thing dropped onto a bag (not into its open window) goes in it.
pub const STOW_AT: (i32, i32) = (20, 20);

struct Win {
    item: ObjId,
    gump: u32,
    /// Top-left, game pixels in the view.
    at: (i32, i32),
}

/// What dropping a carried item on a window does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Drop {
    /// Into this container, at this window position.
    Into(ObjId, (i32, i32)),
    /// On the paperdoll: wear it (or, if it isn't something to wear, put it in the backpack).
    Wear,
    /// Over a window, but nowhere in it (never onto the map behind).
    Nowhere,
}

impl Win {
    /// Is `it` the backpack the paperdoll's figure wears?
    fn is_pack(&self, gd: &GameData, it: &u8data::map::Item) -> bool {
        self.gump == PAPERDOLL
            && it.flags & u8engine::world::item_flag::EQUIPPED != 0
            && gd.typeflags.get(it.shape).family == u8data::typeflag::family::CONTAINER
    }

    /// What's in the window, bottom first. On the paperdoll the slots go weapon, legs, body,
    /// head, arms, shield (CONTGUMP.C draws slot 5 down to 0: the shield over the rest), then
    /// the backpack over the clothes.
    /// Editor-only things (a chest's TRAP marker) aren't, unless `editor` (ScummVM's ContainerGump).
    fn contents(&self, gd: &GameData, world: &World, editor: bool) -> Vec<ObjId> {
        let mut c = world.contents(self.item);
        let marker =
            |id| world.get(id).is_some_and(|it| gd.typeflags.get(it.shape).has(u8data::typeflag::flag::EDITOR));
        c.retain(|&id| editor || !marker(id));
        if self.gump == PAPERDOLL {
            // Only what's worn: the original's paperdoll draws the slots, nothing else he holds.
            c.retain(|&id| world.get(id).is_some_and(|it| it.flags & u8engine::world::item_flag::EQUIPPED != 0));
            let slot =
                |id| world.get(id).and_then(|it| u8engine::equip::Slot::of(gd, it.shape)).map_or(0, |s| s as i32);
            c.sort_by_key(|&id| (holds(gd, world, id), -slot(id)));
        }
        c
    }

    /// How an item in the window shows: its origin (game pixels in the view) and frame, and
    /// whether it's drawn (the paperdoll's figure stands in for the backpack).
    fn look(&self, gd: &GameData, art: &GumpArt, it: &u8data::map::Item) -> Look {
        if self.is_pack(gd, it) {
            let f = gd.shapes.frame(it.shape, it.frame).map_or((0, 0, 0, 0), |f| (f.xoff, f.yoff, f.width, f.height));
            let centre = (PACK[0] + PACK[2] / 2, PACK[1] + PACK[3] / 2);
            let at = (self.at.0 + centre.0 + f.0 - f.2 as i32 / 2, self.at.1 + centre.1 + f.1 - f.3 as i32 / 2);
            return Look { at, frame: it.frame, shown: false };
        }
        let doll = self.gump == PAPERDOLL;
        let a = art.area(self.gump);
        let worn = if doll { paperdoll_look(gd, it) } else { None };
        let ((x, y), frame) = worn.unwrap_or(((it.x, it.y), it.frame));
        Look { at: (self.at.0 + a[0] + x, self.at.1 + a[1] + y), frame, shown: true }
    }
}

struct Look {
    at: (i32, i32),
    frame: u32,
    shown: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    Item(ObjId),
    Window(usize),
    /// The health and mana gauge.
    Gauge,
}

#[derive(Default)]
pub struct Windows {
    wins: Vec<Win>,
    /// Where the health and mana gauge is (top-left), when it's showing.
    pub gauge: Option<(i32, i32)>,
    /// Show editor-only things in windows (trap markers in chests), as the editor view does.
    pub editor: bool,
}

impl Windows {
    /// Open (or bring to the front) `item`'s window in a vw×vh view: near the middle, or by
    /// the window of `parent` (what it's in) or the one on top.
    pub fn open(&mut self, art: &GumpArt, item: ObjId, gump: u32, parent: Option<ObjId>, (vw, vh): (u32, u32)) {
        if let Some(i) = self.wins.iter().position(|w| w.item == item) {
            let w = self.wins.remove(i);
            self.wins.push(w);
            return;
        }
        let (w, h) = art.size(gump);
        let (vw, vh) = (vw as i32, vh as i32);
        let k = self.wins.len() as i32 * 16;
        let mut at = ((vw - w) / 2 + k - 40, (vh - h) / 2 + k - 30);
        // Beside the window it was in (else the one on top): right, else left, else below, if
        // there's room in the view and it covers no other window. A bag's window with no room
        // beside goes where the window it was in is, as DOS opens it.
        let parent = parent.and_then(|p| self.wins.iter().find(|w| w.item == p));
        if let Some(top) = parent.or(self.wins.last()) {
            let (tw, th) = art.size(top.gump);
            let clear = |(x, y): (i32, i32)| {
                self.wins.iter().all(|o| {
                    let (ow, oh) = art.size(o.gump);
                    x + w <= o.at.0 || o.at.0 + ow <= x || y + h <= o.at.1 || o.at.1 + oh <= y
                })
            };
            let fits = |(x, y): (i32, i32)| x >= 0 && y >= 0 && x + w <= vw && y + h <= vh && clear((x, y));
            let tries = [(top.at.0 + tw + 2, top.at.1), (top.at.0 - w - 2, top.at.1), (top.at.0, top.at.1 + th + 2)];
            // (Not over the paperdoll, though: the backpack's window cascades as before.)
            let bag_in = parent.filter(|p| p.gump != PAPERDOLL);
            at = tries.into_iter().find(|&p| fits(p)).unwrap_or(bag_in.map_or(at, |p| p.at));
        }
        let at = (at.0.min(vw - w).max(0), at.1.min(vh - h).max(0)); // (in the view)
        self.wins.push(Win { item, gump, at });
    }

    /// Close `item`'s window. True if it was open.
    pub fn close(&mut self, item: ObjId) -> bool {
        let n = self.wins.len();
        self.wins.retain(|w| w.item != item);
        n != self.wins.len()
    }

    /// Close them all; returns the items whose windows were open.
    pub fn close_all(&mut self) -> Vec<ObjId> {
        self.wins.drain(..).map(|w| w.item).collect()
    }

    /// The items whose windows are open.
    pub fn items(&self) -> Vec<ObjId> {
        self.wins.iter().map(|w| w.item).collect()
    }

    /// Is (x, y) on window `window`'s stats button (the paperdoll's)?
    pub fn on_stats_button(&self, window: usize, x: i32, y: i32) -> bool {
        self.wins.get(window).is_some_and(|w| {
            let (dx, dy) = (x - w.at.0 - STATS_BUTTON[0], y - w.at.1 - STATS_BUTTON[1]);
            w.gump == PAPERDOLL && (0..STATS_BUTTON[2]).contains(&dx) && (0..STATS_BUTTON[3]).contains(&dy)
        })
    }

    /// Show the health and mana gauge: in the bottom-left corner of a vw×vh view, unless it's
    /// already showing there (one moved off the view comes back).
    pub fn show_gauge(&mut self, art: &GumpArt, (vw, vh): (u32, u32)) {
        let (w, h) = art.size(GAUGE);
        let (w, h) = (w * GAUGE_SCALE, h * GAUGE_SCALE);
        let seen = |(x, y): (i32, i32)| x + w > 2 && y + h > 2 && x < vw as i32 - 2 && y < vh as i32 - 2;
        if !self.gauge.is_some_and(seen) {
            self.gauge = Some((GAUGE_MARGIN, vh as i32 - h - GAUGE_MARGIN));
        }
    }

    /// Draw the gauge (hits and mana out of their most) at the canvas's scale `ss`.
    pub fn draw_gauge(&self, canvas: &mut Canvas, art: &mut GumpArt, gd: &GameData, ss: u32, bars: [(i16, i16); 2]) {
        let Some((x, y)) = self.gauge else { return };
        let k = GAUGE_SCALE;
        art.draw_scaled(canvas, GAUGE, 0, (x, y), k as u32, true); // (without its black outline)
        let s = ss as i32;
        let bottom = y + (GAUGE_BOTTOM + 1) * k; // (bars fill in steps of the bigger size's pixels)
        for ((bx, colours), (now, most)) in GAUGE_BARS.into_iter().zip(bars) {
            let h = if most > 0 { (now.clamp(0, most) as i32 * GAUGE_HEIGHT * k) / most as i32 } else { 0 };
            for (i, c) in colours.into_iter().enumerate() {
                let rgba = gd.palette.0[c as usize];
                let col = x + (bx + i as i32) * k;
                for py in (bottom - h) * s..bottom * s {
                    for px in col * s..(col + k) * s {
                        canvas.put(px, py, [rgba[0], rgba[1], rgba[2], 255]);
                    }
                }
            }
        }
    }

    /// The paperdoll's stats button, up or `pressed`, in a window at `at`.
    pub fn draw_stats_button(canvas: &mut Canvas, art: &mut GumpArt, pressed: bool, at: (i32, i32)) {
        art.draw(canvas, STATS_BUTTON_SHAPE, pressed as u32, at.0 + STATS_BUTTON[0], at.1 + STATS_BUTTON[1]);
    }

    pub fn item_of(&self, window: usize) -> Option<ObjId> {
        self.wins.get(window).map(|w| w.item)
    }

    /// Move `item`'s window.
    pub fn move_by(&mut self, item: ObjId, dx: i32, dy: i32) {
        if item == GAUGE_ID {
            self.gauge = self.gauge.map(|(x, y)| (x + dx, y + dy));
        }
        if let Some(w) = self.wins.iter_mut().find(|w| w.item == item) {
            w.at = (w.at.0 + dx, w.at.1 + dy);
        }
    }

    pub fn raise(&mut self, window: usize) -> usize {
        if window + 1 < self.wins.len() {
            let w = self.wins.remove(window);
            self.wins.push(w);
        }
        self.wins.len() - 1
    }

    /// Where an item in an open window is drawn ([x, y, w, h], game pixels in the view): for
    /// speech over things in a bag.
    pub fn item_rect(&self, gd: &GameData, art: &GumpArt, world: &World, id: ObjId) -> Option<[i32; 4]> {
        let parent = world.container(id)?;
        let w = self.wins.iter().rev().find(|w| w.item == parent)?;
        let it = world.get(id)?;
        let look = w.look(gd, art, &it);
        let f = gd.shapes.frame(it.shape, look.frame)?;
        let (x, y) = look.at;
        Some([x - f.xoff, y - f.yoff, f.width as i32, f.height as i32])
    }

    /// What an item dropped at (x, y) would do. Onto a bag in a window: go into that bag; onto a
    /// window: into its container where it was dropped; onto the paperdoll: be worn. None if
    /// (x, y) isn't over a window (it goes onto the map).
    pub fn drop_target(
        &self,
        gd: &GameData,
        art: &GumpArt,
        world: &World,
        x: i32,
        y: i32,
        dropped: ObjId,
    ) -> Option<Drop> {
        let w = match self.hit(gd, art, world, x, y)? {
            Hit::Item(id) if id != dropped && holds(gd, world, id) && world.get(id).is_some_and(|it| it.npc == 0) => {
                return Some(Drop::Into(id, STOW_AT));
            }
            Hit::Item(id) => {
                let parent = world.container(id);
                match self.wins.iter().rev().find(|w| Some(w.item) == parent) {
                    Some(w) => w,
                    None => return Some(Drop::Nowhere),
                }
            }
            Hit::Window(i) => &self.wins[i],
            Hit::Gauge => return Some(Drop::Nowhere),
        };
        if w.gump == PAPERDOLL {
            return Some(Drop::Wear);
        }
        let a = art.area(w.gump);
        let at = ((x - w.at.0 - a[0]).clamp(0, a[2]), (y - w.at.1 - a[1]).clamp(0, a[3]));
        Some(Drop::Into(w.item, at))
    }

    /// The topmost thing at (x, y): an item in a window, or a window itself.
    pub fn hit(&self, gd: &GameData, art: &GumpArt, world: &World, x: i32, y: i32) -> Option<Hit> {
        let k = GAUGE_SCALE;
        let (w, h) = art.size(GAUGE);
        let on_gauge = |(gx, gy): (i32, i32)| {
            let (px, py) = ((x - gx).div_euclid(k), (y - gy).div_euclid(k));
            (1..w - 1).contains(&px) && (1..h - 1).contains(&py) && art.solid(GAUGE, px, py)
        };
        if self.gauge.is_some_and(on_gauge) {
            return Some(Hit::Gauge); // (on top of the windows)
        }
        for (i, w) in self.wins.iter().enumerate().rev() {
            for id in w.contents(gd, world, self.editor).into_iter().rev() {
                let Some(r) = self.item_rect(gd, art, world, id) else { continue };
                let Some(it) = world.get(id) else { continue };
                let look = w.look(gd, art, &it);
                let f = gd.shapes.frame(it.shape, look.frame);
                let on = match (w.is_pack(gd, &it), look.shown) {
                    (true, _) => {
                        (0..PACK[2]).contains(&(x - w.at.0 - PACK[0])) && (0..PACK[3]).contains(&(y - w.at.1 - PACK[1]))
                    }
                    (false, true) => f.is_some_and(|f| f.hit(x - r[0], y - r[1])),
                    (false, false) => false,
                };
                if on {
                    return Some(Hit::Item(id));
                }
            }
            if art.solid(w.gump, x - w.at.0, y - w.at.1) || self.on_stats_button(i, x, y) {
                return Some(Hit::Window(i)); // (the button sits in a hole in the paperdoll)
            }
        }
        None
    }

    /// Draw the windows, bottom first. `cache` draws the items (at the canvas's scale); one being
    /// dragged stays drawn where it is till it's let go (as the original); `decorate` adds to a
    /// window before its items (the paperdoll's stats).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        canvas: &mut Canvas,
        art: &mut GumpArt,
        gd: &GameData,
        world: &World,
        cache: &mut SpriteCache,
        mut decorate: impl FnMut(&mut Canvas, &mut GumpArt, ObjId, u32, (i32, i32)),
    ) {
        let s = art.scale() as i32;
        for w in &self.wins {
            art.draw(canvas, w.gump, 0, w.at.0, w.at.1);
            decorate(canvas, art, w.item, w.gump, w.at);
            for id in w.contents(gd, world, self.editor) {
                let Some(it) = world.get(id) else { continue };
                let look = w.look(gd, art, &it);
                if !look.shown {
                    continue;
                }
                let flip = it.flags & u8engine::world::item_flag::FLIPPED != 0;
                let Some(sp) = cache.get(gd, it.shape, look.frame, flip) else { continue };
                let (x, y) = look.at;
                canvas.blit(&sp, x * s - sp.xoff, y * s - sp.yoff);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trapped chest's TRAP marker (an editor-only shape) isn't shown in its window, nor can it
    /// be picked out of it — unless the editor view is on (ScummVM's ContainerGump).
    #[test]
    #[ignore]
    fn trap_markers_hide_in_chests() {
        let Ok(dir) = std::env::var("U8_DATA") else { return };
        let gd = GameData::load_fixed(std::path::Path::new(&dir)).unwrap();
        let (world, chest, trap) = (0..256)
            .find_map(|m| {
                let w = World::build(&gd, m);
                let marker =
                    |id| w.get(id).is_some_and(|it| gd.typeflags.get(it.shape).has(u8data::typeflag::flag::EDITOR));
                let trap = (256..u16::MAX).find(|&id| marker(id) && w.container(id).is_some())?;
                let chest = w.container(trap)?;
                Some((w, chest, trap))
            })
            .expect("a trapped chest");
        let win = Win { item: chest, gump: 0, at: (0, 0) };
        assert!(!win.contents(&gd, &world, false).contains(&trap));
        assert!(win.contents(&gd, &world, true).contains(&trap), "the editor view shows it");
    }

    /// The paperdoll draws the worn things weapon, legs, body, head, arms, then the shield over
    /// them all (CONTGUMP.C: slots 5 down to 0), and the backpack last.
    #[test]
    #[ignore]
    fn the_shield_is_drawn_over_the_rest() {
        use u8engine::equip::Slot;
        let Ok(dir) = std::env::var("U8_DATA") else { return };
        let gd = GameData::load_fixed(std::path::Path::new(&dir)).unwrap();
        let mut world = World::build(&gd, 40);
        // (The test save's Avatar has no shield: one put on him.)
        let shape = (1..2048).find(|&s| Slot::of(&gd, s) == Some(Slot::Shield)).unwrap();
        let flags = u8engine::world::item_flag::EQUIPPED;
        let shield = world.add(&gd, u8data::map::Item { shape, flags, ..Default::default() }, false);
        world.move_inside(&gd, shield, u8engine::world::AVATAR, (0, 0));
        let worn =
            u8data::map::Item { flags: flags | u8engine::world::item_flag::CONTAINED, ..world.get(shield).unwrap() };
        world.set(&gd, shield, worn);
        let win = Win { item: u8engine::world::AVATAR, gump: PAPERDOLL, at: (0, 0) };
        let slots: Vec<Option<Slot>> =
            win.contents(&gd, &world, false).iter().map(|&id| Slot::of(&gd, world.get(id).unwrap().shape)).collect();
        assert!(slots.contains(&Some(Slot::Shield)) && slots.len() >= 4, "geared: {slots:?}");
        let worn: Vec<i32> = slots.iter().map_while(|s| s.map(|s| s as i32)).collect();
        assert!(worn.windows(2).all(|w| w[0] > w[1]), "weapon first, shield last: {slots:?}");
        assert_eq!(worn.last(), Some(&(Slot::Shield as i32)));
        assert!(slots[worn.len()..].iter().all(Option::is_none), "the pack after: {slots:?}");
    }

    /// A bag's window opens beside the window it's in when there's room that covers no other
    /// window, else where that window is (as DOS, which opens it on the same spot), kept in the
    /// view.
    #[test]
    #[ignore] // needs U8 data
    fn a_bags_window_opens_by_the_one_it_was_in() {
        let Ok(dir) = std::env::var("U8_DATA") else { return };
        let gd = u8data::GameData::load_fixed(std::path::Path::new(&dir)).unwrap();
        let art = crate::art::GumpArt::load(std::path::Path::new(&dir), &gd.palette).unwrap();
        let at = |w: &Windows, item| w.wins.iter().find(|w| w.item == item).unwrap().at;
        let (pack, inner, sack) = (10, 11, 12);
        // No room beside: on the same spot.
        let small = (art.size(2).0 + 40, art.size(2).1 + 40);
        let small = (small.0 as u32, small.1 as u32);
        let mut w = Windows::default();
        w.open(&art, pack, 2, Some(1), small);
        w.open(&art, sack, 3, None, small); // (something else on top)
        w.open(&art, inner, 2, Some(pack), small);
        assert_eq!(at(&w, inner), at(&w, pack));
        // Room: beside it (not by the window on top), and not over another window: the sack's
        // is on its right, so on its left.
        let mut w = Windows::default();
        w.open(&art, pack, 2, Some(1), (640, 400));
        w.open(&art, sack, 3, None, (640, 400));
        w.open(&art, inner, 2, Some(pack), (640, 400));
        let ((px, py), (ix, iy)) = (at(&w, pack), at(&w, inner));
        assert_eq!((ix, iy), (px - art.size(2).0 - 2, py));
        // The original's 320x200: the pack, the inner pack beside it, a bag in that: no room
        // beside that covers nothing, so the bag's window is on the inner pack's spot.
        let mut w = Windows::default();
        w.open(&art, pack, 2, Some(1), (320, 200));
        w.open(&art, inner, 2, Some(pack), (320, 200));
        assert_ne!(at(&w, inner), at(&w, pack), "beside");
        w.open(&art, sack, 3, Some(inner), (320, 200));
        assert_eq!(at(&w, sack), at(&w, inner));
    }
}
