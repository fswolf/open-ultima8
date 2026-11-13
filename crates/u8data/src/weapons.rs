//! The weapon table compiled into U8.EXE (module WPNCACHE.C). It follows the list of overlay
//! shapes (one per WPNOVLAY.DAT hold type: mace, sword, axe, dagger, then the earth, air and fire
//! glows of spells): 10-byte records of shape (u16), hold type, damage modifier, base damage,
//! attack and defence dexterity bonuses, armour bonus, damage type flags and a spare byte.
//! Found by its signature, so other builds and languages of the EXE should read too.
use crate::u16le;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Weapon {
    pub shape: u32,
    /// WPNOVLAY.DAT hold type (index into [`Weapons::overlay_shapes`]).
    pub hold: usize,
    /// Damage is `base + 0..=modifier` (plus a fifth of strength).
    pub modifier: u8,
    pub base: u8,
    pub dex_attack: u8,
    pub dex_defend: u8,
    pub armour: u8,
    pub damage_type: u8,
}

#[derive(Clone, Debug, Default)]
pub struct Weapons {
    /// Shapes drawn over the Avatar for each hold type.
    pub overlay_shapes: Vec<u32>,
    pub list: Vec<Weapon>,
}

/// The overlay shapes the table starts with: MACE 241, SWORD 243, AXE 244, DAGGER 245.
const SIGNATURE: [u8; 8] = [0xF1, 0, 0xF3, 0, 0xF4, 0, 0xF5, 0];
const RECORD: usize = 10;

impl Weapons {
    /// Find and read the table in the bytes of U8.EXE (None if it isn't there).
    pub fn parse(exe: &[u8]) -> Option<Self> {
        let at = exe.windows(SIGNATURE.len()).position(|w| w == SIGNATURE)?;
        // Overlay shapes (u16) up to the zero padding, then the records after it.
        let mut p = at;
        let mut overlay_shapes = vec![];
        while p + 2 <= exe.len() && u16le(exe, p) != 0 {
            overlay_shapes.push(u16le(exe, p) as u32);
            p += 2;
        }
        while p + 2 <= exe.len() && u16le(exe, p) == 0 && p < at + 64 {
            p += 2;
        }
        let mut list = vec![];
        while let Some(r) = exe.get(p..p + RECORD) {
            let shape = u16le(r, 0) as u32;
            if shape == 0 || shape >= 2048 || r[2] as usize >= overlay_shapes.len() || r[9] != 0 {
                break;
            }
            list.push(Weapon {
                shape,
                hold: r[2] as usize,
                modifier: r[3],
                base: r[4],
                dex_attack: r[5],
                dex_defend: r[6],
                armour: r[7],
                damage_type: r[8],
            });
            p += RECORD;
        }
        (!list.is_empty()).then_some(Self { overlay_shapes, list })
    }

    pub fn get(&self, shape: u32) -> Option<&Weapon> {
        self.list.iter().find(|w| w.shape == shape)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_records_after_the_overlay_shapes() {
        let mut exe = vec![0xAA; 5];
        exe.extend(SIGNATURE);
        exe.extend([0x8E, 1, 0, 0, 0, 0]);
        exe.extend([0xA2, 1, 3, 7, 3, 0, 0, 0, 2, 0]); // a dagger
        exe.extend(b"wpnovlay.dat\0");
        let w = Weapons::parse(&exe).unwrap();
        assert_eq!(w.overlay_shapes, [241, 243, 244, 245, 398]);
        assert_eq!(w.list.len(), 1);
        let d = w.get(418).unwrap();
        assert_eq!((d.hold, d.modifier, d.base, d.damage_type), (3, 7, 3, 2));
    }
}
