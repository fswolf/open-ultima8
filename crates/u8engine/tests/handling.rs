//! The world noticing what the player does: plates pressed and let go, things taken under
//! people's noses, people sent to other maps turning up there.
//! Run against a real install: U8_DATA=/path/to/U8 cargo test -- --ignored
mod common;
use common::*;
use u8engine::usecode::UiEvent;
use u8engine::world::item_flag;

/// The Avatar (from the castle save) put on `map`.
fn avatar_on(g: &mut Game, map: u16) {
    let av = g.world.get(AVATAR).unwrap();
    g.load_map(map);
    g.world.put_actor(&g.data, av);
    g.avatar = u8engine::avatar::Avatar::new(&g.data, &g.world);
}

fn put_avatar(g: &mut Game, at: (i32, i32, i32)) {
    let mut a = g.world.get(AVATAR).unwrap();
    (a.x, a.y, a.z) = at;
    g.world.set(&g.data, AVATAR, a);
    (0..5).for_each(|_| _ = g.update(1.0 / 30.0, &Input::default()));
}

#[test]
#[ignore]
fn plates_go_down_underfoot_and_spring_back() {
    let Some(mut g) = game(40) else { return };
    avatar_on(&mut g, 54);
    let plate = g.world.objects().find(|&id| g.world.get(id).is_some_and(|it| it.shape == 198)).unwrap();
    let p = g.world.get(plate).unwrap();
    put_avatar(&mut g, (p.x + 128, p.y, p.z));
    assert_eq!(g.world.get(plate).unwrap().frame, 0);
    put_avatar(&mut g, (p.x, p.y, p.z));
    assert_eq!(g.world.get(plate).unwrap().frame, 1, "stepped on: down");
    put_avatar(&mut g, (p.x + 128, p.y, p.z));
    assert_eq!(g.world.get(plate).unwrap().frame, 0, "stepped off: up again");
}

#[test]
#[ignore]
fn people_press_plates_too() {
    let Some(mut g) = game(40) else { return };
    let her = g.world.get(7).unwrap(); // Mordea, brought along
    avatar_on(&mut g, 54);
    let plate = g.world.objects().find(|&id| g.world.get(id).is_some_and(|it| it.shape == 198)).unwrap();
    let p = g.world.get(plate).unwrap();
    put_avatar(&mut g, (p.x + 256, p.y + 256, p.z));
    let mut m = her;
    (m.x, m.y, m.z, m.mapnum) = (p.x + 128, p.y, p.z, 54);
    g.world.put_actor(&g.data, m);
    g.world.set_void(7, false);
    (0..15).for_each(|_| _ = g.update(1.0 / 30.0, &Input::default())); // (the fast area notices her)
    (m.x, m.y) = (p.x, p.y);
    g.world.set(&g.data, 7, m);
    (0..3).for_each(|_| _ = g.update(1.0 / 30.0, &Input::default()));
    assert_eq!(g.world.get(plate).unwrap().frame, 1, "she stepped on it");
}

#[test]
#[ignore]
fn taking_things_is_noticed() {
    // In the castle, by the guard at the gate (NPC 61).
    let Some(mut g) = game(40) else { return };
    make_room(&mut g);
    let guard = g.world.get(61).unwrap();
    let near: Vec<_> = g.world.area(guard.x, guard.y, 400).into_iter().filter(|&id| id >= 256).collect();
    let thing = near
        .into_iter()
        .find(|&id| {
            let Some(it) = g.world.get(id).filter(|it| it.flags & item_flag::OWNED == 0) else { return false };
            if g.world.container(id).is_some() {
                return false;
            }
            let mut a = g.world.get(AVATAR).unwrap();
            (a.x, a.y, a.z) = (it.x + 64, it.y + 64, it.z);
            g.world.set(&g.data, AVATAR, a);
            g.can_pick_up(id).is_ok() && g.stow(id).is_ok()
        })
        .expect("something to take");
    let mut said = vec![];
    for _ in 0..120 {
        g.update(1.0 / 30.0, &Input::default());
        for e in g.take_events() {
            if let UiEvent::Bark { text, pid, .. } = e {
                said.push(text);
                g.vm().unwrap().finish(pid, 0);
            }
        }
    }
    assert!(said.iter().any(|t| t.contains("Thief")), "the guard saw: {said:?}");
    assert!(g.world.get(thing).unwrap().flags & item_flag::OWNED != 0, "the Avatar's now");
    // Putting it down and taking it again isn't stealing.
    let a = g.world.get(AVATAR).unwrap();
    let spot = [(64, 0), (0, 64), (-64, 0), (0, -64), (96, 96)].map(|(dx, dy)| (a.x + dx, a.y + dy, a.z));
    let put = spot.into_iter().any(|at| g.move_item(thing, u8engine::Place::Map(at)).is_ok());
    assert!(put, "somewhere to put it");
    g.stow(thing).unwrap();
    for _ in 0..120 {
        g.update(1.0 / 30.0, &Input::default());
        for e in g.take_events() {
            if let UiEvent::Bark { text, pid, .. } = e {
                assert!(!text.contains("Thief"), "it's the Avatar's");
                g.vm().unwrap().finish(pid, 0);
            }
        }
    }
}

