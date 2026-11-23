//! SOUND.FLX (effects) and E<shape>.FLX (speech): Sonarc-compressed 8-bit mono samples.
//!
//! Sample: u32 sample count, u16 rate, then frames from 0x20. Frame: u16 byte size, u16 sample
//! count, u16 (checksum filler: all words XOR to 0xACED), u8 mode + 8, u8 LPC order, `order`
//! i16 LPC factors (4.12 fixed point), then the entropy-coded residuals. Ported from
//! Pentagram/ScummVM's SonarcAudioSample.
use crate::flex::Flex;

/// Decoded audio: unsigned 8-bit PCM (0x80 = silence).
#[derive(Clone, Debug, Default)]
pub struct Sample {
    pub rate: u32,
    pub pcm: Vec<u8>,
}

impl Sample {
    /// Samples as f32 in [-1, 1).
    pub fn to_f32(&self) -> Vec<f32> {
        self.pcm.iter().map(|&s| (s as f32 - 128.0) / 128.0).collect()
    }
    pub fn seconds(&self) -> f32 {
        self.pcm.len() as f32 / self.rate.max(1) as f32
    }
}

/// (sample count, rate) from a sample's header, without decoding it.
pub fn length(d: &[u8]) -> Option<(u32, u32)> {
    let h = d.get(..6)?;
    Some((u32::from_le_bytes([h[0], h[1], h[2], h[3]]), u16::from_le_bytes([h[4], h[5]]) as u32)).filter(|l| l.1 > 0)
}

/// Decode one Sonarc sample. None if it isn't one (or is damaged beyond use).
pub fn decode(d: &[u8]) -> Option<Sample> {
    if d.len() < 0x20 {
        return None;
    }
    let len = u32::from_le_bytes([d[0], d[1], d[2], d[3]]) as usize;
    let rate = u16::from_le_bytes([d[4], d[5]]) as u32;
    if rate == 0 || len > 0x100_0000 {
        return None;
    }
    let mut pcm = Vec::with_capacity(len);
    let mut pos = 0x20;
    while pos + 8 <= d.len() && pcm.len() < len {
        let size = u16::from_le_bytes([d[pos], d[pos + 1]]) as usize;
        if size < 8 || pos + size > d.len() {
            break;
        }
        let frame = decode_frame(&d[pos..pos + size])?;
        pcm.extend(frame);
        pos += size;
    }
    pcm.truncate(len);
    (!pcm.is_empty()).then_some(Sample { rate, pcm })
}

fn decode_frame(f: &[u8]) -> Option<Vec<u8>> {
    let size = f.len();
    let checksum = f.as_chunks::<2>().0.iter().fold(0u16, |c, w| c ^ u16::from_le_bytes([w[0], w[1]]));
    if checksum != 0xACED {
        return None;
    }
    let count = u16::from_le_bytes([f[2], f[3]]) as usize;
    let mode = (f[6] as i32 - 8).clamp(0, 13);
    let order = f[7] as usize;
    let coded = f.get(8 + 2 * order..size)?;
    let mut out = residuals(mode, count, coded);
    predict(order, &mut out, &f[8..8 + 2 * order]);
    // Repair samples that wrapped past full scale.
    for i in 1..out.len() {
        if out[i] == 0 && out[i - 1] > 192 {
            out[i] = 0xFF;
        }
    }
    Some(out)
}

/// Variable-length residuals: per sample, a run of 1 bits picks how many bits follow.
fn residuals(mode: i32, count: usize, src: &[u8]) -> Vec<u8> {
    let (mode, zero_special) = if mode >= 7 { (mode - 7, true) } else { (mode, false) };
    let mut out = Vec::with_capacity(count);
    let (mut data, mut bits, mut src) = (0u32, 0i32, src.iter());
    for _ in 0..count {
        while bits <= 24 {
            let Some(&b) = src.next() else { break };
            data |= (b as u32) << bits.max(0);
            bits += 8;
        }
        if zero_special {
            let bit = data & 1;
            data >>= 1;
            bits -= 1;
            if bit == 0 {
                out.push(0x80);
                continue;
            }
        }
        let ones = (data as u8).trailing_ones() as i32;
        let s: i8 = if ones == 0 {
            data >>= 1;
            let s = (((data & 0xFF) << (7 - mode)) as u8 as i8) >> (7 - mode);
            data >>= mode + 1;
            bits -= mode + 2;
            s
        } else if ones < 7 - mode {
            data >>= ones + 1;
            let sh = 7 - mode - ones;
            let mut s = (((data & 0xFF) << sh) as u8 & 0x7F) as i8;
            if s & 0x40 == 0 {
                s = (s as u8 | 0x80) as i8;
            }
            data >>= mode + ones;
            bits -= mode + 2 * ones + 1;
            s >> sh
        } else {
            data >>= 7 - mode;
            let mut s = (data as u8 & 0x7F) as i8;
            if s & 0x40 == 0 {
                s = (s as u8 | 0x80) as i8;
            }
            data >>= 7;
            bits -= 14 - mode;
            s
        };
        out.push((s as u8).wrapping_add(0x80));
    }
    out
}

/// Linear prediction: each sample is corrected by the weighted previous `order` samples.
fn predict(order: usize, out: &mut [u8], factors: &[u8]) {
    let k: Vec<i32> = factors.as_chunks::<2>().0.iter().map(|w| i16::from_le_bytes([w[0], w[1]]) as i32).collect();
    for i in 0..out.len() {
        let mut acc = 0i32;
        for (j, &f) in k.iter().enumerate().take(order) {
            // Factor j weighs the sample j + 1 back. Before the frame starts the history reads as
            // -128 (the original decoder's quirk; the encoder relied on it — anything else clicks).
            let prev = i.checked_sub(j + 1).map_or(-128, |p| (out[p] ^ 0x80) as i8 as i32);
            acc += prev * f;
        }
        out[i] = out[i].wrapping_sub(((acc + 0x800) >> 12) as u8);
    }
}

/// Effect names from SOUND.FLX entry 0 (8 characters each), indexed by effect number (the table
/// starts at effect 1).
pub fn names(f: &Flex) -> Vec<String> {
    let table = f.get(0).unwrap_or(&[]).chunks(8);
    std::iter::once(String::new())
        .chain(table.map(|c| String::from_utf8_lossy(c).trim_end_matches(['\0', ' ']).to_string()))
        .collect()
}

/// Speech file text (entry 0): one phrase per audio entry, entry n + 1 speaks phrase n.
pub fn phrases(f: &Flex) -> Vec<String> {
    f.get(0).map_or(vec![], |d| {
        d.split(|&b| b == 0).map(|p| crate::text::decode(p).trim().to_string()).filter(|p| !p.is_empty()).collect()
    })
}
