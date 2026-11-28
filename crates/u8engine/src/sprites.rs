//! Sprite effects (ScummVM's SpriteProcess): a shape's frames played out at a point — sparkles,
//! splashes, smoke — then gone. Scripts wait on the process each one hands back.
use crate::game::Game;
use crate::usecode::Pid;
use crate::world::{item_flag, ObjId};
use u8data::map::Item;

/// The Flame Sting's fire (ANIMCALL.C hitAnimCall): a flare where a blow of it lands, a fire
/// field there after 3 ticks for 90, then the field dying down.
pub(crate) struct Flare {
    at: (i32, i32, i32),
    ticks: u32,
    field: Option<ObjId>,
}

const FLARE: u32 = 480;
const FIELD: u32 = 400;
const FIELD_OUT: u32 = 381;
const FIELD_AFTER: u32 = 3;
const FIELD_FOR: u32 = 90;

pub(crate) struct Sprite {
    pub(crate) pid: Pid,
    id: ObjId,
    frames: (u32, u32),
    frame: u32,
    repeats: u32,
    delay: u32,
    wait: u32,
}

impl Game {
    /// Start an effect: `shape` frames `first..=last`, `repeats` times, one every `delay` ticks.
    pub(crate) fn start_sprite(
        &mut self,
        pid: Pid,
        shape: u32,
        (first, last): (u32, u32),
        repeats: u32,
        delay: u32,
        at: (i32, i32, i32),
    ) {
        let (x, y, z) = at;
        let it = Item { shape, frame: first, x, y, z, flags: item_flag::DISPOSABLE, ..Item::default() };
        let id = self.world.add(&self.data, it, true);
        let s = Sprite {
            pid,
            id,
            frames: (first, last.max(first)),
            frame: first,
            repeats: repeats.max(1),
            delay: delay.max(1),
            wait: 0,
        };
        self.sprites.push(s);
    }

    /// The Flame Sting lands a blow: fire 96 ahead of `who`, the way it faces (`dir`).
    pub(crate) fn flame_sting(&mut self, who: ObjId, dir: usize) {
        let Some(it) = self.world.get(who) else { return };
        let d = crate::geom::DIRS[dir];
        let at = (it.x + d.0 * 96, it.y + d.1 * 96, it.z);
        self.effect(FLARE, (0, 9), 2, at);
        let sfx = [0x21, 0x65][self.world.roll(2) as usize];
        self.sfx(sfx, who);
        self.flares.push(Flare { at, ticks: 0, field: None });
    }

    /// A sprite no script waits on.
    fn effect(&mut self, shape: u32, frames: (u32, u32), delay: u32, at: (i32, i32, i32)) {
        let pid = self.scripts.as_mut().map_or(0, |s| s.vm.spawn_external(0, 0));
        self.start_sprite(pid, shape, frames, 1, delay, at);
    }

    fn tick_flares(&mut self) {
        let mut flares = std::mem::take(&mut self.flares);
        flares.retain_mut(|f| {
            f.ticks += 1;
            let (x, y, z) = f.at;
            if f.ticks == FIELD_AFTER {
                let it = Item { shape: FIELD, x, y, z, flags: item_flag::DISPOSABLE, ..Item::default() };
                let id = self.world.add(&self.data, it, true);
                // (Put in the fast area, it's in it at once: ITEMCACH.C pop.)
                self.world.set_fast(id, self.world.in_fast_area(x, y));
                f.field = Some(id);
            }
            if f.ticks < FIELD_AFTER + FIELD_FOR {
                return true;
            }
            if let Some(id) = f.field {
                self.world.remove(id);
            }
            self.effect(FIELD_OUT, (0, 9), 2, f.at);
            false
        });
        self.flares.extend(flares);
    }

    /// One tick of every effect; finished ones are taken away and their scripts told.
    pub(crate) fn tick_sprites(&mut self) {
        self.tick_flares();
        let mut done = vec![];
        let mut sprites = std::mem::take(&mut self.sprites);
        sprites.retain_mut(|s| {
            if s.wait > 0 {
                s.wait -= 1;
                return true;
            }
            if s.frame > s.frames.1 {
                s.repeats -= 1;
                if s.repeats == 0 {
                    self.world.remove(s.id);
                    done.push(s.pid);
                    return false;
                }
                s.frame = s.frames.0;
            }
            if let Some(mut it) = self.world.get(s.id) {
                it.frame = s.frame;
                self.world.set(&self.data, s.id, it);
            }
            s.frame += 1;
            s.wait = s.delay - 1;
            true
        });
        self.sprites = sprites;
        if let Some(sc) = &mut self.scripts {
            done.into_iter().for_each(|pid| sc.vm.finish(pid, 0));
        }
    }
}
