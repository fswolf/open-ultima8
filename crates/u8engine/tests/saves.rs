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

/// A DOS save keeps the area the game had in memory in ITEMCACH.DAT, not NONFIXED.DAT, which
/// still has it as it was when cached. In "ok got key" the key was taken off the table and is on
/// the keyring: it isn't on the table any more, and the double doors stand as the cache has them.
#[test]
#[ignore]
fn a_dos_save_loads_the_area_it_had_in_memory() {
    let Some(dir) = std::env::var_os("U8_DATA") else { return };
    let Ok(g) = Game::open(std::path::Path::new(&dir), Some(2), None) else { return };
    let at = |x: i32, y: i32| -> Vec<u32> {
        g.world
            .objects()
            .filter_map(|id| g.world.get(id))
            .filter(|it| (it.x, it.y) == (x, y))
            .map(|it| it.shape)
            .collect()
    };
    assert!(!at(12622, 9010).contains(&82), "no key on the table");
    assert!(at(12959, 9215).is_empty() && at(13183, 9215).contains(&69), "the doors as cached");
}

/// The DOS game loads a game from the area its item cache holds (ITEMCACH.DAT entry 1), so a
/// save has to hold the map the Avatar is on: after travelling, the cache is that map's, the
/// Avatar is in its chunk's list, and the glob eggs near it are expanded. Loading it back gives
/// the same map, nothing doubled.
#[test]
#[ignore]
fn a_save_holds_the_avatars_map_for_the_dos_game() {
    let Some(dir) = std::env::var_os("U8_DATA") else { return };
    let root = std::path::Path::new(&dir);
    let Ok(mut g) = Game::open(root, Some(2), None) else { return };
    assert!(g.go_to("plateau"));
    g.update(0.1, &Input::default());
    let count = |g: &Game| g.world.objects().filter(|&id| g.world.get(id).is_some()).count();
    let before = count(&g);
    let s = g.snapshot();
    let f = u8data::flex::Flex::parse(s.get("ITEMCACH.DAT").unwrap().to_vec()).unwrap();
    let (e0, e1) = (f.get(0).unwrap(), f.get(1).unwrap());
    let area = u8data::map::cached_area(e0, e1).unwrap();
    assert_eq!(area.map, g.map() as usize, "the cache is the map the Avatar is on");
    let held = u8data::map::cached_map_objects(e0, e1);
    assert!(held.len() > 1000, "its items held ({})", held.len());
    assert!(area.items.iter().any(|(_, it)| it.flags & u8data::map::GLOB_ITEM == 0x82), "glob eggs expanded");
    let gd = u8data::GameData::load_save(root, Some(&s)).unwrap();
    let back = Game::new(gd, None, None).unwrap();
    assert_eq!(count(&back), before, "the same things on the map");
}

/// Chests that are part of the map (FIXED.DAT) hold things, as they're listed after them. What's
/// taken out of one stays out over a save, through the item cache as the original keeps it.
#[test]
#[ignore]
fn fixed_chests_hold_their_contents_over_a_save() {
    let Some(dir) = std::env::var_os("U8_DATA") else { return };
    let root = std::path::Path::new(&dir);
    let Ok(mut g) = Game::open(root, Some(2), None) else { return };
    use u8engine::world::Origin;
    let chest = |g: &Game| {
        g.world
            .objects()
            .find(|&id| {
                let e = g.world.entry(g.world.index(id).unwrap());
                e.origin == Origin::Fixed && (e.it.x, e.it.y, e.it.shape) == (12191, 6783, 205)
            })
            .expect("the chest")
    };
    let c = chest(&g);
    let inside = g.world.contents(c);
    assert_eq!(inside.len(), 4);
    assert!(!g
        .world
        .objects()
        .any(|id| g.world.container(id).is_none() && g.world.get(id).is_some_and(|it| it.x < 64)));
    g.world.destroy(&g.data, inside[0]);
    let s = g.snapshot();
    let back = Game::new(u8data::GameData::load_save(root, Some(&s)).unwrap(), None, None).unwrap();
    assert_eq!(back.world.contents(chest(&back)).len(), 3);
}

/// The original takes a person's facing from NPCDATA (byte 4), not from the frame: a save keeps it.
#[test]
#[ignore]
fn a_save_keeps_which_way_people_face() {
    let Some(dir) = std::env::var_os("U8_DATA") else { return };
    let root = std::path::Path::new(&dir);
    let Ok(mut g) = Game::open(root, Some(2), None) else { return };
    use u8engine::{avatar, world::AVATAR};
    for d in [2, 4, 6] {
        let mut it = g.world.get(AVATAR).unwrap();
        let (frame, flip) = avatar::anim_frame(&g.data, it.shape, u8data::anim::action::STAND, d, 0).unwrap();
        it.frame = frame;
        it.flags = if flip { it.flags | 0x20 } else { it.flags & !0x20 };
        g.world.set(&g.data, AVATAR, it);
        assert_eq!(avatar::facing(&g.data, &it), Some(d), "mirrored pictures tell opposite ways apart");
        let s = g.snapshot();
        let npcdata = u8data::flex::Flex::parse(s.get("NPCDATA.DAT").unwrap().to_vec()).unwrap();
        assert_eq!(u8data::map::npc_dir(npcdata.get(0).unwrap(), AVATAR as usize), Some(d));
    }
}

