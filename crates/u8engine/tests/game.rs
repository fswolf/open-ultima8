//! The map: moving about and colliding, teleporters, eggs, scenery animation, roofs,
//! climbing, things falling and blowing up.
//! Run against a real install: U8_DATA=/path/to/U8 cargo test -- --ignored
mod common;
use common::*;
use u8data::anim::action;
use u8data::map::Item;
use u8engine::avatar::Plight;
use u8engine::usecode::UiEvent;

#[test]
#[ignore]
fn avatar_walks_and_collides() {
    let Some(mut g) = game(40) else { return };
    let start = g.world.get(AVATAR).expect("Avatar on map 40");
    let walk = Input { dir: Some(3), ..Default::default() };
    for _ in 0..30 {
        g.update(1.0 / 30.0, &walk);
    }
    let end = g.world.get(AVATAR).unwrap();
    assert_ne!((start.x, start.y), (end.x, end.y), "walking south-east should move");
    // Standing still settles back to the stand animation and stops moving.
    let st = (0..15).map(|_| g.update(1.0 / 30.0, &Input::default())).last().unwrap();
    assert!(!st.moving);
}

#[test]
#[ignore]
fn changing_map_stops_scripts() {
    let Some(mut g) = game(40) else { return };
    let door = g.world.objects().find(|&id| g.world.get(id).is_some_and(|it| it.shape == 68)).expect("a door");
    assert!(g.trigger(door, u8engine::usecode::event::USE));
    g.update(1.0 / 30.0, &Input::default());
    g.step_map(1);
    assert!(!g.scripting(), "scripts from the old map must not run on the new one");
}

#[test]
#[ignore]
fn teleporters_round_trip_and_keep_map_state() {
    let Some(mut g) = game(40) else { return };
    let still = Input::default();
    // Open a door so we can tell the map was kept, not reloaded.
    let door = g.world.objects().find(|&id| g.world.get(id).is_some_and(|it| it.shape == 68)).expect("a door");
    g.world.set(&g.data, door, u8data::map::Item { frame: 7, ..g.world.get(door).unwrap() });
    // Step onto the exit teleporter to map 3.
    let mut av = g.world.get(AVATAR).unwrap();
    (av.x, av.y, av.z) = (13048, 15400, 48);
    g.world.set(&g.data, AVATAR, av);
    let st = g.update(1.0 / 30.0, &still);
    assert!(st.map_changed);
    assert_eq!(g.map(), 3);
    let there = g.world.get(AVATAR).expect("Avatar came along");
    assert_eq!((there.x, there.y, there.z), (11532, 1980, 48), "arrived at destination egg 0");
    // Standing on arrival doesn't bounce us back.
    for _ in 0..30 {
        assert!(!g.update(1.0 / 30.0, &still).map_changed);
    }
    // Walk back into map 3's teleporter (north-east).
    let walk = Input { dir: Some(1), ..Default::default() };
    assert!((0..120).any(|_| g.update(1.0 / 30.0, &walk).map_changed), "should reach the teleporter");
    assert_eq!(g.map(), 40);
    assert_eq!(g.world.get(door).map(|d| d.frame), Some(7), "map 40 kept its state");
}

#[test]
#[ignore]
fn eggs_are_in_the_world_but_not_physical() {
    let Some(g) = game(40) else { return };
    assert!(g.world.eggs().len() > 20);
    let egg = g.world.entry(g.world.eggs()[0]);
    assert!(egg.present() && !egg.shown());
}

#[test]
#[ignore]
fn the_hourglass_turns_by_its_script_and_still_things_stay_still() {
    // TYPEFLAG byte 4 low nibble is the animation type: the hourglass (227) is type 5, its own
    // `anim` script turns it (with a sound); candlesticks and shores have none.
    let Some(mut g) = game(40) else { return };
    let si = g.data.typeflags.get(227);
    assert_eq!((si.anim_type, si.anim_data, si.anim_speed), (5, 2, 7));
    let still: Vec<_> = g
        .world
        .objects()
        .filter_map(|id| g.world.get(id).map(|it| (id, it)))
        .filter(|(_, it)| g.data.typeflags.get(it.shape).anim_type == 0 && it.npc == 0)
        .take(2000)
        .collect();
    let glass = g.world.objects().find(|&id| g.world.get(id).is_some_and(|it| it.shape == 227));
    let glass = glass.expect("an hourglass on map 40");
    let a = g.world.get(glass).unwrap();
    let mut me = g.world.get(AVATAR).unwrap();
    (me.x, me.y) = (a.x + 96, a.y + 96); // close enough for it to be live
    g.world.set(&g.data, AVATAR, me);
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..30 * 30 {
        g.update(1.0 / 30.0, &Input::default());
        seen.insert(g.world.get(glass).unwrap().frame);
    }
    assert!(seen.len() > 1, "it turned: {seen:?}");
    let moved = still.iter().filter(|(id, it)| g.world.get(*id).is_some_and(|now| now.frame != it.frame)).count();
    assert!(moved < 20, "{moved} things without an animation changed frame");
}

