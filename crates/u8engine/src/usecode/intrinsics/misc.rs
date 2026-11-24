//! Global intrinsics: randomness, time, the Avatar's stasis and name, shape lookups, processes.
use super::Ctx;
use crate::usecode::vm::{Request, UiEvent};

/// random(n): 0..n-1.
pub fn random(c: &mut Ctx) -> u32 {
    let n = c.arg2(0) as u32;
    let s = &mut c.vm.seed;
    *s ^= *s << 13;
    *s ^= *s >> 17;
    *s ^= *s << 5;
    if n == 0 {
        0
    } else {
        *s % n
    }
}

pub fn avatar_name(c: &mut Ctx) -> u32 {
    let name = c.vm.avatar_name.clone();
    c.vm.heap.new_string(name) as u32
}

/// Stasis: a cutscene has the Avatar; the host withholds player control while it's set.
pub fn get_stasis(c: &mut Ctx) -> u32 {
    c.vm.stasis as u32
}
pub fn set_stasis(c: &mut Ctx) -> u32 {
    c.vm.stasis = c.arg2(0) != 0;
    0
}

/// playMusic(track): the MUSIC.FLX entry to play (music objects call it as a map loads).
pub fn play_music(c: &mut Ctx) -> u32 {
    c.vm.events.push_back(UiEvent::Music(c.arg1(0) & 0x7F));
    0
}
/// stopMusic: silence (track 0).
pub fn stop_music(c: &mut Ctx) -> u32 {
    c.vm.events.push_back(UiEvent::Music(0));
    0
}

/// Hours of game time (scripts take it mod 24 for the hour of day).
pub fn game_hours(c: &mut Ctx) -> u32 {
    c.vm.minutes / 60
}

/// getCurrentTimerTick: a 60 Hz count (ScummVM's: "ticks of a 60Hz timer, with the default
/// animrate of 30Hz"), two for each of our game ticks. Scripts wait on it (the drowned Avatar's
/// ripples, 50 apart: 5/6 of a second).
pub fn timer_tick(c: &mut Ctx) -> u32 {
    c.vm.ticks.wrapping_mul(2)
}

pub fn family_of_type(c: &mut Ctx) -> u32 {
    (c.host.typeflag(c.arg2(0) as u32)[1] >> 4) as u32
}

/// resetRef(item, type): stop the item's processes of that type (scripts restarting a routine).
/// The original's animations are processes too (type 0xF0): those stop where they are, at once
/// (realDoAnim stops the last before starting the next), or behind a blow on the same one the
/// script dealt just before, which the game lands after the script's turn (BODYPART cuts the
/// Avatar's death short so its burnt remains, unseen, aren't drawn falling).
pub fn reset_ref(c: &mut Ctx) -> u32 {
    let (item, ty) = (c.arg2(0), c.arg2(2));
    let pid = c.pid;
    c.vm.kill_item_procs(item, ty, pid);
    if ty == crate::usecode::vm::ANIM_PROC && item != 0 {
        let behind = |r: &Request| matches!(r, Request::Hit { target, .. } if *target == item);
        if c.vm.requests.iter().any(behind) {
            c.vm.requests.push(Request::StopActing(item));
        } else {
            c.host.stop_acting(item);
        }
    }
    0
}

/// numToStr(n): the number as a string.
pub fn num_to_str(c: &mut Ctx) -> u32 {
    let n = c.arg2(0) as i16;
    c.vm.heap.new_string(n.to_string()) as u32
}

/// rndRange(lo, hi): lo..=hi.
pub fn rnd_range(c: &mut Ctx) -> u32 {
    let (lo, hi) = (c.arg2(0) as i16 as i32, c.arg2(2) as i16 as i32);
    if hi <= lo {
        return lo as u32;
    }
    let s = &mut c.vm.seed;
    *s ^= *s << 13;
    *s ^= *s >> 17;
    *s ^= *s << 5;
    (lo + (*s % (hi - lo + 1) as u32) as i32) as u32
}

pub fn yes(_: &mut Ctx) -> u32 {
    1
}
/// avatarCanCheat: no (the original's cheat mode isn't offered).
/// closeItemGumps: every container window and the paperdoll close.
pub fn close_item_gumps(c: &mut Ctx) -> u32 {
    c.vm.events.push_back(UiEvent::CloseItemGumps);
    0
}
/// TonysBalls(_, _, x, y, z): a fireball after the Avatar (LEVER).
pub fn tonys_balls(c: &mut Ctx) -> u32 {
    let at = (c.arg2(4) as i16 as i32, c.arg2(6) as i16 as i32, c.arg1(8) as i32);
    c.vm.requests.push(Request::Fireball(at));
    0
}
/// castGrantPeace (ERTHSPEL): the game casts it (see `peace`).
pub fn cast_grant_peace(c: &mut Ctx) -> u32 {
    c.vm.requests.push(Request::GrantPeace);
    0
}

/// Are the cheats on (the Avatar's look opens the developers' menu)?
pub fn can_cheat(c: &mut Ctx) -> u32 {
    c.vm.cheats as u32
}

pub fn no(_: &mut Ctx) -> u32 {
    0
}

/// getNumProcesses(item, type): how many processes run for it (0: any item; type 6: any).
pub fn num_processes(c: &mut Ctx) -> u32 {
    let (item, ty) = (c.arg2(0), c.arg2(2));
    c.vm.count_procs(item, ty)
}

