//! People in combat stance fighting on their own (everyone but the Avatar), after ScummVM's
//! reading of the original's CombatProcess: go after the quarry a blow or a script named, else
//! the nearest enemy (by alignment) about; close in, turn to face it, swing when it's within the
//! attack's reach, and get one's breath back (longer the clumsier) before the next swing.
//! Changelings follow the original's own brain (COMBATBR.C): they hide as a tree when the Avatar
//! is far, and take the Avatar's shape to fight.
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
/// The changeling: its own shape, its actions into a tree and out, into the Avatar and back
/// (the original's Climb 3 to 6), and the shape it takes.
const CHANGELING: u32 = 357;
const INTO_TREE: usize = 20;
const OUT_OF_TREE: usize = 21;
const INTO_AVATAR: usize = 22;
const OUT_OF_AVATAR: usize = 23;
const AVATAR_SHAPE: u32 = 1;
/// From this far from the Avatar (either axis) a changeling hides as a tree rather than become
/// him; how near a tree looks for a victim, and a clone for a quarry.
const FAR: i32 = 0x800;
const TREE_LOOK: i32 = 160;
const CLONE_LOOK: i32 = 600;
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
    /// The wait is the pause after a blow, which runs during it.
    pause: bool,
    /// The `cast` script run on going into combat: no fighting till it's done.
    cast: Option<Pid>,
    /// A changeling (its shape when it went into combat): where it is in its changes, and the
    /// ticks left of being the Avatar and of being a tree.
    pub(crate) changeling: bool,
    pub(crate) morph: Morph,
    pub(crate) clone_wait: u32,
    pub(crate) tree_wait: u32,
}

/// Where a changeling is in its changes. The `Into` and `Out` ones are playing their action.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Morph {
    #[default]
    Itself,
    IntoClone,
    Clone,
    Back,
    IntoTree,
    Tree,
    OutOfTree,
}

impl Morph {
    pub(crate) const ALL: [Morph; 7] =
        [Morph::Itself, Morph::IntoClone, Morph::Clone, Morph::Back, Morph::IntoTree, Morph::Tree, Morph::OutOfTree];
    /// A tree is dead to the world (can't be struck, searched as a body), but still thinks.
    pub(crate) fn tree(self) -> bool {
        matches!(self, Morph::IntoTree | Morph::Tree)
    }
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
            let tree = self.fights.get(&id).is_some_and(|f| f.morph.tree());
            if self.stats.get(id).is_some_and(|s| s.dead) && !tree || self.world.is_void(id) {
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
        self.stop_loitering(id);
        if let Some(s) = &mut self.scripts {
            s.vm.kill_item_procs(id, crate::usecode::vm::PROC_TYPE_ALL, 0);
        }
        let cast = self.start_event(id, event::CAST, &[0, 0]);
        let dir = self.world.get(id).and_then(|it| facing(&self.data, &it));
        let changeling = self.world.get(id).is_some_and(|it| it.shape == CHANGELING);
        self.fights.insert(id, Fighter { dir, cast, changeling, ..Fighter::default() });
    }

