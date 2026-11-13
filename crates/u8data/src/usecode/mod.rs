//! EUSECODE.FLX — U8's compiled game logic ("usecode").
//!
//! Flex entry 0: global flag table (name\0, u16 offset, u16 size)…; entry 1: class names (4-byte header,
//! then 13 bytes per class: 9-byte name + 4 bytes). Class `c` lives in entry `c + 2`; a class is a
//! shape number (< 1024) or 1024 + NPC number. Class layout: 12-byte header, 32 event entries
//! (u32 offset relative to `BASE`), then code. Events: 0 look, 1 use, 2 anim, … (see `EVENTS`).
//! The opcode length table below was validated by sweeping every class end-to-end.
//! The interpreter lives in `u8engine::usecode`.

use crate::flex::Flex;
use crate::u16le;

/// Event/function offsets are relative to this point in a class.
pub const BASE: usize = 0x0C;
pub const EVENT_COUNT: usize = 32;

pub const EVENTS: [&str; EVENT_COUNT] = [
    "look",
    "use",
    "anim",
    "setActivity",
    "cachedIn",
    "hit",
    "gotHit",
    "hatch",
    "schedule",
    "release",
    "equip",
    "unequip",
    "combine",
    "func0D",
    "calledFromAnim",
    "enterFastArea",
    "leaveFastArea",
    "cast",
    "justMoved",
    "AvatarStoleSomething",
    "animGetHit",
    "guardianBark",
    "func16",
    "func17",
    "func18",
    "func19",
    "func1A",
    "func1B",
    "func1C",
    "func1D",
    "func1E",
    "func1F",
];

pub mod event {
    pub const LOOK: usize = 0;
    pub const USE: usize = 1;
    pub const ANIM: usize = 2;
    pub const CACHED_IN: usize = 4;
    /// The mover's side of a contact (args: what it touched, force).
    pub const HIT: usize = 5;
    /// Touched by something moving (args: the mover, force): pressure plates, lava, webs.
    pub const GOT_HIT: usize = 6;
    pub const HATCH: usize = 7;
    pub const SCHEDULE: usize = 8;
    /// No longer touched (or touching) after a move: a plate springs back.
    pub const RELEASE: usize = 9;
    /// Things counted by quantity set their frame by the amount.
    pub const COMBINE: usize = 12;
    pub const ENTER_FAST_AREA: usize = 15;
    pub const LEAVE_FAST_AREA: usize = 16;
    /// Run when someone goes into combat (ScummVM calls it the "special action"): townsfolk cry
    /// out, some decide not to fight after all.
    pub const CAST: usize = 17;
    /// Moved (by the player, or put somewhere).
    pub const JUST_MOVED: usize = 18;
    /// The Avatar took something that wasn't its own, with someone about (arg: the item).
    pub const AVATAR_STOLE: usize = 19;
    /// The Guardian speaks (guardianBark(n) on an item).
    pub const GUARDIAN_BARK: usize = 21;
}

/// A global flag (byte range in the globals block, saved in GAMEDAT/FLAG.DAT).
#[derive(Clone, Debug)]
pub struct Global {
    pub name: String,
    pub offset: u16,
    pub size: u16,
}

pub struct Usecode {
    flex: Flex,
    pub globals: Vec<Global>,
}

impl Usecode {
    pub fn new(flex: Flex) -> Self {
        let mut globals = vec![];
        if let Some(d) = flex.get(0) {
            let mut p = 0;
            while p < d.len() {
                let Some(end) = d[p..].iter().position(|&b| b == 0) else { break };
                if end == 0 || p + end + 5 > d.len() {
                    break;
                }
                let name = String::from_utf8_lossy(&d[p..p + end]).into_owned();
                let o = p + end + 1;
                globals.push(Global { name, offset: u16le(d, o), size: u16le(d, o + 2) });
                p = o + 4;
            }
        }
        Self { flex, globals }
    }