/// Minutes and seconds of game time since the start, as ScummVM's: 900 "seconds" to the game
/// hour (the clock counts 27000 to the hour).
pub fn time_in_minutes(c: &mut Ctx) -> u32 {
    c.vm.clock / (30 * 60)
}
pub fn time_in_seconds(c: &mut Ctx) -> u32 {
    c.vm.clock / 30
}
/// setTimeInGameHours(h): the clock jumps (sleeping, scenes).
pub fn set_time_in_game_hours(c: &mut Ctx) -> u32 {
    let h = c.arg2(0);
    c.vm.requests.push(Request::SetHour(h));
    0
}

/// startQuake(strength) / stopQuake: the view shakes.
pub fn start_quake(c: &mut Ctx) -> u32 {
    c.vm.quake = c.arg2(0);
    0
}
pub fn stop_quake(c: &mut Ctx) -> u32 {
    c.vm.quake = 0;
    0
}

/// accumulateStrength/Intelligence/Dexterity(n): use towards the Avatar's next point.
fn grow(c: &mut Ctx, stat: usize) -> u32 {
    let n = c.arg2(0) as i16;
    if n > 0 {
        c.vm.requests.push(Request::Grow(stat, n as u32));
    }
    0
}
pub fn grow_str(c: &mut Ctx) -> u32 {
    grow(c, 0)
}
pub fn grow_dex(c: &mut Ctx) -> u32 {
    grow(c, 1)
}
pub fn grow_int(c: &mut Ctx) -> u32 {
    grow(c, 2)
}

/// feedAvatar(food): eating sates hunger (and so heals).
pub fn feed_avatar(c: &mut Ctx) -> u32 {
    let food = c.arg2(0);
    c.vm.requests.push(Request::Feed(food));
    0
}

/// The Avatar in or out of combat (scripts: conversations put the weapon away).
pub fn set_avatar_in_combat(c: &mut Ctx) -> u32 {
    c.host.set_in_combat(crate::world::AVATAR, true);
    0
}
pub fn clr_avatar_in_combat(c: &mut Ctx) -> u32 {
    c.host.set_in_combat(crate::world::AVATAR, false);
    0
}
pub fn is_avatar_in_combat(c: &mut Ctx) -> u32 {
    c.host.in_combat(crate::world::AVATAR) as u32
}

/// canExistAt(shape, x, y, z, flags, mover, volume): room for it there, the mover left out, and
/// unless the flags say it floats, standing on something it may stand on (MOTRAK.C; the flags in
/// [`exist`](crate::world::exist)). The scripts pass (1, 0, 0): on land (fire shield, daemons,
/// Beren, the Avatar stepping back from a door, things set on things); and (4, itself, 0): darts
/// and death disks in flight. (The volume, always 0, would replace the shape's own collision
/// flags: not done.)
pub fn can_exist_at(c: &mut Ctx) -> u32 {
    let (shape, x, y, z) = (c.arg2(0) as u32, c.arg2(2) as i32, c.arg2(4) as i32, c.arg1(6) as i32);
    let mover = Some(c.arg2(10)).filter(|&m| m != 0);
    c.host.exists_at(shape, (x, y, z), c.arg1(8), mover) as u32
}

/// createSprite(shape, first, last, [?, repeats,] delay, x, y, z): an effect played out at a
/// point; returns a process that ends with it.
pub fn create_sprite(c: &mut Ctx) -> u32 {
    let a = |k: usize| c.arg2(k * 2) as i16 as i32;
    let (shape, first, last) = (a(0) as u32, a(1) as u32, a(2) as u32);
    let (repeats, rest) = if c.args.len() >= 18 { (a(4).max(1) as u32, 5) } else { (1, 3) };
    let (delay, x, y, z) =
        (a(rest).max(1) as u32, c.arg2(rest * 2 + 2) as i32, c.arg2(rest * 2 + 4) as i32, c.arg1(rest * 2 + 6) as i32);
    let pid = c.vm.spawn_external(0, SPRITE);
    c.vm.requests.push(Request::Sprite { pid, shape, first, last, repeats, delay, at: (x, y, z) });
    pid as u32
}
/// Process type of sprite effects.
const SPRITE: u16 = 0x20E;

/// teleportToEgg(map, id, stasis): the Avatar to a teleport egg (a process that does it).
pub fn teleport_to_egg(c: &mut Ctx) -> u32 {
    let (map, id) = if c.args.len() >= 6 { (c.arg2(0), c.arg2(2) as u8) } else { (c.host.map_num(), c.arg2(0) as u8) };
    c.vm.requests.push(Request::TeleportToEgg { map, id });
    c.vm.spawn_timer(0, 1) as u32
}

/// target: the player picks something with the targeting cursor; a process that ends with
/// its id (0: none).
pub fn target(c: &mut Ctx) -> u32 {
    let pid = c.vm.spawn_external(0, TARGET);
    c.vm.events.push_back(UiEvent::Target { pid });
    pid as u32
}
const TARGET: u16 = 0x20F;

/// playEndgame: the ending film, then the main menu (the host does both).
pub fn play_endgame(c: &mut Ctx) -> u32 {
    c.vm.events.push_back(UiEvent::Endgame);
    0
}
