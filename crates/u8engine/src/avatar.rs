//! Player control of the Avatar: walking and running with collision, jumps that follow ANIM.DAT,
//! climbing onto ledges, hanging from them, being knocked flat by a fall, and the combat stance (readying the weapon, advancing, retreating,
//! swinging, kicking, blocking) with the wielded weapon drawn over the Avatar (WPNOVLAY.DAT).
use crate::equip::{self, Slot};
use crate::geom::DIRS;
use crate::world::{item_flag::FLIPPED, Overlay, World, AVATAR, STEP};
use u8data::{anim::action, anim::AnimFrame, map::Item, GameData};

mod acts;

/// Movement speed in world units per second (a floor tile is 128).
const WALK_SPEED: f32 = 160.0;
/// Careful steps (the short arrow): DOS covers ~45 units a second (57 in 1.5 s, 141 in 3 s).
const CAREFUL_SPEED: f32 = 45.0;
const RUN_SPEED: f32 = 320.0;
/// Seconds per animation frame.
const WALK_FRAME: f32 = 0.066;
const RUN_FRAME: f32 = 0.050;
const JUMP_FRAME: f32 = 0.045;
/// Readying, putting away and swinging a weapon.
const ACT_FRAME: f32 = 0.06;
/// Stepping in combat stance.
const COMBAT_SPEED: f32 = 96.0;

/// What the player wants this frame. The host clears it while the game is waiting on the player
/// (a conversation, a book) so the Avatar doesn't wander off.
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    /// Facing 0–7 to move towards.
    pub dir: Option<usize>,
    pub run: bool,
    /// Walk without stepping off ledges (the original's short-arrow "careful step").
    pub careful: bool,
    /// Start a jump (edge-triggered: set for one update).
    pub jump: bool,
    /// Draw or put away the weapon (edge-triggered).
    pub combat: bool,
    /// Swing, in combat stance (edge-triggered).
    pub attack: bool,
    /// Kick, in combat stance (edge-triggered).
    pub kick: bool,
    /// Block, in combat stance (held).
    pub block: bool,
}

/// Exertion that builds the Avatar's strength and dexterity (the game turns it into growth).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effort {
    /// Climbed onto something: how many 8-unit steps above 16.
    Climb(u32),
    /// A swing or a kick.
    Strike,
}

/// A one-off animation played to its end: a jump, readying the weapon, a swing.
#[derive(Clone, Copy, Debug)]
struct Act {
    action: usize,
    step: usize,
    /// Forward world units per airborne frame.
    fwd: i32,
    wait: f32,
    /// Seconds per frame.
    pace: f32,
    /// This swing already struck something (once per swing).
    struck: bool,
    /// A leap stopped going forward: it ends with the hop it's in.
    stopped: bool,
    /// How it carries the Avatar along.
    moves: Moves,
    /// The frames it plays: all, or one step of a two-step action (a running jump is one hop).
    from: usize,
    to: usize,
}

/// How an action moves the Avatar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Moves {
    /// `fwd` units a frame while airborne (jumps), with collision.
    Fixed,
    /// As ANIM.DAT says (rise, and 4 × the forward step), the path checked before it began
    /// (climbing).
    Checked,
    /// As ANIM.DAT says, on the ground, as far as the way is clear (knocked flat, getting up).
    Ground,
}

impl Act {
    fn new(action: usize, fwd: i32, pace: f32) -> Self {
        Self {
            action,
            step: 0,
            fwd,
            wait: 0.0,
            pace,
            struck: false,
            stopped: false,
            moves: Moves::Fixed,
            from: 0,
            to: usize::MAX,
        }
    }
    fn moving(action: usize, moves: Moves) -> Self {
        Self { moves, ..Self::new(action, 0, ACT_FRAME) }
    }
}

/// What a save can't show of the Avatar (the side file keeps it): hanging from a ledge, or down
/// after a fall (it gets up again on load).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plight {
    Hanging,
    Down,
}

/// Hands reach this far above the feet for a ledge to hang from (the pull-up's rise), and it's
/// looked for this far either side of where the falling Avatar is.
const GRAB_SLACK: i32 = 8;

