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
//! The lines also speak of the ground shaking, and nothing shakes it: the first time the Avatar is
//! in Tenebrae while the dead walk, the Mountain King's own quake plays, as at Lothian's
//! interment (VIVIDOS: startQuake(1), sound 49, 120 ticks, stopQuake), never over a talk or scene.
use crate::game::Game;
use crate::world::{ObjId, AVATAR};

/// Story flags (global bits: FLAGS.unk's order, 14 on).
pub mod flag {
    pub const HAS_HEART: usize = 0x3C;
    pub const MORDEA_DEAD: usize = 0x11A;
    /// Someone is in a conversation (METHOD sets it while one runs).
    pub const SOMEBODY_TALKING: usize = 0x0F;
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
    /// On one of these maps while a story flag is set.
    MapsWhile(&'static [u16], usize),
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
    /// Shake the ground (strength) with a sound (number, priority) for some ticks; not over a talk
    /// or a scene: it waits for them to end.
    Quake(u16, u16, u16, u32),
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
    // The ground shakes as the dead come: the first time in Tenebrae (the docks, the city, the
    // castle) while they walk.
    Entry { name: "quake", hook: Hook::MapsWhile(&[39, 40, 41], flag::HAS_HEART), act: Act::Quake(1, 49, 200, 120) },
    // The Key of the Necromancer on the altar of the Birthplace of Moriens.
    Entry { name: "necromancer-key", hook: Hook::Map(50), act: Act::Place(232, 4, 0, (14212, 12684, 64)) },
];

/// The option and what it has done in this game.
#[derive(Default, Debug)]
pub struct Restore {
    pub on: bool,
    /// The entries that have run, and when (game minutes).
    pub done: Vec<(String, u32)>,
    /// Ticks left of a quake of ours.
    quake_left: u32,
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
            if matches!(e.act, Act::Quake(..)) && self.busy() {
                continue; // (after the talk or scene)
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
                Act::Quake(strength, sfx, priority, ticks) => {
                    if self.scripts.is_none() {
                        continue;
                    }
                    let [a, b] = strength.to_le_bytes();
                    let ([c, d], [p, q]) = (sfx.to_le_bytes(), priority.to_le_bytes());
                    self.call_intrinsic(0xC6, &[a, b]); // Camera::startQuake
                    self.call_intrinsic(0xEC, &[c, d, p, q]); // playSFX
                    self.restored.quake_left = ticks;
                }
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

    /// Each tick: a quake of ours stops when its time is up (Camera::stopQuake).
    pub(crate) fn quake_ends(&mut self) {
        if self.restored.quake_left > 0 {
            self.restored.quake_left -= 1;
            if self.restored.quake_left == 0 {
                self.call_intrinsic(0xC7, &[]);
            }
        }
    }

    /// A talk or a scene is on: a conversation is running, or a script holds the Avatar.
    fn busy(&self) -> bool {
        self.in_stasis() || self.scripts.as_ref().is_some_and(|s| s.vm.global_bits(flag::SOMEBODY_TALKING, 1) != 0)
    }

    fn due(&self, hook: Hook) -> bool {
        match hook {
            Hook::Map(m) => self.map() == m,
            Hook::Holding(shape, frame) => self.holds(AVATAR, shape, frame),
            Hook::Dies(id) => self.stats.get(id).is_some_and(|s| s.dead),
            Hook::MapsWhile(maps, f) => {
                maps.contains(&self.map()) && self.scripts.as_ref().is_some_and(|s| s.vm.global_bits(f, 1) != 0)
            }
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
