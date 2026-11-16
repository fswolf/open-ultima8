//! The player handling things: moving them between the map, bags and the Avatar, wearing and
//! wielding, the weight the Avatar can carry and how far it can reach.
use super::*;
use u8data::typeflag::family;

/// How far the Avatar reaches to pick things up, use them or put them down: the gap between its
/// box and the thing's, on either axis, with nothing solid in between (ScummVM's canReach). ScummVM
/// uses 128; DOS took a bedroll 132 apart and refused it 164 apart.
pub const REACH: i32 = 160;

/// People this near the Avatar (either axis) see what it takes (ScummVM).
const SEEN_FROM: i32 = 640;

/// What `lift` noted about a thing picked up.
#[derive(Default)]
pub(super) struct Lifted {
    from: Option<(i32, i32, i32)>,
    touching: Vec<ObjId>,
    resting: Vec<ObjId>,
}

/// What things thrown fall with.
const THROW_GRAVITY: i32 = 4;

/// The speed per tick (x, y, z) that takes a thing thrown from `from` to `to` at about `speed`
/// across, with `gravity` (ScummVM's MissileTracker): the frames the distance takes at that
/// speed, an upward start that comes back down to the target's height by then, capped.
pub(crate) fn missile(from: (i32, i32, i32), to: (i32, i32, i32), speed: i32, gravity: i32) -> (i32, i32, i32) {
    let range = (to.0 - from.0).abs() + (to.1 - from.1).abs();
    let mut frames = (range + speed / 2) / speed;
    if frames <= 0 {
        return (0, 0, if to.2 > from.2 { speed / 4 } else { -speed / 4 });
    }
    let up = |f: i32| (gravity * f * (f - 1) / 2 + to.2 - from.2) / f;
    if up(frames) > speed / 4 {
        if gravity == 0 || speed / (4 * gravity) <= frames {
            if speed >= 4 && (to.2 - from.2) / (speed / 4) > frames {
                frames = (to.2 - from.2) / (speed / 4);
            }
        } else {
            frames = speed / (4 * gravity);
        }
    }
    let frames = frames.max(1);
    (((to.0 - from.0) + frames / 2) / frames, ((to.1 - from.1) + frames / 2) / frames, up(frames))
}

/// The most a pile holds (ScummVM's MAX_QUANTITY).
const MAX_QUANTITY: u16 = 666;

/// Where things put into a bag without a spot land (window position).
const STOW_AT: (i32, i32) = (20, 20);

/// Where a moved item goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Place {
    /// Inside a container (or a person's belongings) at this window position.
    Into { container: ObjId, at: (i32, i32) },
    /// On the map, its base at this point.
    Map((i32, i32, i32)),
}