#[derive(Clone, Debug)]
pub struct Avatar {
    pub dir: usize,
    step: usize,
    moving: bool,
    /// Fractional world units carried between updates.
    carry: f32,
    /// Seconds until the next walk-cycle frame.
    wait: f32,
    act: Option<Act>,
    /// In combat stance (mirrors the world's flag, which scripts can change too).
    combat: bool,
    /// The action and step last shown (the weapon overlay follows it).
    pose: (usize, usize),
    /// A swing reaching out this update: (facing, range in steps), for the game to resolve.
    swing: Option<(usize, i32)>,
    /// Exertion since last asked (see [`Effort`]).
    effort: Option<Effort>,
    /// Holding a block (the weapon up till the button's let go).
    blocking: bool,
    /// Hanging from a ledge by the hands (jump or step towards it: pull up; away: let go).
    hanging: bool,
    /// Knocked flat by a fall, till it's up again (easier to hit meanwhile).
    stunned: bool,
    /// Actions to play when this one's done (getting up after a fall).
    then: Vec<usize>,
    /// Over nothing (off an edge, let go, a jump into a pit): the game lets it fall.
    drop: bool,
}

/// (frame, mirrored?) of an actor animation step.
pub fn anim_frame(gd: &GameData, shape: u32, act: usize, dir: usize, step: usize) -> Option<(u32, bool)> {
    let frames = gd.anims.get(shape, act)?.frames.get(dir)?;
    let f = frames.get(step % frames.len().max(1))?;
    Some((f.frame, f.flags & AnimFrame::FLIPPED != 0))
}

/// Guess an actor's facing from its current frame (and whether it's mirrored: opposite facings
/// share pictures) by finding it in the stand, walk, run, combat-stance or talk frames.
pub fn facing(gd: &GameData, it: &Item) -> Option<usize> {
    facing_by(gd, it, true).or_else(|| facing_by(gd, it, false))
}

/// [`facing`], with the mirror flag matched (`exact`) or not.
pub fn facing_by(gd: &GameData, it: &Item, exact: bool) -> Option<usize> {
    let flip = it.flags & FLIPPED != 0;
    [action::STAND, action::WALK, action::RUN, action::COMBAT_STAND, action::TALK].iter().find_map(|&act| {
        gd.anims.get(it.shape, act)?.frames.iter().position(|d| {
            d.iter().any(|f| f.frame == it.frame && (!exact || (f.flags & AnimFrame::FLIPPED != 0) == flip))
        })
    })
}

fn show(it: &mut Item, (frame, flip): (u32, bool)) {
    it.frame = frame;
    it.flags = if flip { it.flags | FLIPPED } else { it.flags & !FLIPPED };
}

impl Avatar {
    pub fn new(gd: &GameData, world: &World) -> Option<Self> {
        let it = world.get(world.avatar()?)?;
        let saved = gd.npcdata.as_deref().and_then(|d| u8data::map::npc_dir(d, AVATAR as usize));
        let dir = facing(gd, &it).or(saved).unwrap_or(3);
        let combat = world.in_combat(AVATAR);
        let pose = (if combat { action::COMBAT_STAND } else { action::STAND }, 0);
        Some(Self {
            dir,
            step: 0,
            moving: false,
            carry: 0.0,
            wait: 0.0,
            act: None,
            combat,
            pose,
            swing: None,
            effort: None,
            blocking: false,
            hanging: false,
            stunned: false,
            then: vec![],
            drop: false,
        })
    }

    /// The action it's showing (walking, a swing, the combat stance…).
    pub fn action(&self) -> usize {
        self.pose.0
    }

    /// Walking or mid-animation: the host should keep frames coming.
    pub fn busy(&self) -> bool {
        self.moving || self.act.is_some()
    }

    /// Holding a block.
    pub fn blocking(&self) -> bool {
        self.blocking
    }

    /// Hanging from a ledge.
    pub fn hanging(&self) -> bool {
        self.hanging
    }

    /// Knocked flat, not up yet.
    pub fn stunned(&self) -> bool {
        self.stunned
    }

    /// Hanging or down, if it is.
    pub fn plight(&self) -> Option<Plight> {
        if self.hanging {
            Some(Plight::Hanging)
        } else {
            self.stunned.then_some(Plight::Down)
        }
    }

    /// Back as it was saved, facing `dir`: hanging where it is, or getting up (the fall was
    /// already taken).
    pub fn restore(&mut self, gd: &GameData, world: &mut World, dir: usize, p: Plight) {
        self.dir = dir % 8;
        match p {
            Plight::Hanging => {
                self.hanging = true;
                self.hold_frame(gd, world);
            }
            Plight::Down => {
                self.knock_down();
                self.act = Some(Act::moving(action::STAND_UP, Moves::Ground));
                self.then = vec![action::SHAKE_HEAD, action::SHAKE_HEAD];
            }
        }
    }

