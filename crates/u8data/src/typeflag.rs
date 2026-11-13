//! TYPEFLAG.DAT: 8 bytes per shape — flags, family and footprint (in 32-unit cells).
//! Layout (fitted against known shapes: floor 4×4×0, Avatar 2×2×5, pillar 1×1×5):
//! b0 flags · b1 flags lo | family hi · b2 equip lo | x hi · b3 y lo | z hi · b4 anim type hi · b5 anim data lo, editor 0x10 · b6 weight · b7 volume
#[derive(Clone, Copy, Debug, Default)]
pub struct ShapeInfo {
    pub flags: u16,
    pub family: u8,
    pub x: u8,
    pub y: u8,
    pub z: u8,
    pub weight: u8,
    pub volume: u8,
    /// Where it's worn or held (byte 2, low nibble): 1 shield, 2 arms, 3 head, 4 body, 5 legs,
    /// 6 weapon; 0 for everything else.
    pub equip: u8,
    /// Scenery animation: type (0/1 = none) and data (frames per cycle).
    pub anim_type: u8,
    pub anim_data: u8,
    /// Animates every this many frames (30 a second).
    pub anim_speed: u8,
}

pub mod flag {
    pub const FIXED: u16 = 0x0001;
    pub const SOLID: u16 = 0x0002;
    pub const SEA: u16 = 0x0004;
    pub const LAND: u16 = 0x0008;
    pub const OCCL: u16 = 0x0010;
    pub const BAG: u16 = 0x0020;
    pub const DAMAGING: u16 = 0x0040;
    pub const NOISY: u16 = 0x0080;
    pub const DRAW: u16 = 0x0100;
    pub const IGNORE: u16 = 0x0200;
    pub const ROOF: u16 = 0x0400;
    pub const TRANSL: u16 = 0x0800;
    /// Editor-only marker (byte 5 bit 4).
    pub const EDITOR: u16 = 0x1000;
}

pub mod family {
    pub const GENERIC: u8 = 0;
    /// Has a quality that means something (keys, potions).
    pub const QUALITY: u8 = 1;
    /// Counted by quality (coins).
    pub const QUANTITY: u8 = 2;
    pub const GLOBEGG: u8 = 3;
    pub const UNKEGG: u8 = 4;
    /// Breaks when struck (pots, crates).
    pub const BREAKABLE: u8 = 5;
    pub const CONTAINER: u8 = 6;
    pub const MONSTEREGG: u8 = 7;
    pub const TELEPORTEGG: u8 = 8;
    /// Also counted by quality.
    pub const REAGENT: u8 = 9;
    pub fn is_egg(f: u8) -> bool {
        matches!(f, GLOBEGG | UNKEGG | MONSTEREGG | TELEPORTEGG)
    }
}

impl ShapeInfo {
    pub fn has(&self, f: u16) -> bool {
        self.flags & f != 0
    }
    /// Footprint in world units.
    pub fn dims(&self) -> (i32, i32, i32) {
        (self.x as i32 * 32, self.y as i32 * 32, self.z as i32 * 8)
    }

    /// The footprint in the world of an item of this shape: a mirrored (FLIPPED) one lies the
    /// other way, x and y swapped (ScummVM's getFootpadWorld).
    pub fn footpad(&self, flipped: bool) -> (i32, i32, i32) {
        let (x, y, z) = self.dims();
        if flipped {
            (y, x, z)
        } else {
            (x, y, z)
        }
    }
}

pub struct TypeFlags(pub Vec<ShapeInfo>, Vec<[u8; 8]>);

impl TypeFlags {
    pub fn parse(d: &[u8]) -> Self {
        Self(
            d.as_chunks::<8>()
                .0
                .iter()
                .map(|b| ShapeInfo {
                    flags: b[0] as u16 | ((b[1] & 0x0F) as u16) << 8 | if b[5] & 0x10 != 0 { flag::EDITOR } else { 0 },
                    family: b[1] >> 4,
                    x: b[2] >> 4,
                    y: b[3] & 0x0F,
                    z: b[3] >> 4,
                    anim_type: b[4] & 0x0F,
                    anim_data: b[4] >> 4,
                    anim_speed: b[5] & 0x0F,
                    weight: b[6],
                    volume: b[7],
                    equip: b[2] & 0x0F,
                })
                .collect(),
            d.as_chunks::<8>().0.to_vec(),
        )
    }
    /// The raw 8 bytes (usecode reads individual bits).
    pub fn raw(&self, shape: u32) -> [u8; 8] {
        self.1.get(shape as usize).copied().unwrap_or_default()
    }
    pub fn get(&self, shape: u32) -> ShapeInfo {
        self.0.get(shape as usize).copied().unwrap_or_default()
    }
}
