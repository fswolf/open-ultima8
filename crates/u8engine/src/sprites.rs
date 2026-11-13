//! Sprite effects (ScummVM's SpriteProcess): a shape's frames played out at a point — sparkles,
//! splashes, smoke — then gone. Scripts wait on the process each one hands back.
use crate::game::Game;
use crate::usecode::Pid;
use crate::world::{item_flag, ObjId};
use u8data::map::Item;

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

    /// One tick of every effect; finished ones are taken away and their scripts told.
    pub(crate) fn tick_sprites(&mut self) {
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
