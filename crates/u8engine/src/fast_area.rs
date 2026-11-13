//! The "fast area": the neighbourhood of the Avatar where the world is live. Items get the
//! `enterFastArea` event when they come into it and `leaveFastArea` when they drop out — NPCs
//! start their routines, fires and fountains their sounds, many eggs their scripts.
use crate::world::{ObjId, World, AVATAR};

/// Half-size of the area in world units: the original's 320×200 view plus a margin.
pub const RANGE: i32 = 1024;

/// Which items are in it is kept on the world's entries (`Entry::fast`, ScummVM's FLG_FASTAREA),
/// so scripts' enterFastArea can bring one in early.
#[derive(Default)]
pub struct FastArea;

/// Who came in and who went out since the last update.
#[derive(Default, Debug)]
pub struct Change {
    pub entered: Vec<ObjId>,
    pub left: Vec<ObjId>,
}

impl FastArea {
    /// Is `id` in it (as of the last update, or brought in by a script since)?
    pub fn contains(&self, world: &World, id: ObjId) -> bool {
        world.is_fast(id)
    }

    /// Recompute around the Avatar (nothing is fast on a map without it). A new map's items
    /// all start outside.
    pub fn update(&mut self, world: &mut World) -> Change {
        let now: Vec<ObjId> = match world.get(AVATAR) {
            Some(av) => world.area(av.x, av.y, RANGE),
            None => vec![],
        };
        let mut change = Change::default();
        for &id in &now {
            if !world.is_fast(id) {
                world.set_fast(id, true);
                change.entered.push(id);
            }
        }
        let now: std::collections::HashSet<ObjId> = now.into_iter().collect();
        for i in 0..world.len() {
            let (id, e) = (world.id(i), *world.entry(i));
            if e.fast && !now.contains(&id) {
                world.set_fast_at(i, false);
                if !e.gone {
                    change.left.push(id);
                }
            }
        }
        change
    }
}