#[test]
#[ignore]
fn trees_overhead_dont_cut_the_view_away_but_roofs_do() {
    let Some(mut g) = game(3) else { return };
    // Under the canopy of the tree north of Devon's camp (shape 199 at 14527, 5631, z 120).
    let mut av = g.data.avatar().map(|(_, it)| it).unwrap();
    (av.x, av.y, av.z, av.mapnum) = (14400, 5500, 8, 3);
    g.world.put_actor(&g.data, av);
    assert_eq!(g.world.ceiling(AVATAR), None, "a tree is not a ceiling");
    // Indoors in Tenebrae (where the save left the Avatar) there is a roof.
    let Some(mut g) = game(40) else { return };
    assert!(g.world.ceiling(AVATAR).is_some(), "a house has a roof");
    // Walking from the carpet in Salkind's study down its steps, the roof stays cut away (a
    // point test found gaps between roof tiles and flashed the roof back).
    g.scripts = None;
    g.put_avatar(41, (20112, 12820, 64));
    for _ in 0..30 {
        g.update(1.0 / 30.0, &Input { dir: Some(4), ..Default::default() });
        assert_eq!(g.roof(), Some(103), "at {:?}", g.world.get(AVATAR).map(|a| (a.x, a.y, a.z)));
    }
    assert_eq!(g.world.get(AVATAR).unwrap().z, 48, "down the steps");
}

#[test]
#[ignore]
fn npc_frames_past_255_come_from_npcdata() {
    // In the save in GAMEDAT, Devon (NPC 2) stands in frame 410: 154 in ITEMCACH (an empty frame
    // on its own) plus 256 from NPCDATA.
    let Some(dir) = std::env::var("U8_DATA").ok().map(PathBuf::from) else { return };
    let gd = GameData::load_fixed(&dir).unwrap();
    let (map, _) = u8data::map::load_actor(gd.itemcach.as_ref().unwrap(), 2).expect("Devon");
    let devon = gd.map_layers(map).actors.into_iter().find(|a| a.npc == 2).unwrap();
    assert_eq!(devon.frame, 410);
    let g = Game::new(gd, None, Some(map as u16)).unwrap();
    assert!(g.world.entry(g.world.index(2).unwrap()).shown(), "he can be seen");
}

#[test]
#[ignore]
fn the_avatar_climbs_onto_things() {
    use u8data::typeflag::flag;
    let Some(mut g) = game(40) else { return };
    let a = g.world.get(AVATAR).unwrap();
    // A solid, standable crate-like thing 24 high, at least 64 across.
    let shape = (0..g.data.typeflags.0.len() as u32)
        .find(|&s| {
            let t = g.data.typeflags.get(s);
            t.has(flag::SOLID) && t.has(flag::LAND) && t.dims() == (64, 64, 24)
        })
        .expect("a box");
    // Somewhere open, the Avatar facing +x (facing 2) with the box right in front.
    let open = |g: &Game, x: i32, y: i32| {
        g.world.area(x + 64, y, 128).into_iter().all(|id| {
            let e = g.world.entry(g.world.index(id).unwrap());
            id == AVATAR || e.z1 <= a.z || e.it.z >= a.z + 80
        })
    };
    let near = (-48..48).flat_map(|i| (-48..48).map(move |j| (i * 32, j * 32)));
    let (dx, dy) =
        near.min_by_key(|&(dx, dy)| if open(&g, a.x + dx, a.y + dy) { dx.abs() + dy.abs() } else { i32::MAX }).unwrap();
    assert!(open(&g, a.x + dx, a.y + dy), "an open spot");
    let mut me = a;
    (me.x, me.y) = (a.x + dx, a.y + dy);
    g.world.set(&g.data, AVATAR, me);
    g.avatar.as_mut().unwrap().dir = 2;
    let bx = u8data::map::Item { shape, x: me.x + 8 + 64, y: me.y, z: me.z, ..Default::default() };
    g.world.add(&g.data, bx, true);
    let tick = |g: &mut Game, input: Input| g.update(1.0 / 30.0, &input);
    tick(&mut g, Input { jump: true, ..Default::default() });
    for _ in 0..60 {
        tick(&mut g, Input::default());
    }
    let up = g.world.get(AVATAR).unwrap();
    assert_eq!(up.z, me.z + 24, "on top ({}, {}, {})", up.x - me.x, up.y - me.y, up.z - me.z);
    assert!(up.x > me.x, "and forward");
}

