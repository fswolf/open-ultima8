//! Readers for Ultima 8: Pagan data files — formats only, no game rules or drawing.
//! No game data is bundled; point [`GameData::load`] at your own install.
pub mod anim;
pub mod credits;
pub mod flex;
pub mod font;
pub mod map;
pub mod music;
pub mod overlay;
pub mod palette;
pub mod save;
pub mod shape;
pub mod skf;
pub mod sound;
pub mod typeflag;
pub mod usecode;
pub mod weapons;

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("bad data: {0}")]
    Bad(String),
}
pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn u16le(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([d[o], d[o + 1]])
}
pub(crate) fn i16le(d: &[u8], o: usize) -> i16 {
    u16le(d, o) as i16
}
pub(crate) fn u32le(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

/// Case-insensitive lookup of `rel` (e.g. "STATIC/U8PAL.PAL") under `root` — DOS files may be any case.
pub fn find_file(root: &Path, rel: &str) -> Result<PathBuf> {
    let mut cur = root.to_path_buf();
    for part in rel.split('/') {
        let hit = std::fs::read_dir(&cur)?
            .filter_map(|e| e.ok())
            .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(part))
            .ok_or_else(|| Error::Bad(format!("missing {} in {}", part, cur.display())))?;
        cur = hit.path();
    }
    Ok(cur)
}

/// Load EUSECODE.FLX and the saved global flags (GAMEDAT/FLAG.DAT, if present).
pub fn load_usecode(root: &Path) -> Result<(usecode::Usecode, Option<Vec<u8>>)> {
    let uc = usecode::Usecode::new(flex::Flex::parse(std::fs::read(find_file(root, "USECODE/EUSECODE.FLX")?)?)?);
    let flags = find_file(root, "GAMEDAT/FLAG.DAT").ok().and_then(|p| std::fs::read(p).ok());
    Ok((uc, flags))
}

/// Where the kernel's frame counter is in KERNEL.DAT (entry 1, offset 42), as a file offset.
pub fn ticks_offset(kernel: &[u8]) -> Option<usize> {
    let (o, n) = flex::Flex::parse(kernel.to_vec()).ok()?.span(1)?;
    (n >= 46).then_some(o + 42)
}

/// The kernel's frame counter: KERNEL.DAT entry 1, u32 at offset 42. (Identified by comparing
/// saves: it only grows, and gives 9 a.m. for a save made 9 game hours in.)
pub(crate) fn saved_ticks(kernel: &[u8]) -> Option<u32> {
    let o = ticks_offset(kernel)?;
    Some(u32::from_le_bytes(kernel[o..o + 4].try_into().ok()?))
}

/// A map's items by where a save keeps them.
pub struct MapLayers {
    /// FIXED.DAT (with globs expanded): never saved.
    pub fixed: Vec<map::Item>,
    /// NONFIXED.DAT: items that can change, each with its container contents (raw records).
    pub movable: Vec<(map::Item, Vec<u8>)>,
    /// ITEMCACH.DAT: the NPCs on the map.
    pub actors: Vec<map::Item>,
}

/// The static game data (and the saved game's world state) loaded from an install.
pub struct GameData {
    pub palette: palette::Palette,
    pub shapes: shape::ShapeArchive,
    pub typeflags: typeflag::TypeFlags,
    pub globs: map::Globs,
    pub fixed: flex::Flex,
    /// GAMEDAT state (movable items, NPCs); absent if the install has no GAMEDAT.
    pub nonfixed: Option<flex::Flex>,
    pub itemcach: Option<Vec<u8>>,
    /// NPCDATA.DAT entry 0: NPC stats and the high bytes of their frames.
    pub npcdata: Option<Vec<u8>>,
    pub anims: anim::AnimDat,
    /// Where held weapons go over the Avatar (STATIC/WPNOVLAY.DAT; empty if missing).
    pub overlays: overlay::WeaponOverlays,
    /// The weapon table compiled into U8.EXE (damage, bonuses, how each is held); empty if the
    /// EXE isn't in the install.
    pub weapons: weapons::Weapons,
    /// The game clock when the save was made (60 a second, 27000 to the game hour), from
    /// GAMEDAT/KERNEL.DAT.
    pub ticks: Option<u32>,
    /// Global flags (GAMEDAT/FLAG.DAT) for the scripts.
    pub flags: Option<Vec<u8>>,
    /// The player's name for the Avatar (GAMEDAT/AVATAR.DAT).
    pub avatar_name: Option<String>,
    /// The game state's files as loaded (from the save, or GAMEDAT): saving starts from these, so
    /// whatever we don't model yet (NPC stats, gumps, the kernel) is kept as it was.
    pub state: save::Save,
}

