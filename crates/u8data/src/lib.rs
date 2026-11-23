//! Readers for Ultima 8: Pagan data files — formats only, no game rules or drawing.
//! No game data is bundled; point [`GameData::load`] at your own install.
pub mod anim;
pub mod credits;
pub mod flex;
pub mod font;
pub mod lang;
pub mod map;
pub mod music;
pub mod overlay;
pub mod palette;
pub mod save;
pub mod shape;
pub mod skf;
pub mod sound;
pub mod text;
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

/// The saved game tests use (see [`GameData::load_fixed`]), if it exists.
pub fn fixed_slot(root: &Path) -> Option<u32> {
    let n = std::env::var("U8_SAVE").ok().and_then(|s| s.parse().ok()).unwrap_or(2);
    find_file(root, &format!("SAVEGAME/U8SAVE.{n:03}")).is_ok().then_some(n)
}

/// Load the scripts (EUSECODE.FLX, or a translation's: see [`lang`]) and the saved global flags
/// (GAMEDAT/FLAG.DAT, if present).
pub fn load_usecode(root: &Path) -> Result<(usecode::Usecode, Option<Vec<u8>>)> {
    let uc = usecode::Usecode::new(flex::Flex::parse(std::fs::read(lang::file(root, "USECODE/{}USECODE.FLX")?)?)?);
    let flags = find_file(root, "GAMEDAT/FLAG.DAT").ok().and_then(|p| std::fs::read(p).ok());
    Ok((uc, flags))
}

/// Where the kernel's 60 Hz frame counter is in KERNEL.DAT (entry 1, offset 42), as a file offset.
pub fn frames_offset(kernel: &[u8]) -> Option<usize> {
    let (o, n) = flex::Flex::parse(kernel.to_vec()).ok()?.span(1)?;
    (n >= 46).then_some(o + 42)
}

/// Where the game clock is in KERNEL.DAT, as a file offset: entry 20, u32 at offset 64 (30 a
/// second, 27000 to the game hour), then the hour number (u32 at 68). Found by comparing DOS
/// saves: Divination's hour and day follow it, and resting moves it by the hours rested.
pub fn clock_offset(kernel: &[u8]) -> Option<usize> {
    let (o, n) = flex::Flex::parse(kernel.to_vec()).ok()?.span(20)?;
    (n >= 72).then_some(o + 64)
}

fn read_u32(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?))
}

/// The game clock a save's KERNEL.DAT holds.
pub(crate) fn saved_clock(kernel: &[u8]) -> Option<u32> {
    read_u32(kernel, clock_offset(kernel)?)
}

/// The 60 Hz frame counter a save's KERNEL.DAT holds.
pub fn saved_frames(kernel: &[u8]) -> Option<u32> {
    read_u32(kernel, frames_offset(kernel)?)
}

/// A map's items by where a save keeps them.
pub struct MapLayers {
    /// FIXED.DAT (with globs expanded), each with what it holds (a chest's contents, raw records).
    pub fixed: Vec<(map::Item, Vec<u8>)>,
    /// NONFIXED.DAT: items that can change, each with its container contents (raw records).
    pub movable: Vec<(map::Item, Vec<u8>)>,
    /// ITEMCACH.DAT: the NPCs on the map.
    pub actors: Vec<map::Item>,
}

/// The static game data (and the saved game's world state) loaded from an install.
pub struct GameData {
    pub palette: palette::Palette,
    /// What translucent shapes blend with (XFORMPAL.DAT).
    pub xform: palette::XForm,
    pub shapes: shape::ShapeArchive,
    pub typeflags: typeflag::TypeFlags,
    pub globs: map::Globs,
    pub fixed: flex::Flex,
    /// GAMEDAT state (movable items, NPCs); absent if the install has no GAMEDAT.
    pub nonfixed: Option<flex::Flex>,
    pub itemcach: Option<Vec<u8>>,
    /// ITEMCACH.DAT entry 1: the chunks the DOS game had in memory (see [`map::cached_area`]).
    pub itemcach_area: Option<Vec<u8>>,
    /// NPCDATA.DAT entry 0: NPC stats and the high bytes of their frames.
    pub npcdata: Option<Vec<u8>>,
    pub anims: anim::AnimDat,
    /// Where held weapons go over the Avatar (STATIC/WPNOVLAY.DAT; empty if missing).
    pub overlays: overlay::WeaponOverlays,
    /// The weapon table compiled into U8.EXE (damage, bonuses, how each is held); empty if the
    /// EXE isn't in the install.
    pub weapons: weapons::Weapons,
    /// The game clock when the save was made (30 a second, 27000 to the game hour), from
    /// GAMEDAT/KERNEL.DAT.
    pub clock: Option<u32>,
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

