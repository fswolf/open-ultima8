//! U8PAL.PAL: 4-byte header + 256 × 6-bit RGB.
use crate::{Error, Result};

#[derive(Clone)]
pub struct Palette(pub [[u8; 4]; 256]);

impl Palette {
    pub fn parse(d: &[u8]) -> Result<Self> {
        if d.len() < 4 + 768 {
            return Err(Error::Bad("palette too short".into()));
        }
        let mut p = [[0u8, 0, 0, 255]; 256];
        for (i, c) in p.iter_mut().enumerate() {
            for k in 0..3 {
                let v = d[4 + i * 3 + k] & 63;
                c[k] = (v << 2) | (v >> 4);
            }
        }
        Ok(Self(p))
    }
}

/// U8 "xform" palette: in translucent shapes, indices 8–14 blend over the background
/// instead of drawing their palette colour. Values follow Pentagram/ScummVM (RGB + alpha).
pub fn xform(index: u8) -> Option<[u8; 4]> {
    Some(match index {
        8 => [48, 48, 48, 80],
        9 => [24, 24, 24, 80],
        10 => [64, 64, 24, 64],
        11 => [80, 80, 80, 80],
        12 => [180, 90, 0, 80],
        13 => [0, 0, 0, 160],
        14 => [0, 0, 0, 80],
        _ => return None,
    })
}
