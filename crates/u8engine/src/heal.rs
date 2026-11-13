//! The Avatar's slow recovery (ScummVM's HealProcess): a hit point a minute and a point of mana
//! every other half minute, up to the maximum; each minute adds to hunger (up to 200), and food
//! (feedAvatar) sates it, healing a quarter of what it takes off.
use crate::game::Game;
use crate::world::AVATAR;

/// Ticks between hit points regained (a real minute); mana comes halfway.
const HEAL_TICKS: u32 = 1800;
const MANA_AT: u32 = 900;
const HUNGER_MAX: u16 = 200;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Recovery {
    ticks: u32,
    pub(crate) hunger: u16,
}

impl Recovery {
    /// (ticks towards the next hit point, hunger), for saving.
    pub(crate) fn parts(&self) -> (u32, u16) {
        (self.ticks, self.hunger)
    }
    pub(crate) fn set_parts(&mut self, ticks: u32, hunger: u16) {
        (self.ticks, self.hunger) = (ticks.min(HEAL_TICKS), hunger.min(HUNGER_MAX));
    }
}

impl Game {
    /// One tick of the Avatar's recovery.
    pub(crate) fn heal(&mut self) {
        if self.avatar_dead {
            return;
        }
        let r = &mut self.recovery;
        r.ticks += 1;
        let Some(s) = self.stats.get_mut(AVATAR) else { return };
        if r.ticks == MANA_AT && s.mana < s.max_mana() {
            s.mana += 1;
        }
        if r.ticks >= HEAL_TICKS {
            r.ticks = 0;
            if s.hp < s.max_hp() {
                s.hp += 1;
            }
            r.hunger = (r.hunger + 1).min(HUNGER_MAX);
        }
    }

    /// The Avatar eats `food`: hunger goes down by it (as far as there is any), and hit points
    /// up by a quarter of that, to the maximum.
    pub(crate) fn feed(&mut self, food: u16) {
        let r = &mut self.recovery;
        let food = food.min(r.hunger);
        let old = r.hunger;
        r.hunger -= food;
        let gain = (old / 4 - r.hunger / 4) as i16;
        if let Some(s) = self.stats.get_mut(AVATAR).filter(|s| !s.dead) {
            s.hp = (s.hp + gain).min(s.max_hp().max(s.hp));
        }
    }
}