    /// Has it gone over nothing since last asked? (The game drops it, and a long way down hurts.)
    pub fn take_drop(&mut self) -> bool {
        std::mem::take(&mut self.drop)
    }

    /// A hard fall: flat on its back (sliding back as it goes), then up, shaking its head twice
    /// (ScummVM: fallBackwards, then standUpIfNeeded).
    pub fn knock_down(&mut self) {
        (self.hanging, self.blocking, self.moving, self.stunned) = (false, false, false, true);
        self.act = Some(Act::moving(action::FALL_BACKWARDS, Moves::Ground));
        self.then = vec![action::STAND_UP, action::SHAKE_HEAD, action::SHAKE_HEAD];
    }

    /// Exertion since last asked (climbing, swinging, kicking), if any.
    pub fn take_effort(&mut self) -> Option<Effort> {
        self.effort.take()
    }

    /// The swing reaching out since last asked (a striking frame), if any.
    pub fn take_swing(&mut self) -> Option<(usize, i32)> {
        self.swing.take()
    }

    /// Struck: stagger back (unless busy with something else, blocking or hanging on).
    pub fn hurt(&mut self) {
        if self.act.is_none() && !self.blocking && !self.hanging {
            self.act = Some(Act::new(action::STUMBLE, 0, ACT_FRAME));
            self.moving = false;
        }
    }

    /// A blow caught while blocking: the weapon jolts and comes back up.
    pub fn parry(&mut self) {
        if self.blocking && self.act.is_none() {
            self.act = Some(Act::new(action::START_BLOCK, 0, ACT_FRAME));
        }
    }

    /// The swing struck something: its later frames don't strike again.
    pub fn struck(&mut self) {
        if let Some(a) = &mut self.act {
            a.struck = true;
        }
    }

    /// What it stands in: the combat stance or at ease.
    fn stance(&self) -> usize {
        if self.combat {
            action::COMBAT_STAND
        } else {
            action::STAND
        }
    }

    /// Advance by `dt` seconds. Returns true if the Avatar moved or changed frame.
    pub fn update(&mut self, gd: &GameData, world: &mut World, input: &Input, dt: f32) -> bool {
        let mut changed = false;
        if world.in_combat(AVATAR) != self.combat {
            // A script put the Avatar in or out of combat.
            self.combat = !self.combat;
            if self.act.is_none() {
                changed |= self.stand(gd, world);
            }
        }
        if self.hanging && self.act.is_none() {
            changed |= self.update_hanging(gd, world, input);
            if self.hanging {
                changed |= self.hold_frame(gd, world);
            }
            self.hold(gd, world);
            return changed;
        }
        if self.act.is_none() {
            if self.blocking && !(input.block && self.combat && input.dir.is_none()) {
                // Let go (or moving, or out of combat): the weapon comes down first.
                self.blocking = false;
                self.act = Some(Act::new(action::STOP_BLOCK, 0, ACT_FRAME));
            } else if input.combat {
                self.combat = !self.combat;
                world.set_in_combat(AVATAR, self.combat);
                let a = if self.combat { action::READY_WEAPON } else { action::UNREADY_WEAPON };
                self.act = Some(Act::new(a, 0, ACT_FRAME));
                self.moving = false;
            } else if (input.attack || input.kick) && self.combat {
                let a = if input.attack { action::ATTACK } else { action::KICK };
                self.act = Some(Act::new(a, 0, ACT_FRAME));
                self.moving = false;
                self.effort = Some(Effort::Strike);
            } else if input.block && self.combat && input.dir.is_none() {
                self.blocking = true;
                self.act = Some(Act::new(action::START_BLOCK, 0, ACT_FRAME));
                self.moving = false;
            } else if input.jump {
                // Running leap if already running, forward jump if moving or aimed, otherwise straight up.
                if let Some(d) = input.dir {
                    self.dir = d;
                }
                // From a standstill, onto whatever is in front if it's in reach: the lowest climb
                // that fits (ScummVM's reading of the original).
                let climb = (!self.moving)
                    .then(|| {
                        (action::CLIMB_16..=action::CLIMB_72)
                            .find(|&a| world.anim_fits(gd, AVATAR, a, self.dir).is_some())
                    })
                    .flatten();
                if let Some(a) = climb {
                    self.act = Some(Act::moving(a, Moves::Checked));
                    self.effort = Some(Effort::Climb((a - action::CLIMB_16) as u32));
                } else {
                    let (a, fwd) = match (self.moving, input.dir.is_some(), input.run) {
                        (true, _, true) => (action::RUNNING_JUMP, 24),
                        (true, _, _) | (_, true, _) => (action::JUMP, 16),
                        _ => (action::JUMP_UP, 0),
                    };
                    let mut act = Act::new(a, fwd, JUMP_FRAME);
                    if a == action::RUNNING_JUMP {
                        // A two-step action, as running: one hop, off the foot after the one the
                        // run is on (ScummVM's AnimationTracker plays half of it each time).
                        let len = |a| {
                            world
                                .get(AVATAR)
                                .and_then(|it| gd.anims.get(it.shape, a))
                                .map_or(1, |a| a.frames[0].len().max(1))
                        };
                        let (run, jump) = (len(action::RUN), len(a));
                        let second = self.step % run < run / 2;
                        (act.from, act.to) = if second { (jump / 2, jump) } else { (0, jump / 2) };
                    }
                    self.act = Some(act);
                }
                self.moving = false;
            }
        }
        changed |= if self.act.is_some() {
            self.update_act(gd, world, dt)
        } else if input.dir.is_some() || self.moving {
            self.update_walk(gd, world, input, dt)
        } else {
            false
        };
        self.hold(gd, world);
        changed
    }

