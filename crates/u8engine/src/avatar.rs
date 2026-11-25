//! Player control of the Avatar, as the original drives him (WORLD.C, ANIM.C): every move is an
//! animation from ANIM.DAT played at its own pace, walking, running and careful steps covering
//! what their frames say; turning about with a look over the shoulder; teetering at an edge;
//! jumps (up, onto things, forward, to the pointer), running leaps and bumping into walls; the
//! combat stance (readying the weapon, advancing, retreating, swinging, kicking, blocking) with
//! the wielded weapon drawn over him (WPNOVLAY.DAT); hanging from a ledge caught in a fall.
use crate::equip::{self, Slot};
use crate::geom::DIRS;
use crate::world::{anim_range, item_flag::FLIPPED, ObjId, Overlay, World, AVATAR, STEP};
use std::collections::VecDeque;
use u8data::{anim::action, anim::AnimFrame, map::Item, GameData};

mod acts;

/// What the player wants this frame. The host clears it while the game is waiting on the player
/// (a conversation, a book) so the Avatar doesn't wander off.
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    /// Facing 0–7 to move towards: the move button (the right one) is held.
    pub dir: Option<usize>,
    /// The pointer far off (run) or close by (careful steps); else a walk.
    pub run: bool,
    pub careful: bool,
    /// Jump (both buttons; edge-triggered: set for one update).
    pub jump: bool,
    /// Where the pointer is on the thing under it: a standing jump leaps there if it can.
    pub aim: Option<Aim>,
    /// Draw or put away the weapon (edge-triggered).
    pub combat: bool,
    /// Swing, in combat stance (edge-triggered).
    pub attack: bool,
    /// Kick, in combat stance (edge-triggered).
    pub kick: bool,
    /// Block, in combat stance (held).
    pub block: bool,
}

/// A point on a thing in the world (what the pointer is over).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Aim {
    pub at: (i32, i32, i32),
    pub on: ObjId,
}

/// Exertion that builds the Avatar's strength and dexterity (the game turns it into growth).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effort {
    /// Climbed onto something: how many 8-unit steps above 16.
    Climb(u32),
    /// A swing or a kick.
    Strike,
}

/// ANIM.DAT action flags: stops for nothing (a standing jump); running into things with it
/// hurts 1 time in 3 (running), or always.
const UNSTOPPABLE: u16 = 0x08;
const BUMPS: u16 = 0x10;
const HURTS: u16 = 0x20;
/// The last action, after one that ran into something (the original's 61).
const FAILED: usize = 61;
/// Climbing 40 up: a ledge caught in a fall has to be one this would take him onto.
const CLIMB_40: usize = action::CLIMB_16 + 3;

/// An action being played: its frames in order, each shown for its pace.
#[derive(Clone, Debug)]
struct Act {
    action: usize,
    dir: usize,
    flags: u16,
    frames: Vec<usize>,
    /// Frames shown so far.
    step: usize,
    /// Seconds till the next frame.
    wait: f32,
    pace: f32,
    /// This swing already struck something (once per swing).
    struck: bool,
    /// A standing jump met something: it goes on up and down where it is.
    stopped: bool,
    /// A careful step over the edge he teetered at (no sliding along it).
    over: bool,
    /// A jump aimed at a point (ANIMCLAS.C's StandingLeap).
    leap: Option<Leap>,
}

#[derive(Clone, Copy, Debug)]
struct Leap {
    from: (i32, i32),
    to: (i32, i32),
    /// Its rise or drop, and the forward amount of all its frames.
    rise: i32,
    total: i32,
}

/// A frame's move as it's drawn: he's there at once (as the original moves him), but drawn
/// going there over the time the frame shows.
#[derive(Clone, Copy, Debug)]
struct Glide {
    from: (i32, i32, i32),
    to: (i32, i32, i32),
    t: f32,
    pace: f32,
}

/// What a save can't show of the Avatar (the side file keeps it): hanging from a ledge, or down
/// after a fall (it gets up again on load).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plight {
    Hanging,
    Down,
}

/// Hands reach this far above the feet for a ledge to hang from (the pull-up's rise).
const GRAB_SLACK: i32 = 8;

