//! `Actor::` intrinsics: NPC identity, map, facing, animation, stats, life and death.
use super::Ctx;
use crate::equip::BACKPACK;
use crate::stats::{Npc, FEIGN_TICKS};
use crate::usecode::vm::Request;
use crate::world::item_flag::EQUIPPED;
use u8data::anim::action;

pub fn is_npc(c: &mut Ctx) -> u32 {
    let id = c.this();
    (id > 0 && id < 256) as u32
}

pub fn get_dir(c: &mut Ctx) -> u32 {
    c.host.dir(c.this()) as u32
}

/// doAnim(action, dir, …): show the action's first frame facing `dir`, and return a process that
/// ends after the animation would have played (scripts wait on it).
pub fn do_anim(c: &mut Ctx) -> u32 {
    let id = c.this();
    // (Direction 8 is "the way it faces now".)
    let dir = match c.arg2(6) {
        d @ 0..8 => d as usize,
        _ => c.host.dir(id) as usize,
    };
    let action = c.arg2(4) as usize;
    let ticks = c.host.play(id, action, dir).map_or(4, |t| t.max(2));
    c.vm.spawn_timer(id, ticks) as u32
}

/// getMap: the map an actor is on (one sent to another map is still known here, in the void).
pub fn get_map(c: &mut Ctx) -> u32 {
    let here = c.host.map_num() as u32;
    c.item().map_or(0, |it| if it.npc != 0 && it.mapnum != 0 { it.mapnum as u32 } else { here })
}

/// teleport(x, y, z, map): straight there; to another map means off this one.
pub fn teleport(c: &mut Ctx) -> u32 {
    let (x, y, z, map) = (c.arg2(4) as i32, c.arg2(6) as i32, c.arg2(8) as i32, c.arg2(10));
    let id = c.this();
    if map != c.host.map_num() && id == crate::world::AVATAR {
        // The Avatar to another map: the game goes too; the script waits, and carries on there.
        let pid = c.vm.spawn_external(id, 0);
        c.vm.wait_on(c.pid, pid);
        c.vm.requests.push(Request::AvatarTo { map, at: (x, y, z), by: c.pid, pid });
        return 0;
    }
    if map != c.host.map_num() {
        // Off to another map: gone from this one, but remember where (saving puts it there).
        c.update(|i| (i.x, i.y, i.z, i.mapnum) = (x, y, z, map as u8));
        c.host.set_ethereal(id, true);
        return 0;
    }
    c.host.set_ethereal(id, false);
    c.update(|i| (i.x, i.y, i.z, i.mapnum) = (x, y, z, map as u8));
    let by = c.vm.item_of(c.pid).unwrap_or(0);
    c.vm.requests.push(Request::Teleported { id, by });
    0
}

/// pathfindToItem(item, range): walk next to it; returns a process that ends on arrival.
pub fn pathfind_to_item(c: &mut Ctx) -> u32 {
    let (actor, range) = (c.this(), c.arg2(6) as i32);
    let Some(t) = c.item_arg(4) else { return 0 };
    let pid = c.vm.spawn_external(actor, PATHFINDER);
    c.vm.requests.push(Request::Walk { pid, actor, to: (t.x, t.y, t.z), range });
    pid as u32
}

/// pathfindToPoint(x, y, z, _): walk there; returns a process that ends on arrival.
pub fn pathfind_to_point(c: &mut Ctx) -> u32 {
    let (actor, to) = (c.this(), (c.arg2(4) as i32, c.arg2(6) as i32, c.arg1(8) as i32));
    let pid = c.vm.spawn_external(actor, PATHFINDER);
    c.vm.requests.push(Request::Walk { pid, actor, to, range: 0 });
    pid as u32
}

/// Process type of walking (NPC routines reset it before giving new orders).
const PATHFINDER: u16 = 0x204;

/// isInCombat: in combat stance (the Avatar's comes from the player).
pub fn is_in_combat(c: &mut Ctx) -> u32 {
    c.host.in_combat(c.this()) as u32
}

pub fn set_in_combat(c: &mut Ctx) -> u32 {
    let id = c.this();
    c.host.set_in_combat(id, true);
    0
}

/// clrInCombat: out of combat stance (conversations put the Avatar's weapon away).
pub fn clr_in_combat(c: &mut Ctx) -> u32 {
    let id = c.this();
    c.host.set_in_combat(id, false);
    0
}

/// isBusy: in the middle of an animation.
pub fn is_busy(c: &mut Ctx) -> u32 {
    c.host.busy(c.this()) as u32
}

/// areEnemiesNear: anyone else fighting nearby (only asked about the Avatar).
pub fn are_enemies_near(c: &mut Ctx) -> u32 {
    c.host.fighting_near(c.this(), ENEMIES_NEAR) as u32
}
const ENEMIES_NEAR: i32 = 0x800;

