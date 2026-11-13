//! `PaletteFaderProcess::` intrinsics: fades to and from black or white, tints (storms) and
//! lightning, through [`Fade`](crate::fade::Fade). Each returns a process that ends with the
//! fade, which scripts wait on.
use super::Ctx;
use crate::fade::{self, solid};

/// Start a fade and hand back a process lasting as long (0 if a stronger fade is running).
fn run(c: &mut Ctx, from: [i16; 12], to: [i16; 12], ticks: u32, priority: i32) -> u32 {
    if !c.vm.fade.start(from, to, ticks, priority) {
        return 0;
    }
    c.vm.spawn_timer(0, ticks + 1) as u32
}

/// Ticks given as the first argument (if any), else the usual.
fn ticks(c: &Ctx) -> u32 {
    if c.args.len() >= 2 {
        c.arg2(0) as u32
    } else {
        fade::BLACK_TICKS
    }
}

const BLACK: [i16; 12] = [0; 12];

pub fn to_black(c: &mut Ctx) -> u32 {
    let (now, n) = (c.vm.fade.matrix(), ticks(c));
    run(c, now, BLACK, n, fade::PRIORITY_FADE)
}
pub fn from_black(c: &mut Ctx) -> u32 {
    let (to, n) = (c.vm.fade.settled(), ticks(c));
    run(c, BLACK, to, n, fade::PRIORITY_FADE)
}
pub fn to_white(c: &mut Ctx) -> u32 {
    let now = c.vm.fade.matrix();
    run(c, now, solid(255, 255, 255, 0), fade::BLACK_TICKS, fade::PRIORITY_FADE)
}
pub fn from_white(c: &mut Ctx) -> u32 {
    let to = c.vm.fade.settled();
    run(c, solid(255, 255, 255, 0), to, fade::BLACK_TICKS, fade::PRIORITY_FADE)
}

/// fadeToPaletteTransform(transform, priority): tint the palette (3 rain, 4 fire storm, 5
/// saturate…; 0 back to normal), and stay so.
pub fn to_transform(c: &mut Ctx) -> u32 {
    let (t, priority) = (c.arg2(0), c.arg2(2) as i16 as i32);
    let now = c.vm.fade.matrix();
    let pid = run(c, now, fade::transform(t), fade::TRANSFORM_TICKS, priority);
    if pid != 0 {
        c.vm.fade.transform = t;
    }
    pid
}

/// A lightning flash: near-white, fading back.
/// invertScreen: the screen turns upside down (or back); a process that ends when it has.
pub fn invert_screen(c: &mut Ctx) -> u32 {
    let live = c.vm.inverter_pid.filter(|&p| c.vm.alive(p));
    c.vm.inverter.toggle(live.is_some());
    let pid = live.unwrap_or_else(|| c.vm.spawn_external(0, 0));
    c.vm.inverter_pid = Some(pid);
    pid as u32
}
pub fn lightning(c: &mut Ctx) -> u32 {
    let to = c.vm.fade.settled();
    run(c, solid(0xCF, 0xCF, 0xCF, 0x3F), to, fade::LIGHTNING_TICKS, fade::PRIORITY_LIGHTNING)
}
