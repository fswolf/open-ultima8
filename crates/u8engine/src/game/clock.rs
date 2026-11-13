//! Time and people's routines: the game clock, each NPC's hourly `schedule`, and walking them
//! where their scripts send them.
use super::*;

impl Game {
    /// Game time in minutes since the start of day one.
    pub fn minutes(&self) -> u32 {
        self.clock / TICKS_PER_MINUTE
    }

    /// Set the clock to `minutes` since the start of day one (people go where they should be then).
    pub fn set_minutes(&mut self, minutes: u32) {
        self.clock = minutes * TICKS_PER_MINUTE;
        self.scheduled = None;
    }

    /// Someone was moved by a script out of sight: things that show who's sleeping in them (only
    /// Mordea's bed) look again. Their `enterFastArea` sets the frame, but the scripts move people
    /// once the Avatar is 700 away, still within the fast area, so the bed would keep the old
    /// look until the Avatar went much further (the original's own glitch; wider views show it).
    pub(super) fn redraw_sleepers(&mut self) {
        const SLEEPERS: [u32; 1] = [567];
        let beds: Vec<ObjId> = self
            .world
            .objects()
            .filter(|&id| self.world.get(id).is_some_and(|it| SLEEPERS.contains(&it.shape)))
            .filter(|&id| self.fast.contains(&self.world, id))
            .collect();
        for id in beds {
            self.trigger(id, event::ENTER_FAST_AREA);
        }
    }

    /// Every game hour each NPC's `schedule` event decides where it should be (ScummVM's
    /// SchedulerProcess). The original runs it for everyone, wherever they are; we hold only this
    /// map's people, so on arriving where an hour has turned since they were last scheduled,
    /// theirs runs then.
    pub(super) fn schedule(&mut self) {
        let hour = self.minutes() / 60;
        let map = self.world.map();
        let here = self.map_scheduled.get(&map).copied().unwrap_or(self.loaded_scheduled);
        if self.scheduled == Some(hour) && here == self.scheduled {
            return;
        }
        self.scheduled = Some(hour);
        self.map_scheduled.insert(map, Some(hour));
        let npcs: Vec<ObjId> = self
            .world
            .objects()
            .filter(|&id| id > AVATAR && id < 256 && !self.stats.get(id).is_some_and(|s| s.dead))
            .filter(|&id| !self.world.in_combat(id)) // busy fighting
            .collect();
        for id in npcs {
            self.trigger(id, event::SCHEDULE);
        }
    }

    /// Step walking NPCs; tell their scripts when they arrive (or give up).
    pub(super) fn walk(&mut self) {
        let Some(s) = &mut self.scripts else { return };
        let world = &self.world;
        self.walks.retain(|w| {
            // Someone fighting goes where the fight takes them (see `fight`).
            let fighting = w.actor != AVATAR && world.in_combat(w.actor);
            if fighting {
                s.vm.finish(w.pid, 0);
            }
            !fighting
        });
        self.walks.retain_mut(|w| {
            if !s.vm.alive(w.pid) {
                w.stand(&self.data, &mut self.world); // its script was stopped
                return false;
            }
            match w.step(&self.data, &mut self.world) {
                None => true,
                Some(arrived) => {
                    s.vm.finish(w.pid, arrived as u32);
                    false
                }
            }
        });
    }
}
