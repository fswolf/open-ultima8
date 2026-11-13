//! Things flying and falling (ScummVM's GravityProcess): a speed per tick, pulled down by
//! gravity; walls turn it back at half speed, landing on something not floor-like — or hard —
//! bounces it off at a random angle, and people who fall far are hurt. Also explosions, which
//! hurt and hurl whatever is near.
use crate::combat::dmg;
use crate::game::Game;
use crate::geom::dir_towards;
use crate::usecode::Pid;
use crate::world::{item_flag, ObjId};
use u8data::{anim::action, typeflag::flag};

/// Falls at least this high hurt; this high or more kill.
const HURT_FROM: i32 = 80;
const KILL_FROM: i32 = 104;
/// What scripts' `fall` and knocked-over things fall with.
pub const GRAVITY: i32 = 4;
/// Explosions reach this far (world units, across and up or down) and hurt 6–11.
const BLAST: i32 = 160;
const BLAST_DAMAGE: (u32, u32) = (6, 11);
/// The explosion's look (U8: shape 578 frames 20–34) and sounds (one of two).
const BLAST_SPRITE: (u32, u32, u32) = (578, 20, 34);
const BLAST_SFX: [u16; 2] = [31, 158];

pub(crate) struct Flight {
    pub(crate) id: ObjId,
    v: (i32, i32, i32),
    gravity: i32,
    /// Highest point (people: how far they fall).
    top: i32,
    /// Scripts waiting for it to come to rest.
    pids: Vec<Pid>,
    /// Where it set off from and what it touched there (contact events on landing).
    from: (i32, i32, i32),
    touching: Vec<ObjId>,
}

impl Game {
    /// Send `id` flying (`v` units a tick, pulled down by `gravity`); one already flying gets
    /// the push added. `pid` (if any) ends when it comes to rest. Things inside others don't fly.
    pub fn hurl(&mut self, id: ObjId, v: (i32, i32, i32), gravity: i32, pid: Option<Pid>) {
        let Some(it) = self.world.get(id).filter(|_| self.world.container(id).is_none()) else {
            self.finish_all(pid);
            return;
        };
        match self.flights.iter_mut().find(|f| f.id == id) {
            Some(f) => {
                f.v = (f.v.0 + v.0, f.v.1 + v.1, f.v.2 + v.2);
                f.gravity = f.gravity.max(gravity);
                f.pids.extend(pid);
            }
            None => {
                let touching = self.world.contacts(id);
                let (pids, from) = (pid.into_iter().collect(), (it.x, it.y, it.z));
                self.flights.push(Flight { id, v, gravity, top: it.z, pids, from, touching });
            }
        }
    }

    /// `id` drops if nothing holds it up (things fixed in place, weightless or hanging don't).
    pub fn fall(&mut self, id: ObjId) {
        let Some(it) = self.world.get(id) else { return };
        let tf = self.data.typeflags.get(it.shape);
        if tf.has(flag::FIXED) || tf.weight == 0 || it.flags & item_flag::HANGING != 0 {
            return;
        }
        self.hurl(id, (0, 0, 0), GRAVITY, None);
    }

    fn finish_all(&mut self, pids: impl IntoIterator<Item = Pid>) {
        if let Some(s) = &mut self.scripts {
            pids.into_iter().for_each(|p| s.vm.finish(p, 0));
        }
    }