#[test]
#[ignore]
fn people_sent_to_another_map_are_there() {
    let Some(mut g) = game(40) else { return };
    // As a script's teleport to another map leaves her: in the void here, marked for map 3.
    let m = 7;
    let mut her = g.world.get(m).unwrap();
    (her.x, her.y, her.z, her.mapnum) = (16000, 7000, 0, 3);
    g.world.set(&g.data, m, her);
    g.world.set_void(m, true);
    g.load_map(3);
    let there = g.world.get(m).expect("she's on map 3");
    assert!(!g.world.is_void(m) && (there.x, there.y) == (16000, 7000));
    g.load_map(40);
    assert!(g.world.get(m).is_none(), "and gone from the castle");
}

/// Hold `input` for `ticks`, then let go and settle for a second. Where the Avatar ends.
fn hold(g: &mut Game, input: Input, ticks: usize) -> (i32, i32, i32) {
    for _ in 0..ticks {
        g.update(1.0 / 30.0, &input);
    }
    for _ in 0..30 {
        g.update(1.0 / 30.0, &Input::default());
    }
    let a = g.world.get(AVATAR).unwrap();
    (a.x, a.y, a.z)
}

/// The original's invisible walls (solid shapes flagged editor-only) block: the city wall's
/// base on map 41 (a 497 box, y 17919 to 17951) stops a walk south, and a jump doesn't get
/// past it either.
#[test]
#[ignore]
fn invisible_walls_block() {
    let Some(mut g) = saved(1) else { return };
    g.put_avatar(41, (20379, 17880, 48));
    let (_, y, z) = hold(&mut g, Input { dir: Some(4), ..Input::default() }, 60);
    assert!(y < 17919 && z == 48, "stopped at the wall: y {y} z {z}");
    let (_, y, z) = hold(&mut g, Input { dir: Some(4), jump: true, ..Input::default() }, 1);
    let (_, y2, z2) = hold(&mut g, Input::default(), 60);
    assert!(y2 < 17919 && z2 == 48, "still inside the city after a jump: ({y}, {z}) then ({y2}, {z2})");
}

/// A dungeon cell on map 40 has walls 40 high and an invisible lid (499) on them: no climbing out.
#[test]
#[ignore]
fn the_cell_lid_keeps_the_avatar_in() {
    let Some(mut g) = game(40) else { return };
    for dir in [2, 6] {
        g.put_avatar(40, (4040, 4800, 0));
        hold(&mut g, Input { dir: Some(dir), ..Input::default() }, 40);
        let (x, y, z) = hold(&mut g, Input { dir: Some(dir), jump: true, ..Input::default() }, 1);
        let (x2, y2, z2) = hold(&mut g, Input { dir: Some(dir), ..Input::default() }, 60);
        assert!(z == 0 && z2 == 0, "dir {dir}: in the cell, at ({x}, {y}, {z}) then ({x2}, {y2}, {z2})");
    }
}

/// A jump never carries the Avatar out over nothing: from the plateau's north edge (map 8), one
/// north-west comes down on the ground, inside the map, and walking goes on from there.
#[test]
#[ignore]
fn jumps_dont_land_on_nothing() {
    let Some(mut g) = saved(10) else { return };
    g.put_avatar(8, (3398, 26, 0));
    let (x, y, _) = hold(&mut g, Input { dir: Some(7), jump: true, ..Input::default() }, 1);
    let (x, y) = {
        let (x2, y2, _) = hold(&mut g, Input::default(), 90);
        assert_eq!((x2, y2), (x, y));
        (x, y)
    };
    assert!(y >= 0 && g.world.floor_under(AVATAR).is_some(), "on the ground at ({x}, {y})");
    let (x2, y2, _) = hold(&mut g, Input { dir: Some(3), ..Input::default() }, 30);
    assert!((x2, y2) != (x, y), "and it walks on");
}

