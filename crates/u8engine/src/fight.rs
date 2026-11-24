//! People in combat stance fighting on their own (everyone but the Avatar), after ScummVM's
//! reading of the original's CombatProcess: go after the quarry a blow or a script named, else
//! the nearest enemy (by alignment) about; close in, turn to face it, swing when it's within the
//! attack's reach, and get one's breath back (longer the clumsier) before the next swing.
use crate::avatar::facing;
use crate::game::Game;
use crate::geom::dir_towards;
use crate::npc::Walk;
use crate::stats::FEIGN_TICKS;
use crate::usecode::{event, Pid};
use crate::world::{ObjId, AVATAR};
use u8data::anim::action;

/// How near (beyond touching) a fighter closing in walks to its quarry.
const CLOSE: i32 = 16;
/// How far (world units, either axis) someone in combat looks for an enemy.
const SEEK: i32 = 768;
/// Nobody to fight: look again after this many ticks.
const IDLE_TICKS: u32 = 30;
/// Closing in: plan the route again this often (the quarry moves).
const REPLAN_TICKS: u32 = 15;
/// A walk that got nowhere: wait this long before the next.
const STUCK_TICKS: u32 = 10;
/// Dexterity at which there's no pause between swings; each point below adds this many ticks.
const DEX_QUICK: i32 = 25;
const PAUSE_PER_DEX: i32 = 3;
/// A changeling's actions into a tree and back, and how near its quarry comes before it
/// springs (looked for every 10 ticks).
const SHIFT_AWAY: usize = 20;
const SHIFT_BACK: usize = 21;
const AMBUSH: i32 = 192;
const AMBUSH_TICKS: u32 = 10;
/// Shape of demons (ScummVM): like the undead, they aren't fooled by someone playing dead.
const DEMON: u32 = 96;

/// What a fighter is doing between ticks.
#[derive(Default)]
pub(crate) struct Fighter {
    dir: Option<usize>,
    wait: u32,
    walk: Option<Walk>,
    walked: u32,
    swinging: bool,
    struck: bool,
    /// The `cast` script run on going into combat: no fighting till it's done.
    cast: Option<Pid>,
    /// A changeling waiting in ambush as a tree.
    hidden: bool,
}

impl Game {
    /// One tick of everyone's fighting (only near the Avatar: elsewhere the world is still).
    pub(crate) fn fight(&mut self) {
        let fighters = self.world.fighters();
        self.fights.retain(|id, _| fighters.contains(id));
        if self.world.get(AVATAR).is_none() {
            return;
        }
        for id in fighters.into_iter().filter(|&id| id != AVATAR) {
            if self.stats.get(id).is_some_and(|s| s.dead) || self.world.is_void(id) {
                continue;
            }
            if !self.fights.contains_key(&id) {
                self.join_fight(id);
            }
            // (Only in the fast area, as the original's isInFastArea.)
            if self.world.is_fast(id) {
                let mut f = self.fights.remove(&id).unwrap_or_default();
                self.fight_tick(id, &mut f);
                self.fights.insert(id, f);
            }
        }
    }

    /// Into combat: whatever `id` was doing (its routine, its walk) stops and its `cast` script
    /// runs, however it got there (a blow, a call for help, setActivity, a hatched monster), once
    /// each time it goes in (ScummVM's setInCombatU8): guards shout "Guardsmen, to arms!",
    /// townsfolk cry for Beren, Theurgists flee. Fighting waits for it.
    pub(crate) fn join_fight(&mut self, id: ObjId) {
        self.stop_walking(id);
        self.loiters.remove(&id);
        if let Some(s) = &mut self.scripts {
            s.vm.kill_item_procs(id, crate::usecode::vm::PROC_TYPE_ALL, 0);
        }
        let cast = self.start_event(id, event::CAST, &[0, 0]);
        let dir = self.world.get(id).and_then(|it| facing(&self.data, &it));
        self.fights.insert(id, Fighter { dir, cast, ..Fighter::default() });
    }

    /// Back in a fight it was in when saved (or when the map was left): it carries on, without
    /// its going-into-combat script again (that ran when it went in).
    pub(crate) fn resume_fight(&mut self, id: ObjId) {
        let dir = self.world.get(id).and_then(|it| facing(&self.data, &it));
        self.fights.insert(id, Fighter { dir, ..Fighter::default() });
    }

