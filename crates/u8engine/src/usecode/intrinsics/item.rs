//! `Item::` intrinsics: position, shape/frame/quality, flags, geometry tests, creation and movement.
use super::Ctx;
use crate::geom::dir_towards;
use crate::usecode::stack::le16;
use crate::usecode::{vm::Request, Pop};
use crate::world::ObjId;
use u8data::{map::Item, typeflag::family, usecode::event};

fn get(c: &Ctx, f: impl FnOnce(&Item) -> i32) -> u32 {
    c.item().map_or(0, |i| f(&i) as u32)
}

/// Positions are on the map: a thing in a container reports its outermost container's.
fn at(c: &Ctx, f: impl FnOnce(&Item) -> i32) -> u32 {
    c.abs(c.this()).map_or(0, |i| f(&i) as u32)
}
pub fn get_x(c: &mut Ctx) -> u32 {
    at(c, |i| i.x)
}
pub fn get_y(c: &mut Ctx) -> u32 {
    at(c, |i| i.y)
}
pub fn get_z(c: &mut Ctx) -> u32 {
    at(c, |i| i.z)
}
/// Centre of the item's box (its anchor is the bottom front corner): its own size, placed where
/// its outermost container is.
fn centre(c: &Ctx) -> Option<(i32, i32, i32)> {
    let (it, a) = (c.item()?, c.abs(c.this())?);
    let (dx, dy, dz) = c.dims(&it);
    Some((a.x - dx / 2, a.y - dy / 2, a.z + dz / 2))
}
pub fn get_cx(c: &mut Ctx) -> u32 {
    centre(c).map_or(0, |p| p.0 as u32)
}
pub fn get_cy(c: &mut Ctx) -> u32 {
    centre(c).map_or(0, |p| p.1 as u32)
}
pub fn get_cz(c: &mut Ctx) -> u32 {
    centre(c).map_or(0, |p| p.2 as u32)
}
/// getPoint(&WorldPoint): u16 x, u16 y, u8 z.
pub fn get_point(c: &mut Ctx) -> u32 {
    if let Some(i) = c.abs(c.this()) {
        let mut wp = vec![];
        wp.extend((i.x as u16).to_le_bytes());
        wp.extend((i.y as u16).to_le_bytes());
        wp.push(i.z as u8);
        c.write(c.arg4(4), &wp);
    }
    0
}

pub fn get_shape(c: &mut Ctx) -> u32 {
    get(c, |i| i.shape as i32)
}
pub fn set_shape(c: &mut Ctx) -> u32 {
    let s = c.arg2(4) as u32;
    c.update(|i| i.shape = s);
    0
}
pub fn get_frame(c: &mut Ctx) -> u32 {
    get(c, |i| i.frame as i32)
}
pub fn set_frame(c: &mut Ctx) -> u32 {
    let f = c.arg2(4) as u32;
    c.update(|i| i.frame = f);
    0
}
/// Quality doubles as quantity, egg type and the "Q" value: each getter answers only for its
/// families (else 0) and each setter changes only those (ScummVM); getQ/setQ are raw.
fn quality_of(c: &Ctx, families: &[u8]) -> u32 {
    c.item().map_or(0, |i| if families.contains(&c.family(&i)) { i.quality as u32 } else { 0 })
}
fn set_quality_of(c: &mut Ctx, families: Option<&[u8]>) -> u32 {
    let q = c.arg2(4);
    let ok = c.item().is_some_and(|i| match families {
        Some(f) => f.contains(&c.family(&i)),
        None => c.family(&i) != family::GENERIC,
    });
    if ok {
        c.update(|i| i.quality = q);
    }
    0
}
pub fn get_quality(c: &mut Ctx) -> u32 {
    quality_of(c, &[family::QUALITY])
}
pub fn get_unk_egg_type(c: &mut Ctx) -> u32 {
    quality_of(c, &[family::UNKEGG])
}
pub fn get_quantity(c: &mut Ctx) -> u32 {
    quality_of(c, &[family::QUANTITY, family::REAGENT])
}
pub fn get_q(c: &mut Ctx) -> u32 {
    get(c, |i| i.quality as i32)
}
pub fn set_q(c: &mut Ctx) -> u32 {
    let q = c.arg2(4);
    c.update(|i| i.quality = q);
    0
}
/// setQuality: anything but a generic thing.
pub fn set_quality(c: &mut Ctx) -> u32 {
    set_quality_of(c, None)
}
pub fn set_quantity(c: &mut Ctx) -> u32 {
    set_quality_of(c, Some(&[family::QUANTITY, family::REAGENT]))
}

