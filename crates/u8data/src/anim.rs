//! ANIM.DAT: per-shape actor animations — 64 actions × 8 directions × N frames.
use crate::u32le;

/// One animation frame. U8 stores 6 bytes: frame lo, (frame hi:3 | flags hi:5), dz, sfx, ddir, flags lo.
#[derive(Clone, Copy, Debug, Default)]
pub struct AnimFrame {
    pub frame: u32,
    pub dz: i8,
    pub sfx: u8,
    pub ddir: i8,
    pub flags: u16,
}

impl AnimFrame {
    pub const FLIPPED: u16 = 0x0020;
    /// The step stays on the ground (climbing and dropping as walking does).
    pub const ON_GROUND: u16 = 0x0002;
    /// How far this frame of an attack reaches, in steps (0: it doesn't strike).
    pub fn attack_range(&self) -> i32 {
        ((self.flags >> 2) & 7) as i32
    }
}

#[derive(Clone, Debug, Default)]
pub struct Action {
    pub flags: u16,
    pub repeat: u8,
    /// frames[dir] — 8 directions.
    pub frames: Vec<Vec<AnimFrame>>,
}

/// Actor actions (ANIM.DAT entry per shape), as the Avatar's frames show them.
pub mod action {
    pub const WALK: usize = 0;
    pub const RUN: usize = 1;
    pub const STAND: usize = 2;
    pub const JUMP_UP: usize = 3;
    pub const STAND_UP: usize = 4;
    pub const READY_WEAPON: usize = 5;
    pub const UNREADY_WEAPON: usize = 6;
    pub const ATTACK: usize = 7;
    pub const ADVANCE: usize = 8;
    pub const RETREAT: usize = 9;
    pub const RUNNING_JUMP: usize = 10;
    pub const STUMBLE: usize = 13;
    pub const DIE: usize = 14;
    pub const COMBAT_STAND: usize = 15;
    pub const LAND: usize = 16;
    pub const SHAKE_HEAD: usize = 11;
    /// Knocked flat (a long fall), hanging from a ledge, and pulling up from it.
    pub const FALL_BACKWARDS: usize = 44;
    pub const HANG: usize = 45;
    pub const CLIMB_UP: usize = 46;
    pub const JUMP: usize = 17;
    /// Climbing onto something 16, 24 … 72 units up (19–26).
    pub const CLIMB_16: usize = 19;
    pub const CLIMB_72: usize = 26;
    /// Spellcasting: raising the hands, lowering them, the cast itself.
    pub const CAST1: usize = 27;
    pub const CAST2: usize = 28;
    pub const CAST3: usize = 29;
    /// Gesturing while speaking (ScummVM's Animation::talk; the original plays it itself).
    pub const TALK: usize = 53;
    pub const KICK: usize = 58;
    pub const START_BLOCK: usize = 59;
    pub const STOP_BLOCK: usize = 60;
}

#[derive(Default)]
pub struct AnimDat(pub Vec<Option<Vec<Option<Action>>>>);

impl AnimDat {
    pub fn parse(d: &[u8]) -> Self {
        const ACTIONS: usize = 64;
        let shapes = (d.len() / 4).min(2048);
        let at = |o: usize| (o + 4 <= d.len()).then(|| u32le(d, o) as usize);
        Self(
            (0..shapes)
                .map(|s| {
                    let base = at(s * 4).filter(|&o| o != 0)?;
                    Some(
                        (0..ACTIONS)
                            .map(|a| {
                                let o = at(base + a * 4).filter(|&o| o != 0 && o + 4 <= d.len())?;
                                let (size, flags_lo, repeat, flags_hi) = (d[o] as usize, d[o + 1], d[o + 2], d[o + 3]);
                                let mut p = o + 4;
                                if p + 8 * size * 6 > d.len() {
                                    return None;
                                }
                                let frames = (0..8)
                                    .map(|_| {
                                        (0..size)
                                            .map(|_| {
                                                let b = &d[p..p + 6];
                                                p += 6;
                                                AnimFrame {
                                                    frame: b[0] as u32 | ((b[1] as u32 & 7) << 8),
                                                    dz: b[2] as i8,
                                                    sfx: b[3],
                                                    ddir: b[4] as i8,
                                                    flags: b[5] as u16 | ((b[1] as u16 & 0xF8) << 8),
                                                }
                                            })
                                            .collect()
                                    })
                                    .collect();
                                Some(Action { flags: flags_lo as u16 | (flags_hi as u16) << 8, repeat, frames })
                            })
                            .collect(),
                    )
                })
                .collect(),
        )
    }

    pub fn get(&self, shape: u32, action: usize) -> Option<&Action> {
        self.0.get(shape as usize)?.as_ref()?.get(action)?.as_ref()
    }
}
