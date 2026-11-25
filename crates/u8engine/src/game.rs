//! A play session: game data, the current map's [`World`], the [`Avatar`] and the [`Scripts`].
//! Hosts (the viewer, tests, tools) call [`Game::update`] with elapsed time and player input,
//! forward [`UiEvent`]s to their UI, and draw the world however they like.
use crate::avatar::{Avatar, Effort, Input};
use crate::camera::Camera;
use crate::eggs::{Hatch, Hatcher};
use crate::equip;
use crate::fast_area::FastArea;
use crate::npc::Walk;
use crate::stats::Stats;
use crate::usecode::{event, Request, Scripts, UiEvent, Vm, WorldHost};
use crate::world::{item_flag, item_flag::GUMP_OPEN, ObjId, Origin, World, AVATAR};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use u8data::typeflag::flag;
use u8data::{save::Save, GameData};

mod clock;
mod events;
mod items;
mod travel;

pub(crate) use items::missile;
pub use items::Place;

/// Game ticks per second (the original's clock: scripts and egg checks run on it).
pub const TICK_HZ: f32 = 30.0;
/// Ticks between fast-area checks.
const FAST_AREA_EVERY: u32 = 10;
/// Real seconds per game day: a game hour is 15 real minutes, as in the DOS game (its clock,
/// KERNEL.DAT entry 20, runs 30 a second, 27000 to the hour).
pub const DAY_SECONDS: u32 = 24 * 900;
/// The clock's counts a second: one a game tick.
const CLOCK_HZ: u32 = 30;
/// Clock counts per game minute.
const TICKS_PER_MINUTE: u32 = DAY_SECONDS * CLOCK_HZ / (24 * 60);
/// Clock for a new game without a save: 9 in the morning of day one.
const NEW_GAME_TICKS: u32 = 9 * 60 * TICKS_PER_MINUTE;
/// Never simulate more than this much time in one update (after a stall, skip rather than race).
const MAX_CATCH_UP: f32 = 0.5;

pub struct Game {
    pub data: GameData,
    pub world: World,
    pub avatar: Option<Avatar>,
    pub scripts: Option<Scripts>,
    maps: Vec<u16>,
    /// Maps visited this session, as they were left (opened doors stay open).
    pub(crate) visited: HashMap<u16, World>,
    hatcher: Hatcher,
    pub(crate) fast: FastArea,
    /// NPCs walking where scripts sent them.
    pub(crate) walks: Vec<Walk>,
    /// What people in combat are up to (see [`fight`](crate::fight)).
    pub(crate) fights: HashMap<ObjId, crate::fight::Fighter>,
    /// People wandering about (activity 0; see [`spawn`](crate::spawn)).
    pub(crate) loiters: HashMap<ObjId, crate::spawn::Loiter>,
    /// People speaking: the speech or question's process, and who (see [`Game::gesture`]).
    pub(crate) talkers: Vec<(crate::usecode::Pid, ObjId)>,
    /// The tick each talker may gesture again (a rest of one to three seconds between).
    pub(crate) talk_rest: HashMap<ObjId, u32>,
    /// The dead who'll rise again, and ticks till they do.
    pub(crate) rising: Vec<(ObjId, u32)>,
    /// The Avatar's healing and hunger (see [`heal`](crate::heal)).
    pub(crate) recovery: crate::heal::Recovery,
    /// A Grant Peace being cast (see [`peace`](crate::peace)).
    pub(crate) peace: Option<crate::peace::GrantPeace>,
    /// Fireballs after the Avatar (see [`fireball`](crate::fireball)).
    pub(crate) fireballs: Vec<crate::fireball::Fireball>,
    /// Effects playing out (see [`sprites`](crate::sprites)).
    pub(crate) sprites: Vec<crate::sprites::Sprite>,
    /// Things in the air (see [`gravity`](crate::gravity)).
    pub(crate) flights: Vec<crate::gravity::Flight>,
    /// People's strength, hit points… (NPCDATA, as play changes them).
    pub stats: Stats,
    /// The Avatar died: the game is over until a save is loaded.
    pub(crate) avatar_dead: bool,
    /// Use built up towards the Avatar's next STR, DEX, INT point (see `combat::grow`).
    pub(crate) growth: [u32; 3],
    /// Where people last were and what they touched there (see [`touch`](crate::touch)).
    pub(crate) footing: HashMap<ObjId, crate::touch::Footing>,
    camera: Camera,
    /// The roof the view is cut under, and where the Avatar was when it was looked up.
    roof: (Option<i32>, Option<(i32, i32, i32)>),
    /// Ticks played since the game began (the game clock; continues from the save).
    clock: u32,
    /// Map items just loaded get `cachedIn` on the next tick (map music). Actors don't: they
    /// live in the item cache, not the map.
    cache_in: bool,
    /// A new game: hatch the FIRST egg on the first tick (the opening scene).
    new_game: bool,
    /// Game hour the NPC schedules last ran for (None: run them on the next tick).
    scheduled: Option<u32>,
    /// The hour each map's people were last scheduled for (a map not listed: `scheduled` as the
    /// game was loaded). Arriving where an hourly run has been missed catches it up.
    map_scheduled: HashMap<u16, Option<u32>>,
    /// `scheduled` as the game started (what maps not yet scheduled are taken to have had).
    loaded_scheduled: Option<u32>,
    tick: u32,
    tick_clock: f32,
    map_changed: bool,
    /// The "Restored content" option and what it has done (see [`restore`](crate::restore)).
    pub restored: crate::restore::Restore,
    /// The view's shake while a script's quake runs (stepped each tick).
    pub shake: crate::camera::Shake,
}

