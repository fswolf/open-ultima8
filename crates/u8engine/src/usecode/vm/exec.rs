//! The interpreter: one process's instructions until it yields, suspends or ends.
use super::*;

/// ANCTONES, whose scene two eggs can start at once.
const ANCIENT_ONES: u16 = 0x48B;

impl Vm {
    /// Execute until the process suspends (Ok(None)) or ends (Ok(Some(result))).
    pub(super) fn exec(&mut self, pid: Pid, st: &mut Stack, ucf: &Usecode, host: &mut dyn Host) -> R<Option<u32>> {
        for _ in 0..SLICE {
            let code = ucf.class(st.class as u32).ok_or_else(|| format!("no class {:#x}", st.class))?;
            let p = st.ip;
            let op = *code.get(p).ok_or("ip out of range")?;
            let n = operand_len(code, p).ok_or_else(|| format!("bad opcode {op:#04x}"))?;
            let a = code.get(p + 1..p + 1 + n).ok_or("truncated instruction")?;
            st.ip = p + 1 + n;
            if self.trace {
                eprintln!("[{pid}] {:#x}:{p:04x} op {op:02x} {a:02x?} sp {:04x}", st.class, st.sp);
            }
            let sb = |k: usize| a[k] as i8 as isize;
            let w16 = |k: usize| u16::from_le_bytes([a[k], a[k + 1]]);
            match op {
                // pops into locals
                0x00 => {
                    let v = st.pop2()?;
                    st.write(st.bp_at(sb(0)), &[v as u8]);
                }
                0x01 => {
                    let v = st.pop2()?;
                    st.write(st.bp_at(sb(0)), &v.to_le_bytes());
                }
                0x02 => {
                    let v = st.pop4()?;
                    st.write(st.bp_at(sb(0)), &v.to_le_bytes());
                }
                0x03 => {
                    let v = st.pop(a[1] as usize)?;
                    st.write(st.bp_at(sb(0)), &v);
                }
                0x08 => self.me(pid).result = st.pop4()?,
                0x09 => {
                    let idx = (st.pop2()? as usize).wrapping_sub(1);
                    let mut v = st.pop(a[1] as usize)?;
                    let lid = st.read2(st.bp_at(sb(0)));
                    if a[2] != 0 {
                        v = self.heap.dup_string(le16(&v)).to_le_bytes().to_vec();
                    }
                    self.heap.set_elem(lid, idx, &v);
                }
                // pushes
                0x0A => st.push2(a[0] as i8 as i16 as u16)?,
                0x0B => st.push(&a[..2])?,
                0x0C => st.push(&a[..4])?,
                0x0D => {
                    let id = self.heap.new_string(u8data::text::decode(&a[2..a.len() - 1]));
                    st.push2(id)?;
                }
                0x0E => {
                    let (elem, count) = (a[0] as usize, a[1] as usize);
                    let raw = st.pop(elem * count)?;
                    // The first element was pushed first, i.e. sits deepest (highest address).
                    let data = (0..count).rev().flat_map(|k| raw[k * elem..(k + 1) * elem].to_vec()).collect();
                    let id = self.heap.new_list(elem, data);
                    st.push2(id)?;
                }
                // calls
                0x0F => {
                    let (nargs, num) = (a[0] as usize, w16(1));
                    let args = st.read(st.sp, nargs);
                    let r = intrinsics::call(self, pid, st, num, &args, ucf, host);
                    if self.trace_calls {
                        eprintln!(
                            "[{pid}] calli {num:#05x} {} {args:02x?} -> {r}",
                            intrinsics::name(num).unwrap_or("?")
                        );
                    }
                    self.me(pid).temp = r;
                    if num == 0xD0 && st.class == ANCIENT_ONES {
                        // ScummVM's fix: the Ancient Ones' "started" flag (global bit 0) is set too
                        // late, letting two eggs start the scene twice; set it with the stasis.
                        self.set_global_bits(0, 1, 1);
                    }
                    if self.me(pid).wait_for.is_some() {
                        return Ok(None); // (the intrinsic made us wait)
                    }
                }
                0x11 => st.call(w16(0), w16(2) as usize + BASE)?,
                0x12 => self.me(pid).temp = st.pop2()? as u32,
                0x13 => self.me(pid).temp = st.pop4()?,
                // 16-bit arithmetic and comparisons (x = top of stack)
                0x14 | 0x1C | 0x1E | 0x20 | 0x22 | 0x24 | 0x28 | 0x2A | 0x2C | 0x2E | 0x32 | 0x34 | 0x36 | 0x39
                | 0x3A | 0x3C | 0x3D => {
                    let x = st.pop2()? as i16 as i32;
                    let y = st.pop2()? as i16 as i32;
                    let r = match op {
                        0x14 => y + x,
                        0x1C => y - x,
                        0x1E => y * x,
                        0x20 => y.checked_div(x).unwrap_or(0),
                        0x22 => y.checked_rem(x).unwrap_or(0),
                        0x24 => (y == x) as i32,
                        0x28 => (y < x) as i32,
                        0x2A => (y <= x) as i32,
                        0x2C => (y > x) as i32,
                        0x2E => (y >= x) as i32,
                        0x32 => (y != 0 && x != 0) as i32,
                        0x34 => (y != 0 || x != 0) as i32,
                        0x36 => (y != x) as i32,
                        0x39 => y & x,
                        0x3A => y | x,
                        // Shifts take their operands the other way round in U8 (the value on top,
                        // the count under it; ScummVM), and shift right keeps the sign.
                        0x3C => x << y.clamp(0, 15),
                        _ => x >> y.clamp(0, 15),
                    };
                    st.push2(r as u16)?;
                }
                // 32-bit: arithmetic stays 32-bit, comparisons push a 16-bit boolean
                0x15 | 0x1D | 0x1F | 0x21 | 0x23 | 0x25 | 0x29 | 0x2B | 0x2D | 0x2F | 0x33 | 0x35 | 0x37 => {
                    let x = st.pop4()? as i32;
                    let y = st.pop4()? as i32;
                    let r: i32 = match op {
                        0x15 => y.wrapping_add(x),
                        0x1D => y.wrapping_sub(x),
                        0x1F => y.wrapping_mul(x),
                        0x21 => y.checked_div(x).unwrap_or(0),
                        0x23 => y.checked_rem(x).unwrap_or(0),
                        0x25 => (y == x) as i32,
                        0x29 => (y < x) as i32,
                        0x2B => (y <= x) as i32,
                        0x2D => (y > x) as i32,
                        0x2F => (y >= x) as i32,
                        0x33 => (y != 0 && x != 0) as i32,
                        0x35 => (y != 0 || x != 0) as i32,
                        _ => (y != x) as i32,
                    };
                    if matches!(op, 0x15 | 0x1D | 0x1F | 0x21 | 0x23 | 0x33 | 0x35) {
                        st.push4(r as u32)?;
                    } else {
                        st.push2(r as u16)?;
                    }
                }
                0x30 => {
                    let x = st.pop2()?;
                    st.push2((x == 0) as u16)?;
                }
                0x31 => {
                    let x = st.pop4()?;
                    st.push2((x == 0) as u16)?;
                }
                0x3B => {
                    let x = st.pop2()?;
                    st.push2(!x)?;
                }
                // strings and lists
                0x16 => {
                    let (x, y) = (st.pop2()?, st.pop2()?);
                    self.heap.concat(y, x);
                    st.push2(y)?;
                }
                0x17 | 0x19 => {
                    let (x, y) = (st.pop2()?, st.pop2()?);
                    self.heap.append(y, x, op == 0x19);
                    st.push2(y)?;
                }
                0x1A | 0x1B => {
                    let (x, y) = (st.pop2()?, st.pop2()?);
                    self.heap.remove_all(y, x, op == 0x1A);
                    st.push2(y)?;
                }
                0x26 => {
                    let (x, y) = (st.pop2()?, st.pop2()?);
                    let eq = self.heap.str_eq(x, y);
                    st.push2(eq as u16)?;
                }
                0x38 => {
                    let lid = st.pop2()?;
                    let elem = st.pop(a[0] as usize)?;
                    let found = self.heap.contains(lid, &elem, a[1] != 0);
                    st.push2(found as u16)?;
                }
                0x41 => {
                    let id = self.heap.dup_string(st.read2(st.bp_at(sb(0))));
                    st.push2(id)?;
                }
                0x42 | 0x43 => {
                    let id = self.heap.dup_list(st.read2(st.bp_at(sb(0))), op == 0x43);
                    st.push2(id)?;
                }
                0x44 => {
                    let idx = (st.pop2()? as usize).wrapping_sub(1);
                    let lid = st.pop2()?;
                    let v = self.heap.elem(lid, idx).unwrap_or_else(|| vec![0; a[0] as usize]);
                    if a[1] != 0 {
                        let id = self.heap.dup_string(le16(&v));
                        st.push2(id)?;
                    } else {
                        st.push(&v)?;
                    }
                }
                0x62 => self.heap.free_string(st.read2(st.bp_at(sb(0)))),
                0x63 | 0x64 => self.heap.free_list(st.read2(st.bp_at(sb(0))), op == 0x63),
                0x65 => self.heap.free_string(st.read2(st.sp + a[0] as usize)),
                0x66 | 0x67 => self.heap.free_list(st.read2(st.sp + a[0] as usize), op == 0x67),
                0x6C => {
                    let o = st.bp_at(sb(0));
                    let id = st.read2(o);
                    let nid = match a[1] {
                        1 => self.heap.dup_string(id),
                        2 => self.heap.dup_list(id, true),
                        _ => self.heap.dup_list(id, false),
                    };
                    st.write(o, &nid.to_le_bytes());
                }
                // locals and pointers
                0x3E => st.push2(st.read(st.bp_at(sb(0)), 1)[0] as i8 as i16 as u16)?, // (sign-extended)
                0x3F => st.push(&st.read(st.bp_at(sb(0)), 2))?,
                0x40 => st.push(&st.read(st.bp_at(sb(0)), 4))?,
                0x45 => st.push(&st.read(st.bp_at(sb(0)), a[1] as usize))?,
                0x4B => st.push4(((pid as u32) << 16) | st.bp_at(sb(0)) as u32)?,
                0x4C => {
                    let ptr = st.pop4()?;
                    let v = self.read_ptr(pid, st, ptr, a[0] as usize);
                    st.push(&v)?;
                }
                0x4D => {
                    let ptr = st.pop4()?;
                    let v = st.pop(a[0] as usize)?;
                    self.write_ptr(pid, st, ptr, &v);
                }
                0x4E => st.push2(self.global_bits(w16(0) as usize, a[2] as usize) as u16)?,
                0x4F => {
                    let v = st.pop2()? as u32;
                    self.set_global_bits(w16(0) as usize, a[2] as usize, v);
                }
                0x69 => st.push4((SEG_STRING << 16) | st.read2(st.bp_at(sb(0))) as u32)?,
                0x6B => {
                    let id = st.pop2()?;
                    st.push4((SEG_STRING << 16) | id as u32)?;
                }
                0x6E => st.drop_n(-sb(0))?,
                0x6F => st.push4(((pid as u32) << 16) | st.sp.wrapping_add_signed(-sb(0)) as u32)?,
                // control flow
                0x50 => {
                    if st.ret() {
                        return Ok(Some(self.me(pid).result));
                    }
                }
                // Relative jumps wrap round 16 bits: ORLOK's `use` (over 32K) reaches its far
                // end from the start by going backwards.
                0x51 => {
                    if st.pop2()? == 0 {
                        st.ip = (st.ip as u16).wrapping_add(w16(0)) as usize;
                    }
                }
                0x52 => st.ip = (st.ip as u16).wrapping_add(w16(0)) as usize,
                0x53 => return Ok(None),
                0x54 => {
                    // "implies": the waiter sleeps until the target ends; the target's pid is left on
                    // the stack (ScummVM).
                    let target = st.pop2()?;
                    let waiter = st.pop2()?;
                    st.push2(target)?;
                    // (Already over — spawned processes run at once — it leaves its result.)
                    let (live, ended) = match self.procs.iter().find(|p| p.pid == target) {
                        Some(p) => (!p.done, p.done.then_some(p.result)),
                        None => (false, None),
                    };
                    if let Some(p) = self.procs.iter_mut().find(|p| p.pid == waiter && waiter != target) {
                        match (live, ended) {
                            (true, _) => p.wait_for = Some(target),
                            (_, Some(r)) => p.result = r,
                            _ => {}
                        }
                    }
                }
                0x57 => {
                    let (nargs, this_size, class, off) = (a[0] as usize, a[1] as usize, w16(2), w16(4) as usize + BASE);
                    let thisptr = st.pop4()?;
                    let args = st.read(st.sp, nargs);
                    // The new process gets a copy of `this` now (it may point into our stack).
                    let this_val = self.read_ptr(pid, st, thisptr, this_size);
                    let np = self.spawn(class, off, &this_val, &args);
                    self.me(pid).temp = np as u32;
                    self.run_now(np, ucf, host);
                }
                0x58 => {
                    // Inline spawn: later code of this function runs as a new process on a copy of our frame.
                    let (class, off, delta) = (w16(0), w16(2) as usize, w16(4) as usize);
                    let mut nst = Stack { class, ip: off + delta + BASE, ..st.clone() };
                    nst.write(st.bp + 2, &0xFFFFu16.to_le_bytes());
                    nst.write(st.bp + 4, &0xFFFFu16.to_le_bytes());
                    let item = self.me(pid).item;
                    let np = self.add_proc(None, Body::Uc(nst), item, 0);
                    st.push2(np)?; // (the code goes on to make it wait: `spawn delay; implies`)
                    self.run_now(np, ucf, host);
                }
                0x59 => st.push2(pid)?,
                0x5A => st.alloc((a[0] as usize).next_multiple_of(2))?, // (kept 16-bit aligned)
                0x5B => {}                                              // source line marker
                0x5D => {
                    let t = self.me(pid).temp;
                    st.push2((t & 0xFF) as u16)?;
                }
                0x5E => {
                    let t = self.me(pid).temp;
                    st.push2(t as u16)?;
                }
                0x5F => {
                    let t = self.me(pid).temp;
                    st.push4(t)?;
                }
                0x60 => {
                    let v = st.pop2()? as i16 as i32 as u32;
                    st.push4(v)?;
                }
                0x61 => {
                    let v = st.pop4()? as u16;
                    st.push2(v)?;
                }
                0x6D => {
                    let r = self.me(pid).result;
                    st.push4(r)?;
                }
                // searches and iteration
                0x70 => {
                    let (var, ssize, stype) = (a[0] as i8 as i16, a[1] as usize, a[2]);
                    let (pa, pb) = (st.pop2()?, st.pop2()?);
                    let script = st.read(st.sp, ssize);
                    let found: Vec<u8> = search::candidates(host, stype, pa, pb)
                        .into_iter()
                        .filter(|&o| host.item(o).is_some_and(|it| search::matches(&script, &it, host)))
                        .flat_map(|o| o.to_le_bytes())
                        .collect();
                    let lid = self.heap.new_list(2, found);
                    self.loop_scripts.insert(lid, script);
                    // Replace script and parameters with the search's fixed-size frame; our state is
                    // in its first bytes: result variable, next index, list.
                    st.drop_n(ssize as isize)?;
                    st.alloc(search::frame_size(stype))?;
                    st.write(st.sp, &var.to_le_bytes());
                    st.write(st.sp + 4, &lid.to_le_bytes());
                    self.loop_next(st, host)?;
                }
                0x73 => self.loop_next(st, host)?,
                0x74 => st.push(&[a[0]])?,
                0x75 | 0x76 => {
                    let (var, elem, jump) = (sb(0), a[1] as usize, w16(2) as i16 as isize);
                    let index = match st.read2(st.sp) {
                        0xFFFF => 0,
                        i => i + 1,
                    };
                    let lid = st.read2(st.sp + 2);
                    match self.heap.elem(lid, index as usize) {
                        Some(v) => {
                            st.write(st.sp, &index.to_le_bytes());
                            st.write(st.bp_at(var), &v[..elem.min(v.len())]);
                        }
                        None => {
                            self.heap.free_list(lid, op == 0x76);
                            st.drop_n(4)?;
                            st.ip = st.ip.wrapping_add_signed(jump);
                        }
                    }
                }
                // process info
                0x77 => {
                    let (item, ty) = (st.pop2()?, st.pop2()?);
                    let me = self.me(pid);
                    (me.item, me.ptype) = (item, ty);
                }
                0x78 => {
                    // Exclusive: if another process already has this (item, type), this one
                    // bows out (ScummVM's reading: a second use mid-script is ignored).
                    let (item, ty) = {
                        let m = self.me(pid);
                        (m.item, m.ptype)
                    };
                    if self.count_procs(item, ty) > 1 {
                        return Ok(Some(0));
                    }
                }
                0x79 | 0x7A => return Ok(Some(self.me(pid).result)),
                _ => return Err(format!("unhandled opcode {op:#04x}")),
            }
        }
        Ok(None) // ran long; continue next tick
    }