    pub fn class(&self, c: u32) -> Option<&[u8]> {
        self.flex.get(c as usize + 2).filter(|d| d.len() > BASE + EVENT_COUNT * 4)
    }

    pub fn class_count(&self) -> usize {
        self.flex.len().saturating_sub(2)
    }

    pub fn class_name(&self, c: u32) -> Option<String> {
        self.class(c)?;
        let d = self.flex.get(1)?;
        let o = 4 + 13 * c as usize;
        let n = d.get(o..o + 9)?;
        let end = n.iter().position(|&b| b == 0).unwrap_or(9);
        Some(String::from_utf8_lossy(&n[..end]).into_owned())
    }

    /// Code offset (within the class data) of an event handler.
    pub fn event(&self, c: u32, ev: usize) -> Option<usize> {
        let d = self.class(c)?;
        if ev >= EVENT_COUNT {
            return None;
        }
        let o = crate::u32le(d, BASE + ev * 4) as usize;
        (o != 0 && BASE + o < d.len()).then_some(BASE + o)
    }

    /// Size of the global flag block.
    pub fn globals_size(&self) -> usize {
        self.globals.iter().map(|g| g.offset as usize + g.size as usize).max().unwrap_or(0).max(0x1000)
    }
}

/// Operand bytes after the opcode (0x0D push-string is variable: u16 len + bytes + NUL).
pub fn operand_len(code: &[u8], p: usize) -> Option<usize> {
    let op = *code.get(p)?;
    Some(match op {
        0x0D => 2 + code.get(p + 1..p + 3).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize)? + 1,
        0x00
        | 0x01
        | 0x02
        | 0x0A
        | 0x19
        | 0x1A
        | 0x1B
        | 0x3E..=0x41
        | 0x43
        | 0x4B..=0x4D
        | 0x5A
        | 0x62..=0x67
        | 0x69
        | 0x6E
        | 0x6F
        | 0x74 => 1,
        0x03 | 0x0B | 0x0E | 0x38 | 0x42 | 0x44 | 0x45 | 0x51 | 0x52 | 0x54 | 0x5B | 0x6C => 2,
        0x09 | 0x0F | 0x4E | 0x4F | 0x70 => 3,
        0x0C | 0x11 | 0x75 | 0x76 => 4,
        0x57 => 6,
        0x58 => 8,
        0x08
        | 0x12
        | 0x13
        | 0x14..=0x18
        | 0x1C..=0x37
        | 0x39..=0x3D
        | 0x50
        | 0x53
        | 0x59
        | 0x5D..=0x61
        | 0x6B
        | 0x6D
        | 0x73
        | 0x77..=0x7A => 0,
        _ => return None,
    })
}