/// What an update did, so the host knows whether to redraw and keep the frames coming.
#[derive(Clone, Copy, Debug, Default)]
pub struct Status {
    /// The Avatar moved or changed frame (a following camera should recentre).
    pub avatar_moved: bool,
    /// The Avatar is walking or jumping (keep frames coming).
    pub moving: bool,
    /// Scripts are running (keep ticking, even if nothing on screen changes).
    pub scripting: bool,
    /// Now on another map (teleported): the host should reset its view, UI and music.
    pub map_changed: bool,
}

impl Game {
    /// Start on `map`, else wherever the saved game left the Avatar, else map 3.
    pub fn new(data: GameData, scripts: Option<Scripts>, map: Option<u16>) -> Result<Self, String> {
        let maps: Vec<u16> = (0..data.fixed.len())
            .filter(|&m| data.fixed.get(m).is_some_and(|d| d.len() >= 16))
            .map(|m| m as u16)
            .collect();
        let first = *maps.first().ok_or("FIXED.DAT has no maps")?;
        let want = map.or_else(|| data.avatar().map(|(m, _)| m as u16)).unwrap_or(3);
        let start = if maps.contains(&want) {
            want
        } else {
            eprintln!("map {want} is empty; starting at map {first}");
            first
        };
        let mut world = World::build(&data, start);
        world.clear_flag(GUMP_OPEN); // a save's open windows aren't restored
        let ticks = data.clock.unwrap_or(NEW_GAME_TICKS);
        let stats = Stats::load(data.npcdata.as_deref());
        let mut g = Game {
            stats,
            avatar_dead: false,
            growth: [0; 3],
            footing: HashMap::new(),
            avatar: None,
            world,
            data,
            scripts,
            maps,
            visited: HashMap::new(),
            hatcher: Hatcher::default(),
            fast: FastArea::default(),
            walks: vec![],
            fights: HashMap::new(),
            loiters: HashMap::new(),
            talkers: vec![],
            talk_rest: HashMap::new(),
            rising: vec![],
            recovery: Default::default(),
            peace: None,
            fireballs: vec![],
            sprites: vec![],
            flights: vec![],
            camera: Camera::default(),
            roof: (None, None),
            clock: ticks,
            scheduled: None,
            map_scheduled: HashMap::new(),
            loaded_scheduled: None,
            cache_in: true,
            new_game: false,
            tick: 0,
            tick_clock: 0.0,
            map_changed: false,
            restored: Default::default(),
            shake: Default::default(),
        };
        g.new_game = g.data.clock.is_none(); // no kernel state: the start of a new game
        if g.new_game {
            g.dress_new_avatar();
        }
        g.resume_fights(true);
        g.avatar_dead = g.stats.get(AVATAR).is_some_and(|s| s.dead);
        g.avatar = Avatar::new(&g.data, &g.world);
        Ok(g)
    }

