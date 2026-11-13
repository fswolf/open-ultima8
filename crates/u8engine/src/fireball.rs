//! Tony's balls (LEVER's TonysBalls; ScummVM's FireballProcess): a fireball (260, frame 4) that
//! homes in on the Avatar — speeding up towards it, at most 32 a tick, trailing three flames
//! (261, framed by the way it flies) — bounces back off things, and bursts on the first person it
//! hits (5–9 fire damage). After ten seconds it may go out any tick (1 in 20).
use crate::combat::dmg;
use crate::game::Game;
use crate::geom::{dir_towards, DIRS};
use crate::world::{item_flag, ObjId, AVATAR};
use u8data::map::Item;

const BALL: (u32, u32) = (260, 4);
const FLAME: u32 = 261;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Fireball {
    id: ObjId,
    v: (i32, i32),
    age: u32,
    tail: [ObjId; 3],
}

impl Game {
    /// A fireball at `at`, after the Avatar (none if there's no room for it).
    pub(crate) fn tonys_ball(&mut self, at: (i32, i32, i32)) {
        if !self.world.clear_at(&self.data, BALL.0, at) {
            return;
        }
        let (x, y, z) = at;
        let it = Item { shape: BALL.0, frame: BALL.1, x, y, z, flags: item_flag::FAST_ONLY, ..Default::default() };
        let id = self.world.add(&self.data, it, true);
        self.fireballs.push(Fireball { id, v: (0, 0), age: 0, tail: [0; 3] });
    }

    /// One tick of every fireball.
    pub(crate) fn fly_fireballs(&mut self) {
        let mut balls = std::mem::take(&mut self.fireballs);
        balls.retain_mut(|f| {
            f.age += 1;
            let (Some(b), Some(t)) = (self.world.get(f.id), self.world.abs_box(AVATAR)) else {
                self.put_out(f);
                return false;
            };
            if f.age > 300 && self.world.roll(20) == 0 {
                self.put_out(f);
                return false;
            }
            let (dx, dy) = (t.x1 - b.x, t.y1 - b.y);
            let to = dir_towards((t.x0 + t.x1) / 2 - b.x, (t.y0 + t.y1) / 2 - b.y) as usize;
            if f.v == (0, 0) && dx / 64 == 0 && dy / 64 == 0 {
                f.v = (2 * DIRS[to].0, 2 * DIRS[to].1);
            } else {
                f.v = (f.v.0 + dx / 64, f.v.1 + dy / 64);
            }
            let speed = ((f.v.0 * f.v.0 + f.v.1 * f.v.1) as f64).sqrt() as i32;
            if speed > 32 {
                f.v = (f.v.0 * 32 / speed, f.v.1 * 32 / speed);
            }
            let hit = self.world.sweep(&self.data, f.id, (b.x + f.v.0, b.y + f.v.1, b.z)).map(|(h, _)| h);
            // The tail: a flame where it was, the oldest one moved up to there.
            let flame = match f.tail[2] {
                0 => self.world.add(
                    &self.data,
                    Item { shape: FLAME, flags: item_flag::DISPOSABLE, ..Default::default() },
                    true,
                ),
                id => id,
            };
            if let Some(mut fl) = self.world.get(flame) {
                (fl.x, fl.y, fl.z, fl.frame) = (b.x, b.y, b.z, dir_towards(f.v.0, f.v.1) as u32);
                self.world.set(&self.data, flame, fl);
            }
            f.tail = [flame, f.tail[0], f.tail[1]];
            match hit {
                Some(h) if self.is_person(h) => {
                    let n = 5 + self.world.roll(5) as i32;
                    self.receive_hit(h, 0, (to + 4) % 8, n, dmg::FIRE);
                    self.put_out(f);
                    false
                }
                Some(_) => {
                    f.v = (-f.v.0, -f.v.1);
                    true
                }
                None => true,
            }
        });
        self.fireballs.extend(balls);
    }

    fn put_out(&mut self, f: &Fireball) {
        for id in [f.id].into_iter().chain(f.tail) {
            if id != 0 {
                self.world.remove(id);
            }
        }
    }
}
