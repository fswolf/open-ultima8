//! Argument access and output helpers shared by the commands.
use anyhow::{anyhow, ensure, Context, Result};
use std::path::Path;
use u8render::{upscale, Upscaler};

/// println! that ignores a closed stdout (e.g. piped into `head`).
macro_rules! say {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), $($t)*);
    }};
}

/// Positional arguments: 0 = data dir, 1 = command, then the command's own.
pub struct Args(Vec<String>);

impl Args {
    pub fn parse(mut a: Vec<String>) -> Self {
        // `map <n> all ...` is shorthand for `map <n> c c all all ...`.
        if a.get(1).is_some_and(|c| c == "map") && a.get(3).is_some_and(|x| x == "all") {
            a.splice(3..4, ["c", "c", "all", "all"].map(String::from));
        }
        Self(a)
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn has(&self, s: &str) -> bool {
        self.0.iter().any(|a| a == s)
    }
    pub fn data(&self) -> &Path {
        Path::new(&self.0[0])
    }
    pub fn get(&self, i: usize, default: &str) -> String {
        self.0.get(i).cloned().unwrap_or_else(|| default.to_string())
    }
    /// Argument `i` as a number (default if absent).
    pub fn num<T: std::str::FromStr>(&self, i: usize, default: &str, what: &str) -> Result<T> {
        num(&self.get(i, default), what)
    }
    /// Arguments from `i` on (optional flags and values).
    pub fn rest(&self, i: usize) -> &[String] {
        self.0.get(i..).unwrap_or(&[])
    }
    /// Value of a `key=value` flag among the arguments from `i` on.
    pub fn flag(&self, i: usize, key: &str) -> Option<&str> {
        self.rest(i).iter().find_map(|f| f.strip_prefix(key)?.strip_prefix('='))
    }
    /// The cutaway height from a `cut=Z` flag.
    pub fn cut(&self, i: usize) -> Result<Option<i32>> {
        self.flag(i, "cut").map(|z| num(z, "cut")).transpose()
    }
}

pub fn num<T: std::str::FromStr>(s: &str, what: &str) -> Result<T> {
    s.parse().map_err(|_| anyhow!("{what} must be a number, got {s:?}"))
}

/// Parse a scale: `N` = xBRZ N×, `nN` = nearest N×.
pub fn upscaler(s: &str) -> Result<(u32, Upscaler)> {
    let (n, up): (&str, Upscaler) = match s.strip_prefix('n') {
        Some(n) => (n, Box::new(upscale::nearest)),
        None => (s, Box::new(upscale::xbrz)),
    };
    let n: u32 = num(n, "scale")?;
    ensure!((1..=16).contains(&n), "scale must be 1–16, got {n}");
    Ok((n, up))
}

pub fn save(path: &Path, w: u32, h: u32, rgba: Vec<u8>) -> Result<()> {
    image::RgbaImage::from_raw(w, h, rgba).context("bad buffer")?.save(path)?;
    say!("wrote {} ({w}x{h})", path.display());
    Ok(())
}

/// Copy the opaque pixels of a `w`×`h` RGBA image into a sheet at (ox, oy), clipped to `max` × `max`.
pub fn paste(sheet: &mut [u8], sheet_w: u32, (src, w, h): (&[u8], u32, u32), (ox, oy): (u32, u32), max: u32) {
    for y in 0..h.min(max) {
        for x in 0..w.min(max) {
            let si = ((y * w + x) * 4) as usize;
            if src[si + 3] != 0 {
                let di = (((oy + y) * sheet_w + ox + x) * 4) as usize;
                sheet[di..di + 4].copy_from_slice(&src[si..si + 4]);
            }
        }
    }
}