    /// Open an install's game: saved game `slot` from SAVEGAME/, or (None) the one in GAMEDAT.
    /// Scripts are optional: without EUSECODE.FLX the world still works, it just doesn't talk.
    pub fn open(root: &Path, slot: Option<u32>, map: Option<u16>) -> Result<Self, String> {
        // No game in GAMEDAT (a fresh install has none: DOS makes it from the first save when a
        // game begins): a new game, from SAVEGAME/U8SAVE.000 as the original and ScummVM start it.
        let fresh = slot.is_none() && !Self::has_game(root);
        let from = if fresh { Some(0) } else { slot };
        let save = from
            .map(|n| {
                Save::load(root, n).map_err(|e| match fresh {
                    true => format!("no game to start: GAMEDAT holds none and SAVEGAME/U8SAVE.000 can't be read ({e})"),
                    false => format!("save {n}: {e}"),
                })
            })
            .transpose()?;
        let gd = GameData::load_save(root, save.as_ref()).map_err(|e| format!("loading {}: {e}", root.display()))?;
        let scripts = Scripts::load(root, &gd).map_err(|e| eprintln!("scripts disabled: {e}")).ok();
        let mut g = Self::new(gd, scripts, map)?;
        // A saved game's people are where its last hourly schedule left them: theirs runs again
        // when the hour turns (as the original's scheduler does), not straight away.
        if slot.is_some() {
            let hour = Some(g.minutes() / 60);
            (g.scheduled, g.loaded_scheduled) = (hour, hour);
        }
        // What our side file kept (if it goes with this very save).
        if let Some(p) = slot.and_then(|n| u8data::find_file(root, &format!("SAVEGAME/U8SAVE.{n:03}")).ok()) {
            if let (Ok(d), Ok(text)) = (std::fs::read(&p), std::fs::read_to_string(crate::extra::path_for(&p))) {
                g.apply_extra(&text, crate::extra::fingerprint(&d));
            }
        }
        Ok(g)
    }

    /// Does GAMEDAT hold a game (the map's items, the item cache and the people)?
    fn has_game(root: &Path) -> bool {
        ["NONFIXED.DAT", "ITEMCACH.DAT", "NPCDATA.DAT"]
            .iter()
            .all(|f| u8data::find_file(root, &format!("GAMEDAT/{f}")).is_ok())
    }

    /// The game as it stands, as a save (see [`persist`](crate::persist)).
    pub fn snapshot(&self) -> Save {
        let globals = self.scripts.as_ref().map(|s| &s.vm.globals[..]);
        // Who's fighting is the worlds' to say (for those in one; the current map's last).
        let mut people = self.stats.clone();
        for w in self.visited.values().chain(std::iter::once(&self.world)) {
            for id in (1..256).filter(|&id| w.get(id).is_some()) {
                if let Some(s) = people.get_mut(id) {
                    s.in_combat = w.in_combat(id);
                }
            }
        }
        crate::persist::snapshot(
            &self.data,
            std::iter::once(&self.world).chain(self.visited.values()),
            &people,
            globals,
            self.clock,
        )
    }

    /// Save to SAVEGAME/U8SAVE.<slot> (`desc` shows in the diary).
    pub fn save(&self, root: &Path, slot: u32, desc: &str) -> Result<PathBuf, String> {
        let save = self.snapshot();
        let p = u8data::save::write(root, slot, desc, &save).map_err(|e| format!("saving to slot {slot}: {e}"))?;
        let print =
            crate::extra::fingerprint(&std::fs::read(&p).map_err(|e| format!("reading back {}: {e}", p.display()))?);
        std::fs::write(crate::extra::path_for(&p), self.extra(print))
            .map_err(|e| format!("saving the side file for slot {slot}: {e}"))?;
        Ok(p)
    }

    pub fn vm(&mut self) -> Option<&mut Vm> {
        self.scripts.as_mut().map(|s| &mut s.vm)
    }

    /// The Avatar's armour class (the paperdoll's ARMR).
    pub fn armour_class(&self) -> i32 {
        equip::armour_class(&self.data, &self.world, AVATAR)
    }

    /// The Avatar is in combat stance.
    pub fn in_combat(&self) -> bool {
        self.world.in_combat(AVATAR)
    }

    /// A cutscene holds the Avatar: player movement is ignored.
    pub fn in_stasis(&self) -> bool {
        self.scripts.as_ref().is_some_and(|s| s.vm.stasis)
    }

    /// Scripts are running or waiting on the UI.
    pub fn scripting(&self) -> bool {
        self.scripts.as_ref().is_some_and(|s| s.vm.running() > 0)
    }

