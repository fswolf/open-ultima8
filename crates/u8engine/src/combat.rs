//! Hitting people and things: what a swing reaches, how much it hurts, being hurt and dying.
//! The rules follow ScummVM's reading of the original (its Actor::receiveHitU8 and
//! calculateAttackDamage). The Avatar's numbers come from what it wears and wields (U8.EXE's
//! weapon table, the armour values); everyone else's from [`monsters`] by shape.
use crate::avatar::Avatar;
use crate::equip::{self, Slot};
use crate::game::Game;
use crate::geom::{Bounds, DIRS};
use crate::monsters::{self, Monster};
use crate::world::{item_flag, ObjId, AVATAR};
use u8data::{anim::action, typeflag::family::BREAKABLE, usecode::event};

/// Damage (and defence) type flags, as U8.EXE's weapon table and ScummVM name them.
pub mod dmg {
    pub const NORMAL: u16 = 0x01;
    pub const BLADE: u16 = 0x02;
    pub const BLUNT: u16 = 0x04;
    /// Halves armour (Flame Sting; the magic shield defends against it).
    pub const FIRE: u16 = 0x08;
    /// Double damage on the undead (who defend with this flag).
    pub const UNDEAD: u16 = 0x10;
    /// Only magic hurts those who defend with this.
    pub const MAGIC: u16 = 0x20;
    /// One blow in ten kills (Slayer).
    pub const SLAYER: u16 = 0x40;
    /// Ignores armour and never misses; defending with it halves blunt blows.
    pub const PIERCE: u16 = 0x80;
    /// A fall.
    pub const FALLING: u16 = 0x100;
}

/// Stats grow with use up to this (Dino's guide; ScummVM).
const STAT_MAX: u8 = 25;
/// Points of use that make a stat grow (sooner, by chance, the closer it gets).
const STAT_GROWTH: u32 = 650;

/// How hard a struck thing is knocked (units a tick, each way and up).
const KNOCK: i32 = 16;
/// Blood (U8SHAPES).
const BLOOD: u32 = 620;

/// World units a swing reaches for each step of its frame's attack range (ANIM.DAT).
pub const REACH_STEP: i32 = 32;

/// A blow this heavy doesn't stagger the Avatar, only stops him; a fall this hard doesn't daze
/// him (ITEM.C).
const HEAVY: i32 = 6;

impl Game {
    /// `who` swings facing `dir`, reaching `range` steps: strike the first person there, else
    /// the first thing that minds being hit. True if it struck something.
    pub(crate) fn swing(&mut self, who: ObjId, dir: usize, range: i32) -> bool {
        let target = self.swing_target(who, dir, range);
        if let Some(t) = target {
            self.receive_hit(t, who, dir, 0, 0);
        }
        target.is_some()
    }

    /// What a swing from `who` reaches: its box pushed `range` steps the way it faces.
    pub fn swing_target(&self, who: ObjId, dir: usize, range: i32) -> Option<ObjId> {
        let w = &self.world;
        let b = w.entry(w.index(who)?).bounds();
        let (dx, dy) = (DIRS[dir].0 * range * REACH_STEP, DIRS[dir].1 * range * REACH_STEP);
        let reach = Bounds { x0: b.x0 + dx, y0: b.y0 + dy, x1: b.x1 + dx, y1: b.y1 + dy, ..b };
        let hit = w.touching(reach).into_iter().filter(|&id| id != who);
        let (people, things): (Vec<ObjId>, Vec<ObjId>) = hit.partition(|&id| self.is_person(id));
        let alive = people.into_iter().find(|&id| self.stats.get(id).is_some_and(|s| !s.dead));
        alive.or_else(|| things.into_iter().find(|&id| self.minds_hits(id)))
    }

    pub(crate) fn is_person(&self, id: ObjId) -> bool {
        self.world.get(id).is_some_and(|it| it.npc != 0)
    }

    /// Has a `gotHit` script?
    fn minds_hits(&self, id: ObjId) -> bool {
        let (Some(s), Some(it)) = (&self.scripts, self.world.get(id)) else { return false };
        let family = self.data.typeflags.get(it.shape).family;
        let class = crate::usecode::vm::event_class(&s.usecode, id, &it, family, event::GOT_HIT);
        s.usecode.event(class, event::GOT_HIT).is_some()
    }

