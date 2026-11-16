//! What scripts tell the host: things the UI must show ([`UiEvent`]) and work the game does
//! for them ([`Request`]).
use super::*;

/// Things the UI must show; it answers by calling [`Vm::finish`] (or [`Vm::answer`]) with the pid.
#[derive(Clone, Debug)]
pub enum UiEvent {
    /// Show a container's (or the Avatar's) window: `gump` is its U8GUMPS.FLX shape.
    OpenGump {
        item: ObjId,
        gump: u16,
    },
    CloseGump {
        item: ObjId,
    },
    /// Every item's window (containers, the paperdoll) closes.
    CloseItemGumps,
    /// getSliderInput: ask for a number from `min` to `max` in steps of `step`; finish `pid` with it.
    Slider {
        pid: Pid,
        min: i16,
        max: i16,
        step: i16,
    },
    Bark {
        pid: Pid,
        item: ObjId,
        text: String,
    },
    Ask {
        pid: Pid,
        item: ObjId,
        options: Vec<String>,
    },
    /// Text to read: a book, a scroll, or a grave or plaque (whose U8GUMPS shape is `gump`).
    Read {
        pid: Pid,
        kind: ReadKind,
        item: ObjId,
        gump: u16,
        text: String,
    },
    /// Play MUSIC.FLX entry n (0 = stop).
    Music(u8),
    /// Play effect `sfx` (at `item`'s position if non-zero); `looping` until stopped.
    Sfx {
        sfx: u16,
        item: ObjId,
        looping: bool,
    },
    /// Stop an effect (only the one at `item`, if given).
    StopSfx {
        sfx: u16,
        item: Option<ObjId>,
    },
    /// Effect volume, 0–255.
    SfxVolume {
        sfx: u16,
        volume: u8,
    },
    /// Pick something on screen with the targeting cursor (spells, keys, a thing to use on);
    /// finish `pid` with its id, or 0 if the player declines.
    Target {
        pid: Pid,
    },
    /// The game is won: play the ending film, then the main menu (Game::playEndgame).
    Endgame,
    /// An enemy of the Avatar fell while it fought: the victory fanfare (MUSIC.FLX 109), then
    /// the combat music again.
    Victory,
}

/// Work a script hands to the game; the game finishes `pid` when it's done (result 1 = success).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    /// An actor teleported within the map by a script running for item `by` (what shows where
    /// people are may need redrawing).
    Teleported { id: ObjId, by: ObjId },
    /// Walk `actor` until within `range` of `to`.
    Walk { pid: Pid, actor: ObjId, to: (i32, i32, i32), range: i32 },
    /// Point the camera: follow an item, or (None) hold still.
    CameraFollow(Option<ObjId>),
    /// Glide the camera to a point; `pid` finishes on arrival.
    CameraScroll { pid: Pid, to: (i32, i32, i32) },
    /// Camera::moveTo: look at this point at once (and stay there).
    CameraMove((i32, i32, i32)),
    /// Item::receiveHit: `target` is struck by `by` from direction `dir` (0 damage: the
    /// striker's usual).
    Hit { target: ObjId, by: ObjId, dir: usize, damage: i32, kind: u16 },
    /// hurl(xs, ys, zs, gravity): send a thing flying; `pid` ends when it comes to rest.
    Hurl { id: ObjId, v: (i32, i32, i32), gravity: i32, pid: Pid },
    /// fall: drop, if nothing holds it up.
    Fall(ObjId),
    /// explode: blow up (and go, if `destroy`), hurting what's near.
    Explode { id: ObjId, destroy: bool },
    /// igniteChaos(x, y): set off the oil flasks lying about there.
    Ignite { x: i32, y: i32 },
    /// A script made an actor in NPC slot n (the game frees that slot on other maps).
    Created(ObjId),
    /// teleportToEgg(map, id): the Avatar to teleport egg `id` on `map`.
    TeleportToEgg { map: u16, id: u8 },
    /// Actor::teleport of the Avatar to another map: there at `at`. The script that asked, `by`,
    /// waits (on `pid`) and carries on there, as the one process the map change spares.
    AvatarTo { map: u16, at: (i32, i32, i32), by: crate::usecode::Pid, pid: crate::usecode::Pid },
    /// TonysBalls(_, _, x, y, z): a fireball there, after the Avatar.
    Fireball((i32, i32, i32)),
    /// castGrantPeace: the Avatar casts Grant Peace (the game asks for a target).
    GrantPeace,
    /// A script moved `id` and its contact events ran: what it touches now is its footing.
    Moved(ObjId),
    /// MonsterEgg::hatch: bring the egg's monster into the world (the egg as it was: scripts
    /// often destroy it straight after).
    Hatch(Item),
    /// Actor::setActivity(0): wander about.
    Loiter(ObjId),
    /// accumulateStr/Int/Dex: use towards the Avatar's next point (0 STR, 1 DEX, 2 INT).
    Grow(usize, u32),
    /// feedAvatar: food eaten (sates hunger, which heals).
    Feed(u16),
    /// setTimeInGameHours: the clock jumps to this many game hours since the start.
    SetHour(u16),
    /// createSprite: an effect (`shape` frames `first..=last`, `repeats` times, a frame every
    /// `delay` ticks) at a point; `pid` ends when it does.
    Sprite { pid: Pid, shape: u32, first: u32, last: u32, repeats: u32, delay: u32, at: (i32, i32, i32) },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ReadKind {
    Book,
    Scroll,
    Grave,
    Plaque,
}
