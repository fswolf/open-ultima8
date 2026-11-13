//! Running usecode: starting events on things, one tick of the scripts, and carrying out what
//! they ask of the game (walks, blows, sprites, teleports…).
use super::*;

impl Game {
    /// Run an item's event handler (e.g. [`event::LOOK`](crate::usecode::event::LOOK) on click,
    /// [`event::USE`](crate::usecode::event::USE) on double-click). False if it has none.
    pub fn trigger(&mut self, id: ObjId, ev: usize) -> bool {
        let Some(s) = &mut self.scripts else { return false };
        let host = WorldHost { world: &mut self.world, data: &self.data, stats: &mut self.stats };
        s.vm.item_event(&s.usecode, &host, id, ev).is_some()
    }

    /// Run `id`'s handler for `ev` with arguments (e.g. gotHit's striker and damage).
    pub fn trigger_with(&mut self, id: ObjId, ev: usize, args: &[u8]) -> bool {
        self.start_event(id, ev, args).is_some()
    }

    /// Run `id`'s handler for `ev`; its process, if it has one.
    pub(crate) fn start_event(&mut self, id: ObjId, ev: usize, args: &[u8]) -> Option<crate::usecode::Pid> {
        let s = self.scripts.as_mut()?;
        let it = self.world.get(id)?;
        let family = self.data.typeflags.get(it.shape).family;
        let class = crate::usecode::vm::event_class(&s.usecode, id, &it, family, ev);
        s.vm.call_event(&s.usecode, class, ev, id, args)
    }

    /// Call usecode intrinsic `num` with raw argument bytes (see [`Vm::call_intrinsic`]); what it
    /// asks of the game is carried out on the next update.
    pub fn call_intrinsic(&mut self, num: u16, args: &[u8]) -> u32 {
        let Some(s) = &mut self.scripts else { return 0 };
        let mut host = WorldHost { world: &mut self.world, data: &self.data, stats: &mut self.stats };
        s.vm.call_intrinsic(&s.usecode, &mut host, num, args)
    }

    /// Take what the scripts want shown.
    pub fn take_events(&mut self) -> Vec<UiEvent> {
        self.vm().map_or(vec![], |vm| vm.events.drain(..).collect())
    }

    /// One tick of the scripts, then what they asked for.
    pub(super) fn run_scripts(&mut self) {
        // Free NPC slots for scripts' createActor: other maps only change with the map, and the
        // script checks this one's; once a second is plenty.
        let stale = self.tick.is_multiple_of(30) || self.scripts.as_ref().is_some_and(|s| s.vm.free_npcs.is_empty());
        let free = stale.then(|| self.free_npcs());
        let minutes = self.minutes();
        let Some(s) = &mut self.scripts else { return };
        if let Some(f) = free {
            s.vm.free_npcs = f;
        }
        (s.vm.minutes, s.vm.clock) = (minutes, self.clock);
        s.vm.tick(&s.usecode, &mut WorldHost { world: &mut self.world, data: &self.data, stats: &mut self.stats });
        let asked: Vec<Request> = s.vm.requests.drain(..).collect();
        let mut done = vec![];
        for r in asked {
            match r {
                Request::Walk { pid, actor, to, range } => {
                    self.loiters.remove(&actor);
                    self.walks.retain(|w| w.actor != actor); // new orders replace old ones
                    self.walks.push(Walk::new(pid, actor, to, range));
                }
                Request::Loiter(id) => _ = self.loiters.insert(id, Default::default()),
                Request::CameraFollow(id) => done.extend(self.camera.set_center_on(&self.world, id)),
                Request::CameraScroll { pid, to } => done.extend(self.camera.scroll_to(&self.world, pid, to)),
                Request::CameraMove(to) => done.extend(self.camera.move_to(to)),
                Request::Hit { target, by, dir, damage, kind } => self.receive_hit(target, by, dir, damage, kind),
                Request::GrantPeace => self.cast_grant_peace(),
                Request::Fireball(at) => self.tonys_ball(at),
                Request::Moved(id) => self.note_footing(id),
                Request::Hatch(egg) => _ = self.hatch_monster(egg),
                Request::Grow(stat, n) => self.grow(stat, n),
                Request::Feed(food) => self.feed(food),
                Request::SetHour(h) => {
                    self.clock = h as u32 * TICKS_PER_MINUTE * 60;
                    self.scheduled = None; // everyone to where they should be now
                }
                Request::Sprite { pid, shape, first, last, repeats, delay, at } => {
                    self.start_sprite(pid, shape, (first, last), repeats, delay, at)
                }
                Request::Created(id) => self.made_actor(id),
                Request::Teleported(_) => self.redraw_sleepers(),
                Request::Hurl { id, v, gravity, pid } => self.hurl(id, v, gravity, Some(pid)),
                Request::Fall(id) => self.fall(id),
                Request::Explode { id, destroy } => self.explode(id, destroy),
                Request::Ignite { x, y } => self.ignite(x, y),
                // A new map: its scripts start afresh, the rest was for the old one.
                Request::TeleportToEgg { map, id } if self.teleport(map, id) => return,
                Request::TeleportToEgg { .. } => {}
                Request::AvatarTo { map, at, by, pid } => {
                    self.move_avatar(map, at, Some(by));
                    if let Some(s) = &mut self.scripts {
                        s.vm.finish(pid, 0); // on with the script, on the new map
                    }
                    return;
                }
            }
        }
        done.extend(self.camera.tick());
        if let Some(s) = &mut self.scripts {
            done.into_iter().for_each(|p| s.vm.finish(p, 0));
        }
    }
}