    /// One tick of everything in the air.
    pub(crate) fn fly(&mut self) {
        let mut flights = std::mem::take(&mut self.flights);
        let mut landed = vec![];
        flights.retain_mut(|f| {
            let Some(it) = self.world.get(f.id).filter(|_| !self.world.is_void(f.id)) else {
                landed.push((f.id, 0, std::mem::take(&mut f.pids), None));
                return false;
            };
            f.top = f.top.max(it.z);
            if it.z <= 0 && f.v.2 < 0 {
                landed.push((
                    f.id,
                    f.top - it.z,
                    std::mem::take(&mut f.pids),
                    Some((f.from, std::mem::take(&mut f.touching))),
                ));
                return false;
            }
            let to = (it.x + f.v.0, it.y + f.v.1, (it.z + f.v.2).max(0));
            let Some((hit, dirs)) = self.world.sweep(&self.data, f.id, to) else {
                f.v.2 -= f.gravity; // free flight
                return true;
            };
            if dirs & 4 != 0 && f.v.2 < 0 {
                // Came down on something.
                let actor = it.npc != 0;
                let floor = self.world.index(hit).is_some_and(|i| self.world.entry(i).traits.land);
                let rests = self.world.fits(&self.data, f.id, self.world.get(f.id).map_or(to, |i| (i.x, i.y, i.z)));
                let mut done = true;
                if !rests {
                    // On an edge: slide off.
                    done = false;
                    f.v.2 = 0;
                    if f.v.0.abs() < 16 {
                        f.v.0 = if f.v.0 >= 0 { 16 } else { -16 };
                    }
                    if f.v.1.abs() < 16 {
                        f.v.1 = if f.v.1 >= 0 { 16 } else { -16 };
                    }
                } else if f.v.2 < -2 && !actor && (!floor || f.v.2 < -2 * f.gravity) {
                    // A bounce, off at an angle.
                    f.v.2 = -f.v.2 / 3;
                    let speed = f.v.0.abs() + f.v.1.abs() + f.v.2;
                    let turn = (self.world.roll(1000) as f64 / 1000.0 - 0.5) * std::f64::consts::PI;
                    let heading = (f.v.1 as f64).atan2(f.v.0 as f64) + turn;
                    f.v.0 += (heading.cos() * speed as f64) as i32;
                    f.v.1 += (heading.sin() * speed as f64) as i32;
                    if floor {
                        (f.v.0, f.v.1, f.v.2) = (f.v.0 / 4, f.v.1 / 4, f.v.2 / 2);
                        done = f.v.2 == 0;
                    } else {
                        done = false;
                        if f.v.0.abs() > 2 {
                            f.v.0 /= 2;
                        }
                        if f.v.1.abs() > 2 {
                            f.v.1 /= 2;
                        }
                    }
                }
                let z = self.world.get(f.id).map_or(0, |i| i.z);
                if done {
                    landed.push((
                        f.id,
                        f.top - z,
                        std::mem::take(&mut f.pids),
                        Some((f.from, std::mem::take(&mut f.touching))),
                    ));
                    return false;
                }
                f.top = z; // touched down, bouncing on
                return true;
            }
            // A wall (or a ceiling): back off at half speed.
            if dirs & 1 != 0 {
                f.v.0 = -f.v.0 / 2;
            }
            if dirs & 2 != 0 {
                f.v.1 = -f.v.1 / 2;
            }
            if dirs & 4 != 0 {
                f.v.2 = -f.v.2 / 2;
            }
            true
        });
        flights.append(&mut self.flights); // any started meanwhile
        self.flights = flights;
        for (id, height, pids, from) in landed {
            if let (Some((f, touching)), Some(it)) = (from, self.world.get(id)) {
                // Came down somewhere: what it lands on knows (a plate, say).
                if id != crate::world::AVATAR {
                    self.touched(id, &touching, (it.x - f.0, it.y - f.1, it.z - f.2));
                }
            }
            self.landed(id, height);
            self.finish_all(pids);
        }
    }

    /// `id` came down from `height` above: a person falling far is hurt (or killed), then lands.
    fn landed(&mut self, id: ObjId, height: i32) {
        let alive = self.stats.get(id).filter(|s| !s.dead && self.world.get(id).is_some_and(|it| it.npc != 0));
        let Some(s) = alive else { return };
        let dir = self.world.get(id).and_then(|it| crate::avatar::facing(&self.data, &it)).unwrap_or(0);
        if height >= HURT_FROM {
            let damage = if height < KILL_FROM { (height - 72) / 4 } else { s.hp as i32 };
            self.receive_hit(id, 0, dir, damage, dmg::FALLING | dmg::PIERCE);
        }
        if id != crate::world::AVATAR && self.stats.get(id).is_some_and(|s| !s.dead) {
            self.world.play(&self.data, id, action::LAND, dir);
        }
    }

    /// `id` blows up: a burst and a bang where it was, gone (if `destroy`), and everything near
    /// takes a blunt, fiery blow.
    pub fn explode(&mut self, id: ObjId, destroy: bool) {
        let Some(it) = self.world.get(id) else { return };
        let (shape, first, last) = BLAST_SPRITE;
        let pid = self.scripts.as_mut().map_or(0, |s| s.vm.spawn_external(0, 0));
        self.start_sprite(pid, shape, (first, last), 1, 1, (it.x, it.y, it.z));
        let sfx = BLAST_SFX[self.world.roll(2) as usize];
        if let Some(s) = &mut self.scripts {
            s.vm.events.push_back(crate::usecode::UiEvent::Sfx { sfx, item: 0, looping: false });
        }
        if destroy {
            self.world.destroy(&self.data, id);
        }
        let near: Vec<ObjId> = self.world.area(it.x, it.y, BLAST).into_iter().filter(|&o| o != id).collect();
        for o in near {
            let Some(t) = self.world.get(o).filter(|t| (t.z - it.z).abs() <= BLAST && !self.world.is_void(o)) else {
                continue;
            };
            let dir = dir_towards(t.x - it.x, t.y - it.y) as usize;
            let damage = BLAST_DAMAGE.0 + self.world.roll(BLAST_DAMAGE.1 - BLAST_DAMAGE.0 + 1);
            self.receive_hit(o, 0, dir, damage as i32, dmg::BLUNT | dmg::FIRE);
        }
    }

    /// Is `id` the kind of thing that explodes when struck (TYPEFLAG)?
    pub(crate) fn explosive(&self, id: ObjId) -> bool {
        self.world.get(id).is_some_and(|it| self.data.typeflags.raw(it.shape)[5] & 0x20 != 0)
    }
}

/// Oil flasks (igniteChaos sets them off), and how far round the point it reaches.
const OIL_FLASK: u32 = 592;

impl Game {
    /// igniteChaos(x, y): every oil flask within reach of the point is used (lit).
    pub(crate) fn ignite(&mut self, x: i32, y: i32) {
        let flasks: Vec<ObjId> = self
            .world
            .area(x, y, BLAST)
            .into_iter()
            .filter(|&o| self.world.get(o).is_some_and(|it| it.shape == OIL_FLASK))
            .collect();
        for f in flasks {
            self.trigger(f, crate::usecode::event::USE);
        }
    }
}