/// Every slot of the item cache is on its free list, in a chunk's list or inside something (as in
/// every DOS save), and the free lists say so: the original hands out new objects from them.
/// The biggest maps fit, and nothing is lost on the way back.
#[test]
#[ignore]
fn the_item_cache_keeps_its_free_lists_on_the_biggest_maps() {
    let Some(dir) = std::env::var_os("U8_DATA") else { return };
    let root = std::path::Path::new(&dir);
    use u8data::map;
    for m in [40, 62, 39] {
        let Ok(mut g) = Game::open(root, Some(2), None) else { return };
        assert!(m == g.map() || (0..64).any(|id| g.teleport(m, id)), "to map {m}");
        let s = g.snapshot();
        let f = u8data::flex::Flex::parse(s.get("ITEMCACH.DAT").unwrap().to_vec()).unwrap();
        let (e0, e1) = (f.get(0).unwrap(), f.get(1).unwrap());
        let word = |i: usize| u16::from_le_bytes([e1[2 * i], e1[2 * i + 1]]) as usize;
        let next = |o: usize| map::cached_next(e0, o);
        let mut free = std::collections::HashSet::new();
        let mut o = word(8199);
        while o != 0 && free.insert(o) {
            o = next(o);
        }
        assert_eq!((free.len(), word(8201)), (word(8197), map::OBJECTS - 1));
        let held: std::collections::HashSet<usize> =
            map::cached_map_objects(e0, e1).into_iter().map(usize::from).collect();
        let inside: std::collections::HashSet<usize> =
            map::load_cache_contents(e0).into_iter().map(|c| c.id as usize).collect();
        let lost = (256..map::OBJECTS).filter(|o| !free.contains(o) && !held.contains(o) && !inside.contains(o));
        assert_eq!(lost.count(), 0, "map {m}");
        assert!(free.iter().all(|o| !held.contains(o) && !inside.contains(o)), "map {m}");
        let count = |g: &Game| g.world.objects().filter(|&id| g.world.get(id).is_some()).count();
        let back = Game::new(u8data::GameData::load_save(root, Some(&s)).unwrap(), None, None).unwrap();
        assert_eq!(count(&back), count(&g), "map {m}: everything comes back");
    }
}

/// A question left unanswered gets an empty answer (the original's timeout, ASK_TIMEOUT): Devon
/// asks if you're still there, then gives up on you.
#[test]
#[ignore]
fn devon_gives_up_on_a_question_left_unanswered() {
    use u8engine::usecode::UiEvent;
    let dir = PathBuf::from(std::env::var("U8_DATA").unwrap_or_default());
    let Ok(mut g) = Game::open(&dir, Some(0), None) else { return };
    g.set_avatar_name("Ryan");
    let (mut heard, mut asked) = (String::new(), 0);
    for _ in 0..600 {
        g.update(1.0 / 30.0, &Input::default());
        for e in g.take_events() {
            let vm = g.vm().unwrap();
            match e {
                UiEvent::Bark { pid, text, .. } => {
                    heard += &text;
                    vm.finish(pid, 0)
                }
                UiEvent::Ask { pid, .. } => {
                    asked += 1;
                    vm.answer_nothing(pid)
                }
                _ => {}
            }
        }
    }
    assert!(heard.contains("I thought you were still speaking to me"), "{heard}");
    assert!(heard.contains("I shall not waste any more of your time"), "{heard}");
    assert_eq!(asked, 2, "asked twice, then the conversation ended");
}

/// People talk with their hands while what they say or ask is up (the original's own talk
/// animation, action 53, not in the scripts): Devon gestures over the fire in the opening as DOS
/// shows him (frames 415–417), and stands again when the talking's done.
#[test]
#[ignore]
fn people_gesture_while_they_talk() {
    use u8data::anim::action;
    use u8engine::usecode::UiEvent;
    let dir = PathBuf::from(std::env::var("U8_DATA").unwrap_or_default());
    let Ok(mut g) = Game::open(&dir, Some(0), None) else { return };
    g.set_avatar_name("Ryan");
    let talking = |g: &Game| g.world.acting(2).is_some_and(|a| a.0 == action::TALK);
    let mut barks: Vec<(u16, usize)> = vec![];
    let (mut during, mut asked, mut after) = (false, 0, None);
    for t in 0..1200 {
        g.update(1.0 / 30.0, &Input::default());
        for e in g.take_events() {
            match e {
                UiEvent::Bark { pid, .. } => barks.push((pid, t + 60)),
                UiEvent::Ask { pid, options, .. } => {
                    asked += 1;
                    let vm = g.vm().unwrap();
                    vm.answer(pid, &options, options.len() - 1); // "Goodbye."
                }
                _ => {}
            }
        }
        let due: Vec<u16> = barks.iter().filter(|b| b.1 <= t).map(|b| b.0).collect();
        barks.retain(|b| b.1 > t);
        let ended = !due.is_empty() && asked > 0;
        due.into_iter().for_each(|pid| g.vm().unwrap().finish(pid, 0));
        during |= !barks.is_empty() && talking(&g);
        if ended && barks.is_empty() && after.is_none() {
            after = Some(t);
        }
        if after.is_some_and(|a| t == a + 5) {
            assert!(!talking(&g), "stops when the talking's done");
            let f = g.world.get(2).unwrap().frame;
            assert!(!(415..=417).contains(&f), "standing again, frame {f}");
        }
    }
    assert!(during, "gestures while speaking");
    assert!(asked > 0 && after.is_some());
}