fn mnemonic(op: u8) -> &'static str {
    match op {
        0x00 => "pop.b bp",
        0x01 => "pop.w bp",
        0x02 => "pop.d bp",
        0x03 => "pop.n bp",
        0x08 => "pop.d result",
        0x09 => "assign elem",
        0x0A => "push.b",
        0x0B => "push.w",
        0x0C => "push.d",
        0x0D => "push str",
        0x0E => "create list",
        0x0F => "calli",
        0x11 => "call",
        0x12 => "pop.w temp",
        0x13 => "pop.d temp",
        0x14 => "add",
        0x15 => "add.d",
        0x16 => "str concat",
        0x17 => "list append",
        0x19 => "slist append",
        0x1A => "slist remove",
        0x1B => "list remove",
        0x1C => "sub",
        0x1D => "sub.d",
        0x1E => "mul",
        0x1F => "mul.d",
        0x20 => "div",
        0x21 => "div.d",
        0x22 => "mod",
        0x23 => "mod.d",
        0x24 => "eq",
        0x25 => "eq.d",
        0x26 => "str eq",
        0x28 => "lt",
        0x29 => "lt.d",
        0x2A => "le",
        0x2B => "le.d",
        0x2C => "gt",
        0x2D => "gt.d",
        0x2E => "ge",
        0x2F => "ge.d",
        0x30 => "not",
        0x31 => "not.d",
        0x32 => "and",
        0x33 => "and.d",
        0x34 => "or",
        0x35 => "or.d",
        0x36 => "ne",
        0x37 => "ne.d",
        0x38 => "in list",
        0x39 => "bitand",
        0x3A => "bitor",
        0x3B => "bitnot",
        0x3C => "shl",
        0x3D => "shr",
        0x3E => "push.b bp",
        0x3F => "push.w bp",
        0x40 => "push.d bp",
        0x41 => "push str bp",
        0x42 => "push list bp",
        0x43 => "push slist bp",
        0x44 => "push elem",
        0x45 => "push.n bp",
        0x4B => "push addr bp",
        0x4C => "push indirect",
        0x4D => "pop indirect",
        0x4E => "push global",
        0x4F => "pop global",
        0x50 => "ret",
        0x51 => "jne",
        0x52 => "jmp",
        0x53 => "suspend",
        0x54 => "implies",
        0x57 => "spawn",
        0x58 => "spawn inline",
        0x59 => "push pid",
        0x5A => "init",
        0x5B => "line",
        0x5D => "push ret.b",
        0x5E => "push ret.w",
        0x5F => "push ret.d",
        0x60 => "w2d",
        0x61 => "d2w",
        0x62 => "free str bp",
        0x63 => "free slist bp",
        0x64 => "free list bp",
        0x65 => "free str sp",
        0x66 => "free list sp",
        0x67 => "free slist sp",
        0x69 => "push strptr bp",
        0x6B => "str2ptr",
        0x6C => "param copy",
        0x6D => "push result",
        0x6E => "add sp",
        0x6F => "push addr sp",
        0x70 => "loop",
        0x73 => "loopnext",
        0x74 => "loopscr",
        0x75 => "foreach list",
        0x76 => "foreach slist",
        0x77 => "set info",
        0x78 => "exclude",
        0x79 => "end",
        0x7A => "end2",
        _ => "?",
    }
}

/// Disassemble one function starting at `start` (stops after `end`/0x79).
pub fn disasm(code: &[u8], start: usize, intrinsic_name: impl Fn(u16) -> Option<&'static str>) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let mut p = start;
    while p < code.len() {
        let op = code[p];
        let Some(a) = operand_len(code, p).and_then(|n| code.get(p + 1..p + 1 + n)) else {
            let _ = writeln!(out, "{p:04x}: ?? {op:02x} (bad or truncated)");
            break;
        };
        let n = a.len();
        let w = |i: usize| u16::from_le_bytes([a[i], a[i + 1]]);
        let extra = match op {
            0x0D => format!(" {:?}", String::from_utf8_lossy(&a[2..a.len() - 1])),
            0x0F => format!(" {:#05x} {} ({} arg bytes)", w(1), intrinsic_name(w(1)).unwrap_or("?"), a[0]),
            0x11 => format!(" class {:#x} @{:#x}", w(0), w(2) as usize + BASE),
            0x51 | 0x52 => format!(" -> {:04x}", (p + 3) as i64 + w(0) as i16 as i64),
            0x57 => format!(" class {:#x} @{:#x} (args {}, this {})", w(2), w(4) as usize + BASE, a[0], a[1]),
            0x74 => format!(" '{}'", a[0] as char),
            0x00..=0x03 | 0x3E..=0x45 | 0x4B | 0x62..=0x64 | 0x69 | 0x6C => {
                format!(" {:+}{}", a[0] as i8, a.get(1).map_or(String::new(), |b| format!(" {b}")))
            }
            _ => a.iter().map(|b| format!(" {b:02x}")).collect(),
        };
        let _ = writeln!(out, "{p:04x}: {}{extra}", mnemonic(op));
        p += 1 + n;
        if op == 0x79 {
            break;
        }
    }
    out
}
