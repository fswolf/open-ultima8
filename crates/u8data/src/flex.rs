//! Flex archive: 0x80-byte header (count @0x54), then (offset,size) u32 pairs.
use crate::{u32le, Error, Result};

pub struct Flex {
    data: Vec<u8>,
    index: Vec<(usize, usize)>,
}

impl Flex {
    pub fn parse(data: Vec<u8>) -> Result<Self> {
        if data.len() < 0x80 {
            return Err(Error::Bad("flex too short".into()));
        }
        let count = u32le(&data, 0x54) as usize;
        if 0x80 + count * 8 > data.len() {
            return Err(Error::Bad("flex index overruns file".into()));
        }
        let index = (0..count)
            .map(|i| {
                let (o, s) = (u32le(&data, 0x80 + i * 8) as usize, u32le(&data, 0x84 + i * 8) as usize);
                if o == 0 || s == 0 || o + s > data.len() {
                    (0, 0)
                } else {
                    (o, s)
                }
            })
            .collect();
        Ok(Self { data, index })
    }
    pub fn len(&self) -> usize {
        self.index.len()
    }
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }
    /// Where entry `i` is in the archive's bytes (offset, length).
    pub fn span(&self, i: usize) -> Option<(usize, usize)> {
        self.index.get(i).copied().filter(|&(_, n)| n > 0)
    }
    /// The 0x80-byte header (to keep when rewriting the archive).
    pub fn header(&self) -> &[u8] {
        &self.data[..0x80]
    }
    pub fn get(&self, i: usize) -> Option<&[u8]> {
        match self.index.get(i)? {
            (_, 0) => None,
            &(o, s) => Some(&self.data[o..o + s]),
        }
    }
}

/// Write a Flex archive: `header` (the original's, else the usual 0x1A fill) with the count set,
/// the index, then the entries (None = empty slot).
pub fn build(header: Option<&[u8]>, entries: &[Option<&[u8]>]) -> Vec<u8> {
    let mut out = match header {
        Some(h) if h.len() >= 0x80 => h[..0x80].to_vec(),
        _ => {
            let mut h = vec![0x1A; 0x52];
            h.resize(0x80, 0);
            h[0x58] = 1;
            h
        }
    };
    out[0x54..0x58].copy_from_slice(&(entries.len() as u32).to_le_bytes());
    let mut off = 0x80 + entries.len() * 8;
    let mut index = Vec::with_capacity(entries.len() * 8);
    for e in entries {
        let (o, n) = match e {
            Some(d) if !d.is_empty() => (off, d.len()),
            _ => (0, 0),
        };
        index.extend_from_slice(&(o as u32).to_le_bytes());
        index.extend_from_slice(&(n as u32).to_le_bytes());
        off += n;
    }
    out.extend_from_slice(&index);
    for d in entries.iter().flatten() {
        out.extend_from_slice(d);
    }
    out
}
