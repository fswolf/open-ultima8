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
/// by the wall, so that's where the leap ends, on the ground. Its second hop doesn't start from
/// mid-air and catch the top; 88 is out of reach, as the original's standing climb (72) says.
#[test]
#[ignore]
fn a_running_jump_at_a_high_wall_ends_on_the_ground() {
    let Some(mut g) = saved(10) else { return };
    for run_up in [0, 3, 6, 9, 12] {
        g.put_avatar(8, (2541, 2827, 0));
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

/// Jumping into a teleporter (map 4, to (13634, 2614)): the Avatar arrives standing on the floor
/// there (64), as walking in does, not frozen in the leap 8 below it.
#[test]
#[ignore]
fn a_jump_into_a_teleporter_arrives_standing() {
    use u8data::anim::action;
    let Some(mut g) = saved(2) else { return };
    g.put_avatar(4, (4319, 2815, 0));
    hold(&mut g, Input { dir: Some(3), jump: true, ..Input::default() }, 1);
    let (x, y, z) = hold(&mut g, Input::default(), 120);
    assert_eq!((x, y, z), (13634, 2614, 64));
    let (it, dir) = (g.world.get(AVATAR).unwrap(), g.avatar.as_ref().unwrap().dir);
    let stand = u8engine::avatar::anim_frame(&g.data, it.shape, action::STAND, dir, 0).unwrap().0;
    assert_eq!(it.frame, stand, "standing");
}