#[derive(Clone, Debug)]
pub struct Avatar {
    pub dir: usize,
    act: Option<Act>,
    /// Actions to play next (action, facing: None for whichever way he faces then).
    then: VecDeque<(usize, Option<usize>)>,
    /// The last action begun (the original's lastAnimSet), and the way it faced.
    last: (usize, usize),
    /// A two-step action plays its first half next.
    first: bool,
    glide: Option<Glide>,
    /// In combat stance (mirrors the world's flag, which scripts can change too).
    combat: bool,
    /// Out of the stance to run, back in when he stops.
    pending: bool,
    /// The action and step last shown (the weapon overlay follows it).
    pose: (usize, usize),
    /// A swing reaching out this update: (facing, range in steps), for the game to resolve.
    swing: Option<(usize, i32)>,
    /// Exertion since last asked (see [`Effort`]).
    effort: Option<Effort>,
    /// Holding a block (the weapon up till the button's let go).
    blocking: bool,
    /// Hanging from a ledge by the hands.
    hanging: bool,
    /// Teetering at an edge: the next careful step that way goes over.
    balancing: bool,
    /// Knocked flat by a fall, till he's up.
    down: bool,
    /// The next careful step goes over the edge he teetered at.
    over: bool,
    /// Stopped by a blow: is he still on something?
    stopped: bool,
    /// The move button was held last update.
    held: bool,
    /// A jump at the pointer to make: where to.
    aimed: Option<(i32, i32, i32)>,
    /// Over nothing (off an edge, let go, a jump into a pit): the game lets it fall.
    drop: bool,
    /// Ran into something: hurt for sure, or 1 in 3 (the game decides).
    bump: Option<bool>,
    /// Sounds of the frames shown since last asked (a footstep: running?).
    sounds: Vec<(u16, Option<bool>)>,
    /// Strength, which limits how far a jump at the pointer goes (the game keeps it current).
    pub strength: i32,
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

/// How far the pointer is (U8POINT.C): close (careful steps), mid (a walk), far (a run).
fn erection(input: &Input) -> u8 {
    match (input.careful, input.run) {
        (true, _) => 1,
        (_, true) => 3,
        _ => 2,
    }
}

impl Avatar {
    pub fn new(gd: &GameData, world: &World) -> Option<Self> {
        let it = world.get(world.avatar()?)?;
        let saved = gd.npcdata.as_deref().and_then(|d| u8data::map::npc_dir(d, AVATAR as usize));
        let dir = facing(gd, &it).or(saved).unwrap_or(3);
        let combat = world.in_combat(AVATAR);
        let stand = if combat { action::COMBAT_STAND } else { action::STAND };
        Some(Self {
            dir,
            act: None,
            then: VecDeque::new(),
            last: (stand, dir),
            first: false,
            glide: None,
            combat,
            pending: false,
            pose: (stand, 0),
            swing: None,
            effort: None,
            blocking: false,
            hanging: false,
            balancing: false,
            down: false,
            over: false,
            stopped: false,
            held: false,
            aimed: None,
            drop: false,
            bump: None,
            sounds: vec![],
            strength: 0,
        })
    }

    /// The action it's showing (walking, a swing, the combat stance…).
    pub fn action(&self) -> usize {
        self.pose.0
    }

    /// The last action begun, as the original keeps it (61 after one that ran into something).
    pub fn last_action(&self) -> usize {
        self.last.0
    }

    /// Playing an action or with more to play: the host should keep frames coming.
    pub fn busy(&self) -> bool {
        self.act.is_some() || !self.then.is_empty() || self.glide.is_some()
    }

    /// Holding a block.
    pub fn blocking(&self) -> bool {
        self.blocking
    }

    /// Hanging from a ledge.
    pub fn hanging(&self) -> bool {
        self.hanging
    }

    /// Hanging or down, if it is.
    pub fn plight(&self) -> Option<Plight> {
        if self.hanging {
            Some(Plight::Hanging)
        } else {
            self.down.then_some(Plight::Down)
        }
    }

