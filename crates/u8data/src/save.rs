//! Saved games. SAVEGAME/U8SAVE.nnn bundles a GAMEDAT folder: "Ultima 8 SaveGame File.\0",
//! u16 file count, then per file u32 name length, name, u32 size, data. SAVEGAME/SGHEADER.DAT
//! holds the descriptions: 104 bytes per slot, the text first. U8SAVE.000 is a new game.
use crate::{find_file, Error, Result};
use std::collections::HashMap;
use std::path::Path;

const MAGIC: &[u8] = b"Ultima 8 SaveGame File.\0";
const HEADER_SLOT: usize = 104;
/// Slots SGHEADER.DAT has descriptions for (the diary's pages).
const HEADER_SLOTS: u32 = 12;
/// Our own additions to a save (the original ignores unknown files): `key=value` lines.
pub const EXTRA: &str = "OPENU8.DAT";
/// The original's quick save slot (F7 / F8).
pub const QUICK_SLOT: u32 = 999;

/// The GAMEDAT files inside a save, by upper-case name (e.g. "ITEMCACH.DAT").
#[derive(Clone, Default)]
pub struct Save {
    pub files: HashMap<String, Vec<u8>>,
}

impl Save {
    pub fn parse(d: &[u8]) -> Result<Self> {
        let bad = |what: &str| Error::Bad(format!("save file: {what}"));
        if !d.starts_with(MAGIC) {
            return Err(bad("not an Ultima 8 save"));
        }
        let mut p = MAGIC.len();
        let take = |p: &mut usize, n: usize| -> Result<&[u8]> {
            let s = d.get(*p..*p + n).ok_or_else(|| bad("truncated"))?;
            *p += n;
            Ok(s)
        };
        let u32_at = |s: &[u8]| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize;
        let count = u16::from_le_bytes(take(&mut p, 2)?.try_into().unwrap());
        let mut files = HashMap::new();
        for _ in 0..count {
            let n = u32_at(take(&mut p, 4)?);
            let name = String::from_utf8_lossy(take(&mut p, n)?).trim_end_matches('\0').to_ascii_uppercase();
            let size = u32_at(take(&mut p, 4)?);
            files.insert(name, take(&mut p, size)?.to_vec());
        }
        Ok(Self { files })
    }

    pub fn load(root: &Path, slot: u32) -> Result<Self> {
        Self::parse(&std::fs::read(find_file(root, &format!("SAVEGAME/U8SAVE.{slot:03}"))?)?)
    }

    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.files.get(&name.to_ascii_uppercase()).map(Vec::as_slice)
    }

    pub fn set(&mut self, name: &str, data: Vec<u8>) {
        self.files.insert(name.to_ascii_uppercase(), data);
    }

    /// A GAMEDAT folder's files (the game in progress when the original last ran).
    pub fn from_dir(dir: &Path) -> Self {
        let files = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_file())
            .filter_map(|e| Some((e.file_name().to_string_lossy().to_ascii_uppercase(), std::fs::read(e.path()).ok()?)))
            .collect();
        Self { files }
    }

    /// The U8SAVE.nnn bytes (files in name order, NUL-terminated names as the original writes them).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut names: Vec<&String> = self.files.keys().collect();
        names.sort();
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&(names.len() as u16).to_le_bytes());
        for n in names {
            let d = &self.files[n];
            out.extend_from_slice(&(n.len() as u32 + 1).to_le_bytes());
            out.extend_from_slice(n.as_bytes());
            out.push(0);
            out.extend_from_slice(&(d.len() as u32).to_le_bytes());
            out.extend_from_slice(d);
        }
        out
    }

    /// The game clock when saved (30 a second, 27000 to the game hour): in the kernel, or for a
    /// game begun here (no kernel yet) in our own file ("ticks" in our older saves).
    pub fn clock(&self) -> Option<u32> {
        let kernel = self.get("KERNEL.DAT").and_then(crate::saved_clock);
        kernel.or_else(|| self.extra("clock").or_else(|| self.extra("ticks"))?.parse().ok())
    }

    /// A value from our own [`EXTRA`] file.
    pub fn extra(&self, key: &str) -> Option<String> {
        let text = String::from_utf8_lossy(self.get(EXTRA)?).into_owned();
        text.lines().find_map(|l| Some(l.split_once('=').filter(|(k, _)| k.trim() == key)?.1.trim().to_string()))
    }

    pub fn set_extra(&mut self, values: &[(&str, String)]) {
        let text: String = values.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
        self.set(EXTRA, text.into_bytes());
    }
}

