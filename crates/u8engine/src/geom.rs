//! World geometry. Units: x/y are 32 per TYPEFLAG footprint cell (a floor tile is 128), z is 8 per cell.

/// U8 facings 0–7, clockwise from north (world −y): world (dx, dy) per direction.
pub const DIRS: [(i32, i32); 8] = [(0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1)];

/// Facing (0–7) along (dx, dy), as the original works it out (DIST.C): a diagonal while one
/// offset is within half to twice the other, else the larger axis; no offset gives 1.
pub fn dir_towards(dx: i32, dy: i32) -> u8 {
    let (ax, ay) = (dx.abs(), dy.abs());
    if ax / 2 <= ay && ay <= ax * 2 {
        match (dx.signum(), dy.signum()) {
            (-1, -1) => 7,
            (1, 1) => 3,
            (-1, 1) => 5,
            _ => 1,
        }
    } else if ax * 2 <= ay {
        if dy <= 0 {
            0
        } else {
            4
        }
    } else if dx <= 0 {
        6
    } else {
        2
    }
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

    /// The original's edges: a diagonal from half to twice the other offset (Mordea, waking, turns
    /// to the Avatar 28 west and 12 north of her bed: west, not north-west).
    #[test]
    fn diagonals_end_where_the_originals_do() {
        assert_eq!(dir_towards(-28, -12), 6);
        assert_eq!(dir_towards(-28, -14), 7);
        assert_eq!(dir_towards(10, 21), 4);
        assert_eq!(dir_towards(10, 20), 3);
        assert_eq!(dir_towards(0, 0), 1);
    }
}
