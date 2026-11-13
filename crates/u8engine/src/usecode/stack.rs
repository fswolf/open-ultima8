//! A process's stack: 4 KB growing down, addressed by 16-bit offsets.
//! Frame layout: bp+0 saved bp, bp+2 saved ip, bp+4 saved class, bp+6 `this` pointer,
//! bp+10.. arguments; locals at negative offsets. Reads and writes outside the stack are
//! harmless (zeros / ignored); pushes and pops are checked.
pub(crate) const SIZE: usize = 0x1000;

pub(crate) type R<T> = Result<T, String>;

pub(crate) fn le16(b: &[u8]) -> u16 {
    u16::from_le_bytes([b.first().copied().unwrap_or(0), b.get(1).copied().unwrap_or(0)])
}

#[derive(Clone)]
pub(crate) struct Stack {
    pub class: u16,
    pub ip: usize,
    pub bp: usize,
    pub sp: usize,
    pub mem: Vec<u8>,
}

impl Stack {
    pub fn new() -> Self {
        Self { class: 0xFFFF, ip: 0xFFFF, bp: 0, sp: SIZE, mem: vec![0; SIZE] }
    }

    pub fn push(&mut self, v: &[u8]) -> R<()> {
        self.sp = self.sp.checked_sub(v.len()).ok_or("stack overflow")?;
        self.mem[self.sp..self.sp + v.len()].copy_from_slice(v);
        Ok(())
    }
    pub fn push2(&mut self, v: u16) -> R<()> {
        self.push(&v.to_le_bytes())
    }
    pub fn push4(&mut self, v: u32) -> R<()> {
        self.push(&v.to_le_bytes())
    }
    pub fn pop(&mut self, n: usize) -> R<Vec<u8>> {
        if self.sp + n > SIZE {
            return Err("stack underflow".into());
        }
        let v = self.mem[self.sp..self.sp + n].to_vec();
        self.sp += n;
        Ok(v)
    }
    pub fn pop2(&mut self) -> R<u16> {
        Ok(le16(&self.pop(2)?))
    }
    pub fn pop4(&mut self) -> R<u32> {
        let v = self.pop(4)?;
        Ok(u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
    }
    /// Reserve `n` zeroed bytes.
    pub fn alloc(&mut self, n: usize) -> R<()> {
        self.sp = self.sp.checked_sub(n).ok_or("stack overflow")?;
        self.mem[self.sp..self.sp + n].fill(0);
        Ok(())
    }
    /// Drop `n` bytes (negative: reserve, unzeroed).
    pub fn drop_n(&mut self, n: isize) -> R<()> {
        self.sp = self.sp.checked_add_signed(n).filter(|&s| s <= SIZE).ok_or("stack pointer out of range")?;
        Ok(())
    }

    pub fn read(&self, at: usize, n: usize) -> Vec<u8> {
        (0..n).map(|i| at.checked_add(i).and_then(|a| self.mem.get(a)).copied().unwrap_or(0)).collect()
    }
    pub fn read2(&self, at: usize) -> u16 {
        le16(&self.read(at, 2))
    }
    pub fn write(&mut self, at: usize, v: &[u8]) {
        for (i, b) in v.iter().enumerate() {
            if let Some(s) = at.checked_add(i).and_then(|a| self.mem.get_mut(a)) {
                *s = *b;
            }
        }
    }
    /// Address of a frame slot (bp + signed offset); out-of-range offsets give an address nothing maps to.
    pub fn bp_at(&self, off: isize) -> usize {
        self.bp.checked_add_signed(off).unwrap_or(usize::MAX)
    }

    pub fn call(&mut self, class: u16, ip: usize) -> R<()> {
        self.push2(self.class)?;
        self.push2(self.ip as u16)?;
        self.push2(self.bp as u16)?;
        self.class = class;
        self.ip = ip;
        self.bp = self.sp;
        Ok(())
    }
    /// Pop a frame. True when the outermost frame returned (the process ends).
    pub fn ret(&mut self) -> bool {
        self.sp = self.bp.min(SIZE);
        let bp = self.pop2().unwrap_or(0xFFFF);
        let ip = self.pop2().unwrap_or(0xFFFF);
        let class = self.pop2().unwrap_or(0xFFFF);
        self.bp = bp as usize;
        self.ip = ip as usize;
        self.class = class;
        ip == 0xFFFF && class == 0xFFFF
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflow_and_underflow_are_errors() {
        let mut s = Stack::new();
        assert!(s.pop2().is_err());
        assert!(s.alloc(SIZE).is_ok());
        assert!(s.push2(1).is_err());
        assert_eq!(s.read(usize::MAX - 1, 4), vec![0; 4]);
        s.write(usize::MAX, &[1, 2]); // ignored
    }

    #[test]
    fn call_and_return() {
        let mut s = Stack::new();
        s.call(5, 0x20).unwrap();
        assert_eq!((s.class, s.ip, s.bp), (5, 0x20, SIZE - 6));
        assert!(s.ret());
    }
}
