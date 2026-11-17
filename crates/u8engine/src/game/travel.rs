//! Changing maps: each visited map's world is kept as it was left; teleports take the Avatar
//! (and what it carries) along.
use super::*;

impl Game {
    /// A map just loaded (`fresh`: from the save, else as it was left): those in combat are
    /// fighting again, without their going-into-combat script a second time.
    pub(super) fn resume_fights(&mut self, fresh: bool) {
        for id in 1..256 {
            let here = self.world.get(id).is_some() && !self.world.is_void(id);
            if fresh && here && self.stats.get(id).is_some_and(|s| s.in_combat && !s.dead) {
                self.world.set_in_combat(id, true);
            }
        }
        for id in self.world.fighters().into_iter().filter(|&id| id != AVATAR) {
            self.resume_fight(id);
        }
    }

    /// Maps that have anything on them, in order.
    pub fn maps(&self) -> &[u16] {
        &self.maps
    }

    pub fn map(&self) -> u16 {
        self.world.map()
    }

    /// Switch maps (the Avatar stays behind; see [`teleport`](Self::teleport) to take it along).
    pub fn load_map(&mut self, map: u16) {
        self.enter(map, None);
        self.avatar = Avatar::new(&self.data, &self.world);
    }

    /// Make `map` current: the old world is kept as it is, and scripts are stopped (object ids
    /// are per map) but `keep`, the one that brought the Avatar here.
    pub(super) fn enter(&mut self, map: u16, keep: Option<crate::usecode::Pid>) {
        if map == self.map() {
            return;
        }
        if let Some(s) = &mut self.scripts {
            s.vm.kill_for_map_change(keep);
        }
        self.put_away_all(); // hatched creatures don't outlast the map
        self.world.clear_ethereal(); // nor what scripts left in the void
        let fresh = !self.visited.contains_key(&map);
        let next = self.visited.remove(&map).unwrap_or_else(|| World::build(&self.data, map));
        let old = std::mem::replace(&mut self.world, next);
        self.world.clear_flag(GUMP_OPEN); // windows don't survive leaving a map
        self.visited.insert(old.map(), old);
        self.arrivals();
        self.walks.clear();
        self.fights.clear();
        self.resume_fights(fresh);
        self.loiters.clear();
        self.rising.clear();
        self.sprites.clear(); // (their scripts went with the map)
        self.flights.clear();
        self.footing.clear(); // a fresh look at what people stand on
        self.camera.reset(); // its scripts are gone; follow the Avatar on the new map
                             // (The new map's people are scheduled if an hour turned while we were away: `schedule`.)
        self.cache_in = true;
        self.tick_clock = 0.0;
        self.map_changed = true;
    }

    /// People scripts sent here while we were elsewhere (teleported off another map: in its void,
    /// marked for this one) come over, with what they carry.
    fn arrivals(&mut self) {
        let here = self.map();
        let mut coming = vec![];
        for w in self.visited.values_mut() {
            let sent: Vec<ObjId> = (AVATAR + 1..256)
                .filter(|&id| w.is_void(id) && w.get(id).is_some_and(|it| it.mapnum as u16 == here))
                .collect();
            for id in sent {
                let (it, things) = (w.get(id).unwrap(), w.take_contents(id));
                w.remove(id);
                coming.push((it, things));
            }
        }
        for (it, things) in coming {
            let id = it.npc;
            self.world.take_contents(id); // (an older copy's)
            self.world.put_actor(&self.data, it);
            self.world.set_void(id, false);
            self.world.give_contents(&self.data, id, &things);
        }
    }

    /// Move the Avatar to teleport destination `id` on `map`. False (and nothing happens) if the
    /// Avatar isn't here or the destination doesn't exist.
    pub fn teleport(&mut self, map: u16, id: u8) -> bool {
        if self.world.get(AVATAR).is_none() {
            return false;
        }
        let dest = match self.visited.get(&map) {
            _ if map == self.map() => self.world.teleport_destination(id),
            Some(w) => w.teleport_destination(id),
            None => {
                let w = World::build(&self.data, map);
                let d = w.teleport_destination(id);
                self.visited.insert(map, w);
                d
            }
        };
        let Some(at) = dest else {
            eprintln!("teleport: no destination {id} on map {map}");
            return false;
        };
        self.move_avatar(map, at, None)
    }

    /// Put the Avatar at (x, y, z) on `map` (testing tools). False if there's no Avatar.
    pub fn put_avatar(&mut self, map: u16, at: (i32, i32, i32)) -> bool {
        self.move_avatar(map, at, None)
    }

    /// The Avatar to (x, y, z) on `map` (a script's teleport there: `keep` is that script, which
    /// goes on running). False if there's no Avatar.
    pub(crate) fn move_avatar(
        &mut self,
        map: u16,
        (x, y, z): (i32, i32, i32),
        keep: Option<crate::usecode::Pid>,
    ) -> bool {
        let Some(mut av) = self.world.get(AVATAR) else { return false };
        let dir = self.avatar.as_ref().map(|a| a.dir);
        let fighting = self.world.in_combat(AVATAR);
        let mut carried = None;
        if map != self.map() {
            carried = Some(self.world.take_contents(AVATAR)); // the Avatar takes its belongings along
            self.world.remove(AVATAR);
            self.enter(map, keep);
        }
        (av.x, av.y, av.z, av.mapnum) = (x, y, z, map as u8);
        if carried.is_some() {
            av.flags &= !GUMP_OPEN; // the paperdoll closed on the way
        }
        self.world.put_actor(&self.data, av);
        self.world.set_in_combat(AVATAR, fighting);
        if let Some(mut items) = carried {
            items.iter_mut().for_each(|(_, _, it)| it.flags &= !GUMP_OPEN); // their windows closed
            self.world.take_contents(AVATAR); // nothing stale left from an earlier visit
            self.world.give_contents(&self.data, AVATAR, &items);
        }
        self.avatar = Avatar::new(&self.data, &self.world).map(|mut a| {
            a.dir = dir.unwrap_or(a.dir);
            a.arrive(&self.data, &mut self.world);
            a
        });
        self.hatcher.arrived_by_teleport();
        true
    }

    /// The next (or previous, for negative `step`) map with content.
    pub fn step_map(&mut self, step: isize) {
        let i = self.maps.iter().position(|&m| m == self.map()).unwrap_or(0) as isize;
        let next = self.maps[(i + step).rem_euclid(self.maps.len() as isize) as usize];
        self.load_map(next);
    }
}
