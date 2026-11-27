//! The usecode interpreter: cooperative processes, each with its own [`Stack`], run a slice per tick.
//! Pointers are 32-bit: the high word is a segment — a process id for stack addresses, or one of
//! the `SEG_*` below — and the low word an offset.
use super::{
    heap::Heap,
    host::Host,
    intrinsics, search,
    stack::{le16, Stack, R},
};
use crate::world::ObjId;
use std::collections::{HashMap, VecDeque};
use u8data::map::Item;
use u8data::usecode::{operand_len, Usecode, BASE};

mod exec;
mod memory;
mod messages;
mod procs;

pub use messages::{ReadKind, Request, UiEvent};

pub type Pid = u16;

pub(crate) const SEG_OBJ: u32 = 0x8000;
pub(crate) const SEG_GLOBAL: u32 = 0x8001;
pub(crate) const SEG_STRING: u32 = 0x8002;
/// Instructions per process per tick before we assume a busy loop and yield.
const SLICE: usize = 20_000;
/// More live processes than this means a script is spawning out of control.
const MAX_PROCS: usize = 1024;
/// Spawned processes run at once inside their parent's turn, at most this deep.
const MAX_NESTING: u32 = 64;

/// The usecode class that handles event `ev` for item `id`: usecode eggs use class
/// quality + 0x47F (their quality picks the script), NPCs their own class (1024 + npc) when it has
/// the event, everything else its shape's class.
pub fn event_class(uc: &Usecode, id: ObjId, it: &Item, family: u8, ev: usize) -> u32 {
    if family == u8data::typeflag::family::UNKEGG {
        return it.quality as u32 + 0x47F;
    }
    if id > 0 && id < 256 && uc.event(1024 + id as u32, ev).is_some() {
        return 1024 + id as u32;
    }
    it.shape
}

enum Body {
    Uc(Stack),
    /// Finished by the UI (barks, questions, reading).
    Native,
    /// Finishes by itself after this many ticks (animations, delays).
    Timer(u32),
}

struct Proc {
    pid: Pid,
    body: Body,
    item: ObjId,
    ptype: u16,
    temp: u32,
    result: u32,
    wait_for: Option<Pid>,
    done: bool,
    /// The wait ends with this call's return value (getSliderInput).
    wake_temp: bool,
}

pub struct Vm {
    procs: Vec<Proc>,
    next_pid: Pid,
    pub(crate) heap: Heap,
    /// Global flags (bit-addressed), initialised from GAMEDAT/FLAG.DAT.
    pub globals: Vec<u8>,
    pub events: VecDeque<UiEvent>,
    /// Work for the game (actor movement) — see [`Request`].
    pub requests: Vec<Request>,
    /// Game time in minutes (set by the game; scripts read the hour).
    pub minutes: u32,
    /// Intrinsics called that aren't implemented (number → count), for diagnostics.
    pub missing: HashMap<u16, u32>,
    /// Scripts that died with an error, plus refused spawns.
    pub errors: u32,
    /// Log every instruction (env `UC_TRACE`) / every intrinsic call (env `UC_CALLS`).
    pub trace: bool,
    pub trace_calls: bool,
    /// Game ticks since start (what usecode timers read).
    pub ticks: u32,
    /// Avatar in stasis: a scripted sequence is running, player control should be off.
    pub stasis: bool,
    /// What scripts call the Avatar (the name the player chose).
    pub avatar_name: String,
    /// Length of each sound effect in ticks (so scripts can ask whether one is still playing).
    pub sfx_ticks: Vec<u32>,
    /// NPC slots free for scripts' createActor, lowest first (the game keeps it current).
    pub free_npcs: Vec<ObjId>,
    /// Effects started and not stopped: (sfx, item, tick it ends; u32::MAX = looping).
    pub(crate) sounds: Vec<(u16, ObjId, u32)>,
    pub(crate) seed: u32,
    /// The palette's colour matrix and the fade under way (scripts' fades and storms).
    pub fade: crate::fade::Fade,
    /// The screen turned upside down (invertScreen), and the process turning it.
    pub inverter: crate::fade::Inverter,
    pub(crate) inverter_pid: Option<Pid>,
    /// Game clock in ticks since the game began (set by the game; scripts read the time).
    pub clock: u32,
    /// Earthquake: the view shakes up to this many pixels (0: still).
    pub quake: u16,
    /// The original's cheats are on: clicking the Avatar opens its developers' menu (item
    /// creation, every spell, jumping ahead in the story…).
    pub cheats: bool,
    /// How many spawned processes deep [`run_now`](Self::run_now) is.
    nesting: u32,
    /// Each running loop's search (by its list), to check things against as it goes.
    loop_scripts: HashMap<u16, Vec<u8>>,
}