    /// `target` is struck by `by` (from `dir`, the way `by` faces), as ScummVM's reading of
    /// the original has it. `damage` 0: the striker's own; `kind` 0: the striker's damage type.
    /// A person's defences, armour and dexterity decide what gets through; hurt, they
    /// stagger (and fight back), or die. A thing struck runs its `gotHit` script, or breaks.
    pub fn receive_hit(&mut self, target: ObjId, by: ObjId, dir: usize, damage: i32, kind: u16) {
        let Some(mut s) = self.stats.get(target).filter(|_| self.is_person(target)) else {
            self.thing_hit(target, by, dir);
            return;
        };
        if s.dead {
            return;
        }
        // (Already acting something out, they don't stagger: the original's doAnim won't queue
        // the stumble behind it. Bentic reading in his chair stays in it.)
        let busy = self.world.is_acting(target);
        let damage = if damage == 0 && self.is_person(by) { self.damage_by(by) } else { damage };
        let kind = if kind == 0 { self.damage_type(by) } else { kind };
        if by == AVATAR && self.world.last_anim(AVATAR) != action::KICK {
            self.grow(0, damage.max(0) as u32 / 4); // swinging builds strength (kicks count apart)
        }
        let damage = self.attack_damage(target, by, damage, kind);
        if target == AVATAR && damage >= 4 && by != 0 {
            self.bleed(dir);
        }
        if damage > 0 && !s.immortal && !s.invincible {
            if damage >= s.hp as i32 {
                if s.withstand_death {
                    // Saved, once.
                    (s.hp, s.withstand_death) = (s.max_hp(), false);
                    if let Some(n) = self.stats.get_mut(target) {
                        *n = s;
                    }
                } else {
                    self.die(target, (dir + 4) % 8, kind);
                    return;
                }
            } else if let Some(n) = self.stats.get_mut(target) {
                n.hp -= damage as i16;
            }
        }
        if target == AVATAR && kind & dmg::FALLING != 0 && damage > 0 {
            // Hurt by a fall (ITEM.C): flat on his back, then up (a light fall: dazed).
            if let Some(a) = self.avatar.as_mut() {
                a.knock_down(damage < HEAVY);
            }
            return;
        }
        if target != AVATAR && kind & dmg::FALLING == 0 {
            // Fight back (ITEM.C receiveHit): after whoever struck; struck by nothing (a blast,
            // the water): the Avatar did it; by a thing (a campfire walked into): nobody in
            // particular. A fall of its own: no fight. Only the Avatar's own blow calls for help.
            let at = match by {
                0 => AVATAR,
                _ if self.is_person(by) => by,
                _ => 0,
            };
            // (A stray blow from a friend in the melee, someone it's no enemy of: no quarrel.)
            let friend =
                at > AVATAR && self.stats.get(target).zip(self.stats.get(at)).is_some_and(|(t, a)| !t.is_enemy(&a));
            if !friend {
                match at {
                    0 => self.world.set_in_combat(target, true),
                    _ => self.world.set_target(target, at),
                }
                // In at once (the original's receiveHit): its script starts before those it
                // calls in, so a struck townsperson's scene is the one Beren comes to.
                if !self.fights.contains_key(&target) {
                    self.join_fight(target);
                }
            }
            if by == AVATAR {
                self.call_for_help(target);
            }
        }
        if target == AVATAR && self.world.last_anim(AVATAR) == action::START_BLOCK {
            // Caught on the weapon: a clang (a thud if it still got through), no stagger.
            self.avatar.as_mut().map(Avatar::parry);
            let sfx = if damage > 0 { 50 + self.world.roll(2) } else { 20 + self.world.roll(3) };
            if let Some(sc) = &mut self.scripts {
                sc.vm.events.push_back(crate::usecode::UiEvent::Sfx { sfx: sfx as u16, item: AVATAR, looping: false });
            }
        } else if damage > 0 {
            match target {
                AVATAR => {
                    if let Some(a) = self.avatar.as_mut() {
                        a.hurt(damage >= HEAVY);
                    }
                }
                _ if !busy => {
                    // Then back to standing (in stance if fighting) the way they faced before: a
                    // reader thrown back in his chair is soon reading again.
                    let facing = self.world.facing(&self.data, target).unwrap_or((dir + 4) % 8);
                    self.world.play(&self.data, target, action::STUMBLE, (dir + 4) % 8);
                    let stand = if self.world.in_combat(target) { action::COMBAT_STAND } else { action::STAND };
                    self.world.then_play(target, stand, facing);
                }
                _ => {}
            }
        }
    }