    /// Back as it was saved, facing `dir`: hanging where it is, or getting up (the fall was
    /// already taken).
    pub fn restore(&mut self, gd: &GameData, world: &mut World, dir: usize, p: Plight) {
        self.dir = dir % 8;
        match p {
            Plight::Hanging => self.hang(gd, world),
            Plight::Down => {
                self.knock_down(false);
                self.then.pop_front(); // (already flat)
            }
        }
    }

    /// Has it gone over nothing since last asked? (The game drops it, and a long way down hurts.)
    pub fn take_drop(&mut self) -> bool {
        std::mem::take(&mut self.drop)
    }

    /// Ran into something since last asked: (sure to hurt, else 1 in 3; the way he faced).
    pub fn take_bump(&mut self) -> Option<(bool, usize)> {
        self.bump.take().map(|b| (b, self.dir))
    }

    /// Sounds of the frames shown since last asked.
    pub fn take_sounds(&mut self) -> Vec<(u16, Option<bool>)> {
        std::mem::take(&mut self.sounds)
    }

    /// Hurt by a fall (ITEM.C): flat on his back, up again, and (a light fall, at ease) dazed,
    /// looking about.
    pub fn knock_down(&mut self, dazed: bool) {
        self.stop();
        (self.hanging, self.blocking, self.down) = (false, false, true);
        let stand = self.stance();
        self.then.extend([action::FALL_BACKWARDS, action::STAND_UP, stand].map(|a| (a, None)));
        if dazed && !self.combat {
            let look = [action::LOOK_LEFT, stand, action::LOOK_RIGHT, stand];
            self.then.extend(look.iter().chain(&look).map(|&a| (a, None)));
        }
    }

    /// Landed from a fall that didn't hurt (GRAVITY.C): down on his feet.
    pub fn land(&mut self) {
        self.stop();
        self.queue(action::LAND, None);
        if self.combat {
            self.queue(action::COMBAT_STAND, None);
        }
    }

    /// Exertion since last asked (climbing, swinging, kicking), if any.
    pub fn take_effort(&mut self) -> Option<Effort> {
        self.effort.take()
    }

    /// The swing reaching out since last asked (a striking frame), if any.
    pub fn take_swing(&mut self) -> Option<(usize, i32)> {
        self.swing.take()
    }

    /// Struck (not while blocking, hanging on or down): a light blow staggers him back; a
    /// heavy one only stops what he was doing (ITEM.C).
    pub fn hurt(&mut self, heavy: bool) {
        if self.blocking || self.hanging || self.down {
            return;
        }
        self.stop();
        self.stopped = true;
        if !heavy {
            self.then.extend([(action::STUMBLE, None), (self.stance(), None)]);
        }
    }