/// The record's spare bytes, which scripts use as per-item variables (an NPC's map, an egg's id
/// and flags). See `Item::mapnum`.
pub fn get_map_array(c: &mut Ctx) -> u32 {
    get(c, |i| i.mapnum as i32)
}
pub fn get_npc_array(c: &mut Ctx) -> u32 {
    get(c, |i| i.npcnum as i32)
}
pub fn set_map_array(c: &mut Ctx) -> u32 {
    let v = c.arg2(4) as u8;
    c.update(|i| i.mapnum = v);
    0
}

pub fn get_family(c: &mut Ctx) -> u32 {
    c.item().map_or(0, |i| (c.host.typeflag(i.shape)[1] >> 4) as u32)
}
/// getTypeFlag(bit): one bit of the shape's 64-bit TYPEFLAG entry.
pub fn get_type_flag(c: &mut Ctx) -> u32 {
    let bit = c.arg2(4) as usize;
    c.item().map_or(0, |i| {
        let tf = c.host.typeflag(i.shape);
        (bit < 64 && tf[bit / 8] >> (bit % 8) & 1 != 0) as u32
    })
}
/// Weight in tenths (ScummVM's getWeight): TYPEFLAG weight × 10, quantities per hundred
/// (rounded up), reagents per ten.
pub fn get_weight(c: &mut Ctx) -> u32 {
    c.item().map_or(0, |i| {
        let w = c.host.typeflag(i.shape)[6] as u32;
        match c.family(&i) {
            family::QUANTITY => (w * i.quality as u32).div_ceil(10),
            family::REAGENT => w * i.quality as u32,
            _ => w * 10,
        }
    })
}
/// getWeightIncludingContents: with everything in it (the paperdoll's reckoning).
pub fn get_total_weight(c: &mut Ctx) -> u32 {
    c.host.weight(c.this())
}

pub fn get_status(c: &mut Ctx) -> u32 {
    get(c, |i| i.flags as i32)
}
pub fn or_status(c: &mut Ctx) -> u32 {
    let m = c.arg2(4);
    c.update(|i| i.flags |= m);
    0
}
pub fn and_status(c: &mut Ctx) -> u32 {
    let m = c.arg2(4);
    c.update(|i| i.flags &= m);
    0
}

/// getFootpad(&x, &y, &z): footprint in TYPEFLAG cells (x and y swapped for a flipped thing).
pub fn get_footpad(c: &mut Ctx) -> u32 {
    if let Some(i) = c.item() {
        let (x, y, z) = c.cells(&i);
        for (o, v) in [(4, x), (8, y), (12, z)] {
            c.write(c.arg4(o), &(v as u16).to_le_bytes());
        }
    }
    0
}

/// Footprints overlap; then either the boxes overlap in z (`stacked = false`) or `a` sits on `b`.
fn boxes(c: &Ctx, stacked: bool) -> u32 {
    let (Some(a), Some(b)) = (c.item(), c.item_arg(4)) else { return 0 };
    let ((ax, ay, az), (bx, by, bz)) = (c.dims(&a), c.dims(&b));
    let xy = a.x - ax < b.x && b.x - bx < a.x && a.y - ay < b.y && b.y - by < a.y;
    (xy && if stacked { a.z == b.z + bz } else { a.z < b.z + bz && b.z < a.z + az }) as u32
}
pub fn overlaps(c: &mut Ctx) -> u32 {
    boxes(c, false)
}
pub fn is_on(c: &mut Ctx) -> u32 {
    boxes(c, true)
}