    /// Advance `dt` seconds: Avatar movement, scenery animation and scripts.
    pub fn update(&mut self, dt: f32, input: &Input) -> Status {
        let dt = dt.clamp(0.0, MAX_CATCH_UP);
        // (Not steered while falling.)
        let falling = self.flights.iter().any(|f| f.id == AVATAR);
        let input = if self.in_stasis() { Input::default() } else { *input };
        let strength = self.stats.get(AVATAR).map_or(0, |s| s.str as i32);
        let avatar_moved = match &mut self.avatar {
            // (Falling, only the move button counts: held, he may catch a ledge.)
            Some(a) if !self.avatar_dead && falling => a.keep_holding(&mut self.world, input.dir.is_some()),
            Some(a) if !self.avatar_dead => {
                a.strength = strength;
                a.update(&self.data, &mut self.world, &input, dt)
            }
            _ => false,
        };
        if let Some((sure, dir)) = self.avatar.as_mut().and_then(Avatar::take_bump) {
            // Ran into something: it hurts, for sure or 1 time in 3 (ANIMPRIM.C).
            if sure || self.world.roll(3) == 0 {
                self.receive_hit(AVATAR, 0, dir, 1, crate::combat::dmg::PIERCE);
            }
        }
        if let Some(a) = &self.avatar {
            self.world.set_last_anim(AVATAR, a.action());
        }
        if self.avatar.as_mut().is_some_and(Avatar::take_drop) {
            self.fall(AVATAR);
        }
        self.step_contacts(AVATAR);
        match self.avatar.as_mut().and_then(Avatar::take_effort) {
            // Climbing builds strength and dexterity by the height; swings and kicks a little.
            Some(Effort::Climb(k)) => {
                self.grow(0, 2 + k);
                self.grow(1, 2 * (2 + k));
            }
            Some(Effort::Strike) => {
                let (s, d) = (1 + self.world.roll(2), 2 + self.world.roll(2));
                self.grow(0, s);
                self.grow(1, d);
            }
            None => {}
        }
        if let Some((dir, range)) = self.avatar.as_mut().and_then(Avatar::take_swing) {
            if self.swing(AVATAR, dir, range) {
                self.avatar.as_mut().map(Avatar::struck);
            }
        }
        self.tick_clock += dt;
        while self.tick_clock >= 1.0 / TICK_HZ {
            self.tick_clock -= 1.0 / TICK_HZ;
            self.tick();
        }
        // The frames' sounds (the Avatar's, everyone else's).
        let mine = self.avatar.as_mut().map(Avatar::take_sounds).unwrap_or_default();
        for (sfx, who, step) in mine.into_iter().map(|(s, step)| (s, AVATAR, step)).chain(self.world.take_sounds()) {
            match step {
                Some(run) => self.ui_event(UiEvent::Step { sfx, item: who, run }),
                None => self.sfx(sfx, who),
            }
        }
        if let Some(s) = &mut self.scripts {
            if s.vm.running() == 0 {
                // Never leave the player frozen, or the camera looking elsewhere, if a script
                // died mid-scene.
                s.vm.stasis = false;
                self.camera.reset();
            }
        }
        if let (Some(r), Some(a)) = (self.fresh_roof(), self.world.get(AVATAR)) {
            self.roof = (r, Some((a.x, a.y, a.z)));
        }
        Status {
            avatar_moved: avatar_moved || self.map_changed,
            moving: self.avatar.as_ref().is_some_and(Avatar::busy),
            scripting: self.scripting(),
            map_changed: std::mem::take(&mut self.map_changed),
        }
    }

    /// Play sound `sfx` (from `item`, 0 for none).
    pub(crate) fn sfx(&mut self, sfx: u16, item: ObjId) {
        self.ui_event(UiEvent::Sfx { sfx, item, looping: false });
    }

