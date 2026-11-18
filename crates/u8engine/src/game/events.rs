//! Running usecode: starting events on things, one tick of the scripts, and carrying out what
//! they ask of the game (walks, blows, sprites, teleports…).
use super::*;
use u8data::anim::action;

impl Game {
    /// Run an item's event handler (e.g. [`event::LOOK`](crate::usecode::event::LOOK) on click,
    /// [`event::USE`](crate::usecode::event::USE) on double-click). False if it has none.
    pub fn trigger(&mut self, id: ObjId, ev: usize) -> bool {
        let Some(s) = &mut self.scripts else { return false };
        let host = WorldHost {
            world: &mut self.world,
            data: &self.data,
            stats: &mut self.stats,
            others: Some(&mut self.visited),
        };
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
        let mut host = WorldHost {
            world: &mut self.world,
            data: &self.data,
            stats: &mut self.stats,
            others: Some(&mut self.visited),
        };
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
        s.vm.tick(
            &s.usecode,
            &mut WorldHost {
                world: &mut self.world,
                data: &self.data,
                stats: &mut self.stats,
                others: Some(&mut self.visited),
            },
        );
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
                Request::SetDead(id, dead) => _ = self.stats.get_mut(id).map(|s| s.dead = dead),
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
                Request::Teleported { id, by } => self.redraw_sleepers(id, by),
                Request::Fetch { id, at, pid } => {
                    self.fetch(id, at);
                    done.push(pid);
                }
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
        done.extend(self.camera.tick(&self.world));
        if let Some(s) = &mut self.scripts {
            done.into_iter().for_each(|p| s.vm.finish(p, 0));
        }
        self.gesture();
    }

    /// People talk with their hands while what they say, or ask, is up: the original plays their
    /// talk animation itself (it isn't in the scripts; seen in DOS on Devon and the castle guard,
    /// frames of action 53 throughout), and they stand again when it's done. Only when nothing
    /// else has them moving: a script's animation, a walk.
    fn gesture(&mut self) {
        let Some(s) = &self.scripts else { return };
        for e in &s.vm.events {
            if let UiEvent::Bark { pid, item, .. } | UiEvent::Ask { pid, item, .. } = *e {
                if (2..256).contains(&item) {
                    self.talkers.push((pid, item));
                }
            }
        }
        let alive = |pid| self.scripts.as_ref().is_some_and(|s| s.vm.alive(pid));
        let (talking, done): (Vec<_>, Vec<_>) = self.talkers.iter().copied().partition(|&(pid, _)| alive(pid));
        self.talkers = talking;
        let speaking = |id| self.talkers.iter().any(|&(_, t)| t == id);
        // (Back to standing the way they were facing, if the talking was still theirs; but the
        // talk stays their last action, as the original's saves have it (Devon and Mordea keep
        // 53 long after speaking): class 1419 then has the accuser say "That person..." without
        // turning, as DOS does, where a last action of standing gives "This person...".)
        let facing = |w: &crate::world::World, gd: &GameData, id| {
            w.last_dir(id).or_else(|| w.get(id).and_then(|it| crate::avatar::facing(gd, &it))).unwrap_or(0)
        };
        for (_, id) in done.into_iter().filter(|&(_, id)| !speaking(id)) {
            let acting = self.world.acting(id).map(|a| a.0);
            if self.world.last_anim(id) == action::TALK && acting.is_none_or(|a| a == action::TALK) {
                self.world.play(&self.data, id, action::STAND, facing(&self.world, &self.data, id));
                self.world.set_last_anim(id, action::TALK);
            }
        }
        let quiet: Vec<ObjId> = self.talkers.iter().map(|&(_, id)| id).collect();
        for id in quiet {
            let busy = self.world.is_acting(id) || self.walks.iter().any(|w| w.actor == id);
            let able = self.world.get(id).is_some_and(|it| self.data.anims.get(it.shape, action::TALK).is_some())
                && !self.world.in_combat(id)
                && self.stats.get(id).is_none_or(|s| !s.dead);
            if !busy && able {
                let dir = facing(&self.world, &self.data, id);
                self.world.play(&self.data, id, action::TALK, dir);
            }
        }
    }
}
