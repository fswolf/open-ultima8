//! Scenery animating on its own (TYPEFLAG byte 4: type in the low nibble, data in the high;
//! byte 5's low nibble: every how many frames), as ScummVM's Item::animateItem.
use super::*;

/// How a shape animates: its TYPEFLAG type, data and speed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Cycle {
    pub(super) kind: u8,
    pub(super) data: u32,
    pub(super) speed: u32,
}

/// Scenery animation for a shape: (cycle, total frames).
pub(super) fn anim_info(gd: &GameData, shape: u32) -> Option<(Cycle, u32)> {
    let si = gd.typeflags.get(shape);
    let n = gd.shapes.get(shape).len() as u32;
    let kind = si.anim_type;
    if kind == 0 || kind > 6 || (n < 2 && kind != 5) {
        return None;
    }
    Some((Cycle { kind, data: si.anim_data as u32, speed: (si.anim_speed as u32).max(1) }, n))
}

impl World {
    /// Number of animated scenery items.
    pub fn animated(&self) -> usize {
        self.anims.len()
    }

    /// Frame `frame` (30 a second) of scenery animation, for what's on the map (as its shape is
    /// now: scripts change shapes). Returns what wants its `anim` script run (type 5: the
    /// hourglass and the like set their own frame).
    pub fn animate(&mut self, gd: &GameData, frame: u32) -> Vec<ObjId> {
        let mut scripted = vec![];
        let listed: Vec<usize> = self.anims.iter().copied().collect();
        for idx in listed {
            let e = &self.entries[idx];
            let Some((Cycle { kind, data, speed }, n)) = anim_info(gd, e.it.shape) else {
                self.anims.remove(&idx); // turned into something still (or gone)
                continue;
            };
            if !e.present() || e.parent.is_some() || !frame.is_multiple_of(speed) {
                continue;
            }
            let f = e.it.frame;
            let next = match kind {
                2 if self.random() & 1 == 1 => (self.random() >> 8) % n, // flicker anywhere
                1 | 3 if data == 0 || (data == 1 && self.random() & 1 == 1) => (f + 1) % n,
                1 | 3 if data > 1 => {
                    // Loop within blocks of `data` frames.
                    let block = f / data;
                    if (f + 1).is_multiple_of(data) {
                        block * data
                    } else {
                        f + 1
                    }
                }
                // Now and then run once through every frame.
                4 if f != 0 || (self.random() >> 8).is_multiple_of(data + 2) => (f + 1) % n,
                5 => {
                    scripted.push(self.ids[idx]);
                    continue;
                }
                // Frame 0 stays; the others loop (1..n, or within blocks of `data` past the first).
                6 if f != 0 && (data == 0 || (data == 1 && self.random() & 1 == 1)) => {
                    if f + 1 >= n {
                        1
                    } else {
                        f + 1
                    }
                }
                6 if data > 1 && !f.is_multiple_of(data) => {
                    if (f + 1).is_multiple_of(data) {
                        f / data * data + 1
                    } else {
                        f + 1
                    }
                }
                _ => continue,
            };
            if next < n && next != f {
                self.entries[idx].it.frame = next;
                self.touch(idx);
            }
        }
        scripted
    }
}