    /// `struck` was attacked by the Avatar: everyone alive with the same alignment (side and
    /// enemies both) within 1024 of the Avatar goes into combat against it, and so runs its
    /// going-into-combat script: guards shout and come, acolytes and the undead join in,
    /// townsfolk cry for Beren. ScummVM leaves this a TODO. As DOS has it (claude/attack-compare.md):
    /// a second acolyte (1/0) joins each time, guard 47 from 1024 away, nobody from 1152 or more;
    /// Orlok cries out when Jenna is struck and the other way round; peasant 66 (1/2) stays out
    /// when guard 65 (1/e) is struck, 640 away.
    pub(crate) fn call_for_help(&mut self, struck: ObjId) {
        /// (Measured in DOS: guard 47 came from 1024 away, nobody from 1152.)
        const CALL_RANGE: i32 = 1024;
        let Some(side) = self.stats.get(struck).map(|s| (s.alignment, s.enemies)) else { return };
        let Some(a) = self.world.get(AVATAR) else { return };
        let near = |it: &u8data::map::Item| (it.x - a.x).abs().max((it.y - a.y).abs()) <= CALL_RANGE;
        let helpers: Vec<ObjId> = (2..256)
            .filter(|&id| id != struck && !self.world.in_combat(id) && !self.world.is_void(id))
            .filter(|&id| self.world.get(id).is_some_and(|it| near(&it)))
            .filter(|&id| self.stats.get(id).is_some_and(|s| !s.dead && (s.alignment, s.enemies) == side))
            .collect();
        for id in helpers {
            self.world.set_target(id, AVATAR);
            self.join_fight(id);
        }
    }

    /// A thing (not a person) struck from `dir`: its `gotHit` script if it has one; else an
    /// explosive blows up, a breakable breaks, and anything loose is knocked flying.
    fn thing_hit(&mut self, target: ObjId, by: ObjId, dir: usize) {
        if self.trigger_with(target, event::GOT_HIT, &[by.to_le_bytes(), [0, 0]].concat()) {
            return;
        }
        let Some(it) = self.world.get(target) else { return };
        let tf = self.data.typeflags.get(it.shape);
        if self.explosive(target) {
            self.explode(target, true);
        } else if tf.family == BREAKABLE {
            self.world.destroy(&self.data, target);
        } else if !tf.has(u8data::typeflag::flag::FIXED) && tf.weight != 0 {
            let push = (DIRS[dir].0 * KNOCK, DIRS[dir].1 * KNOCK, KNOCK);
            self.hurl(target, push, crate::gravity::GRAVITY, None);
        }
    }

    /// What of a blow gets through to `target`: its defences may block the damage type (or all
    /// but magic), Slayer may kill outright, undead take double, piercing-proof hide halves
    /// blunt blows; then 3% off per point of armour (half against fire), then a dexterity roll
    /// to hit at all (which also builds the Avatar's dexterity).
    /// Knocked flat, the Avatar's armour counts half and every blow lands.
    fn attack_damage(&mut self, target: ObjId, by: ObjId, damage: i32, kind: u16) -> i32 {
        let defence = self.defence_type(target);
        let kind = kind & !(defence & !(dmg::MAGIC | dmg::UNDEAD | dmg::PIERCE));
        let mut damage = if defence & dmg::MAGIC != 0 && kind & dmg::MAGIC == 0 { 0 } else { damage };
        let mut slain = false;
        if damage != 0 && kind != 0 {
            if kind & dmg::SLAYER != 0 && self.world.roll(10) == 0 {
                (slain, damage) = (true, 255);
            }
            if kind & dmg::UNDEAD != 0 && defence & dmg::UNDEAD != 0 {
                damage *= 2;
            }
            if defence & dmg::PIERCE != 0 && kind & (dmg::BLADE | dmg::FIRE | dmg::PIERCE) == 0 {
                damage /= 2;
            }
        } else {
            damage = 0;
        }
        if damage != 0 && kind & dmg::PIERCE == 0 && !slain {
            // Blocking takes a fifth of strength off.
            if matches!(self.world.last_anim(target), action::START_BLOCK | action::STOP_BLOCK) {
                damage -= self.stats.get(target).map_or(0, |s| s.str as i32) / 5;
            }
            let ac = 3 * self.armour(target).max(0);
            let ac = if kind & dmg::FIRE != 0 { ac / 2 } else { ac }.min(100);
            damage = ((100 - ac) * damage / 100).max(0);
        }
        if damage != 0 && kind & dmg::PIERCE == 0 && self.is_person(by) {
            let attack = self.dex(by) + self.avatar_weapon(by).map_or(0, |w| w.dex_attack as i32);
            let defend = (self.dex(target) + self.avatar_weapon(target).map_or(0, |w| w.dex_defend as i32)).max(1);
            let attack = attack.max(0);
            let hit = self.world.roll(attack as u32 + 3) > self.world.roll(defend as u32);
            if hit && by == AVATAR {
                self.grow(1, if attack > defend { 2 * (attack - defend) as u32 } else { 2 });
            }
            if !hit {
                damage = 0;
            }
        }
        damage
    }