impl GameData {
    /// Load an install with the game state in its GAMEDAT folder (the last game played).
    pub fn load(root: &Path) -> Result<Self> {
        Self::load_save(root, None)
    }

    /// Load an install with the game state from a saved game instead of GAMEDAT.
    pub fn load_save(root: &Path, save: Option<&save::Save>) -> Result<Self> {
        let rd = |p: &str| -> Result<Vec<u8>> {
            match (save, p.strip_prefix("GAMEDAT/")) {
                (Some(s), Some(name)) => {
                    s.get(name).map(<[u8]>::to_vec).ok_or_else(|| Error::Bad(format!("save has no {name}")))
                }
                _ => Ok(std::fs::read(find_file(root, p)?)?),
            }
        };
        let state = match save {
            Some(s) => save::Save { files: s.files.clone() },
            None => find_file(root, "GAMEDAT").map(|d| save::Save::from_dir(&d)).unwrap_or_default(),
        };
        // Our own saves record the clock themselves (a new game has no kernel to keep it in).
        let ticks = state.ticks();
        let anims = rd("STATIC/ANIM.DAT").map(|d| anim::AnimDat::parse(&d)).unwrap_or_default();
        let overlays = rd("STATIC/WPNOVLAY.DAT")
            .ok()
            .and_then(|d| flex::Flex::parse(d).ok())
            .map(|f| overlay::WeaponOverlays::parse(&f, &anims))
            .unwrap_or_default();
        Ok(Self {
            palette: palette::Palette::parse(&rd("STATIC/U8PAL.PAL")?)?,
            shapes: shape::ShapeArchive::new(flex::Flex::parse(rd("STATIC/U8SHAPES.FLX")?)?),
            typeflags: typeflag::TypeFlags::parse(&rd("STATIC/TYPEFLAG.DAT")?),
            globs: map::Globs::parse(&flex::Flex::parse(rd("STATIC/GLOB.FLX")?)?),
            fixed: flex::Flex::parse(rd("STATIC/FIXED.DAT")?)?,
            nonfixed: rd("GAMEDAT/NONFIXED.DAT").ok().and_then(|d| flex::Flex::parse(d).ok()),
            itemcach: rd("GAMEDAT/ITEMCACH.DAT")
                .ok()
                .and_then(|d| flex::Flex::parse(d).ok())
                .and_then(|f| f.get(0).map(<[u8]>::to_vec)),
            npcdata: rd("GAMEDAT/NPCDATA.DAT")
                .ok()
                .and_then(|d| flex::Flex::parse(d).ok())
                .and_then(|f| f.get(0).map(<[u8]>::to_vec)),
            anims,
            overlays,
            weapons: rd("U8.EXE").ok().and_then(|d| weapons::Weapons::parse(&d)).unwrap_or_default(),
            ticks,
            flags: rd("GAMEDAT/FLAG.DAT").ok(),
            avatar_name: rd("GAMEDAT/AVATAR.DAT").ok().and_then(|d| save::avatar_name(&d)),
            state,
        })
    }
    /// Fixed items of a map with glob eggs expanded.
    pub fn map_items(&self, map: usize) -> Vec<map::Item> {
        map::load_fixed(self.fixed.get(map).unwrap_or(&[]), &self.globs, &self.typeflags)
    }
    /// Everything on a map: fixed + nonfixed items + NPCs.
    pub fn all_items(&self, map: usize) -> Vec<map::Item> {
        let l = self.map_layers(map);
        l.fixed.into_iter().chain(l.movable.into_iter().map(|(it, _)| it)).chain(l.actors).collect()
    }
    /// A map's items by how they're saved.
    pub fn map_layers(&self, map: usize) -> MapLayers {
        MapLayers {
            fixed: self.map_items(map),
            movable: self
                .nonfixed
                .as_ref()
                .and_then(|f| f.get(map))
                .map_or(vec![], |d| map::load_nonfixed_with_contents(d, &self.typeflags)),
            actors: self
                .itemcach
                .as_ref()
                .map_or(vec![], |d| map::load_actors(d, map))
                .into_iter()
                .map(|a| self.full_frame(a))
                .collect(),
        }
    }
    /// The Avatar (NPC 1) as saved in GAMEDAT, with its map number.
    pub fn avatar(&self) -> Option<(usize, map::Item)> {
        map::load_actor(self.itemcach.as_ref()?, 1).map(|(m, a)| (m, self.full_frame(a)))
    }
    /// An NPC with its whole frame number (the high byte lives in NPCDATA.DAT).
    fn full_frame(&self, mut a: map::Item) -> map::Item {
        if let Some(d) = &self.npcdata {
            a.frame |= map::npc_frame_high(d, a.npc as usize);
        }
        a
    }
}