/// legalCreateAtCoords(&id, shape, frame, x, y, z): the first argument is the out-parameter.
/// Nothing is made (0) where something solid is in the way — the Avatar too: doors check the
/// swing they're about to make this way, and send the Avatar back out of it.
pub fn legal_create_at_coords(c: &mut Ctx) -> u32 {
    let at = (c.arg2(8) as i32, c.arg2(10) as i32, c.arg1(12) as i32);
    if !c.host.can_exist_at(c.arg2(4) as u32, at) {
        return 0;
    }
    let it = Item {
        x: c.arg2(8) as i32,
        y: c.arg2(10) as i32,
        z: c.arg1(12) as i32,
        shape: c.arg2(4) as u32,
        frame: c.arg2(6) as u32,
        ..Default::default()
    };
    let id = c.host.create(it, true);
    c.write(c.arg4(0), &id.to_le_bytes());
    1
}
/// create(&id, shape, frame): a new item off the map (in the void).
pub fn create(c: &mut Ctx) -> u32 {
    let it = Item { shape: c.arg2(4) as u32, frame: c.arg2(6) as u32, ..Default::default() };
    let id = c.host.create(it, false);
    c.write(c.arg4(0), &id.to_le_bytes());
    1
}

/// push: into the ethereal void, on top of the stack.
pub fn push(c: &mut Ctx) -> u32 {
    let id = c.this();
    c.host.push_ethereal(id);
    0
}
/// pop: the top of the ethereal stack back where it was. (These work on the stack's top, not
/// the item named: ScummVM's reading; scripts name the same one anyway.)
pub fn pop(c: &mut Ctx) -> u32 {
    let Some(id) = c.host.pop_ethereal(Pop::Back) else { return 0 };
    event_on(c, id, event::JUST_MOVED, &[]); // (Item::move and moveToContainer run it)
    id as u32
}
/// popToCoords(x, y, z): the top of the ethereal stack onto the map there.
pub fn pop_to_coords(c: &mut Ctx) -> u32 {
    let (x, y, z) = (c.arg2(4) as i32, c.arg2(6) as i32, c.arg1(8) as i32);
    let Some(id) = c.host.pop_ethereal(Pop::At((x, y, z))) else { return 0 };
    event_on(c, id, event::JUST_MOVED, &[]);
    id as u32
}
/// popToContainer(container) / popToEnd(container): the top of the ethereal stack into it.
pub fn pop_to_container(c: &mut Ctx) -> u32 {
    let into = c.arg2(4);
    let Some(id) = c.host.pop_ethereal(Pop::Into(into)) else { return 0 };
    if c.host.container(id).is_some() {
        event_on(c, id, event::JUST_MOVED, &[]); // (moveToContainer's)
    }
    id as u32
}
/// legalCreateInCont(&id, shape, frame, container, _): a new thing put straight into a
/// container (no room or weight check: ScummVM); 1 and its id, or 0.
pub fn legal_create_in_cont(c: &mut Ctx) -> u32 {
    let (ptr, into) = (c.arg4(0), c.arg2(8));
    c.write(ptr, &[0, 0]);
    let it = Item { shape: c.arg2(4) as u32, frame: c.arg2(6) as u32, ..Default::default() };
    let id = c.host.create(it, false);
    if !c.host.move_into(id, into, false) {
        c.host.destroy(id);
        return 0;
    }
    c.write(ptr, &id.to_le_bytes());
    event_on(c, id, event::JUST_MOVED, &[]);
    1
}
/// legalMoveToContainer(&container, _): into it if there's room and the Avatar can carry it.
pub fn legal_move_to_container(c: &mut Ctx) -> u32 {
    let (id, into) = (c.this(), le16(&c.read(c.arg4(4), 2)));
    if !c.host.move_into(id, into, true) {
        return 0;
    }
    event_on(c, id, event::JUST_MOVED, &[]);
    1
}
/// getEtherealTop: what's on top of the ethereal stack (0: nothing).
pub fn ethereal_top(c: &mut Ctx) -> u32 {
    c.host.ethereal_top().unwrap_or(0) as u32
}
/// legalCreateAtPoint(&id, shape, frame, &WorldPoint): a new thing there, if there's room.
pub fn legal_create_at_point(c: &mut Ctx) -> u32 {
    let (shape, frame) = (c.arg2(4) as u32, c.arg2(6) as u32);
    let wp = c.read(c.arg4(8), 5);
    let (x, y, z) =
        (u16::from_le_bytes([wp[0], wp[1]]) as i32, u16::from_le_bytes([wp[2], wp[3]]) as i32, wp[4] as i32);
    if !c.host.can_exist_at(shape, (x, y, z)) {
        return 0;
    }
    let id = c.host.create(Item { x, y, z, shape, frame, ..Default::default() }, true);
    c.write(c.arg4(0), &id.to_le_bytes());
    1
}
/// guardianBark(n): the Guardian's voice (the item's guardianBark event).
pub fn guardian_bark(c: &mut Ctx) -> u32 {
    let (id, n) = (c.this(), c.arg2(4));
    event_on(c, id, event::GUARDIAN_BARK, &n.to_le_bytes())
}
/// Start `id`'s handler for `ev` with `args`; its process (0: none).
pub(super) fn event_on(c: &mut Ctx, id: ObjId, ev: usize, args: &[u8]) -> u32 {
    let Some(it) = c.host.item(id) else { return 0 };
    let family = c.host.typeflag(it.shape)[1] >> 4;
    let class = crate::usecode::vm::event_class(c.usecode, id, &it, family, ev);
    let pid = c.vm.call_event(c.usecode, class, ev, id, args);
    event_now(c, pid)
}