    /// Turn to `dir` where it stands (ScummVM's doAnim(stand, dir) on a throw; DOS on any drop),
    /// unless it's busy (mid-action, walking, blocking, hanging).
    pub fn turn(&mut self, gd: &GameData, world: &mut World, dir: usize) -> bool {
        if self.act.is_some() || self.moving || self.blocking || self.hanging || self.dir == dir % 8 {
            return false;
        }
        self.dir = dir % 8;
        self.stand(gd, world);
        self.hold(gd, world);
        true
    }

    /// Just arrived somewhere (a teleport): standing, whatever it was doing (a jump into the
    /// teleporter stays no frozen leap), on the floor if that's within a step.
    pub(crate) fn arrive(&mut self, gd: &GameData, world: &mut World) {
        self.stand(gd, world);
        if let (Some(mut it), Some(z)) = (world.get(AVATAR), world.floor_under(AVATAR)) {
            if (z - it.z).abs() <= STEP {
                it.z = z;
                world.set(gd, AVATAR, it);
            }
        }
        self.hold(gd, world);
    }

    /// Show the standing frame (at ease or in combat stance).
    fn stand(&mut self, gd: &GameData, world: &mut World) -> bool {
        let Some(mut it) = world.get(AVATAR) else { return false };
        self.pose(gd, &mut it, self.stance(), 0);
        world.set(gd, AVATAR, it);
        true
    }

    /// Show frame `step` of `act`.
    fn pose(&mut self, gd: &GameData, it: &mut Item, act: usize, step: usize) {
        if let Some(f) = anim_frame(gd, it.shape, act, self.dir, step) {
            show(it, f);
            self.pose = (act, step);
        }
    }

    /// Draw the wielded weapon where WPNOVLAY.DAT puts it for the pose (actions without a
    /// weapon in hand, like walking at ease, have none; nor does a frame a script set).
    fn hold(&self, gd: &GameData, world: &mut World) {
        let (act, step) = self.pose;
        let posed =
            world.get(AVATAR).and_then(|a| anim_frame(gd, a.shape, act, self.dir, step).filter(|f| f.0 == a.frame));
        let (Some((_, flip)), true) = (posed, gd.overlays.has(act)) else {
            world.set_overlay(AVATAR, None); // (cheap: no looking for the weapon)
            return;
        };
        let weapon = equip::worn(gd, world, AVATAR, Slot::Weapon).and_then(|w| world.get(w));
        let o = weapon.and_then(|w| equip::weapon_overlay(gd, w.shape)).and_then(|(shape, kind)| {
            let f = gd.overlays.get(act, kind, self.dir, step)?;
            Some(Overlay { shape, frame: f.frame, dx: if flip { -f.dx } else { f.dx }, dy: f.dy, flip })
        });
        world.set_overlay(AVATAR, o);
    }

