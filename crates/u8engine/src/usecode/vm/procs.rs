//! Processes: spawning, finishing, killing and counting them.
use super::*;

impl Vm {
    /// `waiter` sleeps until `target` ends (an intrinsic that blocks its caller: it stops at
    /// once and resumes there).
    pub(crate) fn wait_on(&mut self, waiter: Pid, target: Pid) {
        if let Some(p) = self.procs.iter_mut().find(|p| p.pid == waiter) {
            p.wait_for = Some(target);
        }
    }

    /// When what `pid` waits on ends, its result is also this call's return value (`push ret`).
    pub(crate) fn wake_with_value(&mut self, pid: Pid) {
        if let Some(p) = self.procs.iter_mut().find(|p| p.pid == pid) {
            p.wake_temp = true;
        }
    }

    /// A new map: every process ends but `keep` and what it waits on (the script that took the
    /// Avatar there; ScummVM spares the running process), and with them all they asked for.
    pub fn kill_for_map_change(&mut self, keep: Option<Pid>) {
        let Some(keep) = keep else { return self.kill_all() };
        let waits = self.procs.iter().find(|p| p.pid == keep).and_then(|p| p.wait_for);
        self.procs.retain(|p| p.pid == keep || Some(p.pid) == waits);
        (self.events, self.requests) = (Default::default(), vec![]);
        self.sounds.clear();
    }

    /// Stop every script and drop everything they held (map change, or an escape hatch).
    pub fn kill_all(&mut self) {
        self.procs.clear();
        self.events.clear();
        self.requests.clear();
        self.heap.clear();
        self.sounds.clear();
        self.stasis = false;
    }

    /// Is process `pid` still going? (The game stops work for processes scripts have killed.)
    pub fn alive(&self, pid: Pid) -> bool {
        self.procs.iter().any(|p| p.pid == pid && !p.done)
    }

    /// The item process `pid` runs for (its `this`).
    pub fn item_of(&self, pid: Pid) -> Option<ObjId> {
        self.procs.iter().find(|p| p.pid == pid).map(|p| p.item)
    }

    /// Processes not yet finished (including ones waiting on the UI).
    /// Each live process, for diagnostics: pid, item, type, where it is (class and ip, or what
    /// kind of native wait) and what it waits on.
    pub fn describe(&self) -> Vec<String> {
        self.procs
            .iter()
            .filter(|p| !p.done)
            .map(|p| {
                let at = match &p.body {
                    Body::Uc(s) => format!("class {:#x} ip {:#x}", s.class, s.ip),
                    Body::Native => "native".into(),
                    Body::Timer(n) => format!("timer {n}"),
                };
                format!("[{}] item {} type {:#x} {at} waits {:?}", p.pid, p.item, p.ptype, p.wait_for)
            })
            .collect()
    }

    pub fn running(&self) -> usize {
        self.procs.iter().filter(|p| !p.done).count()
    }

    pub(super) fn alloc_pid(&mut self) -> Pid {
        loop {
            let p = self.next_pid;
            self.next_pid = if self.next_pid >= 0x7FF0 { 1 } else { self.next_pid + 1 };
            if !self.procs.iter().any(|q| q.pid == p) {
                return p;
            }
        }
    }

    /// Too many processes to start another (counted and reported as an error).
    pub(super) fn at_capacity(&mut self) -> bool {
        if self.procs.len() < MAX_PROCS {
            return false;
        }
        self.errors += 1;
        if self.errors.is_power_of_two() {
            eprintln!("usecode: {MAX_PROCS} processes running; refusing to start more");
        }
        true
    }

    /// Register a process. Pid 0 means it was refused.
    pub(super) fn add_proc(&mut self, pid: Option<Pid>, body: Body, item: ObjId, ptype: u16) -> Pid {
        if self.at_capacity() {
            return 0;
        }
        let pid = pid.unwrap_or_else(|| self.alloc_pid());
        self.procs.push(Proc {
            pid,
            body,
            item,
            ptype,
            temp: 0,
            result: 0,
            wait_for: None,
            done: false,
            wake_temp: false,
        });
        pid
    }

    /// Start a function: `this_val` (the object, usually its 2-byte id) and `args` go on a fresh stack.
    pub(super) fn spawn(&mut self, class: u16, ip: usize, this_val: &[u8], args: &[u8]) -> Pid {
        if self.at_capacity() {
            return 0;
        }
        let pid = self.alloc_pid();
        let mut st = Stack::new();
        let built = (|| -> R<()> {
            st.push(this_val)?;
            let this_at = st.sp;
            st.push(args)?;
            if !this_val.is_empty() {
                st.push4(((pid as u32) << 16) | this_at as u32)?;
            }
            st.call(class, ip)
        })();
        if built.is_err() {
            self.errors += 1;
            return 0;
        }
        let item = if this_val.len() >= 2 { le16(this_val) } else { 0 };
        self.add_proc(Some(pid), Body::Uc(st), item, 0)
    }