/// move(x, y, z): z is a byte.
pub fn move_to(c: &mut Ctx) -> u32 {
    let (x, y, z) = (c.arg2(4) as i32, c.arg2(6) as i32, c.arg1(8) as i32);
    let id = c.this();
    c.host.move_to(id, (x, y, z)); // (out of its container or the void)
    event_on(c, id, event::JUST_MOVED, &[]);
    0
}
pub fn destroy(c: &mut Ctx) -> u32 {
    let id = c.this();
    c.host.destroy(id);
    0
}
/// destroyContents: everything inside, all the way down (nothing spills).
pub fn destroy_contents(c: &mut Ctx) -> u32 {
    fn wipe(c: &mut Ctx, id: ObjId) {
        for k in c.host.contents(id) {
            wipe(c, k);
            c.host.destroy(k);
        }
    }
    let id = c.this();
    wipe(c, id);
    0
}
/// gotHit(hitter, force): as if struck — runs the item's gotHit event; returns its process.
pub fn got_hit(c: &mut Ctx) -> u32 {
    let (id, args) = (c.this(), [c.arg2(4).to_le_bytes(), c.arg2(6).to_le_bytes()].concat());
    event_on(c, id, event::GOT_HIT, &args)
}
/// legalMoveToPoint(&WorldPoint, ...): always legal for now.
pub fn legal_move_to_point(c: &mut Ctx) -> u32 {
    let wp = c.read(c.arg4(4), 5);
    let to = (u16::from_le_bytes([wp[0], wp[1]]) as i32, u16::from_le_bytes([wp[2], wp[3]]) as i32, wp[4] as i32);
    let (id, abort) = (c.this(), c.arg2(8) != 0);
    let Some(from) = c.item() else { return 0 };
    let Some((clear, before, now)) = c.host.collide_move(id, to, abort) else { return 0 };
    // What it ends up in or on is struck (gotHit; the mover's hit), what it left is let go.
    let (dx, dy, dz) = ((to.0 - from.x).abs() / 4, (to.1 - from.y).abs() / 4, (to.2 - from.z).abs());
    let force = ((dx + dy + dz + dy.max(dz)) / 2) as i16; // (ScummVM's, y and z only in the max)
    let args = |o: ObjId| [o.to_le_bytes(), force.to_le_bytes()].concat();
    for &o in &now {
        event_on(c, o, event::GOT_HIT, &args(id));
        event_on(c, id, event::HIT, &args(o));
    }
    let gone: Vec<ObjId> = before.into_iter().filter(|o| !now.contains(o)).collect();
    for &o in &gone {
        event_on(c, o, event::RELEASE, &[]);
    }
    if !gone.is_empty() {
        event_on(c, id, event::RELEASE, &[]);
    }
    c.vm.requests.push(Request::Moved(id));
    clear as u32
}

