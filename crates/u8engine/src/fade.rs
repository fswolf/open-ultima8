//! Palette fades (ScummVM's PaletteFaderProcess and PaletteManager): the game palette goes
//! through a colour matrix — three rows of (red, green, blue, constant), 0x800 = 1 — that scripts
//! fade between: to black and back for scene changes, to a tint for storms, a white flash for
//! lightning. Hosts draw with [`Fade::matrix`].

/// Untouched colours.
pub const IDENTITY: [i16; 12] = [0x800, 0, 0, 0, 0, 0x800, 0, 0, 0, 0, 0x800, 0];

/// Palette transforms scripts name (fadeToPaletteTransform).
pub fn transform(n: u16) -> [i16; 12] {
    // Grey as the original weighs it: 3/8 red, 1/2 green, 1/8 blue.
    const GREY: [i16; 3] = [0x300, 0x400, 0x100];
    let mut m = [0i16; 12];
    match n {
        1 => (0..3).for_each(|i| m[i * 4..i * 4 + 3].copy_from_slice(&GREY)),
        2 => m = [0, 0, 0, 0, 0, 0x800, 0, 0, 0, 0, 0x800, 0], // no red
        3 => {
            // Rain storm: (colour + grey) / 4 + 0.1875.
            for i in 0..3 {
                (0..3).for_each(|k| m[i * 4 + k] = GREY[k] / 4);
                m[i * 4 + i] += 0x200;
                m[i * 4 + 3] = 0x180;
            }
        }
        4 => {
            // Fire storm: red (r + grey) / 2 + 0.1875, green g / 2 + grey / 4, blue b / 2.
            m[..4].copy_from_slice(&[GREY[0] / 2 + 0x400, GREY[1] / 2, GREY[2] / 2, 0x180]);
            m[4..8].copy_from_slice(&[GREY[0] / 4, GREY[1] / 4 + 0x400, GREY[2] / 4, 0]);
            m[8..].copy_from_slice(&[0, 0, 0x400, 0]);
        }
        5 => {
            // Saturate: 2 × colour − grey.
            for i in 0..3 {
                (0..3).for_each(|k| m[i * 4 + k] = -GREY[k]);
                m[i * 4 + i] += 0x1000;
            }
        }
        6 => m = [0, 0, 0x800, 0, 0x800, 0, 0, 0, 0, 0x800, 0, 0], // red ← blue, green ← red, blue ← green
        7 => m = [0, 0x800, 0, 0, 0, 0, 0x800, 0, 0x800, 0, 0, 0], // red ← green, green ← blue, blue ← red
        _ => m = IDENTITY,
    }
    m
}

/// Every colour becomes (r, g, b) mixed with `keep` of the original (0–255 each).
pub fn solid(r: u8, g: u8, b: u8, keep: u8) -> [i16; 12] {
    let k = |v: u8| (v as i32 * 0x800 / 255) as i16;
    [k(keep), 0, 0, k(r), 0, k(keep), 0, k(g), 0, 0, k(keep), k(b)]
}

/// Ticks of a fade to or from black or white, and of a transform.
pub const BLACK_TICKS: u32 = 30;
pub const TRANSFORM_TICKS: u32 = 45;
pub const LIGHTNING_TICKS: u32 = 16; // (63 steps, 4 a tick)
/// Fades to and from black and white outrank transforms; lightning gives way to anything.
pub const PRIORITY_FADE: i32 = 0x7FFF;
pub const PRIORITY_LIGHTNING: i32 = -1;

#[derive(Clone, Debug)]
pub struct Fade {
    /// What the palette shows now.
    matrix: [i16; 12],
    /// The transform it settles on (after a fade from black, back to this).
    pub transform: u16,
    running: Option<Running>,
}

/// A fade under way.
#[derive(Clone, Copy, Debug)]
struct Running {
    from: [i16; 12],
    to: [i16; 12],
    left: u32,
    all: u32,
    priority: i32,
}

impl Default for Fade {
    fn default() -> Self {
        Self { matrix: IDENTITY, transform: 0, running: None }
    }
}

impl Fade {
    /// The colour matrix to draw with.
    pub fn matrix(&self) -> [i16; 12] {
        self.matrix
    }

