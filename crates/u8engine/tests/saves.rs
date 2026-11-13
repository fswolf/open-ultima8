//! Saving, loading and the start of a new game.
//! Run against a real install: U8_DATA=/path/to/U8 cargo test -- --ignored
mod common;
use common::*;

#[test]
#[ignore]
fn saves_round_trip() {
    use u8data::save::Save;
    let Some(mut g) = game(40) else { return };
    let dir = PathBuf::from(std::env::var("U8_DATA").unwrap());
    let still = Input::default();
    // Change things on two maps: open a door here, then go through the teleporter to map 3 and walk.
    let door = g.world.objects().find(|&id| g.world.get(id).is_some_and(|it| it.shape == 68)).expect("a door");
    let at = g.world.get(door).unwrap();
    g.world.set(&g.data, door, u8data::map::Item { frame: 7, ..at });
    let mut av = g.world.get(AVATAR).unwrap();
    (av.x, av.y, av.z) = (13048, 15400, 48);
    g.world.set(&g.data, AVATAR, av);
    assert!(g.update(1.0 / 30.0, &still).map_changed);
    let walk = Input { dir: Some(3), ..Default::default() };
    for _ in 0..20 {
        g.update(1.0 / 30.0, &walk);
    }
    g.vm().unwrap().globals[200] = 0xA5;
    let (pos, minutes) = (g.world.get(AVATAR).unwrap(), g.minutes());

    let back = Save::parse(&g.snapshot().to_bytes()).expect("a save we can read back");
    let gd = GameData::load_save(&dir, Some(&back)).unwrap();
    let scripts = Scripts::load(&dir, &gd).ok();
    let mut g2 = Game::new(gd, scripts, None).unwrap();
    assert_eq!(g2.map(), 3, "starts where the Avatar was");
    let p = g2.world.get(AVATAR).unwrap();
    assert_eq!((p.x, p.y, p.z), (pos.x, pos.y, pos.z));
    assert_eq!(g2.minutes(), minutes, "clock kept");
    assert_eq!(g2.vm().unwrap().globals[200], 0xA5, "global flags kept");
    g2.load_map(40);
    let same = g2
        .world
        .objects()
        .filter_map(|id| g2.world.get(id))
        .find(|it| (it.x, it.y, it.z, it.shape) == (at.x, at.y, at.z, 68));
    assert_eq!(same.map(|d| d.frame), Some(7), "the door stayed open");
}

#[test]
#[ignore]
fn saving_while_looking_at_another_map_keeps_the_avatar() {
    use u8data::save::Save;
    let Some(mut g) = game(40) else { return };
    let dir = PathBuf::from(std::env::var("U8_DATA").unwrap());
    let walk = Input { dir: Some(3), ..Default::default() };
    for _ in 0..20 {
        g.update(1.0 / 30.0, &walk);
    }
    let pos = g.world.get(AVATAR).unwrap();
    g.step_map(1); // look at the next map; the Avatar stays on 40
    assert_ne!(g.map(), 40);
    let back = Save::parse(&g.snapshot().to_bytes()).unwrap();
    let g2 = Game::new(GameData::load_save(&dir, Some(&back)).unwrap(), None, None).unwrap();
    assert_eq!(g2.map(), 40);
    let p = g2.world.get(AVATAR).unwrap();
    assert_eq!((p.x, p.y, p.z), (pos.x, pos.y, pos.z));
}

#[test]
#[ignore]
fn the_opening_scene_points_the_camera() {
    // A new game: the camera holds, glides to the campfire, follows Devon (NPC 2) while he
    // talks, then glides back and follows the Avatar.
    let dir = PathBuf::from(std::env::var("U8_DATA").unwrap_or_default());
    let Ok(mut g) = Game::open(&dir, Some(0), None) else { return };
    let still = Input::default();
    let devon = |g: &Game| g.world.get(2).map(|it| (it.x, it.y, it.z + u8engine::camera::EYE));
    let mut on_devon = false;
    for _ in 0..200 {
        g.update(1.0 / 30.0, &still);
        on_devon |= g.camera().is_some() && g.camera() == devon(&g);
        answer(&mut g);
    }
    assert!(on_devon, "the camera centred on Devon during the conversation");
    for _ in 0..600 {
        g.update(1.0 / 30.0, &still);
        answer(&mut g);
    }
    let av = g.world.get(AVATAR).unwrap();
    assert_eq!(g.camera(), Some((av.x, av.y, av.z + u8engine::camera::EYE)), "back on the Avatar");
    assert!(!g.in_stasis(), "the player has control again");
}

#[test]
#[ignore]
fn a_new_game_starts_with_an_empty_backpack() {
    // As the original's paperdoll shows it: WGHT 1, nothing drawn on the figure.
    let dir = PathBuf::from(std::env::var("U8_DATA").unwrap_or_default());
    let Ok(g) = Game::open(&dir, Some(0), None) else { return };
    let pack = g.backpack().expect("a backpack");
    assert!(g.world.contents(pack).is_empty());
    assert_eq!(g.load().0, 1);
}

#[test]
#[ignore]
fn a_new_game_asks_for_a_name_and_devon_uses_it() {
    use u8engine::usecode::UiEvent;
    let dir = PathBuf::from(std::env::var("U8_DATA").unwrap_or_default());
    let Ok(mut g) = Game::open(&dir, Some(0), None) else { return };
    assert!(g.needs_name());
    g.set_avatar_name("Ryan");
    assert!(!g.needs_name());
    let mut heard = String::new();
    for _ in 0..400 {
        g.update(1.0 / 30.0, &Input::default());
        for e in g.take_events() {
            let vm = g.vm().unwrap();
            match e {
                UiEvent::Bark { pid, text, .. } => {
                    heard += &text;
                    vm.finish(pid, 0)
                }
                UiEvent::Ask { pid, options, .. } => vm.answer(pid, &options, 0), // "Who are you?"
                _ => {}
            }
        }
    }
    assert!(heard.contains("feeling better, Ryan"), "Devon greets us by name: {heard}");
    assert_eq!(g.snapshot().get("AVATAR.DAT").and_then(u8data::save::avatar_name).as_deref(), Some("Ryan"));
}
