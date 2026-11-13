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