    /// Stop walks the scripts ordered for `id` (telling them it didn't get there).
    pub(crate) fn stop_walking(&mut self, id: ObjId) {
        let stopped: Vec<_> = self.walks.iter().filter(|w| w.actor == id).map(|w| w.pid).collect();
        self.walks.retain(|w| w.actor != id);
        if let Some(s) = &mut self.scripts {
            stopped.into_iter().for_each(|pid| s.vm.finish(pid, 0));
        }
    }

    fn fight_tick(&mut self, id: ObjId, f: &mut Fighter) {
        if let Some(pid) = f.cast {
            if self.scripts.as_ref().is_some_and(|s| s.vm.alive(pid)) {
                return;
            }
            f.cast = None;
        }
        if let Some((act, dir, step)) = self.world.acting(id) {
            // Mid-swing: the blow lands on the frame that reaches.
            if f.swinging && act == action::ATTACK && !f.struck {
                let r = self.strike_range(id, dir, step);
                f.struck = r > 0 && self.swing(id, dir, r);
            }
            return;
        }
        if std::mem::take(&mut f.swinging) {
            if let Some(d) = f.dir {
                self.stance(id, d); // weapon back up
            }
        }
        if f.wait > 0 {
            f.wait -= 1;
            return;
        }
        let Some(t) = self.quarry(id) else {
            f.walk = None;
            f.wait = IDLE_TICKS;
            // Nobody about: half the time a changeling turns into a tree (not those at the
            // pentagram, map 43) and waits in ambush (ScummVM's waitForTarget).
            let shifter = self.monster(id).is_some_and(|m| m.shifter) && self.world.map() != 43;
            if shifter && !f.hidden && self.world.roll(2) == 0 {
                let dir = f.dir.unwrap_or(0);
                f.hidden = self.world.play(&self.data, id, SHIFT_AWAY, dir).is_some();
            }
            return;
        };
        if f.hidden {
            // Ambush: until the quarry comes close, then back into shape.
            let (a, b) = (self.world.get(id), self.world.get(t));
            let near = a.zip(b).is_some_and(|(a, b)| (a.x - b.x).abs().max((a.y - b.y).abs()) <= AMBUSH);
            if near {
                self.world.play(&self.data, id, SHIFT_BACK, f.dir.unwrap_or(0));
                f.hidden = false;
            } else {
                f.wait = AMBUSH_TICKS;
            }
            return;
        }
        let want = self.dir_to(id, t);
        if self.in_reach(id, want, t) {
            f.walk = None;
            match f.dir {
                Some(d) if d == want => {
                    if self.world.play(&self.data, id, action::ATTACK, want).is_some() {
                        (f.swinging, f.struck) = (true, false);
                        let dex = self.stats.get(id).map_or(10, |s| s.dex as i32);
                        f.wait = (PAUSE_PER_DEX * (DEX_QUICK - dex)).max(0) as u32;
                    } else {
                        f.wait = IDLE_TICKS; // can't fight
                    }
                }
                Some(d) => {
                    // Turn a step at a time, the shorter way.
                    let next = if (want + 8 - d) % 8 <= 4 { (d + 1) % 8 } else { (d + 7) % 8 };
                    self.stance(id, next);
                    f.dir = Some(next);
                }
                None => {
                    self.stance(id, want);
                    f.dir = Some(want);
                }
            }
            return;
        }
        // Close in, planning again as the quarry moves.
        f.walked += 1;
        if f.walk.is_none() || f.walked >= REPLAN_TICKS {
            let Some(to) = self.world.get(t).map(|it| (it.x, it.y, it.z)) else { return };
            // (Up to it, not onto its spot: big people's anchors can't come within a walk's
            // slack of each other, and a goal out of reach is no route at all.)
            let size = |i| {
                self.world.index(i).map(|e| self.world.entry(e).bounds()).map_or(0, |b| (b.x1 - b.x0).max(b.y1 - b.y0))
            };
            let range = size(id).max(size(t)) - crate::npc::SLACK + CLOSE;
            (f.walk, f.walked) = (Some(Walk::new(0, id, to, range)), 0);
        }
        let Some(w) = f.walk.as_mut() else { return };
        let done = w.step(&self.data, &mut self.world);
        f.dir = Some(w.dir());
        if done.is_some() {
            f.walk = None;
            f.wait = STUCK_TICKS;
        }
    }

    /// Who `id` goes after: its quarry while still about, else the nearest enemy (None: nobody).
    fn quarry(&mut self, id: ObjId) -> Option<ObjId> {
        let tg = self.world.target(id);
        if tg.now != 0 && self.fair_game(id, tg.now) {
            return Some(tg.now);
        }
        let t = if tg.fixed != 0 && self.fair_game(id, tg.fixed) { Some(tg.fixed) } else { self.seek(id) };
        self.world.retarget(id, t.unwrap_or(0));
        t
    }