/// A spot near the Avatar with floor at its feet, and nothing in the way within `r` and from its
/// feet up to `high`.
fn open_spot(g: &mut Game, r: i32, high: i32) -> (i32, i32) {
    let a = g.world.get(AVATAR).unwrap();
    let mut floor_at = |x: i32, y: i32| {
        g.world.set(&g.data, AVATAR, Item { x, y, ..a });
        let f = g.world.floor_under(AVATAR) == Some(a.z);
        g.world.set(&g.data, AVATAR, a);
        f
    };
    let floored: Vec<(i32, i32)> = (-48..48)
        .flat_map(|i| (-48..48).map(move |j| (a.x + i * 32, a.y + j * 32)))
        .filter(|&(x, y)| floor_at(x, y))
        .collect();
    let open = |x: i32, y: i32| {
        g.world.area(x + 64, y, r).into_iter().all(|id| {
            let e = g.world.entry(g.world.index(id).unwrap());
            id == AVATAR || e.z1 <= a.z || e.it.z >= a.z + high
        })
    };
    floored
        .into_iter()
        .filter(|&(x, y)| open(x, y))
        .min_by_key(|&(x, y)| (x - a.x).abs() + (y - a.y).abs())
        .expect("an open spot")
}

/// The 64 × 64 × 24 box.
fn box_shape(g: &Game) -> u32 {
    use u8data::typeflag::flag;
    (0..g.data.typeflags.0.len() as u32)
        .find(|&s| {
            let t = g.data.typeflags.get(s);
            t.has(flag::SOLID) && t.has(flag::LAND) && t.dims() == (64, 64, 24)
        })
        .expect("a box")
}

/// On a box, a gap, then a tower two boxes higher: a jump across falls short and, the move
/// button held, catches the ledge (GRAVITY.C). The game, where the Avatar stood, and where it
/// hangs.
fn hanging_from_a_ledge() -> Option<(Game, Item, Item)> {
    let mut g = game(40)?;
    let (shape, a) = (box_shape(&g), g.world.get(AVATAR).unwrap());
    let (x, y) = open_spot(&mut g, 192, 200);
    let put = |g: &mut Game, x: i32, z: i32| {
        g.world.add(&g.data, u8data::map::Item { shape, x, y: y + 16, z, ..Default::default() }, true);
    };
    put(&mut g, x, a.z);
    for k in 0..3 {
        put(&mut g, x + 128, a.z + 24 * k);
    }
    let mut me = a;
    (me.x, me.y, me.z) = (x, y, a.z + 24);
    g.world.set(&g.data, AVATAR, me);
    g.avatar.as_mut().unwrap().dir = 2;
    tick(&mut g, Input { jump: true, dir: Some(2), ..Default::default() });
    for _ in 0..60 {
        tick(&mut g, HOLD);
        if g.avatar.as_ref().unwrap().hanging() {
            let h = g.world.get(AVATAR).unwrap();
            return Some((g, a, h));
        }
    }
    panic!("didn't catch the ledge: {:?}", g.world.get(AVATAR).map(|i| (i.x - x, i.z - a.z)));
}

/// The move button held, towards the ledge.
const HOLD: Input = Input {
    dir: Some(2),
    run: false,
    careful: false,
    jump: false,
    aim: None,
    combat: false,
    attack: false,
    kick: false,
    block: false,
};

fn tick(g: &mut Game, input: Input) {
    g.update(1.0 / 30.0, &input);
}