/// Write `save` as SAVEGAME/U8SAVE.<slot>, with `desc` in SGHEADER.DAT for the diary's slots.
/// A save the original game made is kept as U8SAVE.<slot>.bak before it is first overwritten.
pub fn write(root: &Path, slot: u32, desc: &str, save: &Save) -> Result<std::path::PathBuf> {
    if slot == 0 {
        return Err(Error::Bad("slot 0 is the new game the original ships with; pick another".into()));
    }
    let dir = match find_file(root, "SAVEGAME") {
        Ok(d) => d,
        Err(_) => {
            let d = root.join("SAVEGAME");
            std::fs::create_dir_all(&d)?;
            d
        }
    };
    let name = format!("U8SAVE.{slot:03}");
    let path = find_file(&dir, &name).unwrap_or_else(|_| dir.join(&name));
    if path.exists() && Save::parse(&std::fs::read(&path)?).is_ok_and(|s| s.get(EXTRA).is_none()) {
        let bak = path.with_extension(format!("{slot:03}.bak"));
        if !bak.exists() {
            std::fs::copy(&path, &bak)?;
        }
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, save.to_bytes())?;
    std::fs::rename(&tmp, &path)?;
    if slot < HEADER_SLOTS {
        let hp = find_file(&dir, "SGHEADER.DAT").unwrap_or_else(|_| dir.join("SGHEADER.DAT"));
        let mut h = std::fs::read(&hp).unwrap_or_default();
        h.resize(h.len().max(HEADER_SLOT * HEADER_SLOTS as usize), 0);
        let rec = &mut h[slot as usize * HEADER_SLOT..(slot as usize + 1) * HEADER_SLOT];
        rec.fill(0);
        let text = &crate::text::encode(desc)[..];
        let n = text.len().min(HEADER_SLOT - 1);
        rec[..n].copy_from_slice(&text[..n]);
        std::fs::write(&hp, h)?;
    }
    Ok(path)
}

/// Saves present: (slot, description). Slot 0 is the start of a new game.
pub fn list(root: &Path) -> Vec<(u32, String)> {
    let header = find_file(root, "SAVEGAME/SGHEADER.DAT").ok().and_then(|p| std::fs::read(p).ok()).unwrap_or_default();
    let mut v: Vec<(u32, String)> = find_file(root, "SAVEGAME")
        .ok()
        .and_then(|d| std::fs::read_dir(d).ok())
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let name = e.ok()?.file_name().to_string_lossy().to_ascii_uppercase();
            let slot: u32 = name.strip_prefix("U8SAVE.")?.parse().ok()?;
            let text =
                header.get(slot as usize * HEADER_SLOT..(slot as usize + 1) * HEADER_SLOT).map_or(String::new(), |s| {
                    crate::text::decode(&s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())])
                });
            Some((slot, if slot == 0 && text.is_empty() { "new game".into() } else { text }))
        })
        .collect();
    v.sort();
    v
}

/// Put the Avatar's name into AVATAR.DAT (offset 6, 32 bytes, NUL-padded).
pub fn set_avatar_name(avatar_dat: &mut [u8], name: &str) {
    if let Some(field) = avatar_dat.get_mut(6..38) {
        field.fill(0);
        let name = crate::text::encode(name);
        let n = name.len().min(31);
        field[..n].copy_from_slice(&name[..n]);
    }
}

/// The Avatar's name from AVATAR.DAT (offset 6, NUL-terminated).
pub fn avatar_name(avatar_dat: &[u8]) -> Option<String> {
    let s = avatar_dat.get(6..)?;
    let s = &s[..s.len().min(32)];
    let s = &s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())];
    (!s.is_empty()).then(|| crate::text::decode(s))
}