    /// Can `id` fight `t`: someone else, alive (and not fooling the undead), in the fast area?
    fn fair_game(&self, id: ObjId, t: ObjId) -> bool {
        let Some(it) = self.world.get(t) else { return false };
        let Some(s) = self.stats.get(t).filter(|_| it.npc != 0 && t != id) else { return false };
        let sees_through = self.defence_type(id) & crate::combat::dmg::UNDEAD != 0
            || self.world.get(id).is_some_and(|a| a.shape == DEMON);
        !s.dead && !(s.feigning > 0 && sees_through) && self.world.is_fast(t)
    }

    /// The nearest enemy of `id` about.
    fn seek(&self, id: ObjId) -> Option<ObjId> {
        let me = self.world.get(id)?;
        let mine = self.stats.get(id)?;
        let enemy = |t: ObjId| self.stats.get(t).is_some_and(|s| mine.is_enemy(&s));
        let mut found: Vec<(i32, ObjId)> = self
            .world
            .area(me.x, me.y, SEEK)
            .into_iter()
            .filter(|&t| self.fair_game(id, t) && enemy(t))
            .filter_map(|t| self.world.get(t).map(|it| ((it.x - me.x).abs().max((it.y - me.y).abs()), t)))
            .collect();
        found.sort_unstable();
        found.first().map(|&(_, t)| t)
    }

    /// Facing from `id`'s middle to `t`'s.
    pub(crate) fn dir_to(&self, id: ObjId, t: ObjId) -> usize {
        let mid = |id: ObjId| {
            let w = &self.world;
            w.index(id).map(|i| w.entry(i).bounds()).map(|b| ((b.x0 + b.x1) / 2, (b.y0 + b.y1) / 2))
        };
        match (mid(id), mid(t)) {
            (Some(a), Some(b)) => dir_towards(b.0 - a.0, b.1 - a.1) as usize,
            _ => 0,
        }
    }

    /// Would a swing facing `dir` reach `t`? (Those who strike from afar always do.)
    fn in_reach(&self, id: ObjId, dir: usize, t: ObjId) -> bool {
        if self.monster(id).is_some_and(|m| m.ranged) {
            return true;
        }
        (1..=self.reach(id, dir)).any(|r| self.swing_target(id, dir, r) == Some(t))
    }

    /// How far `id`'s attack reaches facing `dir` (1 if its frames don't say).
    fn reach(&self, id: ObjId, dir: usize) -> i32 {
        let Some(it) = self.world.get(id) else { return 0 };
        let frames = self.data.anims.get(it.shape, action::ATTACK).and_then(|a| a.frames.get(dir));
        frames.map_or(0, |f| f.iter().map(|f| f.attack_range()).max().unwrap_or(0).max(1))
    }

    /// The reach of frame `step` of `id`'s attack (0: this frame doesn't strike). Attacks whose
    /// frames don't say strike once, halfway through.
    fn strike_range(&self, id: ObjId, dir: usize, step: usize) -> i32 {
        let Some(it) = self.world.get(id) else { return 0 };
        let Some(frames) = self.data.anims.get(it.shape, action::ATTACK).and_then(|a| a.frames.get(dir)) else {
            return 0;
        };
        if frames.iter().all(|f| f.attack_range() == 0) {
            return (step == frames.len() / 2) as i32;
        }
        frames.get(step).map_or(0, |f| f.attack_range())
    }

    /// Stand ready facing `dir` (the combat stance, if `id` has one).
    fn stance(&mut self, id: ObjId, dir: usize) {
        if self.world.play(&self.data, id, action::COMBAT_STAND, dir).is_none() {
            self.world.play(&self.data, id, action::STAND, dir);
        }
    }

    /// One tick of people faking death (Actor::setFeignDeath): having fallen, they get up.
    pub(crate) fn feign(&mut self) {
        for id in 1..256u16 {
            let Some(s) = self.stats.get_mut(id).filter(|s| s.feigning > 0) else { continue };
            s.feigning -= 1;
            if s.feigning == FEIGN_TICKS && !s.dead {
                let dir = self.world.get(id).and_then(|it| facing(&self.data, &it)).unwrap_or(0);
                self.world.play(&self.data, id, action::STAND_UP, dir);
            }
        }
    }
}