    /// A blow caught while blocking: the weapon jolts and comes back up.
    pub fn parry(&mut self) {
        if self.blocking && !self.busy() {
            self.queue(action::START_BLOCK, None);
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

    /// Stop everything he's playing, where he is.
    fn stop(&mut self) {
        (self.act, self.glide, self.aimed, self.over) = (None, None, None, false);
        self.then.clear();
    }

    /// Swings and kicks playing or to come.
    fn strikes(&self) -> usize {
        let a = self.act.iter().map(|j| j.action).chain(self.then.iter().map(|q| q.0));
        a.filter(|&a| matches!(a, action::ATTACK | action::KICK)).count()
    }

    /// Play `action` next (`dir`: None for whichever way he faces then).
    fn queue(&mut self, action: usize, dir: Option<usize>) {
        self.then.push_back((action, dir));
    }

    /// Falling: only whether the move button is held (to catch a ledge) counts.
    pub fn keep_holding(&mut self, world: &mut World, held: bool) -> bool {
        self.held = held;
        self.glide_by(world, 0.0)
    }

    /// Advance by `dt` seconds. Returns true if the Avatar moved or changed frame.
    pub fn update(&mut self, gd: &GameData, world: &mut World, input: &Input, dt: f32) -> bool {
        let mut changed = false;
        if world.in_combat(AVATAR) != self.combat {
            // A script put the Avatar in or out of combat.
            (self.combat, self.pending) = (!self.combat, false);
            if !self.busy() {
                changed |= self.stand(gd, world);
            }
        }
        let held = input.dir.is_some();
        let (pressed, released) = (held && !self.held, self.held && !held);
        self.held = held;
        if self.hanging && !self.busy() {
            changed |= self.update_hanging(gd, world, input, released);
            if self.hanging {
                changed |= self.hold_frame(gd, world) | self.glide_by(world, 0.0);
                self.hold(gd, world);
                return changed;
            }
        } else if !self.down {
            self.command(gd, world, input, pressed, released);
        }
        changed |= self.update_act(gd, world, dt);
        self.hold(gd, world);
        changed
    }

    /// The player's wishes, as WORLD.C takes the mouse: the weapon, a jump, the move button
    /// pressed, held and let go.
    fn command(&mut self, gd: &GameData, world: &mut World, input: &Input, pressed: bool, released: bool) {
        if self.blocking && !(input.block && self.combat && input.dir.is_none()) {
            // Let go (or moving, or out of combat): the weapon comes down first.
            self.blocking = false;
            self.then.extend([(action::STOP_BLOCK, None), (self.stance(), None)]);
        } else if input.combat {
            (self.combat, self.pending) = (!self.combat, false);
            world.set_in_combat(AVATAR, self.combat);
            let a = if self.combat { action::READY_WEAPON } else { action::UNREADY_WEAPON };
            self.then.extend([(a, None), (self.stance(), None)]);
        } else if (input.attack || input.kick) && self.combat && self.strikes() < 2 {
            // (One swing playing and one to come at most: ANIM.C's avatarSwing.)
            let a = if input.attack { action::ATTACK } else { action::KICK };
            self.then.extend([(a, None), (self.stance(), None)]);
        } else if input.block && self.combat && input.dir.is_none() && !self.busy() {
            self.blocking = true;
            self.queue(action::START_BLOCK, None);
        }
        if released {
            if self.pending {
                // Back into the stance he ran out of.
                (self.pending, self.combat) = (false, true);
                world.set_in_combat(AVATAR, true);
                self.queue(action::WALK, None);
            } else if self.last.0 == action::RUN && !self.combat {
                self.queue(action::WALK, None);
            }
            self.queue(self.stance(), None);
        } else if input.jump && !self.combat {
            match self.last.0 {
                action::STAND if !self.busy() => {
                    let (dir, how) = input.dir.map_or((self.dir, 1), |d| (d, erection(input)));
                    self.jump(gd, world, dir, how, input.aim);
                }
                action::RUN | action::RUNNING_JUMP if self.then.is_empty() => self.queue(action::RUNNING_JUMP, None),
                _ => {}
            }
        } else if let Some(dir) = input.dir.filter(|_| !self.blocking) {
            if pressed && !self.combat {
                if dir != self.dir {
                    if !self.busy() {
                        self.turn_to(dir, true);
                    }
                } else if input.careful {
                    self.careful_step(gd, world, dir);
                } else {
                    self.queue(action::WALK, Some(dir));
                }
            } else if !self.busy() {
                self.keep_going(gd, world, input, dir);
            }
        }
        self.next(gd, world);
    }

    /// The move button held, nothing playing (WORLD.C's commandNoEvents): fighting, advance or
    /// back off (running drops the stance); at ease, turn about if it's far round, else on at
    /// the pace the pointer asks (a run only once moving).
    fn keep_going(&mut self, gd: &GameData, world: &mut World, input: &Input, dir: usize) {
        if self.combat {
            let back = (dir + 4) % 8;
            if input.run {
                (self.combat, self.pending) = (false, true);
                world.set_in_combat(AVATAR, false);
            } else if self.dir == back {
                self.queue(action::RETREAT, Some(back));
            } else if !self.turn_to(dir, false) {
                self.queue(action::ADVANCE, Some(dir));
            }
            return;
        }
        if self.turn_to(dir, false) {
            return;
        }
        let a = match erection(input) {
            1 => return self.careful_now(gd, world, dir),
            3 if matches!(self.last.0, action::RUN | action::RUNNING_JUMP | action::WALK) => action::RUN,
            _ => action::WALK,
        };
        self.queue(a, Some(dir));
    }

    /// Turn about to `to` (ANIM.C's turnToFace): a look over the shoulder for each way he turns
    /// through (the stance, fighting), standing at the end. One way round, while moving, needs
    /// none (unless `force`d). False if there was nothing to do.
    fn turn_to(&mut self, to: usize, force: bool) -> bool {
        let (dir, stand) = (self.dir, self.stance());
        if to != dir && (force || (to != (dir + 7) % 8 && to != (dir + 1) % 8)) {
            let clockwise = (1..=4).any(|k| (dir + k) % 8 == to);
            let n = if clockwise { (to + 8 - dir) % 8 } else { (dir + 8 - to) % 8 };
            if matches!(self.last.0, action::RUN | action::RUNNING_JUMP) {
                self.queue(action::WALK, Some(dir));
            }
            let look = match (self.combat, clockwise) {
                (true, _) => stand,
                (_, true) => action::LOOK_RIGHT,
                _ => action::LOOK_LEFT,
            };
            for k in 0..n {
                self.queue(look, Some(if clockwise { (dir + k) % 8 } else { (dir + 8 - k) % 8 }));
            }
            self.queue(stand, Some(to));
            return true;
        }
        if force && self.last.0 != stand {
            self.queue(stand, Some(dir));
            return true;
        }
        false
    }

    fn careful_step(&mut self, gd: &GameData, world: &mut World, dir: usize) {
        if !self.busy() {
            self.careful_now(gd, world, dir);
            self.next(gd, world);
        }
    }

    /// A careful step (ANIM.C): one that would go over an edge teeters there instead, twice;
    /// trying again the same way, over he goes.
    fn careful_now(&mut self, gd: &GameData, world: &World, dir: usize) {
        if self.balancing && self.dir == dir {
            self.balancing = false;
            self.queue(action::STEP, Some(dir));
            self.over = true;
            return;
        }
        self.balancing = self.step_falls(gd, world, dir);
        let a = if self.balancing { action::EDGE_BALANCE } else { action::STEP };
        self.queue(a, Some(dir));
        if self.balancing {
            self.queue(a, Some(dir));
        }
    }

    /// Both buttons, standing (ANIM.C's jumpAvatar): facing elsewhere, turn; else onto what's in
    /// front if it's in reach, else straight up (the pointer close), else a jump forward: to the
    /// pointer if that's on something to land on and he's strong enough to get there, otherwise
    /// after a shrug, as far as the jump goes.
    fn jump(&mut self, gd: &GameData, world: &World, dir: usize, how: u8, aim: Option<Aim>) {
        if self.dir != dir {
            self.turn_to(dir, true);
            return;
        }
        let climb = (action::CLIMB_16..=action::CLIMB_72).find(|&a| world.anim_fits(gd, AVATAR, a, dir).is_some());
        if let Some(a) = climb {
            self.queue(a, None);
        } else if how == 1 {
            self.queue(action::JUMP_UP, None);
        } else {
            if let (Some(aim), Some(it)) = (aim, world.get(AVATAR)) {
                let far = (aim.at.0 - it.x).abs().max((aim.at.1 - it.y).abs()) >> 5;
                let land = world.index(aim.on).is_some_and(|i| world.entry(i).traits.land);
                if self.strength >= far && land {
                    self.aimed = Some(aim.at);
                } else {
                    let s = action::STAND;
                    self.then.extend([action::LOOK_LEFT, s, action::LOOK_RIGHT, s].map(|a| (a, None)));
                }
            }
            self.queue(action::JUMP, None);
        }
        self.queue(action::STAND, None);
    }

    /// Turn to `dir` where it stands (ScummVM's doAnim(stand, dir) on a throw; DOS on any drop),
    /// unless it's busy (mid-action, walking, blocking, hanging).
    pub fn turn(&mut self, gd: &GameData, world: &mut World, dir: usize) -> bool {
        if self.busy() || self.blocking || self.hanging || self.dir == dir % 8 {
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
        self.stop();
        (self.hanging, self.down, self.last) = (false, false, (self.stance(), self.dir));
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
}