#[test]
#[ignore]
fn the_avatar_hangs_from_a_ledge_and_pulls_up() {
    let Some((mut g, a, h)) = hanging_from_a_ledge() else { return };
    assert_eq!(h.z, a.z + 72 - 40, "hands at the top");
    for _ in 0..30 {
        tick(&mut g, HOLD);
    }
    assert!(g.avatar.as_ref().unwrap().hanging(), "still hanging");
    assert_eq!(g.world.get(AVATAR).unwrap().z, h.z);
    tick(&mut g, Input { jump: true, ..HOLD }); // (both buttons)
    for _ in 0..60 {
        tick(&mut g, Input::default()); // (let go once climbing: he stands there)
    }
    let up = g.world.get(AVATAR).unwrap();
    assert_eq!(up.z, a.z + 72, "pulled up onto the top");
    assert!(up.x > h.x && !g.avatar.as_ref().unwrap().hanging());
}

#[test]
#[ignore]
fn letting_go_of_a_ledge_drops() {
    let Some((mut g, a, _)) = hanging_from_a_ledge() else { return };
    let hp = g.stats.get(AVATAR).unwrap().hp;
    for _ in 0..60 {
        tick(&mut g, Input::default()); // (the button let go)
    }
    let down = g.world.get(AVATAR).unwrap();
    assert_eq!(down.z, a.z, "on the ground");
    assert!(!g.avatar.as_ref().unwrap().hanging());
    assert_eq!(g.stats.get(AVATAR).unwrap().hp, hp, "not far enough to hurt");
}

/// The same short jump with the move button let go: no catch; down he drops, and lands.
#[test]
#[ignore]
fn without_the_button_held_a_short_jump_drops() {
    let Some(mut g) = game(40) else { return };
    let (shape, a) = (box_shape(&g), g.world.get(AVATAR).unwrap());
    let (x, y) = open_spot(&mut g, 192, 200);
    let put = |g: &mut Game, x: i32, z: i32| {
        g.world.add(&g.data, u8data::map::Item { shape, x, y: y + 16, z, ..Default::default() }, true);
    };
    put(&mut g, x, a.z);
    for k in 0..3 {
        put(&mut g, x + 128, a.z + 24 * k);
    }
    g.world.set(&g.data, AVATAR, Item { x, y, z: a.z + 24, ..a });
    g.avatar.as_mut().unwrap().dir = 2;
    tick(&mut g, Input { jump: true, dir: Some(2), ..Default::default() });
    let mut landed = false;
    for _ in 0..90 {
        tick(&mut g, Input::default());
        assert!(!g.avatar.as_ref().unwrap().hanging());
        landed |= g.world.last_anim(AVATAR) == action::LAND;
    }
    assert!(landed && g.world.get(AVATAR).unwrap().z == a.z, "dropped and landed");
}

/// The sounds of the Avatar's frames (ANIM.DAT): a grunt climbing anything 40 high or more,
/// and the funny noise of teetering at an edge.
fn sounds(g: &mut Game) -> Vec<u16> {
    let e = g.take_events();
    e.into_iter().filter_map(|e| if let UiEvent::Sfx { sfx, .. } = e { Some(sfx) } else { None }).collect()
}

/// A careful step off an edge teeters there (twice, with a yelp) and stays up; the next one that
/// way goes over (ANIM.C's carefulStepAvatar).
#[test]
#[ignore]
fn a_careful_step_at_an_edge_teeters_first() {
    let Some(mut g) = game(40) else { return };
    let (shape, a) = (box_shape(&g), g.world.get(AVATAR).unwrap());
    let (x, y) = open_spot(&mut g, 192, 200);
    g.world.add(&g.data, Item { shape, x, y: y + 16, z: a.z, ..Default::default() }, true);
    let top = a.z + 24;
    // As far east on the box as he still stands on it.
    let edge = (0..64).rev().step_by(4).find(|k| {
        g.world.set(&g.data, AVATAR, Item { x: x + k, y, z: top, ..a });
        g.world.floor_under(AVATAR) == Some(top)
    });
    g.world.set(&g.data, AVATAR, Item { x: x + edge.unwrap(), y, z: top, ..a });
    g.avatar.as_mut().unwrap().dir = 2;
    sounds(&mut g);
    let careful = Input { dir: Some(2), careful: true, ..Default::default() };
    tick(&mut g, careful);
    let mut seen = vec![];
    for _ in 0..60 {
        tick(&mut g, Input::default());
        seen.push(g.world.last_anim(AVATAR));
    }
    seen.dedup();
    assert_eq!(seen[..2], [action::EDGE_BALANCE, action::STAND], "teetered: {seen:?}");
    assert_eq!(g.world.get(AVATAR).unwrap().z, top, "still up");
    assert!(sounds(&mut g).contains(&147), "with its yelp");
    tick(&mut g, careful);
    for _ in 0..60 {
        tick(&mut g, Input::default());
    }
    assert_eq!(g.world.get(AVATAR).unwrap().z, a.z, "over the edge");
}

