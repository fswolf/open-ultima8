//! Pixel-art upscalers over RGBA buffers: `(rgba, w, h, factor) → rgba of (w·f) × (h·f)`.

pub type Upscaler = Box<dyn Fn(&[u8], u32, u32, u32) -> Vec<u8>>;

pub fn nearest(src: &[u8], w: u32, h: u32, f: u32) -> Vec<u8> {
    let (w2, h2) = (w * f, h * f);
    let mut out = vec![0u8; (w2 * h2 * 4) as usize];
    for y in 0..h2 {
        for x in 0..w2 {
            let s = (((y / f) * w + x / f) * 4) as usize;
            let d = ((y * w2 + x) * 4) as usize;
            out[d..d + 4].copy_from_slice(&src[s..s + 4]);
        }
    }
    out
}

/// xBRZ (native 2–6×; larger factors finish with nearest). Alpha is kept, so edges come out anti-aliased.
pub fn xbrz(src: &[u8], w: u32, h: u32, f: u32) -> Vec<u8> {
    let k = (2..=6).rev().find(|&k| f.is_multiple_of(k)).unwrap_or(1);
    let mut out = if k > 1 { xbrz::scale_rgba(src, w as usize, h as usize, k as usize) } else { src.to_vec() };
    if f / k > 1 {
        out = nearest(&out, w * k, h * k, f / k);
    }
    out
}

/// [`xbrz`] split into horizontal bands across CPU cores (bands overlap so seams match exactly).
pub fn xbrz_par(src: &[u8], w: u32, h: u32, f: u32) -> Vec<u8> {
    const PAD: u32 = 4;
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(16) as u32;
    let band = h.div_ceil(threads).max(32);
    if threads < 2 || h <= band {
        return xbrz(src, w, h, f);
    }
    let row = (w * f * 4) as usize;
    let mut out = vec![0u8; row * (h * f) as usize];
    std::thread::scope(|sc| {
        for (i, dst) in out.chunks_mut(row * (band * f) as usize).enumerate() {
            let y0 = i as u32 * band;
            let y1 = (y0 + band).min(h);
            let (a, b) = (y0.saturating_sub(PAD), (y1 + PAD).min(h));
            let strip = &src[(a * w * 4) as usize..(b * w * 4) as usize];
            sc.spawn(move || {
                let up = xbrz(strip, w, b - a, f);
                let skip = ((y0 - a) * f) as usize * row;
                dst.copy_from_slice(&up[skip..skip + dst.len()]);
            });
        }
    });
    out
}
