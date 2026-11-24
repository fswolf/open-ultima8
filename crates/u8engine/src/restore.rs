//! "Restored content" (an option, off by default): pieces the original has every word and picture
//! of but never links up, put in with only the game's own flags and shapes, so a save made with
//! it still loads in DOS (claude/restore-plan.md). One table, an entry per piece: when it runs
//! (its hook) and what it does. Each runs once; which have run is kept in the save's side file.
//! Turned off, nothing new runs (a follow-up still finishes its entry); what already happened stays.
//!
//! hasHeart also wakes the original's own walking dead: its guard posts in the towns (GUARDMEG
//! eggs) make ghouls instead of guards while it is set, and the townsfolk and guards it silences
//! (Jenna, Darion, Shaana, Tarna, Aramina, the guards) won't talk. Nothing in the scripts clears
//! it, so here it clears a game day after it was set: the dead go back and people talk again.
use crate::game::Game;
use crate::world::{ObjId, AVATAR};

/// Story flags (global bits: FLAGS.unk's order, 14 on).
pub mod flag {
    pub const HAS_HEART: usize = 0x3C;
    pub const MORDEA_DEAD: usize = 0x11A;
}

/// When an entry runs.
#[derive(Clone, Copy, Debug)]
pub enum Hook {
    /// On this map (entered, or a game started or loaded there).
    Map(u16),
    /// The Avatar holds something of this shape and frame (in hand or in a bag).
    Holding(u32, u32),
    /// This person is dead.
    Dies(ObjId),
    /// This many game minutes after the named entry ran.
    After(&'static str, u32),
}

/// What an entry does.
#[derive(Clone, Copy, Debug)]
pub enum Act {
    /// Set a story flag.
    Flag(usize),
    /// Clear one.
    Clear(usize),
    /// Put a thing (shape, frame, quality) at a spot on the hook's map.
    Place(u32, u32, u16, (i32, i32, i32)),
}

#[derive(Debug)]
pub struct Entry {
    pub name: &'static str,
    pub hook: Hook,
    pub act: Act,
}

pub const ENTRIES: &[Entry] = &[
    // A trowel in the cemetery, on the ground by the Necromancer's tomb door.
    Entry { name: "trowel", hook: Hook::Map(6), act: Act::Place(834, 0, 0, (7903, 25823, 8)) },
    // The Mysteries of Lithos on the Necromancer's bookcase, by the Grim Book.
    Entry { name: "lithos-book", hook: Hook::Map(6), act: Act::Place(613, 23, 110, (8159, 24863, 80)) },
    // A hand torch on Bentic's desk.
    Entry { name: "torch", hook: Hook::Map(41), act: Act::Place(568, 0, 0, (15295, 1951, 112)) },
    Entry { name: "mordea-dead", hook: Hook::Dies(7), act: Act::Flag(flag::MORDEA_DEAD) },
    // The glowing Heart of Earth (EBROCK frame 1).
    Entry { name: "has-heart", hook: Hook::Holding(793, 1), act: Act::Flag(flag::HAS_HEART) },
    // A game day later the dead have gone back.
    Entry { name: "dead-rest", hook: Hook::After("has-heart", 24 * 60), act: Act::Clear(flag::HAS_HEART) },
    // The Key of the Necromancer on the altar of the Birthplace of Moriens.
    Entry { name: "necromancer-key", hook: Hook::Map(50), act: Act::Place(232, 4, 0, (14212, 12684, 64)) },
];

/// The option and what it has done in this game.
#[derive(Default, Debug)]
pub struct Restore {
    pub on: bool,
    /// The entries that have run, and when (game minutes).
    pub done: Vec<(String, u32)>,
}

impl Game {
    /// Run whatever entries are due, each once: on a map entered or a game started or loaded,
    /// and once a second. With the option off nothing new starts, but a follow-up still finishes
    /// what its entry began (the dead go back a day after the Heart woke them).
    pub(crate) fn restore(&mut self) {
        for e in ENTRIES {
            let follow = matches!(e.hook, Hook::After(..));
            if !(self.restored.on || follow) || self.restored.done.iter().any(|d| d.0 == e.name) || !self.due(e.hook) {
                continue;
            }
            match e.act {
                Act::Flag(f) | Act::Clear(f) => {
                    let set = matches!(e.act, Act::Flag(_)) as u32;
                    let Some(vm) = self.vm() else { continue }; // (no scripts: try again once there are)
                    vm.set_global_bits(f, 1, set);
                }
                // (Not if one already lies there: a save whose side file was lost or replaced.)
                Act::Place(shape, frame, quality, (x, y, z)) if !self.lies_at(shape, frame, (x, y, z)) => {
                    let it = u8data::map::Item { shape, frame, quality, x, y, z, ..Default::default() };
                    self.world.add(&self.data, it, true);
                }
                Act::Place(..) => {}
            }
            self.restored.done.push((e.name.to_string(), self.minutes()));
        }
    }

    /// Is a thing of `shape` and `frame` on the map within a few steps of `at`?
    fn lies_at(&self, shape: u32, frame: u32, (x, y, z): (i32, i32, i32)) -> bool {
        self.world
            .area(x, y, 64)
            .into_iter()
            .filter_map(|id| self.world.get(id))
            .any(|it| it.shape == shape && it.frame == frame && (it.z - z).abs() <= 64)
    }

    fn due(&self, hook: Hook) -> bool {
        match hook {
            Hook::Map(m) => self.map() == m,
            Hook::Holding(shape, frame) => self.holds(AVATAR, shape, frame),
            Hook::Dies(id) => self.stats.get(id).is_some_and(|s| s.dead),
            Hook::After(name, mins) => {
                self.restored.done.iter().any(|d| d.0 == name && self.minutes() >= d.1.saturating_add(mins))
            }
        }
    }

    /// Does `id` hold something of `shape` and `frame`, at any depth?
    fn holds(&self, id: ObjId, shape: u32, frame: u32) -> bool {
        self.world.contents(id).into_iter().any(|t| {
            self.world.get(t).is_some_and(|it| it.shape == shape && it.frame == frame) || self.holds(t, shape, frame)
        })
    }
}