#[test]
#[ignore]
fn climbing_high_grunts() {
    let Some(mut g) = game(40) else { return };
    let (shape, a) = (box_shape(&g), g.world.get(AVATAR).unwrap());
    let (x, y) = open_spot(&mut g, 192, 200);
    for k in 0..2 {
        g.world.add(&g.data, Item { shape, x: x + 96, y: y + 16, z: a.z + 24 * k, ..Default::default() }, true);
    }
    // Close enough to climb onto the two boxes (48).
    let near = (0..96).step_by(4).find(|k| {
        g.world.set(&g.data, AVATAR, Item { x: x + k, y, z: a.z, ..a });
        g.world.anim_fits(&g.data, AVATAR, action::CLIMB_16 + 4, 2).is_some()
    });
    assert!(near.is_some(), "in reach");
    g.avatar.as_mut().unwrap().dir = 2;
    sounds(&mut g);
    tick(&mut g, Input { jump: true, dir: Some(2), ..Default::default() });
    for _ in 0..60 {
        tick(&mut g, Input::default());
    }
    assert_eq!(g.world.get(AVATAR).unwrap().z, a.z + 48, "up");
    assert!(sounds(&mut g).contains(&51), "a grunt");
}

#[test]
#[ignore]
fn a_long_fall_knocks_the_avatar_down() {
    let Some(mut g) = game(40) else { return };
    let a = g.world.get(AVATAR).unwrap();
    let (x, y) = open_spot(&mut g, 64, 160);
    let mut me = a;
    (me.x, me.y, me.z) = (x, y, a.z + 100);
    g.world.set(&g.data, AVATAR, me);
    let hp = g.stats.get(AVATAR).unwrap().hp;
    g.fall(AVATAR);
    let (mut flat, mut seen) = (false, vec![]);
    for _ in 0..300 {
        tick(&mut g, Input { dir: Some(2), ..Default::default() });
        let act = g.world.last_anim(AVATAR);
        if seen.last() != Some(&act) {
            seen.push(act);
        }
        flat |= g.avatar.as_ref().unwrap().plight() == Some(Plight::Down);
    }
    let flat_up = seen.windows(3).any(|w| w == [action::FALL_BACKWARDS, action::STAND_UP, action::STAND]);
    assert!(flat_up, "flat, up, standing: {seen:?}");
    assert!(flat);
    assert!(g.avatar.as_ref().unwrap().plight().is_none(), "up again");
    assert_eq!(g.stats.get(AVATAR).unwrap().hp, hp - 7, "(100 - 72) / 4");
    assert_eq!(g.world.get(AVATAR).unwrap().z, a.z);
}

#[test]
#[ignore]
fn jumping_off_a_tower_falls_and_knocks_the_avatar_down() {
    let Some(mut g) = game(40) else { return };
    let (shape, a) = (box_shape(&g), g.world.get(AVATAR).unwrap());
    let (x, y) = open_spot(&mut g, 192, 200);
    for k in 0..4 {
        g.world.add(&g.data, Item { shape, x, y: y + 16, z: a.z + 24 * k, ..Default::default() }, true);
    }
    g.world.set(&g.data, AVATAR, Item { x, y, z: a.z + 96, ..a });
    g.avatar.as_mut().unwrap().dir = 2;
    tick(&mut g, Input { jump: true, dir: Some(2), ..Default::default() });
    let mut flat = false;
    for _ in 0..90 {
        tick(&mut g, Input::default());
        flat |= g.avatar.as_ref().unwrap().plight() == Some(Plight::Down);
    }
    let down = g.world.get(AVATAR).unwrap();
    assert_eq!(down.z, a.z, "came down off it ({})", down.x - x);
    assert!(flat, "a fall of 96: knocked flat");
}