    /// One game tick: eggs, the fast area (a few times a second), then scripts.
    fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        if std::mem::take(&mut self.new_game) {
            self.start_new_game();
        }
        self.quake_ends();
        if std::mem::take(&mut self.cache_in) {
            let ids: Vec<ObjId> = self.world.objects().filter(|&id| id >= 256).collect();
            for id in ids {
                self.trigger(id, event::CACHED_IN);
            }
            self.restore();
        } else if self.tick.is_multiple_of(30) {
            self.restore();
        }
        self.clock += CLOCK_HZ / TICK_HZ as u32;
        self.shake.step(self.scripts.as_ref().map_or(0, |s| s.vm.quake));
        for id in self.world.tick_acting(&self.data) {
            self.fall(id);
        }
        for id in self.world.animate(&self.data, self.tick) {
            if self.fast.contains(&self.world, id) {
                self.trigger(id, event::ANIM);
            }
        }
        for h in self.hatcher.check(&mut self.world) {
            match h {
                Hatch::Usecode(id) => {
                    self.trigger(id, event::HATCH);
                }
                Hatch::Teleport { map, id } => {
                    if self.teleport(map, id) {
                        return; // a new place: its eggs are checked next tick
                    }
                }
            }
        }
        if self.tick % FAST_AREA_EVERY == 1 || self.map_changed {
            self.refresh_fast();
        }
        self.schedule();
        self.walk();
        self.loiter();
        self.rise();
        self.heal();
        self.grant_peace();
        self.fly_fireballs();
        self.tick_sprites();
        self.fly();
        self.feign();
        self.fight();
        self.people_contacts();
        self.run_scripts();
    }

    /// The opening: the original hatches the "FIRST" usecode egg (shape 73, quality 36) that sits
    /// near the shore where a new game starts.
    fn start_new_game(&mut self) {
        let first =
            self.world.objects().find(|&id| self.world.get(id).is_some_and(|it| it.shape == 73 && it.quality == 36));
        match first {
            Some(id) => {
                self.trigger(id, event::HATCH);
            }
            None => eprintln!("new game: no FIRST egg on map {}", self.map()),
        }
    }

    /// The new game's Avatar as the original has him: the magic armour the first save puts on
    /// him gone (DOS shows ARMR 0, no armour and WGHT 1), and the pack he carries worn as his
    /// backpack (DOS gives him an empty one the first time it's opened).
    fn dress_new_avatar(&mut self) {
        let worn = |w: &World, id| w.get(id).is_some_and(|it| it.flags & item_flag::EQUIPPED != 0);
        let mut packed = self
            .world
            .contents(AVATAR)
            .iter()
            .any(|&id| worn(&self.world, id) && equip::holds(&self.data, &self.world, id));
        for id in self.world.contents(AVATAR) {
            let Some(mut it) = self.world.get(id) else { continue };
            if it.shape == equip::STARTING_ARMOUR {
                self.world.destroy(&self.data, id);
            } else if !packed && equip::holds(&self.data, &self.world, id) {
                packed = true; // (one pack)
                (it.x, it.y, it.z) =
                    (equip::BACKPACK_SPOT.0, equip::BACKPACK_SPOT.1, equip::worn_z(&self.data, it.shape));
                it.flags |= item_flag::EQUIPPED;
                self.world.set(&self.data, id, it);
            }
        }
    }

    /// A new game whose Avatar has no name yet (the original asks before the opening).
    pub fn needs_name(&self) -> bool {
        self.new_game && self.data.avatar_name.is_none()
    }

    /// Name the Avatar: scripts say it from now on, and saves keep it.
    pub fn set_avatar_name(&mut self, name: &str) {
        self.data.avatar_name = Some(name.to_string());
        if let Some(s) = &mut self.scripts {
            s.vm.avatar_name = name.to_string();
        }
        if let Some(mut a) = self.data.state.get("AVATAR.DAT").map(<[u8]>::to_vec) {
            u8data::save::set_avatar_name(&mut a, name);
            self.data.state.set("AVATAR.DAT", a);
        }
    }

    /// Where the view should centre (world coordinates), while it follows the game: on the
    /// Avatar, or wherever a cutscene points it. None if what it follows isn't on this map.
    /// Have the view follow the Avatar again (as after a scene).
    pub fn follow_avatar(&mut self) {
        self.camera.reset();
    }

    pub fn camera(&self) -> Option<(i32, i32, i32)> {
        self.camera.focus(&self.world)
    }

    /// Where the view is cut away (what's at or above it isn't drawn), as the original's camera
    /// keeps it (CAMERA.C): the roof over the Avatar, looked up again whenever it has moved, but
    /// only while the camera is centred on it. A scene that takes the camera off it (Flash, before
    /// parking it at 0,0,0) leaves the cut where it was. None: no roof.
    pub fn roof(&self) -> Option<i32> {
        self.fresh_roof().unwrap_or(self.roof.0).map(|z| z - 1)
    }

    /// The roof looked up again, if it would be now (camera on the Avatar, moved since).
    fn fresh_roof(&self) -> Option<Option<i32>> {
        let a = self.world.get(AVATAR).filter(|_| self.camera_on_avatar())?;
        (self.roof.1 != Some((a.x, a.y, a.z))).then(|| self.world.ceiling(AVATAR))
    }

    /// How much of the world the player sees (game pixels): the fast area grows to cover it
    /// (see [`crate::fast_area`]); never less than the original's 320×200.
    pub fn set_view(&mut self, w: f32, h: f32) {
        self.fast.view = (w, h);
    }

    /// Is the camera following the Avatar (rather than a scene's glide, or something else)?
    pub fn camera_on_avatar(&self) -> bool {
        self.camera.following() == Some(AVATAR)
    }

    /// The Avatar is dead: the game is over until a save is loaded.
    pub fn avatar_dead(&self) -> bool {
        self.avatar_dead
    }
}