/// A running jump at a wall (Mythran's house, 88 high, on the plateau): the first hop is stopped
/// by the wall, so that's where the leap ends, on the ground; 88 is out of reach, as the original's standing climb (72) says.
#[test]
#[ignore]
fn a_running_jump_at_a_high_wall_ends_on_the_ground() {
    let Some(mut g) = saved(10) else { return };
    // (From standing, or once running: a walk's first step is no run, and a jump then nothing.)
    for run_up in [0, 10, 14, 18] {
        g.put_avatar(8, (2541, 3327, 0)); // (room to get running: the wall's at 2711)
        g.avatar.as_mut().unwrap().dir = 0;
        let run = Input { dir: Some(0), run: true, ..Input::default() };
        for _ in 0..run_up {
            g.update(1.0 / 30.0, &run);
        }
        g.update(1.0 / 30.0, &Input { jump: true, ..run });
        let mut top = 0;
        for _ in 0..90 {
            g.update(1.0 / 30.0, &Input::default());
            top = top.max(g.world.get(AVATAR).unwrap().z);
        }
        let (_, _, z) = hold(&mut g, Input { dir: Some(0), ..Input::default() }, 90);
        assert!(top > 0, "run-up {run_up}: it jumped");
        let av = g.avatar.as_ref().unwrap();
        assert!(z <= 16 && !av.hanging(), "run-up {run_up}: on the ground, not up the wall (z {z})");
    }
}

/// A running jump is one hop, as the original's (ANIM 10 is two-step like the run: one half a
/// jump, off whichever foot is next), with the keys held through it as the keyboard does. (Only
/// once running: the first step is a walk.)
#[test]
#[ignore]
fn a_running_jump_is_one_hop() {
    let Some(mut g) = saved(10) else { return };
    for run_up in 10..20 {
        g.put_avatar(8, (2541, 2827, 0));
        g.avatar.as_mut().unwrap().dir = 4;
        let run = Input { dir: Some(4), run: true, ..Input::default() };
        for _ in 0..run_up {
            g.update(1.0 / 30.0, &run);
        }
        let (x0, y0) = g.world.get(AVATAR).map(|a| (a.x, a.y)).unwrap();
        g.update(1.0 / 30.0, &Input { jump: true, ..run });
        let (mut hops, mut was, mut air) = (0, 0, 0);
        while air < 60 {
            g.update(1.0 / 30.0, &run);
            let z = g.world.get(AVATAR).unwrap().z;
            hops += (was == 0 && z > 0) as i32;
            (was, air) = (z, air + 1);
        }
        let it = g.world.get(AVATAR).unwrap();
        let (x, y, z) = (it.x, it.y, it.z);
        assert_eq!((hops, z), (1, 0), "run-up {run_up}: one hop, back on the ground");
        assert!((x - x0).abs() + (y - y0).abs() > 100, "run-up {run_up}: it went somewhere");
    }
}

/// Jumping into a teleporter (map 4, to (13634, 2614)): the Avatar arrives standing on the floor
/// there (64), as walking in does, not frozen in the leap 8 below it.
#[test]
#[ignore]
fn a_jump_into_a_teleporter_arrives_standing() {
    use u8data::anim::action;
    let Some(mut g) = saved(2) else { return };
    g.put_avatar(4, (4319, 2815, 0));
    g.avatar.as_mut().unwrap().dir = 3;
    hold(&mut g, Input { dir: Some(3), jump: true, ..Input::default() }, 1);
    let (x, y, z) = hold(&mut g, Input::default(), 120);
    assert_eq!((x, y, z), (13634, 2614, 64));
    let (it, dir) = (g.world.get(AVATAR).unwrap(), g.avatar.as_ref().unwrap().dir);
    let stand = u8engine::avatar::anim_frame(&g.data, it.shape, action::STAND, dir, 0).unwrap().0;
    assert_eq!(it.frame, stand, "standing");
}