/// shoot(&WorldPoint, speed, gravity): thrown at the point (darts, the fishing line, death
/// disks), as ScummVM's MissileTracker aims.
pub fn shoot(c: &mut Ctx) -> u32 {
    let wp = c.read(c.arg4(4), 5);
    let to = (u16::from_le_bytes([wp[0], wp[1]]) as i32, u16::from_le_bytes([wp[2], wp[3]]) as i32, wp[4] as i32);
    let (speed, gravity) = (c.arg2(8) as i32, c.arg2(10) as i32);
    let Some(it) = c.item().filter(|_| c.host.container(c.this()).is_none()) else { return 0 };
    let v = crate::game::missile((it.x, it.y, it.z), to, speed.max(1), gravity);
    c.vm.requests.push(Request::Hurl { id: c.this(), v, gravity, pid: 0 });
    0
}
/// ascend(dz): up or down with what rests on it (lifts); 1 if it went all the way.
pub fn ascend(c: &mut Ctx) -> u32 {
    let (id, dz) = (c.this(), c.arg2(4) as i16 as i32);
    let (all, left) = c.host.ascend(id, dz);
    for o in left {
        c.vm.requests.push(Request::Fall(o));
    }
    all as u32
}
/// getSurfaceWeight: what everything resting on it weighs.
pub fn get_surface_weight(c: &mut Ctx) -> u32 {
    c.host.surface_weight(c.this())
}
/// grab: taken from where it lies — what rests on it falls, what it rested on is let go.
pub fn grab(c: &mut Ctx) -> u32 {
    let id = c.this();
    for o in c.host.grab(id) {
        c.vm.requests.push(Request::Fall(o));
    }
    for o in c.host.search_surface(id, false, true) {
        event_on(c, o, event::RELEASE, &[]);
    }
    0
}
/// canReach(other, range): close enough (box to box) and in sight.
pub fn can_reach(c: &mut Ctx) -> u32 {
    c.host.can_reach(c.this(), c.arg2(4), c.arg2(6) as i16 as i32) as u32
}
/// getRange(other): the gap between the two.
pub fn get_range(c: &mut Ctx) -> u32 {
    c.host.range_between(c.this(), c.arg2(4)).map_or(0, |r| r as u32)
}

fn dir_coords(c: &Ctx) -> u32 {
    c.item().map_or(0, |i| dir_towards(c.arg2(4) as i32 - i.x, c.arg2(6) as i32 - i.y) as u32)
}
fn dir_item(c: &Ctx) -> u32 {
    match (c.item(), c.item_arg(4)) {
        (Some(a), Some(b)) => dir_towards(b.x - a.x, b.y - a.y) as u32,
        _ => 0,
    }
}
pub fn dir_to_coords(c: &mut Ctx) -> u32 {
    dir_coords(c)
}
pub fn dir_from_coords(c: &mut Ctx) -> u32 {
    (dir_coords(c) + 4) % 8
}
pub fn dir_to_item(c: &mut Ctx) -> u32 {
    dir_item(c)
}
pub fn dir_from_item(c: &mut Ctx) -> u32 {
    (dir_item(c) + 4) % 8
}

/// Run one of `this`'s event handlers; returns the new process.
fn trigger(c: &mut Ctx, ev: usize) -> u32 {
    let id = c.this();
    let pid = c.vm.item_event(c.usecode, &*c.host, id, ev);
    event_now(c, pid)
}