    /// Step a `loop` search: set the result variable to the next live item and push 1, or push 0 when done.
    pub(super) fn loop_next(&mut self, st: &mut Stack, host: &dyn Host) -> R<()> {
        let var = st.read2(st.sp) as i16 as isize;
        let mut index = st.read2(st.sp + 2) as usize;
        let lid = st.read2(st.sp + 4);
        let list = self.heap.list(lid).map_or(vec![], |l| l.handles());
        // The original searches as it goes: what's gone, or no longer fits the search (a door
        // now mid-swing, another shape), is passed over.
        let script = self.loop_scripts.get(&lid);
        let fits = |o| host.item(o).is_some_and(|it| script.is_none_or(|s| search::matches(s, &it, host)));
        while index < list.len() && !fits(list[index]) {
            index += 1;
        }
        if index < list.len() {
            st.write(st.bp_at(var), &list[index].to_le_bytes());
            st.write(st.sp + 2, &((index + 1) as u16).to_le_bytes());
            st.push2(1)
        } else {
            // Run out: the variable is left 0 ("found nothing"), as the original has it. Scripts
            // read it after the loop: Beren's verdict after his warnings is "assault" for 0
            // (DOS), a fire spell's flames clear on time, a puzzle's sound plays at volume 0.
            // ScummVM leaves the last thing looked at there.
            st.write(st.bp_at(var), &0u16.to_le_bytes());
            self.heap.free_list(lid, false);
            self.loop_scripts.remove(&lid);
            st.write(st.sp + 4, &0u16.to_le_bytes());
            st.push2(0)
        }
    }
}