    /// A process the UI finishes (bark, question, page).
    pub(crate) fn spawn_native(&mut self, item: ObjId) -> Pid {
        self.add_proc(None, Body::Native, item, 0x100)
    }

    /// A process the game finishes (via [`Vm::finish`]), of process type `ptype`.
    pub(crate) fn spawn_external(&mut self, item: ObjId, ptype: u16) -> Pid {
        self.add_proc(None, Body::Native, item, ptype)
    }

    /// A process that finishes by itself after `ticks` (stand-in for an animation).
    pub(crate) fn spawn_timer(&mut self, item: ObjId, ticks: u32) -> Pid {
        self.add_proc(None, Body::Timer(ticks), item, 0xF0)
    }

    /// Start `class`'s handler for `ev` on item `id`. None if the class has no such handler.
    pub fn call_event(&mut self, uc: &Usecode, class: u32, ev: usize, id: ObjId, args: &[u8]) -> Option<Pid> {
        let off = uc.event(class, ev)?;
        Some(self.spawn(class as u16, off, &id.to_le_bytes(), args)).filter(|&p| p != 0)
    }

    /// Run an item's handler for `ev` (see [`event_class`] for which class handles it).
    pub fn item_event(&mut self, uc: &Usecode, host: &dyn Host, id: ObjId, ev: usize) -> Option<Pid> {
        let it = host.item(id)?;
        let class = event_class(uc, id, &it, host.typeflag(it.shape)[1] >> 4, ev);
        self.call_event(uc, class, ev, id, &[])
    }

    /// Call intrinsic `num` with raw argument bytes (as `calli` would) from a throwaway process;
    /// its result. For tests and debugging: pointer arguments have no stack to point into.
    pub fn call_intrinsic(&mut self, uc: &Usecode, host: &mut dyn Host, num: u16, args: &[u8]) -> u32 {
        let pid = self.spawn_native(0);
        let mut st = super::Stack::new();
        let r = super::intrinsics::call(self, pid, &mut st, num, args, uc, host);
        self.terminate(pid, r);
        r
    }

    /// A process that waited on another, now ended: what that one ended with.
    pub(crate) fn woken(&self, pid: Pid) -> Option<u32> {
        self.procs.iter().find(|p| p.pid == pid && !p.done && p.wait_for.is_none()).map(|p| p.result)
    }

    /// The UI finished showing something; wakes whoever waits on it with `result`.
    pub fn finish(&mut self, pid: Pid, result: u32) {
        self.terminate(pid, result);
    }

    /// Answer an `ask`: `choice` indexes the options it showed.
    pub fn answer(&mut self, pid: Pid, options: &[String], choice: usize) {
        let s = options.get(choice).cloned().unwrap_or_default();
        let id = self.heap.new_string(s);
        self.finish(pid, id as u32);
    }

    /// A question left unanswered: the original gives the script an empty answer after
    /// [`super::ASK_TIMEOUT`] (Devon then asks if you're still there).
    pub fn answer_nothing(&mut self, pid: Pid) {
        self.answer(pid, &[], 0);
    }

    pub(super) fn terminate(&mut self, pid: Pid, result: u32) {
        if let Some(p) = self.procs.iter_mut().find(|p| p.pid == pid) {
            p.done = true;
            p.result = result;
        }
        for p in self.procs.iter_mut().filter(|p| p.wait_for == Some(pid)) {
            p.wait_for = None;
            p.result = result;
            if std::mem::take(&mut p.wake_temp) {
                p.temp = result;
            }
        }
    }

    /// End every other process running for `item` (0: any item, but not the game's own) with
    /// type `ptype` ([`PROC_TYPE_ALL`]: any type), as Kernel::killProcesses.
    pub(crate) fn kill_item_procs(&mut self, item: ObjId, ptype: u16, except: Pid) {
        let victims: Vec<Pid> = self
            .procs
            .iter()
            .filter(|p| p.pid != except && !p.done && p.item != 0 && (item == 0 || p.item == item))
            .filter(|p| ptype == PROC_TYPE_ALL || p.ptype == ptype)
            .map(|p| p.pid)
            .collect();
        victims.into_iter().for_each(|v| self.terminate(v, 0));
    }

    /// How many processes run for `item` (0: any) of type `ptype` ([`PROC_TYPE_ALL`]: any).
    pub(crate) fn count_procs(&self, item: ObjId, ptype: u16) -> u32 {
        let live = self.procs.iter().filter(|p| !p.done && (item == 0 || p.item == item));
        live.filter(|p| ptype == PROC_TYPE_ALL || p.ptype == ptype).count() as u32
    }

    pub(super) fn me(&mut self, pid: Pid) -> &mut Proc {
        self.procs.iter_mut().find(|p| p.pid == pid).expect("own process")
    }
}