/// Process type meaning "any" (Kernel::PROC_TYPE_ALL).
pub const PROC_TYPE_ALL: u16 = 6;
/// Process type of an actor's animations (the original's AnimPrimitive; the scripts' PT_ANIM).
pub const ANIM_PROC: u16 = 0xF0;
/// Process type of a walk (the original's PathFinder).
pub const PATHFINDER: u16 = 0x204;
/// What someone does of their own accord (the original's Loiter, scripts' routines: PT_ACTIVITY).
pub const ACTIVITY: u16 = 0x205;

/// How long the original waits on an unanswered question (measured: 30.8 and 30.9 s in DOS).
pub const ASK_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(30_800);

impl Vm {
    pub fn new(uc: &Usecode, flags: Option<&[u8]>) -> Self {
        let mut globals = vec![0u8; uc.globals_size().div_ceil(8).max(1024)];
        if let Some(f) = flags {
            let n = f.len().min(globals.len());
            globals[..n].copy_from_slice(&f[..n]);
        }
        Self {
            procs: vec![],
            next_pid: 1,
            heap: Heap::new(),
            globals,
            events: VecDeque::new(),
            requests: vec![],
            minutes: 0,
            missing: HashMap::new(),
            errors: 0,
            trace: std::env::var_os("UC_TRACE").is_some(),
            trace_calls: std::env::var_os("UC_CALLS").is_some(),
            ticks: 0,
            stasis: false,
            avatar_name: "Avatar".into(),
            sfx_ticks: vec![],
            sounds: vec![],
            seed: 0x9E37_79B9,
            fade: Default::default(),
            inverter: Default::default(),
            inverter_pid: None,
            clock: 0,
            quake: 0,
            cheats: false,
            nesting: 0,
            loop_scripts: HashMap::new(),
            free_npcs: vec![],
        }
    }

    /// Live (strings, lists) on the script heap — for spotting leaks.
    pub fn heap_size(&self) -> (usize, usize) {
        self.heap.counts()
    }

    /// Run every ready process for one game tick (the game runs 30 per second).
    pub fn tick(&mut self, uc: &Usecode, host: &mut dyn Host) {
        self.ticks += 1;
        self.fade.tick();
        if self.inverter.tick() {
            if let Some(pid) = self.inverter_pid.take() {
                self.terminate(pid, 0);
            }
        }
        let expired: Vec<Pid> = self
            .procs
            .iter_mut()
            .filter_map(|p| match &mut p.body {
                Body::Timer(n) if !p.done => {
                    *n = n.saturating_sub(1);
                    (*n == 0).then_some(p.pid)
                }
                _ => None,
            })
            .collect();
        for pid in expired {
            self.terminate(pid, 0);
        }
        // Processes spawned during this tick also get to run (they're appended).
        let mut i = 0;
        while i < self.procs.len() {
            let p = &self.procs[i];
            if !p.done && p.wait_for.is_none() && matches!(p.body, Body::Uc(_)) {
                self.run(i, uc, host);
            }
            i += 1;
        }
        // Drop finished processes nobody can wait on any more.
        let waited: Vec<Pid> = self.procs.iter().filter_map(|p| p.wait_for).collect();
        self.procs.retain(|p| !p.done || waited.contains(&p.pid));
    }

    /// A process just spawned runs once, now, before its parent goes on (ScummVM's
    /// addProcessExec; the original's UnkProcess runs from its constructor, events started by
    /// the engine's own calls too): until it first waits or ends. (It still gets its turn this
    /// tick.)
    pub(crate) fn run_now(&mut self, pid: Pid, uc: &Usecode, host: &mut dyn Host) {
        if self.nesting >= MAX_NESTING {
            return; // (a script spawning itself over and over runs in turn instead)
        }
        let runnable = |p: &Proc| p.pid == pid && !p.done && p.wait_for.is_none() && matches!(p.body, Body::Uc(_));
        if let Some(i) = self.procs.iter().position(runnable) {
            self.nesting += 1;
            self.run(i, uc, host);
            self.nesting -= 1;
        }
    }

    fn run(&mut self, i: usize, uc: &Usecode, host: &mut dyn Host) {
        let pid = self.procs[i].pid;
        // Check the stack out while it runs (intrinsics may add processes, moving the table).
        let Body::Uc(mut st) = std::mem::replace(&mut self.procs[i].body, Body::Native) else { return };
        let res = self.exec(pid, &mut st, uc, host);
        let at = (st.class, st.ip);
        if let Some(p) = self.procs.iter_mut().find(|p| p.pid == pid) {
            p.body = Body::Uc(st);
        }
        match res {
            Ok(Some(result)) => self.terminate(pid, result),
            Ok(None) => {}
            Err(e) => {
                eprintln!("usecode: pid {pid} class {:#x} ip {:#x}: {e}", at.0, at.1);
                self.errors += 1;
                self.terminate(pid, 0);
            }
        }
    }
}
