//! World geometry. Units: x/y are 32 per TYPEFLAG footprint cell (a floor tile is 128), z is 8 per cell.

/// U8 facings 0–7, clockwise from north (world −y): world (dx, dy) per direction.
pub const DIRS: [(i32, i32); 8] = [(0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1)];

/// Facing (0–7) that best points along (dx, dy); 0 for no offset.
pub fn dir_towards(dx: i32, dy: i32) -> u8 {
    if dx == 0 && dy == 0 {
        return 0;
    }
    let ang = (dx as f64).atan2(-dy as f64).to_degrees();
    ((((ang + 360.0 + 22.5) / 45.0) as u32) % 8) as u8
}

/// Axis-aligned world box. U8 anchors items at their bottom *front* corner (x, y, z); the box
/// extends back to (x0, y0) and up to z1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bounds {
    pub x0: i32,
    pub y0: i32,
    pub z0: i32,
    pub x1: i32,
    pub y1: i32,
    pub z1: i32,
}

impl Bounds {
    /// Footprints overlap (open intervals: touching edges don't count).
    pub fn overlaps_xy(&self, o: &Bounds) -> bool {
        self.x0 < o.x1 && o.x0 < self.x1 && self.y0 < o.y1 && o.y0 < self.y1
    }
    /// Boxes overlap in all three axes.
    pub fn overlaps(&self, o: &Bounds) -> bool {
        self.overlaps_xy(o) && self.z0 < o.z1 && o.z0 < self.z1
    }
    /// The same box moved by (dx, dy, dz).
    pub fn shifted(&self, dx: i32, dy: i32, dz: i32) -> Bounds {
        Bounds {
            x0: self.x0 + dx,
            y0: self.y0 + dy,
            z0: self.z0 + dz,
            x1: self.x1 + dx,
            y1: self.y1 + dy,
            z1: self.z1 + dz,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directions_round_trip() {
        for (d, (dx, dy)) in DIRS.iter().enumerate() {
            assert_eq!(dir_towards(dx * 10, dy * 10) as usize, d);
        }
    }
}