#[test]
#[ignore]
fn things_fall_fly_and_blow_up() {
    let Some(mut g) = game(40) else { return };
    let tick = |g: &mut Game, secs: f32| {
        for _ in 0..(secs * 30.0) as usize {
            g.update(1.0 / 30.0, &Input::default());
        }
    };
    let a = g.world.get(AVATAR).unwrap();
    // A clear spot near the Avatar: floor at its height, nothing above for a while.
    let clear = |g: &Game, x: i32, y: i32| {
        g.world.area(x, y, 48).into_iter().all(|id| {
            let e = g.world.entry(g.world.index(id).unwrap());
            e.z1 <= a.z || e.it.z >= a.z + 40
        })
    };
    let near = (2..12).flat_map(|r| [(r * 32, 0), (-r * 32, 0), (0, r * 32), (0, -r * 32)]);
    let (dx, dy) = near.into_iter().find(|&(dx, dy)| clear(&g, a.x + dx, a.y + dy)).expect("a clear spot");
    // A coin let go in the air comes down onto the floor there.
    let coin =
        u8data::map::Item { shape: 143, quality: 1, x: a.x + dx, y: a.y + dy, z: a.z + 32, ..Default::default() };
    let coin = g.world.add(&g.data, coin, true);
    g.fall(coin);
    tick(&mut g, 2.0);
    let c = g.world.get(coin).unwrap();
    assert_eq!(c.z, a.z, "on the floor ({}, {}, {})", c.x, c.y, c.z);
    // Thrown hard at the floor it bounces before settling.
    g.hurl(coin, (0, 0, 12), 4, None);
    let mut highest = 0;
    for _ in 0..90 {
        tick(&mut g, 1.0 / 30.0);
        highest = highest.max(g.world.get(coin).unwrap().z);
    }
    assert!(highest > a.z + 8, "up it went");
    assert_eq!(g.world.get(coin).unwrap().z, a.z, "and back down");
    // A blast next to a guard hurts him and the coin goes with it.
    let guard = 50;
    let gp = g.world.get(guard).unwrap();
    g.world.move_to_map(&g.data, coin, (gp.x + 64, gp.y, gp.z));
    let hp = g.stats.get(guard).unwrap().hp;
    g.explode(coin, true);
    assert!(g.world.get(coin).is_none());
    assert!(g.stats.get(guard).unwrap().hp < hp, "blunt and fire get through a guard's armour");
}

#[test]
/// Mythran loitering upstairs on the Plateau once climbed a bed and the walls and stepped off
/// into the void: people walk only stairs-sized steps.
fn people_loiter_on_their_own_floor() {
    let Some(mut g) = game(40) else { return };
    assert!(g.go_to("plateau"));
    g.set_minutes(24 * 60 + 60); // in his upstairs room
    let mut a = g.world.get(AVATAR).unwrap();
    (a.x, a.y, a.z) = (1888, 1280, 48);
    g.world.set(&g.data, AVATAR, a);
    let mut z = None;
    for _ in 0..30 * 60 {
        g.update(1.0 / 30.0, &Input::default());
        answer(&mut g);
        let m = g.world.get(33).unwrap();
        assert_eq!(g.world.floor_under(33), Some(m.z), "on a floor at ({}, {}, {})", m.x, m.y, m.z);
        assert!(z.is_none_or(|z: i32| (m.z - z).abs() <= u8engine::world::WALK_STEP), "a step of {:?} → {}", z, m.z);
        z = Some(m.z);
    }
}

#[test]
#[ignore]
fn a_door_opened_into_the_avatar_doesnt_trap_it() {
    let Some(mut g) = game(40) else { return };
    let tick = |g: &mut Game, n, input: &Input| (0..n).for_each(|_| _ = g.update(1.0 / 30.0, input));
    // The nearest door that opens (some are locked), and where it swings to.
    let a = g.world.get(AVATAR).unwrap();
    let mut doors: Vec<_> = g.world.objects().filter(|&id| g.world.get(id).is_some_and(|it| it.shape == 68)).collect();
    doors.sort_by_key(|&d| g.world.get(d).map(|it| (it.x - a.x).abs() + (it.y - a.y).abs()));
    let (door, shut, open) = doors
        .into_iter()
        .find_map(|door| {
            let shut = g.world.get(door)?;
            g.trigger(door, u8engine::usecode::event::USE);
            tick(&mut g, 30, &Input::default());
            let open = g.world.get(door)?;
            ((open.x, open.y, open.frame) != (shut.x, shut.y, shut.frame)).then_some((door, shut, open))
        })
        .expect("a door that opens");
    g.world.set(&g.data, door, shut);
    // The Avatar standing there, the door opened onto it: it can still walk away.
    let at = (open.x - 8, open.y - 8, open.z);
    let walked = (0..8).any(|dir| {
        g.put_avatar(40, at);
        g.world.set(&g.data, door, open);
        tick(&mut g, 20, &Input { dir: Some(dir), ..Default::default() });
        let a = g.world.get(AVATAR).unwrap();
        (a.x - at.0).abs().max((a.y - at.1).abs()) >= 32
    });
    assert!(walked, "stuck in the door");
}

