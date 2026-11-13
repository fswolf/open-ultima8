//! U8 shapes: RLE-compressed, palette-indexed sprite frames.
use crate::{flex::Flex, i16le, u16le, Error, Result};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Decoded frame. `pixels` are palette indices; `mask` marks opaque pixels.
#[derive(Clone, Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// Hotspot: frame is drawn at (sx - xoff, sy - yoff).
    pub xoff: i32,
    pub yoff: i32,
    pub pixels: Vec<u8>,
    pub mask: Vec<bool>,
}

impl Frame {
    pub fn empty() -> Self {
        Self { width: 0, height: 0, xoff: 0, yoff: 0, pixels: vec![], mask: vec![] }
    }
    /// RGBA; with `transl`, palette indices 8–14 become semi-transparent effect pixels (glass, glow, shadow).
    pub fn to_rgba(&self, pal: &crate::palette::Palette, transl: bool) -> Vec<u8> {
        let mut out = vec![0u8; self.pixels.len() * 4];
        for (i, (&p, &m)) in self.pixels.iter().zip(&self.mask).enumerate() {
            if m {
                let c = if transl { crate::palette::xform(p) } else { None };
                out[i * 4..i * 4 + 4].copy_from_slice(&c.unwrap_or(pal.0[p as usize]));
            }
        }
        out
    }
    /// Hotspot-relative left edge: the frame spans [left, left + width) from the item's screen x.
    pub fn left(&self, flip: bool) -> i32 {
        if flip {
            self.xoff + 1 - self.width as i32
        } else {
            -self.xoff
        }
    }
    /// Is pixel (x, y) opaque?
    pub fn hit(&self, x: i32, y: i32) -> bool {
        x >= 0
            && y >= 0
            && (x as u32) < self.width
            && (y as u32) < self.height
            && self.mask[(y as u32 * self.width + x as u32) as usize]
    }
}

pub type Shape = Vec<Frame>;

/// Shape header: 4 unk, u16 nframes, then 6-byte entries (u24 offset, u8 unk, u16 len).
pub fn parse_shape(d: &[u8]) -> Result<Shape> {
    if d.len() < 6 {
        return Err(Error::Bad("shape too short".into()));
    }
    let n = u16le(d, 4) as usize;
    (0..n)
        .map(|i| {
            let e = 6 + i * 6;
            if e + 6 > d.len() {
                return Err(Error::Bad("frame table overrun".into()));
            }
            let off = d[e] as usize | (d[e + 1] as usize) << 8 | (d[e + 2] as usize) << 16;
            if off == 0 || off >= d.len() {
                return Ok(Frame::empty());
            }
            parse_frame(&d[off..])
        })
        .collect()
}

/// Frame: 8 unk, u16 compression, i16 w, h, xoff, yoff, h × u16 line offsets, RLE data.
fn parse_frame(f: &[u8]) -> Result<Frame> {
    frame_at(f, 8)
}

/// A frame whose header (compression, w, h, xoff, yoff, line offsets) starts at `b` (the films'
/// frames have only their 2-byte object type before it).
pub(crate) fn frame_at(f: &[u8], b: usize) -> Result<Frame> {
    let bad = || Error::Bad("frame overrun".into());
    if f.len() < b + 10 {
        return Err(bad());
    }
    let comp = u16le(f, b);
    let (w, h) = (i16le(f, b + 2), i16le(f, b + 4));
    let (xoff, yoff) = (i16le(f, b + 6) as i32, i16le(f, b + 8) as i32);
    if w <= 0 || h <= 0 || comp > 1 {
        return Ok(Frame { xoff, yoff, ..Frame::empty() });
    }
    let (w, h) = (w as usize, h as usize);
    let lines = b + 10;
    let rle = lines + h * 2;
    if rle > f.len() {
        return Err(bad());
    }
    let (mut pixels, mut mask) = (vec![0u8; w * h], vec![false; w * h]);
    for y in 0..h {
        let lo = (u16le(f, lines + y * 2) as usize).checked_sub((h - y) * 2).ok_or_else(bad)?;
        let mut p = rle + lo;
        let mut x = 0usize;
        let byte = |p: &mut usize| -> Result<u8> {
            let v = *f.get(*p).ok_or_else(bad)?;
            *p += 1;
            Ok(v)
        };
        loop {
            x += byte(&mut p)? as usize;
            if x >= w {
                break;
            }
            let mut len = byte(&mut p)? as usize;
            let fill = comp == 1 && len & 1 == 1;
            if comp == 1 {
                len >>= 1;
            }
            let fv = if fill { byte(&mut p)? } else { 0 };
            for _ in 0..len {
                let v = if fill { fv } else { byte(&mut p)? };
                if x < w {
                    pixels[y * w + x] = v;
                    mask[y * w + x] = true;
                }
                x += 1;
            }
            if x >= w {
                break;
            }
        }
    }
    Ok(Frame { width: w as u32, height: h as u32, xoff, yoff, pixels, mask })
}

/// Lazily-decoding cache over U8SHAPES.FLX.
pub struct ShapeArchive {
    flex: Flex,
    cache: RefCell<HashMap<u32, Rc<Shape>>>,
}

impl ShapeArchive {
    pub fn new(flex: Flex) -> Self {
        Self { flex, cache: RefCell::default() }
    }
    pub fn count(&self) -> usize {
        self.flex.len()
    }
    pub fn get(&self, shape: u32) -> Rc<Shape> {
        if let Some(s) = self.cache.borrow().get(&shape) {
            return s.clone();
        }
        let s = Rc::new(self.flex.get(shape as usize).and_then(|d| parse_shape(d).ok()).unwrap_or_default());
        self.cache.borrow_mut().insert(shape, s.clone());
        s
    }
    pub fn frame(&self, shape: u32, frame: u32) -> Option<Frame> {
        self.get(shape).get(frame as usize).filter(|f| f.width > 0).cloned()
    }
}