/// A dart thrown (DART's use: pick a target, then shoot) and what it strikes gets its `hit`
/// event, as ScummVM's collideMove tells what a missile runs into: at the dartboard it sticks
/// (DART's hit: hanging, its frame for the board), at someone they say so ("Hey, watch it!" or
/// "Go play somewhere else!").
#[test]
#[ignore]
fn darts_stick_in_the_board_and_sting_people() {
    use u8engine::usecode::{event, UiEvent};
    use u8engine::world::item_flag::HANGING;
    let throw = |g: &mut Game, dart: u16, at: u16| {
        let mut said = vec![];
        g.trigger(dart, event::USE);
        for _ in 0..4 * 30 {
            g.update(1.0 / 30.0, &Input::default());
            for e in g.take_events() {
                match e {
                    UiEvent::Target { pid } => g.vm().unwrap().finish(pid, at as u32),
                    UiEvent::Bark { pid, item, text } => {
                        said.push((item, text));
                        g.vm().unwrap().finish(pid, 0);
                    }
                    _ => {}
                }
            }
        }
        said
    };
    let Some(mut g) = saved(1) else { return };
    g.set_view(640.0, 400.0);
    g.put_avatar(39, (0, 0, 0));
    g.set_minutes(600);
    // The board in the Tenebrae tavern, on its wall, and a dart lying by it.
    let (board, dart) = (17605, 30265);
    g.put_avatar(39, (15615, 11967, 48));
    let free = |g: &Game| g.world.get(dart).unwrap().flags & HANGING == 0;
    assert!(free(&g));
    throw(&mut g, dart, board);
    let d = g.world.get(dart).unwrap();
    assert!(!free(&g), "it sticks in the board: {d:?}");
    // At someone standing by.
    let who =
        (2..256).find(|&n| g.world.get(n).is_some_and(|p| (p.x - 15615).abs() < 600 && (p.y - 11967).abs() < 600));
    let who = who.expect("someone in the tavern");
    let p = g.world.get(who).unwrap();
    g.put_avatar(39, (p.x + 160, p.y, p.z));
    let said = throw(&mut g, 30267, who);
    assert!(said.iter().any(|(i, t)| *i == who && (t.contains("watch it") || t.contains("Go play"))), "{said:?}");
}

/// Every plaque on the maps that have them gives its words when used, and the ones the reach
/// rule kept from the player read once he walks up: those on the Upper Catacombs' pillar faces
/// (map 50), on Tenebrae's walls (maps 40, 41) and one hung a unit off its wall. ITEM.C
/// canReach: touching, or a clear line (a point is stopped only inside something, so a line
/// ending on the wall's face where the plaque hangs isn't blocked by the wall).
#[test]
#[ignore]
fn plaques_can_be_read() {
    use u8engine::usecode::{event, UiEvent};
    let dir = PathBuf::from(std::env::var("U8_DATA").unwrap());
    let Ok(mut g) = Game::open(&dir, Some(0), Some(3)) else { return };
    g.set_view(640.0, 400.0);
    let run = |g: &mut Game, n: usize, input: &Input| (0..n).for_each(|_| _ = g.update(1.0 / 30.0, input));
    run(&mut g, 3, &Input::default());
    let mut read = 0;
    for m in [6, 12, 14, 37, 40, 41, 50, 51, 54, 57, 60] {
        g.put_avatar(m, (0, 0, 0));
        let plaques: Vec<u16> =
            g.world.objects().filter(|&id| g.world.get(id).is_some_and(|it| matches!(it.shape, 599 | 614))).collect();
        for p in plaques {
            g.take_events();
            g.trigger(p, event::USE);
            run(&mut g, 3, &Input::default());
            let said =
                g.take_events().into_iter().any(|e| matches!(e, UiEvent::Read { text, .. } if !text.trim().is_empty()));
            assert!(said, "map {m} plaque {p} says nothing");
            read += 1;
        }
    }
    assert_eq!(read, 60, "all the plaques");
    // Walked up to (from a few steps off, facing them), they're in reach.
    let walks = [
        (50, 5598, (5700, 2719, 48), 6),
        (50, 5599, (5503, 2900, 48), 0),
        (41, 4611, (13887, 3250, 48), 0),
        (41, 19521, (19071, 13450, 48), 0),
        (40, 7265, (12100, 7136, 48), 6),
        (50, 18339, (11450, 8856, 48), 6),
    ];
    for (m, p, start, dir) in walks {
        g.put_avatar(m, start);
        run(&mut g, 5, &Input::default());
        run(&mut g, 60, &Input { dir: Some(dir), ..Default::default() });
        run(&mut g, 20, &Input::default());
        assert!(g.can_use(p), "map {m} plaque {p} out of reach");
    }
}