impl Game {
    /// Move an item the player picked up: into a container (or a person's belongings) at a
    /// window position, or onto the map. The Avatar has to be able to reach both ends.
    pub fn move_item(&mut self, id: ObjId, to: Place) -> Result<(), &'static str> {
        self.can_pick_up(id)?;
        self.check_move(id, to)?;
        let lifted = self.lift(id);
        match to {
            Place::Into { container, at } => self.world.move_inside(&self.data, id, container, at),
            Place::Map(at) if !self.reaches(at) => {
                self.fling(id, at, lifted);
                return Ok(());
            }
            Place::Map(at) => {
                self.world.move_to_map(&self.data, id, at);
                self.face(Ok(id));
            }
        }
        self.set_down(id, lifted);
        Ok(())
    }

    /// Is a spot on the map close enough to put something down (farther, it's thrown)?
    fn reaches(&self, (x, y, z): (i32, i32, i32)) -> bool {
        // (The gap from the Avatar's box to the spot; height by the same measure.)
        let Some(b) = self.world.index(AVATAR).map(|i| self.world.entry(i).bounds()) else { return false };
        let gap = |lo: i32, hi: i32, v: i32| (lo - v).max(v - hi).max(0);
        gap(b.x0, b.x1, x).max(gap(b.y0, b.y1, y)).max(gap(b.z0, b.z1, z)) <= REACH
    }

    /// Throw `id` at a spot out of reach (ScummVM's DropItem and MissileTracker): from over the
    /// Avatar's head, as hard as 64 − its weight + STR allows, off by up to 4 × (30 − DEX) (20 at
    /// least), and gravity brings it down; it lands with its contact events.
    fn fling(&mut self, id: ObjId, at: (i32, i32, i32), l: Lifted) {
        for o in l.touching {
            self.trigger(o, event::RELEASE);
        }
        for o in l.resting {
            self.fall(o);
        }
        let (Some(a), Some(i)) = (self.world.get(AVATAR), self.world.index(AVATAR)) else { return };
        let from = (a.x, a.y, a.z + (self.world.entry(i).z1 - a.z) / 2); // (its middle)
        self.world.move_to_map(&self.data, id, from);
        self.keep(id);
        self.trigger(id, event::JUST_MOVED);
        let s = self.stats.get(AVATAR);
        let (str, dex) = s.map_or((10, 10), |s| (s.str as i32, s.dex as i32));
        let weight = (equip::weight(&self.data, &self.world, id) / 10) as i32;
        let speed = (64 - weight + str).max(1);
        let off = (4 * (30 - dex)).max(20);
        let mut aim = |c: i32| c - off + self.world.roll(2 * off as u32 + 1) as i32;
        let to = (aim(at.0), aim(at.1), at.2);
        self.face(Err(at));
        self.hurl(id, missile(from, to, speed, THROW_GRAVITY), THROW_GRAVITY, None);
    }

    /// The Avatar turns to `id` (on the map), or the spot it threw something at (box centre to
    /// box centre; the anchors are front corners).
    fn face(&mut self, at: Result<ObjId, (i32, i32, i32)>) {
        let mid = |w: &crate::world::World, id| {
            w.index(id).map(|i| w.entry(i).bounds()).map(|b| ((b.x0 + b.x1) / 2, (b.y0 + b.y1) / 2))
        };
        let to = match at {
            Ok(id) => mid(&self.world, id),
            Err((x, y, _)) => Some((x, y)),
        };
        let (Some((ax, ay)), Some((x, y)), Some(av)) = (mid(&self.world, AVATAR), to, self.avatar.as_mut()) else {
            return;
        };
        if (x, y) != (ax, ay) {
            av.turn(&self.data, &mut self.world, crate::geom::dir_towards(x - ax, y - ay) as usize);
        }
    }

    /// Would `id` go `to`? Why not, if not.
    fn check_move(&self, id: ObjId, to: Place) -> Result<(), &'static str> {
        let w = &self.world;
        match to {
            Place::Into { container, .. } => {
                let c = w.get(container).ok_or("It's gone.")?;
                let holds = equip::holds(&self.data, w, container);
                let someone_else = c.npc != 0 && container != AVATAR;
                if !holds || someone_else {
                    return Err("It won't go in there.");
                }
                if !self.within_reach(container) {
                    return Err("You can't reach that.");
                }
                // Room by volume, as the original refuses it, and the Avatar's load.
                equip::can_add(&self.data, w, id, container, false, 0)?;
                equip::has_room(&self.data, w, id, container)?;
                self.check_load(id, container)?;
            }
            Place::Map(at) => {
                // (Out of reach it's thrown.)
                if self.reaches(at) && !self.world.fits(&self.data, id, at) {
                    return Err("It won't fit there.");
                }
            }
        }
        Ok(())
    }

    /// The player picks `id` up: anyone about may see it taken (if it isn't the Avatar's own).
    /// Returns where it was, what it touched there, and what rested on it.
    fn lift(&mut self, id: ObjId) -> Lifted {
        let Some(it) = self.world.get(id) else { return Lifted::default() };
        self.taking(id);
        let on_map = self.world.container(id).is_none();
        let top = self.world.index(id).map(|i| self.world.entry(i).bounds());
        let resting = match top.filter(|_| on_map) {
            Some(b) => self
                .world
                .area(it.x, it.y, 256)
                .into_iter()
                .filter(|&o| o != id && o >= 256)
                .filter(|&o| {
                    self.world.index(o).is_some_and(|i| {
                        let o = self.world.entry(i).bounds();
                        o.z0 == b.z1 && o.overlaps_xy(&b)
                    })
                })
                .collect(),
            None => vec![],
        };
        Lifted { from: on_map.then_some((it.x, it.y, it.z)), touching: self.world.contacts(id), resting }
    }

    /// The player takes `id`: if it isn't the Avatar's, anyone about sees (their scripts
    /// decide what to make of it). What the Avatar already carries is its own, flag or not (some
    /// saves leave it off).
    fn taking(&mut self, id: ObjId) {
        let Some(it) = self.world.get(id) else { return };
        if it.flags & item_flag::OWNED != 0 || self.world.root(id) == AVATAR {
            return;
        }
        let a = self.world.get(AVATAR);
        let near: Vec<ObjId> = a.map_or(vec![], |a| self.world.area(a.x, a.y, SEEN_FROM));
        for p in near.into_iter().filter(|&p| p != AVATAR && p < 256) {
            if self.stats.get(p).is_some_and(|s| !s.dead) && !self.world.is_void(p) {
                self.trigger_with(p, event::AVATAR_STOLE, &id.to_le_bytes());
            }
        }
    }

    /// Take `n` of pile `id` (the original asks how many) to `to`, the rest staying where it is.
    /// The part is a new pile (ScummVM's SplitItemProcess); all of it moves the pile itself.
    pub fn split_to(&mut self, id: ObjId, n: u16, to: Place) -> Result<(), &'static str> {
        let it = self.world.get(id).ok_or("It's gone.")?;
        if n >= it.quality {
            return self.move_item(id, to);
        }
        if n == 0 {
            return Ok(());
        }
        self.can_pick_up(id)?;
        let part = self.part_of(id, n);
        if let Err(why) = self.check_move(part, to) {
            self.world.remove(part);
            return Err(why);
        }
        self.taking(id);
        self.take_from(id, n);
        self.trigger(part, event::COMBINE);
        match to {
            Place::Into { container, at } => self.world.move_inside(&self.data, part, container, at),
            Place::Map(at) if !self.reaches(at) => {
                self.fling(part, at, Lifted::default());
                return Ok(());
            }
            Place::Map(at) => {
                self.world.move_to_map(&self.data, part, at);
                self.face(Ok(part));
            }
        }
        let from = self.world.get(id).filter(|_| self.world.container(id).is_none()).map(|i| (i.x, i.y, i.z));
        self.set_down(part, Lifted { from, ..Lifted::default() });
        Ok(())
    }

    /// Take `n` of pile `id` onto pile `onto` (up to 666 there).
    pub fn split_onto(&mut self, id: ObjId, n: u16, onto: ObjId) -> Result<(), &'static str> {
        let it = self.world.get(id).ok_or("It's gone.")?;
        if n >= it.quality {
            return self.merge(id, onto);
        }
        self.can_pick_up(id)?;
        if !self.can_merge(id, onto) {
            return Err("They don't go together.");
        }
        if !self.within_reach(onto) {
            return Err("You can't reach that.");
        }
        let mut pile = self.world.get(onto).ok_or("It's gone.")?;
        let n = n.min(MAX_QUANTITY.saturating_sub(pile.quality));
        if n == 0 {
            return Ok(());
        }
        if let Some(c) = self.world.container(onto) {
            let part = self.part_of(id, n);
            let heavy = self.check_load(part, c);
            self.world.remove(part);
            heavy?;
        }
        self.taking(id);
        pile.quality += n;
        self.world.set(&self.data, onto, pile);
        self.take_from(id, n);
        self.combined(onto);
        Ok(())
    }

    /// Pile `onto` grew: its look by the amount, the Avatar's if it carries it, and the Avatar
    /// turns to it on the floor.
    fn combined(&mut self, onto: ObjId) {
        self.trigger(onto, event::COMBINE);
        match self.world.container(onto) {
            None => self.face(Ok(onto)),
            Some(_) if self.world.root(onto) == AVATAR => self.world.own(&self.data, onto),
            Some(_) => {}
        }
    }

    /// A new pile of `n` like `id`, beside it in its bag (what the Avatar carries stays carried
    /// for the weight), or in the void for one on the map (for moving or weighing).
    fn part_of(&mut self, id: ObjId, n: u16) -> ObjId {
        let it = self.world.get(id).unwrap_or_default();
        let keep = item_flag::DISPOSABLE | item_flag::OWNED | item_flag::INVISIBLE | item_flag::FLIPPED;
        let part = u8data::map::Item {
            shape: it.shape,
            frame: it.frame,
            quality: n,
            flags: it.flags & keep,
            ..Default::default()
        };
        match self.world.container(id) {
            Some(bag) => self.world.add_inside(&self.data, part, bag),
            None => self.world.add(&self.data, part, false),
        }
    }

    /// `n` fewer in pile `id` (its look by what's left).
    fn take_from(&mut self, id: ObjId, n: u16) {
        if let Some(mut it) = self.world.get(id) {
            it.quality -= n.min(it.quality);
            self.world.set(&self.data, id, it);
            self.trigger(id, event::COMBINE);
        }
    }

    /// Is `id` a pile the player could take part of (more than one, counted)?
    pub fn is_pile(&self, id: ObjId) -> bool {
        self.world.get(id).is_some_and(|it| {
            let f = self.data.typeflags.get(it.shape).family;
            (f == family::QUANTITY || f == family::REAGENT) && it.quality > 1
        })
    }

    /// `id` is where the player put it: contact events there (a plate pressed, one let go),
    /// what rested on it falls, its `justMoved`, and in the Avatar's keeping it's the Avatar's.
    fn set_down(&mut self, id: ObjId, l: Lifted) {
        self.keep(id);
        let to = self.world.get(id).filter(|_| self.world.container(id).is_none()).map(|i| (i.x, i.y, i.z));
        let d = match (l.from, to) {
            (Some(f), Some(t)) => (t.0 - f.0, t.1 - f.1, t.2 - f.2),
            _ => (0, 0, 0), // from or into a bag: no force (ScummVM)
        };
        self.touched(id, &l.touching, d);
        for o in l.resting {
            self.fall(o);
        }
        self.trigger(id, event::JUST_MOVED);
        if self.world.root(id) == AVATAR {
            self.world.own(&self.data, id);
        }
    }

    /// Could `id` be piled onto `onto` (coins on coins, a reagent on the same reagent)? As
    /// ScummVM's canMergeWith: the same shape counted by quantity, or a reagent of the same kind
    /// (some kinds span several frames).
    pub fn can_merge(&self, id: ObjId, onto: ObjId) -> bool {
        let (Some(a), Some(b)) = (self.world.get(id), self.world.get(onto)) else { return false };
        if id == onto || a.shape != b.shape {
            return false;
        }
        match self.data.typeflags.get(a.shape).family {
            family::QUANTITY => true,
            family::REAGENT => {
                let kinds: &[(u32, u32)] = match a.shape {
                    395 => &[(0, 5), (6, 7), (10, 12), (14, 15), (16, 20)], // necromancy
                    398 => &[(0, 1), (2, 5), (6, 9), (10, 13), (14, 17), (18, 20)], // sorcery
                    _ => &[],
                };
                a.frame == b.frame
                    || kinds.iter().any(|&(lo, hi)| (lo..=hi).contains(&a.frame) && (lo..=hi).contains(&b.frame))
            }
            _ => false,
        }
    }

    /// The player drops `id` on a pile it merges with (see [`can_merge`](Self::can_merge)):
    /// one pile, up to 666; what doesn't fit stays where it was. The pile's `combine` picks its
    /// look by the amount.
    pub fn merge(&mut self, id: ObjId, onto: ObjId) -> Result<(), &'static str> {
        self.can_pick_up(id)?;
        if !self.can_merge(id, onto) {
            return Err("They don't go together.");
        }
        if !self.within_reach(onto) {
            return Err("You can't reach that.");
        }
        if let Some(c) = self.world.container(onto) {
            self.check_load(id, c)?;
            // A bigger pile takes more room (coins by the hundred): it must still fit, unless
            // the two were in that bag already.
            let (Some(a), Some(b)) = (self.world.get(id), self.world.get(onto)) else { return Err("It's gone.") };
            let grown = u8data::map::Item {
                quality: (a.quality as u32 + b.quality as u32).min(MAX_QUANTITY as u32) as u16,
                ..b
            };
            let more = equip::volume(&self.data, &grown).saturating_sub(equip::volume(&self.data, &b));
            let cap = self.world.get(c).map_or(0, |c| equip::capacity(&self.data, &c));
            let roomy = c < 256 || self.world.container(id) == Some(c);
            if !roomy && equip::used_room(&self.data, &self.world, c) + more > cap {
                return Err("There's no room for that.");
            }
        }
        let lifted = self.lift(id);
        let (Some(mut a), Some(mut b)) = (self.world.get(id), self.world.get(onto)) else { return Err("It's gone.") };
        let total = a.quality as u32 + b.quality as u32;
        b.quality = total.min(MAX_QUANTITY as u32) as u16;
        self.world.set(&self.data, onto, b);
        if total > MAX_QUANTITY as u32 {
            a.quality = (total - MAX_QUANTITY as u32) as u16;
            self.world.set(&self.data, id, a);
            self.trigger(id, event::COMBINE); // (the rest's look, by what's left)
        } else {
            self.world.remove(id);
            // Gone from where it lay: what it touched is let go, what was on it falls.
            for o in lifted.touching {
                self.trigger(o, event::RELEASE);
            }
            for o in lifted.resting {
                self.fall(o);
            }
        }
        self.combined(onto);
        Ok(())
    }

    /// Wear or wield `id` (it must have a slot); what was in that slot goes into the backpack.
    pub fn equip(&mut self, id: ObjId) -> Result<(), &'static str> {
        self.can_pick_up(id)?;
        let shape = self.world.get(id).ok_or("It's gone.")?.shape;
        let slot = equip::Slot::of(&self.data, shape).ok_or("You can't wear that.")?;
        self.check_load(id, AVATAR)?; // before anything moves
        if let Some(old) = equip::worn(&self.data, &self.world, AVATAR, slot).filter(|&o| o != id) {
            // What it replaces goes into the pack, if there's room (the original won't put a
            // thing on over another at all: ScummVM's setEquip refuses an occupied slot).
            let pack = self.backpack().ok_or("Take that off first.")?;
            if self.world.container(id) == Some(pack) {
                // (Swapped for one out of the pack itself: the pack mustn't end up any fuller
                // than it may be, or than it was.)
                let (w, d) = (&self.world, &self.data);
                let vol = |o: ObjId| w.get(o).map_or(0, |it| equip::volume(d, &it));
                let used = equip::used_room(d, w, pack);
                let cap = w.get(pack).map_or(0, |p| equip::capacity(d, &p));
                if used - vol(id) + vol(old) > cap.max(used) {
                    return Err("There's no room for that.");
                }
            } else {
                equip::has_room(&self.data, &self.world, old, pack)?;
            }
            self.world.move_inside(&self.data, old, pack, STOW_AT);
        }
        self.wear(id, slot.spot())
    }

    /// Put `id` into the Avatar's backpack (a bag, with no backpack: wear it as one).
    pub fn stow(&mut self, id: ObjId) -> Result<(), &'static str> {
        match self.backpack() {
            Some(pack) if pack == id => Ok(()),
            Some(pack) => self.move_item(id, Place::Into { container: pack, at: STOW_AT }),
            None if equip::holds(&self.data, &self.world, id) => {
                self.can_pick_up(id)?;
                self.wear(id, equip::BACKPACK_SPOT)
            }
            None => Err("You have no backpack."),
        }
    }

    /// Put `id` on the Avatar at paperdoll spot `at`, worn.
    pub(super) fn wear(&mut self, id: ObjId, at: (i32, i32)) -> Result<(), &'static str> {
        self.check_load(id, AVATAR)?;
        let lifted = self.lift(id);
        self.world.move_inside(&self.data, id, AVATAR, at);
        if let Some(mut it) = self.world.get(id) {
            it.flags |= item_flag::EQUIPPED;
            it.z = equip::worn_z(&self.data, it.shape);
            self.world.set(&self.data, id, it);
        }
        self.set_down(id, lifted);
        Ok(())
    }

    /// The player moved `id`: loot taken from a body is no longer thrown away with it.
    pub(super) fn keep(&mut self, id: ObjId) {
        if let Some(mut it) = self.world.get(id) {
            it.flags &= !(item_flag::DISPOSABLE | item_flag::FAST_ONLY);
            self.world.set(&self.data, id, it);
        }
    }

    /// The Avatar's backpack (the bag it carries).
    pub fn backpack(&self) -> Option<ObjId> {
        self.world.contents(AVATAR).into_iter().find(|&id| equip::holds(&self.data, &self.world, id))
    }

    /// What the Avatar carries, and the most it can, as the paperdoll shows them.
    pub fn load(&self) -> (u32, u32) {
        let (carried, most) = self.load_tenths();
        (carried.div_ceil(10), most / 10)
    }

    pub(super) fn load_tenths(&self) -> (u32, u32) {
        let carried = self.world.contents(AVATAR).iter().map(|&id| equip::weight(&self.data, &self.world, id)).sum();
        let strength = self.stats.get(AVATAR).map_or(15, |s| s.str);
        (carried, equip::max_weight(strength))
    }

    /// Refuse to load the Avatar past what it can carry (moving `id` into `into`).
    pub(super) fn check_load(&self, id: ObjId, into: ObjId) -> Result<(), &'static str> {
        let to_avatar = into == AVATAR || self.world.is_inside(into, AVATAR);
        if !to_avatar || id == AVATAR || self.world.is_inside(id, AVATAR) {
            return Ok(());
        }
        let (carried, most) = self.load_tenths();
        if carried + equip::weight(&self.data, &self.world, id) > most {
            return Err("That's too heavy.");
        }
        Ok(())
    }

    /// Could the player pick `id` up (before deciding where it goes)? Why not, if not.
    pub fn can_pick_up(&self, id: ObjId) -> Result<(), &'static str> {
        let w = &self.world;
        let it = w.get(id).ok_or("It's gone.")?;
        let e = w.entry(w.index(id).ok_or("It's gone.")?);
        let tf = self.data.typeflags.get(it.shape);
        if e.traits.actor || e.traits.egg || e.origin != Origin::Movable || tf.has(flag::FIXED) {
            return Err("You can't move that.");
        }
        if !self.within_reach(id) {
            return Err("You can't reach that.");
        }
        Ok(())
    }

    /// Is `id` (on the map, or in something there, or the Avatar's own) close enough for the
    /// Avatar to handle: within [`REACH`] of it, nothing solid in the way?
    pub fn within_reach(&self, id: ObjId) -> bool {
        let root = self.world.root(id);
        if root == AVATAR {
            return self.world.get(id).is_some();
        }
        let shown = self.world.index(root).is_some_and(|i| self.world.entry(i).present());
        shown && self.world.can_reach(AVATAR, root, REACH)
    }

    /// Is `id` (or what it's in) within `xy` units of the Avatar across and `z` up or down? The
    /// Avatar's own things always are; things gone, or in nothing on the map, never.
    pub fn near_avatar(&self, id: ObjId, xy: i32, z: i32) -> bool {
        let root = self.world.root(id);
        if root == AVATAR {
            return self.world.get(id).is_some();
        }
        let shown = self.world.index(root).is_some_and(|i| self.world.entry(i).present());
        match (self.world.get(AVATAR), self.world.get(root).filter(|_| shown)) {
            (Some(a), Some(it)) => (a.x - it.x).abs().max((a.y - it.y).abs()) <= xy && (a.z - it.z).abs() <= z,
            _ => false,
        }
    }

    /// The player closed `item`'s window: clear its "window open" flag (its scripts toggle on it).
    pub fn window_closed(&mut self, item: ObjId) {
        if let Some(mut it) = self.world.get(item) {
            it.flags &= !GUMP_OPEN;
            self.world.set(&self.data, item, it);
        }
    }
}
