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
                c[k] = six_bit(d[4 + i * 3 + k]);
            }
        }
        Ok(Self(p))
    }
}

/// A VGA 6-bit colour channel to 8 bits (63 → 255).
fn six_bit(v: u8) -> u8 {
    let v = v & 63;
    (v << 2) | (v >> 4)
}

/// The colours translucent shapes blend with: palette indices 8–14 in a TRANSL shape mix this colour
/// over what's behind instead of drawing their own (glass, glows, shadows).
/// Also how the unseen Avatar is drawn: `unseen` maps each colour behind him to the one shown
/// (XFORMPAL.DAT entry 0's first table; the original's SkipTransformDraw, SDXF.ASM).
#[derive(Clone)]
pub struct XForm(pub [[u8; 4]; 7], pub [u8; 256]);

impl XForm {
    /// XFORMPAL.DAT entry 1: 6-bit (r, g, b, alpha out of 63) for indices 8–15. The original draws
    /// through lookup tables made from these (entry 0: each background index to the nearest
    /// palette colour of the blend; they match a blend at alpha/63).
    pub fn parse(f: &crate::flex::Flex) -> Option<Self> {
        let d = f.get(1).filter(|d| d.len() >= 28)?;
        let mut unseen = Self::default().1;
        if let Some(t) = f.get(0).filter(|t| t.len() >= 256) {
            unseen.copy_from_slice(&t[..256]);
        }
        let mut x = [[0u8; 4]; 7];
        for (i, c) in x.iter_mut().enumerate() {
            let r = &d[i * 4..i * 4 + 4];
            *c = [six_bit(r[0]), six_bit(r[1]), six_bit(r[2]), ((r[3].min(63) as u32 * 255 + 31) / 63) as u8];
        }
        Some(Self(x, unseen))
    }

    /// The colour shown for `rgb` behind the unseen Avatar: its nearest palette colour, recoloured
    /// by the `unseen` table.
    pub fn unseen(&self, pal: &Palette, rgb: [u8; 3]) -> [u8; 3] {
        let d = |c: &[u8; 4]| (0..3).map(|k| (c[k] as i32 - rgb[k] as i32).pow(2)).sum::<i32>();
        let near = (0..256).min_by_key(|&i| d(&pal.0[i])).unwrap_or(0);
        let c = pal.0[self.1[near] as usize];
        [c[0], c[1], c[2]]
    }

    pub fn get(&self, index: u8) -> Option<[u8; 4]> {
        (8..15).contains(&index).then(|| self.0[index as usize - 8])
    }
}

impl Default for XForm {
    /// The file's values, for when it's missing.
    fn default() -> Self {
        Self(
            [
                [109, 109, 109, 101],
                [36, 36, 36, 142],
                [190, 195, 105, 93],
                [255, 255, 255, 81],
                [255, 81, 81, 146],
                [60, 60, 255, 215],
                [0, 0, 121, 134],
            ],
            std::array::from_fn(|i| i as u8),
        )
    }
}