#[test]
#[ignore]
fn a_door_opened_where_the_avatar_stands_sends_it_back_first() {
    // As in the original: the door's script finds the Avatar in its swing (legalCreateAtCoords
    // fails), walks it back a few steps, turns it round, then opens.
    let Some(mut g) = game(40) else { return };
    let tick = |g: &mut Game, n, input: &Input| (0..n).for_each(|_| _ = g.update(1.0 / 30.0, input));
    let a = g.world.get(AVATAR).unwrap();
    let kind = |g: &Game, id| g.world.get(id).map_or((0, 0), |it| (it.shape, it.quality));
    let mut doors: Vec<_> = g
        .world
        .objects()
        .filter(|&id| {
            let (s, q) = kind(&g, id);
            s == 68 && q & 0x100 == 0
        })
        .collect();
    doors.sort_by_key(|&d| g.world.get(d).map(|it| (it.x - a.x).abs() + (it.y - a.y).abs()));
    let (door, shut, open) = doors
        .into_iter()
        .find_map(|door| {
            let shut = g.world.get(door)?;
            g.trigger(door, u8engine::usecode::event::USE);
            // The first step of its swing (where the Avatar will be in the way).
            let open = (0..60).find_map(|_| {
                g.update(1.0 / 30.0, &Input::default());
                g.world.get(door).filter(|o| (o.x, o.y, o.shape) != (shut.x, shut.y, shut.shape))
            })?;
            tick(&mut g, 60, &Input::default());
            Some((door, shut, open))
        })
        .expect("a door that opens");
    let _ = open;
    // Afresh (its script remembers it opened), the Avatar a little into the room, in the
    // swing: one step back clears it.
    let Some(mut g) = game(40) else { return };
    let at = (shut.x - 40, shut.y - 8, shut.z);
    g.put_avatar(40, at);
    assert!(g.trigger(door, u8engine::usecode::event::USE));
    tick(&mut g, 150, &Input::default());
    let (d, av) = (g.world.get(door).unwrap(), g.world.get(AVATAR).unwrap());
    assert_ne!((d.x, d.y, d.shape), (shut.x, shut.y, shut.shape), "it opened");
    assert!((av.x - at.0).abs().max((av.y - at.1).abs()) >= 64, "the Avatar stepped back: {:?}", (av.x, av.y));
    assert!(!g.in_stasis(), "and is free again");
}

#[test]
#[ignore]
fn a_double_door_by_a_glob_wall_opens_with_one_step_back() {
    // Ryan: doors said "blocked" when nothing was in the way (glob walls sat a unit off,
    // overlapping the door's swing), and the Avatar stepped back twice as far as the original
    // (a scripted walk played the whole two-step cycle).
    use u8engine::usecode::UiEvent;
    let Some(mut g) = game(40) else { return };
    let door = g
        .world
        .objects()
        .find(|&id| g.world.get(id).is_some_and(|it| it.shape == 68 && (it.x, it.y) == (12799, 10431)))
        .unwrap();
    let d = g.world.get(door).unwrap();
    let at = (d.x - 40, d.y - 40, d.z);
    let mut a = g.world.get(AVATAR).unwrap();
    (a.x, a.y, a.z) = at;
    g.world.set(&g.data, AVATAR, a);
    g.update(1.0 / 30.0, &Input::default());
    assert!(g.trigger(door, u8engine::usecode::event::USE));
    let mut said = vec![];
    for _ in 0..150 {
        g.update(1.0 / 30.0, &Input::default());
        for e in g.take_events() {
            if let UiEvent::Bark { pid, text, .. } = e {
                said.push(text);
                g.vm().unwrap().finish(pid, 0);
            }
        }
    }
    assert!(!said.iter().any(|t| t.contains("blocked")), "{said:?}");
    assert_ne!(g.world.get(door).unwrap().shape, d.shape, "it opened");
    let av = g.world.get(AVATAR).unwrap();
    assert_eq!((av.x - at.0, av.y - at.1), (-96, 0), "one step back, west");
}