    fn update_walk(&mut self, gd: &GameData, world: &mut World, input: &Input, dt: f32) -> bool {
        let Some(was) = world.get(AVATAR) else { return false };
        let mut it = was;
        let mut moving = false;
        if self.combat && input.dir.is_some() {
            if input.run {
                // Running puts the weapon away.
                self.combat = false;
                world.set_in_combat(AVATAR, false);
            } else {
                return self.update_combat_step(gd, world, input, dt);
            }
        }
        if let Some(d) = input.dir {
            self.dir = d;
            let speed = match (input.run, input.careful) {
                (true, _) => RUN_SPEED,
                (false, true) => CAREFUL_SPEED,
                (false, false) => WALK_SPEED,
            };
            self.carry += dt * speed;
            let n = self.carry as i32;
            self.carry -= n as f32;
            if n == 0 {
                moving = self.moving; // sub-unit update: keep walking state
            } else {
                let (dx, dy) = (DIRS[d].0 * n, DIRS[d].1 * n);
                // The full step, else slide along either axis.
                if let Some((x, y, z)) =
                    [(dx, dy), (dx, 0), (0, dy)].iter().filter(|t| **t != (0, 0)).find_map(|&(tx, ty)| {
                        world.try_move(AVATAR, tx, ty).filter(|&(_, _, z)| !input.careful || z >= it.z - STEP)
                    })
                {
                    // Off an edge, more than a step down: over it, to fall.
                    let over = (z < it.z - STEP).then(|| world.try_move_air(AVATAR, x - it.x, y - it.y, 0)).flatten();
                    self.drop = over.is_some();
                    (it.x, it.y, it.z) = over.unwrap_or((x, y, z));
                    moving = true;
                }
            }
        } else {
            self.carry = 0.0;
        }
        let act = match (moving, input.run) {
            (false, _) => self.stance(),
            (true, false) => action::WALK,
            (true, true) => action::RUN,
        };
        if !moving {
            self.step = 0;
            self.wait = 0.0;
        } else {
            self.wait -= dt;
            if self.wait <= 0.0 {
                self.step += 1;
                // (Careful steps animate as slowly as they go.)
                self.wait = match (input.run, input.careful) {
                    (true, _) => RUN_FRAME,
                    (false, true) => WALK_FRAME * WALK_SPEED / CAREFUL_SPEED,
                    (false, false) => WALK_FRAME,
                };
            }
        }
        self.pose(gd, &mut it, act, self.step);
        self.moving = moving;
        let changed = (was.x, was.y, was.z, was.frame, was.flags) != (it.x, it.y, it.z, it.frame, it.flags);
        if changed {
            world.set(gd, AVATAR, it);
        }
        changed
    }

    /// In combat stance: towards where it faces is an advance, away a retreat (still facing
    /// forward); any other way turns to face it.
    fn update_combat_step(&mut self, gd: &GameData, world: &mut World, input: &Input, dt: f32) -> bool {
        let (Some(d), Some(was)) = (input.dir, world.get(AVATAR)) else { return false };
        let mut it = was;
        let back = (self.dir + 4) % 8;
        let act = if d == self.dir {
            action::ADVANCE
        } else if d == back {
            action::RETREAT
        } else {
            self.dir = d;
            self.moving = false;
            self.pose(gd, &mut it, action::COMBAT_STAND, 0);
            let turned = (was.frame, was.flags) != (it.frame, it.flags);
            if turned {
                world.set(gd, AVATAR, it);
            }
            return turned;
        };
        self.carry += dt * COMBAT_SPEED;
        let n = self.carry as i32;
        self.carry -= n as f32;
        if n > 0 {
            let (dx, dy) = (DIRS[d].0 * n, DIRS[d].1 * n);
            if let Some((x, y, z)) = world.try_move(AVATAR, dx, dy).filter(|&(_, _, z)| z >= it.z - STEP) {
                (it.x, it.y, it.z) = (x, y, z);
            }
        }
        if !self.moving || self.pose.0 != act {
            self.step = 0;
            self.wait = WALK_FRAME;
        } else {
            self.wait -= dt;
            if self.wait <= 0.0 {
                self.step += 1;
                self.wait = WALK_FRAME;
            }
        }
        self.moving = true;
        self.pose(gd, &mut it, act, self.step);
        world.set(gd, AVATAR, it);
        (was.x, was.y, was.z, was.frame, was.flags) != (it.x, it.y, it.z, it.frame, it.flags)
    }
}
