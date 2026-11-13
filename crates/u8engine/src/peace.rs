//! Grant Peace (ERTHSPEL's castGrantPeace; ScummVM's GrantPeaceProcess). The Avatar raises its
//! hands, casts, and lowers them while the player picks someone. Someone undead lays every undead
//! within 768 to rest, each struck with the chosen one's hit points (by Khumash-Gor's throne his
//! ghosts flare up and the throne changes); anyone else dies one time in ten. A blow brings
//! lightning and thunder.
use crate::combat::dmg;
use crate::game::Game;
use crate::usecode::{Pid, UiEvent};
use crate::world::{ObjId, AVATAR};
use u8data::anim::action;

/// Its process type (ScummVM's), which scripts can count.
const PROCESS: u16 = 0x21D;
const UNDEAD_RANGE: i32 = 768;
/// Khumash-Gor's throne, near enough to matter; his ghosts; their flare (shape, frames).
const THRONE: (u32, i32) = (289, 2048);
const GHOST: u32 = 411;
const FLARE: (u32, (u32, u32)) = (480, (0, 9));
const THUNDER: [u16; 3] = [91, 94, 96];

#[derive(Clone, Copy, Debug)]
pub(crate) struct GrantPeace {
    pid: Pid,
    /// 0 raising hands, 1 casting, 2 waiting for the player's pick.
    stage: u8,
    wait: u32,
}

impl Game {
    pub(crate) fn cast_grant_peace(&mut self) {
        let Some(vm) = self.vm() else { return };
        let pid = vm.spawn_external(AVATAR, PROCESS);
        let wait = self.cast_anim(action::CAST1);
        self.peace = Some(GrantPeace { pid, stage: 0, wait });
    }

    /// Play a casting move facing the way the Avatar faces; its length in ticks.
    fn cast_anim(&mut self, act: usize) -> u32 {
        let dir = self.world.get(AVATAR).and_then(|it| crate::avatar::facing(&self.data, &it)).unwrap_or(0);
        self.world.play(&self.data, AVATAR, act, dir).unwrap_or(1)
    }

    /// One tick of a Grant Peace being cast.
    pub(crate) fn grant_peace(&mut self) {
        let Some(mut p) = self.peace.take() else { return };
        if !self.vm().is_some_and(|vm| vm.alive(p.pid)) {
            return; // (a new map ends every process)
        }
        if p.wait > 0 {
            p.wait -= 1;
        } else if p.stage == 0 {
            (p.stage, p.wait) = (1, self.cast_anim(action::CAST3));
        } else if p.stage == 1 {
            self.cast_anim(action::CAST2);
            let target = self.call_intrinsic(0x00, &[]) as Pid; // TargetGump::target
            self.vm().unwrap().wait_on(p.pid, target);
            p.stage = 2;
        } else if let Some(picked) = self.vm().unwrap().woken(p.pid) {
            self.grant_peace_on(picked as ObjId);
            self.vm().unwrap().finish(p.pid, 0);
            return;
        }
        self.peace = Some(p);
    }

    fn grant_peace_on(&mut self, target: ObjId) {
        let Some(t) = self.stats.get(target).filter(|_| target != AVATAR && self.is_person(target)) else { return };
        let Some(av) = self.world.get(AVATAR) else { return };
        let kind = dmg::MAGIC | dmg::PIERCE | dmg::FIRE;
        let mut hit = false;
        if self.defence_type(target) & dmg::UNDEAD != 0 {
            // (Around the Avatar's footprint, as ScummVM's areaSearch with an item.)
            let b = self.world.index(AVATAR).map(|i| self.world.entry(i).bounds()).unwrap_or_default();
            let back = (b.x1 - b.x0, b.y1 - b.y0);
            let near = |r| self.world.search_area((av.x, av.y), r, back, false);
            let throne = near(THRONE.1).into_iter().find(|&i| self.world.get(i).is_some_and(|it| it.shape == THRONE.0));
            for id in near(UNDEAD_RANGE) {
                let undead = self.is_person(id) && self.defence_type(id) & dmg::UNDEAD != 0;
                if id == AVATAR || !undead || self.stats.get(id).is_none_or(|s| s.dead) {
                    continue;
                }
                let dir = self.dir_to(AVATAR, id);
                self.receive_hit(id, AVATAR, dir, t.hp as i32, kind);
                hit = true;
                if let (Some(throne), Some(g)) = (throne, self.world.get(id).filter(|g| g.shape == GHOST)) {
                    self.start_sprite(0, FLARE.0, FLARE.1, 1, 1, (g.x, g.y, g.z));
                    if let Some(mut th) = self.world.get(throne) {
                        th.frame = 1;
                        self.world.set(&self.data, throne, th);
                    }
                }
            }
        } else if !(t.dead || t.immortal || t.invincible) && self.world.roll(10) == 0 {
            let dir = self.dir_to(AVATAR, target);
            self.receive_hit(target, AVATAR, dir, t.hp as i32, kind);
            hit = true;
        }
        if hit {
            self.call_intrinsic(0xE1, &[]); // lightningBolt
            let sfx = THUNDER[self.world.roll(3) as usize];
            if let Some(vm) = self.vm() {
                vm.events.push_back(UiEvent::Sfx { sfx, item: AVATAR, looping: false });
            }
        }
    }
}