/// setTarget(who): into combat (if need be) and after `who`.
pub fn set_target(c: &mut Ctx) -> u32 {
    let (id, t) = (c.this(), c.arg2(4));
    c.host.set_target(id, t);
    0
}
pub fn get_target(c: &mut Ctx) -> u32 {
    c.host.target(c.this()) as u32
}

/// Alignment bits as scripts give them: some as a nibble (2, 5, 8), some already shifted into
/// NPCDATA's enemies nibble (0x20, 0x40); either way the same four sides.
fn sides(v: u16) -> u8 {
    (if v > 0xF { v >> 4 } else { v } & 0xF) as u8
}
pub fn set_alignment(c: &mut Ctx) -> u32 {
    let v = sides(c.arg2(4));
    with_stats(c, |s| s.alignment = v)
}
pub fn get_alignment(c: &mut Ctx) -> u32 {
    stat(c, |s| s.alignment as i32)
}
pub fn set_enemy_alignment(c: &mut Ctx) -> u32 {
    let v = sides(c.arg2(4));
    with_stats(c, |s| s.enemies = v)
}
pub fn get_enemy_alignment(c: &mut Ctx) -> u32 {
    stat(c, |s| s.enemies as i32)
}
/// isEnemy(other): would this person fight `other`?
pub fn is_enemy(c: &mut Ctx) -> u32 {
    let other = c.arg2(4);
    match (c.host.stats(c.this()), c.host.stats(other)) {
        (Some(a), Some(b)) => a.is_enemy(&b) as u32,
        _ => 0,
    }
}

pub fn is_feign_death(c: &mut Ctx) -> u32 {
    stat(c, |s| (s.feigning > 0) as i32)
}
/// setFeignDeath: fall as if dead and get up again; for a while after, the undead (and demons)
/// take the faker for dead (ScummVM's reading: fall, stand up, 30 seconds, done).
pub fn set_feign_death(c: &mut Ctx) -> u32 {
    let id = c.this();
    if c.host.stats(id).is_some_and(|s| s.feigning > 0) {
        return 0;
    }
    let dir = c.host.dir(id) as usize;
    let fall = c.host.play(id, action::DIE, dir).unwrap_or(0);
    with_stats(c, |s| s.feigning = FEIGN_TICKS + fall.min(1000) as u16)
}

/// getLastAnimSet: the ANIM.DAT action the actor last started.
pub fn get_last_anim_set(c: &mut Ctx) -> u32 {
    c.host.last_anim(c.this()) as u32
}
pub fn clr_feign_death(c: &mut Ctx) -> u32 {
    with_stats(c, |s| s.feigning = 0)
}

/// setActivity(n): 0 loiter (about its routine), 1 fight, 2 stand. Returns a process that ends
/// when it's under way.
pub fn set_activity(c: &mut Ctx) -> u32 {
    let (id, activity) = (c.this(), c.arg2(4));
    match activity {
        1 => {
            c.host.set_in_combat(id, true);
            0
        }
        2 => {
            let dir = c.host.dir(id) as usize;
            let ticks = c.host.play(id, action::STAND, dir).unwrap_or(1);
            c.vm.spawn_timer(id, ticks) as u32
        }
        0 => {
            c.vm.requests.push(Request::Loiter(id));
            c.vm.spawn_timer(id, 1) as u32
        }
        _ => c.vm.spawn_timer(id, 1) as u32,
    }
}

/// Change a person's numbers (nothing for things that aren't people).
fn with_stats(c: &mut Ctx, f: impl FnOnce(&mut Npc)) -> u32 {
    let id = c.this();
    if let Some(mut s) = c.host.stats(id) {
        f(&mut s);
        c.host.set_stats(id, s);
    }
    0
}

fn stat(c: &mut Ctx, f: impl FnOnce(Npc) -> i32) -> u32 {
    c.host.stats(c.this()).map_or(0, |s| f(s) as u32)
}