    /// Back in a fight it was in when saved (or when the map was left): it carries on, without
    /// its going-into-combat script again (that ran when it went in).
    pub(crate) fn resume_fight(&mut self, id: ObjId) {
        let dir = self.world.get(id).and_then(|it| facing(&self.data, &it));
        let changeling = self.world.get(id).is_some_and(|it| it.shape == CHANGELING);
        self.fights.insert(id, Fighter { dir, changeling, ..Fighter::default() });
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
        // (The original's waits count on whatever is playing.)
        (f.clone_wait, f.tree_wait) = (f.clone_wait.saturating_sub(1), f.tree_wait.saturating_sub(1));
        // The pause after a blow runs from the swing's start (COMBATBR.C attackTarget).
        let waiting = f.wait > 0;
        if waiting && (f.pause || !self.world.is_acting(id)) {
            f.wait -= 1;
        }
        f.pause &= f.wait > 0;
        if self.stats.get(id).is_some_and(|s| s.stunned > 0) {
            return; // kicked senseless
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
        if waiting {
            return;
        }
        if f.changeling && self.changeling(id, f) {
            return;
        }
        let Some(t) = self.quarry(id) else {
            f.walk = None;
            f.wait = IDLE_TICKS;
            return;
        };
        let want = self.dir_to(id, t);
        if self.in_reach(id, want, t) {
            f.walk = None;
            match f.dir {
                Some(d) if d == want => {
                    if self.world.play(&self.data, id, action::ATTACK, want).is_some() {
                        (f.swinging, f.struck, f.pause) = (true, false, true);
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

    /// A changeling's turn to think (COMBATBR.C CombatBrain::process, with nothing playing). True
    /// if it's dealt with; false: fight as any creature does (it has a quarry). Back from being
    /// the Avatar once that has run its time; with a quarry, fight, unless in its own shape and a
    /// 1 in 8 roll, or far from the Avatar; in its own shape otherwise, a tree if far (not at the
    /// pentagram, map 43), else the Avatar for 5 to 25 seconds; as the Avatar with no quarry,
    /// look for one within 600 or wait about. It never looks for a quarry in its own shape.
    fn changeling(&mut self, id: ObjId, f: &mut Fighter) -> bool {
        match f.morph {
            Morph::IntoClone => {
                // (TypeChanger: the Avatar's shape, first frame, facing north.)
                self.reshape(id, AVATAR_SHAPE);
                (f.morph, f.dir) = (Morph::Clone, Some(0));
                return true;
            }
            // WaitForVictim: until something comes within reach, or its time is up.
            Morph::IntoTree | Morph::Tree => {
                f.morph = Morph::Tree;
                if f.tree_wait > 0 && self.held_target(id).is_none() && !self.select_target(id, TREE_LOOK) {
                    return true;
                }
                if let Some(s) = self.stats.get_mut(id) {
                    s.dead = false;
                }
                self.world.play(&self.data, id, OUT_OF_TREE, 0);
                (f.morph, f.dir) = (Morph::OutOfTree, Some(0));
                return true;
            }
            Morph::OutOfTree | Morph::Back => f.morph = Morph::Itself,
            Morph::Itself | Morph::Clone => {}
        }
        let (Some(me), Some(av)) = (self.world.get(id), self.world.get(AVATAR)) else { return true };
        let far = (me.x - av.x).abs().max((me.y - av.y).abs()) >= FAR;
        if me.shape == AVATAR_SHAPE && f.clone_wait == 0 {
            self.stop_all(id, f);
            self.reshape(id, CHANGELING);
            self.world.play(&self.data, id, OUT_OF_AVATAR, 0);
            (f.morph, f.dir) = (Morph::Back, Some(0));
            return true;
        }
        let held = self.held_target(id).is_some();
        if held && me.shape == CHANGELING && f.walk.is_some() && f.walked + 1 < REPLAN_TICKS {
            return false; // (still on its way: the original thinks when a walk ends)
        }
        if held && (me.shape != CHANGELING || self.world.roll(8) != 0 && !far) {
            return false;
        }
        if me.shape == CHANGELING {
            self.stop_all(id, f);
            f.dir = Some(0); // (its changes all play facing north)
            if far && self.world.map() != 43 {
                if let Some(s) = self.stats.get_mut(id) {
                    s.dead = true;
                }
                self.world.play(&self.data, id, INTO_TREE, 0);
                (f.morph, f.tree_wait) = (Morph::IntoTree, (25 + self.world.roll(21)) * 30);
            } else {
                self.world.play(&self.data, id, INTO_AVATAR, 0);
                (f.morph, f.clone_wait) = (Morph::IntoClone, (5 + self.world.roll(21)) * 30);
            }
            return true;
        }
        if self.select_target(id, CLONE_LOOK) {
            return false;
        }
        // (wander: the original also loiters half of these times; this waits instead.)
        f.wait = (2 + self.world.roll(2)) * 30;
        true
    }

    /// `id` takes `shape`, first frame, facing north (the original's setType and TypeChanger).
    fn reshape(&mut self, id: ObjId, shape: u32) {
        if let Some(it) = self.world.get(id) {
            self.world.set(&self.data, id, u8data::map::Item { shape, frame: 0, ..it });
            self.world.set_last_dir(id, 0);
        }
    }

    /// Everything `id` was up to but fighting stops (its walks, its scripts).
    fn stop_all(&mut self, id: ObjId, f: &mut Fighter) {
        f.walk = None;
        self.stop_walking(id);
        if let Some(s) = &mut self.scripts {
            s.vm.kill_item_procs(id, crate::usecode::vm::PROC_TYPE_ALL, 0);
        }
    }

    /// `id`'s quarry if it's still about, else the one it was first sent after; nobody, and both
    /// are forgotten (CombatBrain::hasTarget: never looks for anyone new).
    fn held_target(&mut self, id: ObjId) -> Option<ObjId> {
        let mut tg = self.world.target(id);
        let held = loop {
            if tg.now == 0 {
                tg.now = tg.fixed;
            }
            if tg.now == 0 {
                break None;
            }
            if self.fair_game(id, tg.now) {
                break Some(tg.now);
            }
            if tg.now == tg.fixed {
                tg = crate::world::Target::default();
                break None;
            }
            tg.now = tg.fixed;
        };
        self.world.restore_target(id, tg);
        held
    }

    /// The first enemy of `id` within `range` (either axis), in the search's order, but not the
    /// one it was first after: now its quarry (CombatBrain::selectTarget).
    fn select_target(&mut self, id: ObjId, range: i32) -> bool {
        let (Some(me), Some(mine)) = (self.world.get(id), self.stats.get(id)) else { return false };
        let fixed = self.world.target(id).fixed;
        let found = self
            .world
            .search_area((me.x, me.y), range, (0, 0), false)
            .into_iter()
            .find(|&t| t != fixed && self.fair_game(id, t) && self.stats.get(t).is_some_and(|s| mine.is_enemy(&s)));
        found.inspect(|&t| self.world.set_target(id, t)).is_some()
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

    /// One tick of people faking death (Actor::setFeignDeath): having fallen, they get up; and
    /// of the stunned coming round.
    pub(crate) fn feign(&mut self) {
        for id in 1..256u16 {
            let Some(s) = self.stats.get_mut(id) else { continue };
            s.stunned = s.stunned.saturating_sub(1);
            if s.feigning == 0 {
                continue;
            }
            s.feigning -= 1;
            if s.feigning == FEIGN_TICKS && !s.dead {
                let dir = self.world.get(id).and_then(|it| facing(&self.data, &it)).unwrap_or(0);
                self.world.play(&self.data, id, action::STAND_UP, dir);
            }
        }
    }
}