/// A handler just started runs at once, till it first waits (the original's UnkProcess).
fn event_now(c: &mut Ctx, pid: Option<crate::usecode::Pid>) -> u32 {
    let Some(pid) = pid else { return 0 };
    c.vm.run_now(pid, c.usecode, c.host);
    pid as u32
}
pub fn look(c: &mut Ctx) -> u32 {
    trigger(c, event::LOOK)
}
pub fn use_item(c: &mut Ctx) -> u32 {
    trigger(c, event::USE)
}
/// enterFastArea: brings it into the fast area now, and runs its event even if it was in
/// already (ITEM.C: NPC.C's exitConvo uses it to set people going again after a talk).
pub fn enter_fast_area(c: &mut Ctx) -> u32 {
    let id = c.this();
    enter_fast(c, id)
}

/// `id` into the fast area, its event run at once (not for the dead).
pub(super) fn enter_fast(c: &mut Ctx, id: crate::world::ObjId) -> u32 {
    match c.host.enter_fast(id) {
        Some(true) => event_on(c, id, event::ENTER_FAST_AREA, &[]),
        _ => 0,
    }
}

use crate::world::item_flag::GUMP_OPEN;

/// getContainer: the container this item is in (0: on the map).
pub fn get_container(c: &mut Ctx) -> u32 {
    c.host.container(c.this()).unwrap_or(0) as u32
}

/// getRootContainer: the outermost container (0: on the map).
pub fn get_root_container(c: &mut Ctx) -> u32 {
    let mut id = c.this();
    let mut root = 0;
    for _ in 0..32 {
        match c.host.container(id) {
            Some(p) => {
                root = p;
                id = p;
            }
            None => break,
        }
    }
    root as u32
}

/// openGump(gump): show the item's window (a container's contents, the Avatar's paperdoll).
pub fn open_gump(c: &mut Ctx) -> u32 {
    let (item, gump) = (c.this(), c.arg2(4));
    c.update(|i| i.flags |= GUMP_OPEN);
    c.vm.events.push_back(crate::usecode::UiEvent::OpenGump { item, gump });
    0
}

pub fn close_gump(c: &mut Ctx) -> u32 {
    let item = c.this();
    c.update(|i| i.flags &= !GUMP_OPEN);
    c.vm.events.push_back(crate::usecode::UiEvent::CloseGump { item });
    0
}

/// receiveHit(other, dir, damage, type): struck by `other` (the game works out the damage
/// when it's 0, the hurt or death, and the target's `gotHit`).
pub fn receive_hit(c: &mut Ctx) -> u32 {
    let (by, dir, damage, kind) = (c.arg2(4), (c.arg2(6) & 7) as usize, c.arg2(8) as i16 as i32, c.arg2(10));
    let target = c.this();
    c.vm.requests.push(Request::Hit { target, by, dir, damage, kind });
    0
}

/// hurl(xs, ys, zs, gravity): send this flying; a process that ends when it comes to rest.
pub fn hurl(c: &mut Ctx) -> u32 {
    let id = c.this();
    let a = |k: usize| c.arg2(4 + 2 * k) as i16 as i32;
    let (v, gravity) = ((a(0), a(1), a(2)), a(3));
    let pid = c.vm.spawn_external(id, GRAVITY);
    c.vm.requests.push(Request::Hurl { id, v, gravity, pid });
    pid as u32
}
/// Process type of things in the air.
const GRAVITY: u16 = 0x203;
/// fall: drop if nothing holds it up.
pub fn fall(c: &mut Ctx) -> u32 {
    let id = c.this();
    c.vm.requests.push(Request::Fall(id));
    0
}
/// explode([type, destroy]): blow up.
pub fn explode(c: &mut Ctx) -> u32 {
    let (id, destroy) = (c.this(), c.args.len() <= 4 || c.arg2(6) != 0);
    c.vm.requests.push(Request::Explode { id, destroy });
    0
}
/// isExplosive: does this blow up when struck (TYPEFLAG)?
pub fn is_explosive(c: &mut Ctx) -> u32 {
    c.item().map_or(0, |it| (c.host.typeflag(it.shape)[5] & 0x20 != 0) as u32)
}
/// igniteChaos(x, y, z): set off the oil flasks about the point.
pub fn ignite_chaos(c: &mut Ctx) -> u32 {
    let (x, y) = (c.arg2(0) as i32, c.arg2(2) as i32);
    c.vm.requests.push(Request::Ignite { x, y });
    0
}