pub fn get_str(c: &mut Ctx) -> u32 {
    stat(c, |s| s.str as i32)
}
pub fn get_int(c: &mut Ctx) -> u32 {
    stat(c, |s| s.int as i32)
}
pub fn get_dex(c: &mut Ctx) -> u32 {
    stat(c, |s| s.dex as i32)
}
pub fn get_hp(c: &mut Ctx) -> u32 {
    stat(c, |s| s.hp as i32)
}
pub fn get_mana(c: &mut Ctx) -> u32 {
    stat(c, |s| s.mana as i32)
}
pub fn set_str(c: &mut Ctx) -> u32 {
    let v = c.arg2(4);
    with_stats(c, |s| s.str = v.min(255) as u8)
}
pub fn set_int(c: &mut Ctx) -> u32 {
    let v = c.arg2(4);
    with_stats(c, |s| s.int = v.min(255) as u8)
}
pub fn set_dex(c: &mut Ctx) -> u32 {
    let v = c.arg2(4);
    with_stats(c, |s| s.dex = v.min(255) as u8)
}
pub fn set_hp(c: &mut Ctx) -> u32 {
    let v = c.arg2(4) as i16;
    with_stats(c, |s| s.hp = v)
}
pub fn set_mana(c: &mut Ctx) -> u32 {
    let v = c.arg2(4) as i16;
    with_stats(c, |s| s.mana = v)
}
/// isDead: someone who isn't there at all counts as dead (ScummVM); people elsewhere answer
/// for themselves.
pub fn is_dead(c: &mut Ctx) -> u32 {
    c.host.npc_dead(c.this()).unwrap_or(true) as u32
}
/// setDead / clrDead. A blow on the same one that the script dealt just before (receiveHit,
/// which the game lands after the script's turn) comes first, as the script's order has it, and
/// so does a change of these already waiting behind one: Mythran's death spell strikes the
/// Avatar for all its hit points, then marks it dead; marked first, the blow struck a corpse and
/// the Avatar stood on, dead but unhurt. (isDead reads the mark as it is now.)
fn mark_dead(c: &mut Ctx, dead: bool) -> u32 {
    let id = c.this();
    let behind = |r: &Request| matches!(r, Request::Hit { target, .. } | Request::SetDead(target, _) if *target == id);
    if c.vm.requests.iter().any(behind) {
        c.vm.requests.push(Request::SetDead(id, dead));
        return 0;
    }
    with_stats(c, |s| s.dead = dead)
}
pub fn set_dead(c: &mut Ctx) -> u32 {
    mark_dead(c, true)
}
pub fn clr_dead(c: &mut Ctx) -> u32 {
    mark_dead(c, false)
}
pub fn is_immortal(c: &mut Ctx) -> u32 {
    stat(c, |s| s.immortal as i32)
}
pub fn set_immortal(c: &mut Ctx) -> u32 {
    with_stats(c, |s| s.immortal = true)
}
pub fn clr_immortal(c: &mut Ctx) -> u32 {
    with_stats(c, |s| s.immortal = false)
}
pub fn is_withstand_death(c: &mut Ctx) -> u32 {
    stat(c, |s| s.withstand_death as i32)
}
pub fn set_withstand_death(c: &mut Ctx) -> u32 {
    with_stats(c, |s| s.withstand_death = true)
}
pub fn clr_withstand_death(c: &mut Ctx) -> u32 {
    with_stats(c, |s| s.withstand_death = false)
}

/// getEquip(type): what the actor wears in slot `type` + 1 (1 shield … 6 weapon, 7 backpack).
pub fn get_equip(c: &mut Ctx) -> u32 {
    let (id, slot) = (c.this(), c.arg2(4) as u8 + 1);
    let worn = c.host.contents(id).into_iter().find(|&t| {
        c.host.item(t).is_some_and(|it| {
            let kind = c.host.typeflag(it.shape)[2] & 0x0F;
            it.flags & EQUIPPED != 0 && (kind == slot || (slot == 7 && it.shape == BACKPACK))
        })
    });
    worn.unwrap_or(0) as u32
}

/// createActor(&id, shape, frame): a new person or creature in a free NPC slot, in the void on
/// the ethereal stack for the script to pop out where it wants it; its id (0: no slot).
pub fn create_actor(c: &mut Ctx) -> u32 {
    let (shape, frame) = (c.arg2(4) as u32, c.arg2(6) as u32);
    let id = loop {
        if c.vm.free_npcs.is_empty() {
            return 0;
        }
        let id = c.vm.free_npcs.remove(0);
        if c.host.create_actor(id, shape, frame) {
            break id;
        }
    };
    c.vm.requests.push(Request::Created(id));
    c.write(c.arg4(0), &id.to_le_bytes());
    id as u32
}

/// setEquip(slot, item): wear it (1), if it's wearable and nothing's on there already (0).
pub fn set_equip(c: &mut Ctx) -> u32 {
    let (who, id) = (c.this(), c.arg2(6));
    c.host.set_equip(who, id) as u32
}

/// schedule(time): run the actor's schedule handler (what to do at that time).
pub fn schedule(c: &mut Ctx) -> u32 {
    let (id, time) = (c.this(), c.arg4(4));
    if c.host.stats(id).is_some_and(|s| s.dead) {
        return 0;
    }
    super::item::event_on(c, id, u8data::usecode::event::SCHEDULE, &time.to_le_bytes())
}

/// set/getAirWalkEnabled: walking on air (a spell).
pub fn set_air_walk(c: &mut Ctx) -> u32 {
    let on = c.arg2(4) != 0;
    with_stats(c, |s| s.airwalk = on)
}
pub fn get_air_walk(c: &mut Ctx) -> u32 {
    stat(c, |s| s.airwalk as i32)
}