    /// As floats for a shader: rows of (r, g, b, constant), colours 0–1.
    pub fn rows(&self) -> [f32; 12] {
        self.matrix.map(|v| v as f32 / 2048.0)
    }

    /// Start fading from `from` to `to` over `ticks`, unless a fade of higher priority runs.
    /// False if refused.
    pub fn start(&mut self, from: [i16; 12], to: [i16; 12], ticks: u32, priority: i32) -> bool {
        if self.running.is_some_and(|r| r.priority > priority) {
            return false;
        }
        self.running = Some(Running { from, to, left: ticks, all: ticks.max(1), priority });
        true
    }

    /// The matrix of the settled transform.
    pub fn settled(&self) -> [i16; 12] {
        transform(self.transform)
    }

    /// One tick of the fade under way.
    pub fn tick(&mut self) {
        let Some(r) = &mut self.running else { return };
        let (l, a) = (r.left as i32, r.all as i32);
        let (from, to) = (r.from, r.to);
        self.matrix = std::array::from_fn(|i| ((from[i] as i32 * l + to[i] as i32 * (a - l)) / a) as i16);
        if r.left == 0 {
            self.running = None;
        } else {
            r.left -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fades_to_black_and_back() {
        let mut f = Fade::default();
        assert!(f.start(f.matrix(), solid(0, 0, 0, 0), 2, PRIORITY_FADE));
        (0..3).for_each(|_| f.tick());
        assert_eq!(f.matrix(), [0; 12]);
        f.start(solid(0, 0, 0, 0), f.settled(), 2, PRIORITY_FADE);
        assert!(!f.start(IDENTITY, IDENTITY, 2, PRIORITY_LIGHTNING), "lightning waits for a fade");
        (0..3).for_each(|_| f.tick());
        assert_eq!(f.matrix(), IDENTITY);
    }
}

/// The screen turning upside down (ScummVM's InverterProcess and InverterGump): 0 is upright,
/// 0x8000 upside down, and it always turns the same way round (on through 0x10000, which is 0
/// again), a step every other tick. Hosts draw with [`Inverter::row`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Inverter {
    pub state: u16,
    target: u16,
    clock: u32,
}

const INVERT_STEPS: [u32; 61] = [
    0, 8, 63, 211, 493, 945, 1594, 2459, 3552, 4870, 6406, 8139, 10042, 12078, 14207, 16384, 18561, 20690, 22726,
    24629, 26362, 27898, 29216, 30308, 31174, 31823, 32274, 32556, 32704, 32760, 32768, 32775, 32831, 32979, 33261,
    33713, 34362, 35227, 36320, 37638, 39174, 40907, 42810, 44846, 46975, 49152, 51328, 53457, 55494, 57396, 59129,
    60665, 61984, 63076, 63942, 64591, 65042, 65324, 65472, 65528, 65536,
];

impl Inverter {
    /// invertScreen: head for the other way up (or back, if it's on its way).
    pub fn toggle(&mut self, turning: bool) {
        self.target = match turning {
            true if self.target == 0 => 0x8000,
            true => 0,
            false if self.state != 0 => 0,
            false => 0x8000,
        };
    }
    /// Still turning?
    pub fn turning(&self) -> bool {
        self.state != self.target
    }
    /// One tick; true when it has just come round to where it was heading.
    pub fn tick(&mut self) -> bool {
        if !self.turning() {
            return false;
        }
        self.clock += 1;
        if self.clock % 2 == 1 {
            return false;
        }
        let next = INVERT_STEPS.iter().copied().find(|&s| s > self.state as u32).unwrap_or(65536);
        self.state = next as u16; // (0x10000 is upright again)
        !self.turning()
    }

    /// Which row of an `h`-row picture shows on screen row `i` (InverterGump's interleave).
    pub fn row(state: u16, i: i32, h: i32) -> i32 {
        let n = (h / 2).max(1);
        match state {
            0 => i,
            0x8000 => h - 1 - i,
            _ => {
                let t = (state as i64 * h as i64 / 0x10000) as i32;
                let index = if i % 2 == 0 { i / 2 } else { 2 * n - 1 - i / 2 };
                let j = (index + t) % (2 * n);
                if j >= n {
                    2 * n - 1 - 2 * (j - n)
                } else {
                    2 * j
                }
            }
        }
    }
}
