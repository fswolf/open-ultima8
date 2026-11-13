//! Memory: 32-bit pointers into process stacks, objects, globals and strings; global flag bits.
use super::*;

impl Vm {
    /// Read through a pointer; `st` is the calling process's own (checked-out) stack.
    pub(crate) fn read_ptr(&self, pid: Pid, st: &Stack, ptr: u32, n: usize) -> Vec<u8> {
        if ptr >> 16 == pid as u32 {
            st.read((ptr & 0xFFFF) as usize, n)
        } else {
            self.deref(ptr, n)
        }
    }

    pub(crate) fn write_ptr(&mut self, pid: Pid, st: &mut Stack, ptr: u32, v: &[u8]) {
        if ptr >> 16 == pid as u32 {
            st.write((ptr & 0xFFFF) as usize, v)
        } else {
            self.assign(ptr, v)
        }
    }

    pub(super) fn deref(&self, ptr: u32, n: usize) -> Vec<u8> {
        let (seg, off) = (ptr >> 16, (ptr & 0xFFFF) as usize);
        let handle = |h: u16| h.to_le_bytes().into_iter().chain(std::iter::repeat(0)).take(n).collect();
        match seg {
            SEG_OBJ | SEG_STRING => handle(off as u16),
            SEG_GLOBAL => (0..n).map(|i| self.globals.get(off + i).copied().unwrap_or(0)).collect(),
            pid => match self.procs.iter().find(|p| p.pid as u32 == pid).map(|p| &p.body) {
                Some(Body::Uc(st)) => st.read(off, n),
                _ => vec![0; n],
            },
        }
    }

    pub(super) fn assign(&mut self, ptr: u32, v: &[u8]) {
        let (seg, off) = (ptr >> 16, (ptr & 0xFFFF) as usize);
        match seg {
            SEG_GLOBAL => {
                for (i, b) in v.iter().enumerate() {
                    if let Some(g) = self.globals.get_mut(off + i) {
                        *g = *b;
                    }
                }
            }
            SEG_OBJ | SEG_STRING => {}
            pid => {
                if let Some(Body::Uc(st)) = self.procs.iter_mut().find(|p| p.pid as u32 == pid).map(|p| &mut p.body) {
                    st.write(off, v);
                }
            }
        }
    }

    /// `n` bits of the global flags from bit `off` (at most 32).
    pub fn global_bits(&self, off: usize, n: usize) -> u32 {
        (0..n.min(32)).fold(0, |v, i| {
            let b = off + i;
            v | ((self.globals.get(b / 8).map_or(0, |x| (x >> (b % 8)) & 1) as u32) << i)
        })
    }

    pub fn set_global_bits(&mut self, off: usize, n: usize, v: u32) {
        for i in 0..n.min(32) {
            let b = off + i;
            if let Some(x) = self.globals.get_mut(b / 8) {
                if v >> i & 1 != 0 {
                    *x |= 1 << (b % 8);
                } else {
                    *x &= !(1 << (b % 8));
                }
            }
        }
    }
}