#[test]
#[ignore]
fn a_scripted_walk_is_one_step_each_foot_in_turn() {
    // ScummVM's two-step walk: doAnim(walk) plays half the cycle — frames 4–7, then 8, 1–3.
    let Some(mut g) = game(40) else { return };
    // Somewhere with 256 clear to the west.
    let mut a = g.world.get(AVATAR).unwrap();
    let clear = |g: &Game, x, y, z| (0..=32).all(|k| g.world.fits(&g.data, AVATAR, (x - 8 * k, y, z)));
    let (x, y) = (-20..20)
        .flat_map(|i| (-20..20).map(move |j| (a.x + 64 * i, a.y + 64 * j)))
        .find(|&(x, y)| clear(&g, x, y, a.z))
        .expect("somewhere to walk");
    (a.x, a.y) = (x, y);
    g.world.set(&g.data, AVATAR, a);
    let start = (a.x, a.y);
    let walk = |g: &mut Game| {
        let mut args = (0x8000_0000u32 | AVATAR as u32).to_le_bytes().to_vec();
        args.extend([0u16, 6, 0, 0].iter().flat_map(|v| v.to_le_bytes()));
        g.call_intrinsic(0x9F, &args);
        (0..30).for_each(|_| _ = g.update(1.0 / 30.0, &Input::default()));
        g.world.get(AVATAR).unwrap().x
    };
    let f = g.data.anims.get(1, 0).unwrap().frames[6].clone();
    let step = |r: &[usize]| r.iter().map(|&i| f[i].ddir as i32 * 4).sum::<i32>();
    let (second, first) = (step(&[4, 5, 6, 7]), step(&[8, 1, 2, 3]));
    let x1 = walk(&mut g);
    let x2 = walk(&mut g);
    assert_eq!(x1, start.0 - second, "the second half of the cycle first");
    assert_eq!(x2, x1 - first, "then the other foot");
}

/// The clock runs as the DOS game's: KERNEL.DAT entry 20 at 30 a second, 27000 to the game
/// hour, so a real minute is 4 game minutes.
#[test]
#[ignore]
fn a_real_minute_is_four_game_minutes() {
    let Some(mut g) = game(40) else { return };
    let start = g.minutes();
    (0..60 * 30).for_each(|_| _ = g.update(1.0 / 30.0, &Input::default()));
    assert_eq!(g.minutes() - start, 4);
}

/// The clock comes from where the DOS game keeps it (KERNEL.DAT entry 20): the castle save
/// "ok got key" is day 3, 03:02, as Divination there says. Saving writes it back there.
#[test]
#[ignore]
fn the_clock_is_read_and_saved_where_the_original_keeps_it() {
    let Some(dir) = std::env::var_os("U8_DATA") else { return };
    let mut g = Game::open(std::path::Path::new(&dir), Some(2), None).unwrap();
    assert_eq!(g.minutes(), 2 * 1440 + 3 * 60 + 2, "day 3, 03:02");
    (0..30 * 60).for_each(|_| _ = g.update(1.0 / 30.0, &Input::default()));
    let s = g.snapshot();
    assert_eq!(s.clock().map(|c| c / 450), Some(g.minutes()));
    let k = s.get("KERNEL.DAT").unwrap();
    let o = u8data::clock_offset(k).unwrap();
    assert_eq!(u32::from_le_bytes(k[o + 4..o + 8].try_into().unwrap()), g.minutes() / 60, "the hour number");
}

/// The short arrow takes careful steps, slowly as in DOS (141 units in 3 s there), and they
/// don't go over an edge; the medium arrow walks.
#[test]
#[ignore]
fn careful_steps_are_slow() {
    let Some(g0) = game(40) else { return };
    let start = g0.world.get(AVATAR).unwrap();
    let went = |careful: bool, dir: usize| {
        let mut g = game(40).unwrap();
        let input = Input { dir: Some(dir), careful, ..Default::default() };
        for _ in 0..90 {
            g.update(1.0 / 30.0, &input);
        }
        let a = g.world.get(AVATAR).unwrap();
        (a.x - start.x).abs().max((a.y - start.y).abs())
    };
    // A way with room to walk for 3 s.
    let dir = (0..8).find(|&d| went(false, d) > 300).expect("room to walk");
    let (walk, careful) = (went(false, dir), went(true, dir));
    assert!((100..=180).contains(&careful), "careful: {careful} units in 3 s (DOS 141)");
    assert!(walk > 3 * careful, "walking: {walk}");
}