    fn dex(&self, id: ObjId) -> i32 {
        self.stats.get(id).map_or(10, |s| s.dex as i32)
    }

    /// The fighting numbers for `id`'s kind (not the Avatar's: it has its gear).
    pub(crate) fn monster(&self, id: ObjId) -> Option<&'static Monster> {
        self.world.get(id).filter(|_| id != AVATAR).and_then(|it| monsters::get(it.shape))
    }

    /// Armour class: the Avatar's from what it wears, others' from their kind.
    fn armour(&self, who: ObjId) -> i32 {
        match who {
            AVATAR => equip::armour_class(&self.data, &self.world, AVATAR),
            _ => self.monster(who).map_or(0, |m| m.armour as i32),
        }
    }

    /// What `who` is proof against: the Avatar by its armour (the magic shield: fire), others
    /// by their kind (the undead, magic beings…).
    pub(crate) fn defence_type(&self, who: ObjId) -> u16 {
        if who != AVATAR {
            return self.monster(who).map_or(0, |m| m.defence);
        }
        let worn = self.world.contents(who).into_iter().filter_map(|id| self.world.get(id));
        worn.filter(|it| it.flags & crate::world::item_flag::EQUIPPED != 0)
            .fold(0, |d, it| d | equip::armour_defence(it.shape))
    }

    /// What kind of damage `by` deals: the Avatar's weapon's (else plain), others' by kind;
    /// a thing (thrown, falling) as a weapon of its shape would, else none.
    fn damage_type(&self, by: ObjId) -> u16 {
        if !self.is_person(by) {
            let shape = self.world.get(by).map(|it| it.shape);
            return shape.and_then(|s| equip::weapon(&self.data, s)).map_or(0, |w| w.damage_type as u16);
        }
        match (self.avatar_weapon(by), self.monster(by)) {
            (Some(w), _) => w.damage_type as u16,
            (None, Some(m)) => m.damage_type,
            _ => dmg::NORMAL,
        }
    }

    /// The Avatar's weapon (others' weapons are part of their figures: their kind says how
    /// they hit).
    fn avatar_weapon(&self, who: ObjId) -> Option<u8data::weapons::Weapon> {
        (who == AVATAR).then(|| self.held(who)).flatten()
    }

    /// Use builds the Avatar's stats (0 STR, 1 DEX, 2 INT): `n` points towards the next one,
    /// which comes at 650 or, by chance, sooner the nearer it gets (ScummVM's reading).
    pub(crate) fn grow(&mut self, stat: usize, n: u32) {
        let Some(s) = self.stats.get(AVATAR) else { return };
        let value = [s.str, s.dex, s.int][stat];
        if value >= STAT_MAX {
            return;
        }
        self.growth[stat] += n;
        let acc = self.growth[stat];
        if acc >= STAT_GROWTH || self.world.roll(STAT_GROWTH - acc + 1) == 0 {
            self.growth[stat] = 0;
            if let Some(s) = self.stats.get_mut(AVATAR) {
                match stat {
                    0 => s.str += 1,
                    1 => s.dex += 1,
                    _ => s.int += 1,
                }
            }
        }
    }

    /// The weapon `who` wields, with its numbers.
    fn held(&self, who: ObjId) -> Option<u8data::weapons::Weapon> {
        let w = equip::worn(&self.data, &self.world, who, Slot::Weapon).and_then(|w| self.world.get(w))?;
        equip::weapon(&self.data, w.shape)
    }

    /// How hard `by` hits: the Avatar its weapon's base + 0..=modifier + a fifth of strength,
    /// with fists 1 + 0..=half strength; others as their kind does, else 1.
    fn damage_by(&mut self, by: ObjId) -> i32 {
        let str = self.stats.get(by).map_or(10, |s| s.str as i32);
        if by == AVATAR && self.world.last_anim(AVATAR) == action::KICK {
            // A kick: up to half strength, and what the legs add.
            let legs = equip::worn(&self.data, &self.world, AVATAR, Slot::Legs).and_then(|l| self.world.get(l));
            let bonus = legs.map_or(0, |l| equip::kick_bonus(l.shape));
            return self.world.roll(str as u32 / 2 + 1) as i32 + bonus;
        }
        if let Some(m) = self.monster(by) {
            let (lo, hi) = (m.damage.0 as u32, m.damage.1.max(m.damage.0) as u32);
            return (lo + self.world.roll(hi - lo + 1)) as i32;
        }
        match self.avatar_weapon(by) {
            Some(w) => w.base as i32 + self.world.roll(w.modifier as u32 + 1) as i32 + str / 5,
            None if by == AVATAR => 1 + self.world.roll(str as u32 / 2 + 1) as i32,
            None => 1,
        }
    }

    /// `id` dies (of a blow of `kind`): falls (and stays down, unless its kind rises again),
    /// stops walking and fighting, and leaves its kind's loot. The Avatar's death ends the game.
    fn die(&mut self, id: ObjId, facing: usize, kind: u16) {
        if let Some(s) = self.stats.get_mut(id) {
            s.dead = true;
            s.hp = 0;
        }
        self.stop_walking(id);
        self.world.set_in_combat(id, false);
        self.world.set_overlay(id, None);
        // A kind with no death in ANIM.DAT but a way of getting up (shape 209): that, played
        // backwards, lays them down. (Most single-frame deaths are the original's own pose, as
        // Salkind's stoop; their getting-up frames are empty.)
        let shape = self.world.get(id).map_or(0, |it| it.shape);
        let frames = |a| self.data.anims.get(shape, a).and_then(|x| x.frames.get(facing).cloned()).unwrap_or_default();
        let drawable =
            |f: &u8data::anim::AnimFrame| self.data.shapes.frame(shape, f.frame).is_some_and(|f| f.width > 1);
        let up = frames(action::STAND_UP);
        if frames(action::DIE).is_empty() && up.len() > 1 && up.iter().all(drawable) {
            self.world.play_backwards(&self.data, id, action::STAND_UP, facing, action::DIE);
        } else {
            self.world.play(&self.data, id, action::DIE, facing);
        }
        if id == AVATAR {
            self.avatar_dead = true;
            return;
        }
        // An enemy of the Avatar's down while it fights: a fanfare.
        let foe = match (self.stats.get(id), self.stats.get(AVATAR)) {
            (Some(s), Some(a)) => s.is_enemy(&a),
            _ => false,
        };
        if foe && self.world.in_combat(AVATAR) {
            self.ui_event(crate::usecode::UiEvent::Victory);
        }
        self.monster_died(id, kind);
        if let Some(piece) = self.monster(id).and_then(|m| m.bursts_into) {
            self.burst(id, piece);
        }
    }

    /// The Avatar takes a real blow: blood (U8SHAPES 620, frames 0–12 struck from the north and
    /// east, 13–25 otherwise), somewhere up its body.
    fn bleed(&mut self, dir: usize) {
        let Some(a) = self.world.get(AVATAR) else { return };
        let frames = if dir > 2 { (13, 25) } else { (0, 12) };
        let z = a.z + self.world.roll(24) as i32;
        let pid = self.scripts.as_mut().map_or(0, |s| s.vm.spawn_external(0, 0));
        self.start_sprite(pid, BLOOD, frames, 1, 1, (a.x, a.y, z));
    }

    /// `id` blows apart (seekers): five pieces of `piece` flung about (ScummVM's numbers).
    fn burst(&mut self, id: ObjId, piece: u32) {
        let Some(it) = self.world.get(id) else { return };
        let frames = self.data.shapes.get(piece).len().max(1) as u32;
        for _ in 0..5 {
            let r = |g: &mut Game, lo: i32, hi: i32| lo + g.world.roll((hi - lo + 1) as u32) as i32;
            let (x, y, z) = (it.x + 32 * r(self, -4, 4), it.y + 32 * r(self, -4, 4), it.z + r(self, 0, 7));
            let frame = self.world.roll(frames);
            let p =
                u8data::map::Item { shape: piece, frame, flags: item_flag::FAST_ONLY, x, y, z, ..Default::default() };
            let p = self.world.add(&self.data, p, true);
            let v = (r(self, -25, 25), r(self, -25, 25), r(self, 10, 20));
            self.hurl(p, v, 4, None);
        }
    }

    pub(crate) fn ui_event(&mut self, e: crate::usecode::UiEvent) {
        if let Some(s) = &mut self.scripts {
            s.vm.events.push_back(e);
        }
    }
}