    /// For tests: the install with a fixed game state, saved game `U8_SAVE` (default 2, the
    /// castle save) if it's there, else GAMEDAT. Playing the game rewrites GAMEDAT, so tests
    /// that read it would change with each session.
    pub fn load_fixed(root: &Path) -> Result<Self> {
        match fixed_slot(root) {
            Some(n) => Self::load_save(root, Some(&save::Save::load(root, n)?)),
            None => Self::load(root),
        }
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
        let clock = state.clock();
        let anims = rd("STATIC/ANIM.DAT").map(|d| anim::AnimDat::parse(&d)).unwrap_or_default();
        let overlays = rd("STATIC/WPNOVLAY.DAT")
            .ok()
            .and_then(|d| flex::Flex::parse(d).ok())
            .map(|f| overlay::WeaponOverlays::parse(&f, &anims))
            .unwrap_or_default();
        let cache = rd("GAMEDAT/ITEMCACH.DAT").ok().and_then(|d| flex::Flex::parse(d).ok());
        let (mut itemcach, itemcach_area) = match &cache {
            Some(f) => (f.get(0).map(<[u8]>::to_vec), f.get(1).map(<[u8]>::to_vec)),
            None => (None, None),
        };
        if let (Some(e0), Some(e1)) = (&mut itemcach, &itemcach_area) {
            map::drop_free_objects(e0, e1);
        }
        Ok(Self {
            palette: palette::Palette::parse(&rd("STATIC/U8PAL.PAL")?)?,
            xform: rd("STATIC/XFORMPAL.DAT")
                .ok()
                .and_then(|d| flex::Flex::parse(d).ok())
                .and_then(|f| palette::XForm::parse(&f))
                .unwrap_or_default(),
            shapes: shape::ShapeArchive::new(flex::Flex::parse(rd("STATIC/U8SHAPES.FLX")?)?),
            typeflags: typeflag::TypeFlags::parse(&rd("STATIC/TYPEFLAG.DAT")?),
            globs: map::Globs::parse(&flex::Flex::parse(rd("STATIC/GLOB.FLX")?)?),
            fixed: flex::Flex::parse(rd("STATIC/FIXED.DAT")?)?,
            nonfixed: rd("GAMEDAT/NONFIXED.DAT").ok().and_then(|d| flex::Flex::parse(d).ok()),
            itemcach,
            itemcach_area,
            npcdata: rd("GAMEDAT/NPCDATA.DAT")
                .ok()
                .and_then(|d| flex::Flex::parse(d).ok())
                .and_then(|f| f.get(0).map(<[u8]>::to_vec)),
            anims,
            overlays,
            weapons: rd("U8.EXE").ok().and_then(|d| weapons::Weapons::parse(&d)).unwrap_or_default(),
            clock,
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
        l.fixed.into_iter().chain(l.movable).map(|(it, _)| it).chain(l.actors).collect()
    }
    /// A map's items by how they're saved.
    pub fn map_layers(&self, map: usize) -> MapLayers {
        let mut fixed = map::load_fixed_with_contents(self.fixed.get(map).unwrap_or(&[]), &self.globs, &self.typeflags);
        let mut movable = self
            .nonfixed
            .as_ref()
            .and_then(|f| f.get(map))
            .map_or(vec![], |d| map::load_nonfixed_with_contents(d, &self.typeflags));
        // The area the DOS game had in memory: its chunks come from the cache, not NONFIXED.
        let cached = match (&self.itemcach, &self.itemcach_area) {
            (Some(e0), Some(e1)) => map::cached_area(e0, e1).filter(|c| c.map == map),
            _ => None,
        };
        if let (Some(map::CachedArea { chunks, items, .. }), Some(e0)) = (cached, &self.itemcach) {
            let chunk = |it: &map::Item| (it.x >> 9, it.y >> 9);
            // (Scenery that animates is saved at whatever frame it was showing: matched without
            // the frame, or it'd come back as a second, movable copy each load.)
            let key = |it: &map::Item| {
                (it.x, it.y, it.z, it.shape, if self.animates(it.shape) { u32::MAX } else { it.frame })
            };
            let mut held: std::collections::HashMap<_, usize> = std::collections::HashMap::new();
            for (_, it) in &items {
                *held.entry(key(it)).or_default() += 1;
            }
            // Every FIXED.DAT record of those chunks is held (glob eggs too, and near the Avatar
            // what they expand to): one that isn't has changed (a bed slept in), and the cache
            // has it as it is now.
            let plain = map::load_fixed(self.fixed.get(map).unwrap_or(&[]), &map::Globs(vec![]), &self.typeflags);
            let mut changed: std::collections::HashMap<_, usize> = std::collections::HashMap::new();
            for it in plain.iter().filter(|it| chunks.contains(&chunk(it))) {
                match held.get_mut(&key(it)) {
                    Some(n) if *n > 0 => *n -= 1,
                    _ => *changed.entry(key(it)).or_default() += 1,
                }
            }
            fixed.retain(|(it, _)| match changed.get_mut(&key(it)) {
                Some(n) if *n > 0 && chunks.contains(&chunk(it)) => {
                    *n -= 1;
                    false
                }
                _ => true,
            });
            // What's left of the held items beyond the fixed ones is the movable layer. The held
            // fixed ones hold what the cache has inside them (a chest emptied stays empty).
            let mut fixed_here: std::collections::HashMap<_, usize> = std::collections::HashMap::new();
            for (it, _) in fixed.iter().filter(|(it, _)| chunks.contains(&chunk(it))) {
                *fixed_here.entry(key(it)).or_default() += 1;
            }
            movable.retain(|(it, _)| !chunks.contains(&chunk(it)));
            let inside = map::load_cache_contents(e0);
            let mut slots: std::collections::HashMap<_, Vec<u16>> = std::collections::HashMap::new();
            for (o, it) in items {
                match fixed_here.get_mut(&key(&it)) {
                    Some(n) if *n > 0 => {
                        *n -= 1;
                        slots.entry(key(&it)).or_default().push(o);
                    }
                    // A second copy of animated scenery on its very spot: what saves made before
                    // the frame was left out of the match hold. Dropped.
                    Some(_) if self.animates(it.shape) => {}
                    _ => movable.push((it, map::cache_contents_records(&inside, o))),
                }
            }
            for (it, held) in fixed.iter_mut().filter(|(it, _)| chunks.contains(&chunk(it))) {
                if let Some(o) = slots.get_mut(&key(it)).and_then(Vec::pop) {
                    *held = map::cache_contents_records(&inside, o);
                }
            }
        }
        MapLayers {
            fixed,
            movable,
            actors: self
                .itemcach
                .as_ref()
                .map_or(vec![], |d| map::load_actors(d, map))
                .into_iter()
                .map(|a| self.full_frame(a))
                .collect(),
        }
    }

    /// Does `shape` animate as scenery (TYPEFLAG animation type 1 to 6, with frames to cycle
    /// through, or type 5, which its script drives)? As u8engine's scenery animation reads it.
    pub fn animates(&self, shape: u32) -> bool {
        let k = self.typeflags.get(shape).anim_type;
        (1..=6).contains(&k) && (k == 5 || self.shapes.get(shape).len() >= 2)
    }

    /// The Avatar (NPC 1) as saved in GAMEDAT, with its map number.
    pub fn avatar(&self) -> Option<(usize, map::Item)> {
        self.actor(1)
    }
    /// NPC `npc` as saved, with its map number.
    pub fn actor(&self, npc: u16) -> Option<(usize, map::Item)> {
        map::load_actor(self.itemcach.as_ref()?, npc as usize).map(|(m, a)| (m, self.full_frame(a)))
    }
    /// An NPC with its whole frame number (the high byte lives in NPCDATA.DAT).
    fn full_frame(&self, mut a: map::Item) -> map::Item {
        if let Some(d) = &self.npcdata {
            a.frame |= map::npc_frame_high(d, a.npc as usize);
        }
        a
    }
}
