//! Contact events (ScummVM's collideMove): when something moves, what it newly touches —
//! overlaps, or stands on — gets `gotHit` (and the mover `hit`); what it no longer touches gets
//! `release` (and so does the mover). Pressure plates, lava, webs and the like hang on these.
//! Run for everyone's steps in the fast area (the Avatar's every update), things the player
//! puts down or picks up, and things landing.
use crate::game::Game;
use crate::usecode::event;
use crate::world::ObjId;

/// Farthest a tick's movement goes (a run, a leap, a knock); more is a teleport.
const MOST_IN_A_TICK: i32 = 128;

fn step(a: (i32, i32, i32), b: (i32, i32, i32)) -> bool {
    (a.0 - b.0).abs().max((a.1 - b.1).abs()).max((a.2 - b.2).abs()) <= MOST_IN_A_TICK
}

/// How hard a move of (dx, dy, dz) strikes what it meets (MOTRAK.C makeItSo).
pub(crate) fn force((dx, dy, dz): (i32, i32, i32)) -> i16 {
    let (x, y, z) = ((dx >> 2).abs(), (dy >> 2).abs(), dz.abs());
    ((x + y + z + x.max(y).max(z)) / 2) as i16
}

/// Where someone last was, and what they touched there.
pub(crate) type Footing = ((i32, i32, i32), Vec<ObjId>);

impl Game {
    /// `id` moved by (dx, dy, dz) from where it touched `before`: the events. Returns what it
    /// touches now.
    pub(crate) fn touched(&mut self, id: ObjId, before: &[ObjId], d: (i32, i32, i32)) -> Vec<ObjId> {
        let now = self.world.contacts(id);
        let force = force(d);
        let args = |o: ObjId| [o.to_le_bytes(), force.to_le_bytes()].concat();
        for &o in now.iter().filter(|o| !before.contains(o)) {
            self.trigger_with(o, event::GOT_HIT, &args(id));
            self.trigger_with(id, event::HIT, &args(o));
        }
        let gone: Vec<ObjId> =
            before.iter().copied().filter(|o| !now.contains(o) && self.world.get(*o).is_some()).collect();
        for &o in &gone {
            self.trigger(o, event::RELEASE);
        }
        if !gone.is_empty() {
            self.trigger(id, event::RELEASE);
        }
        now
    }

    /// Someone moved (walking, jumping, carried by a script): their contact events. The first
    /// look at someone (on a map) only notes what they stand on.
    pub(crate) fn step_contacts(&mut self, id: ObjId) {
        let Some(p) = self.world.get(id).filter(|_| !self.world.is_void(id)) else {
            self.footing.remove(&id);
            return;
        };
        let at = (p.x, p.y, p.z);
        if self.footing.get(&id).is_some_and(|(from, _)| *from != at) {
            // Every move runs justMoved (ScummVM's Item::move): the Avatar's keeps the glow of
            // Endurance and magic armour with it.
            self.trigger(id, event::JUST_MOVED);
        }
        match self.footing.remove(&id) {
            // A step (not a script's teleport: ScummVM moves those without collisions).
            Some((from, before)) if from != at && step(from, at) => {
                let d = (at.0 - from.0, at.1 - from.1, at.2 - from.2);
                let now = self.touched(id, &before, d);
                self.footing.insert(id, (at, now));
            }
            Some((from, f)) if from == at => _ = self.footing.insert(id, (from, f)),
            _ => _ = self.footing.insert(id, (at, self.world.contacts(id))),
        }
    }

    /// `id`'s contact events were run for where it is now (a script's collideMove).
    pub(crate) fn note_footing(&mut self, id: ObjId) {
        if let Some(p) = self.world.get(id).filter(|_| self.footing.contains_key(&id)) {
            self.footing.insert(id, ((p.x, p.y, p.z), self.world.contacts(id)));
        }
    }

    /// Everyone else in the fast area who moved this tick (monsters on plates, townsfolk into
    /// webs: ScummVM runs collideMove for every actor's step).
    pub(crate) fn people_contacts(&mut self) {
        for id in crate::world::AVATAR + 1..256 {
            if self.world.get(id).is_some() && self.fast.contains(&self.world, id) {
                self.step_contacts(id);
            } else if self.footing.contains_key(&id) {
                self.footing.remove(&id);
            }
        }
    }
}
