//! The tester's kit (`Game::give_test_kit`), and every school's magic used from it: no script
//! errors or missing intrinsics, and what each item says or does (printed with --nocapture).
//! Run against a real install: U8_DATA=/path/to/U8 cargo test -- --ignored
mod common;
use common::*;
use u8engine::usecode::{event, UiEvent};

/// The kit's sorcery focus of `frame` holding `spell` (9 charges).
fn sorcery(g: &mut Game, frame: u32, spell: u16) -> u16 {
    let bag = g.give_test_kit().expect("a bag");
    let want = (397, frame, spell << 8 | 9);
    let found = g
        .world
        .contents(bag)
        .into_iter()
        .find(|&o| g.world.get(o).is_some_and(|i| (i.shape, i.frame, i.quality) == want));
    found.expect("the focus")
}

/// Use `id` and play on for `secs`, answering questions with the first choice and pointing the
/// target cursor at the Avatar. What was said.
fn use_and_watch(g: &mut Game, id: u16, secs: f32) -> Vec<String> {
    let mut said = vec![];
    g.trigger(id, event::USE);
    for _ in 0..(secs * 30.0) as usize {
        g.update(1.0 / 30.0, &Input::default());
        for e in g.take_events() {
            let vm = g.vm().unwrap();
            match e {
                UiEvent::Bark { pid, text, .. } => {
                    said.push(text);
                    vm.finish(pid, 0)
                }
                UiEvent::Ask { pid, options, .. } => {
                    said.push(format!("[asks {options:?}]"));
                    vm.answer(pid, &options, 0)
                }
                UiEvent::Read { pid, text, .. } => {
                    said.push(format!("[reads {:?}]", text.chars().take(40).collect::<String>()));
                    vm.finish(pid, 0)
                }
                UiEvent::Target { pid } => {
                    said.push("[target: the Avatar]".into());
                    vm.finish(pid, AVATAR as u32)
                }
                _ => {}
            }
        }
    }
    said
}

#[test]
#[ignore]
fn every_school_from_the_test_kit() {
    let Some(mut g) = game(40) else { return };
    let bag = g.give_test_kit().expect("a bag");
    let items = g.world.contents(bag);
    assert!(items.len() >= 60, "{} things", items.len());
    let s = g.stats.get(AVATAR).unwrap();
    assert_eq!((s.str, s.int, s.mana), (25, 25, 50));
    for id in items {
        let Some(it) = g.world.get(id) else { continue }; // (used up)
        let said = use_and_watch(&mut g, id, 3.0);
        let mana = g.stats.get(AVATAR).map_or(0, |s| s.mana);
        let after = g.world.get(id).map_or("used up".to_string(), |i| format!("q{}", i.quality));
        println!(
            "{:>3}/{:<2} q{:<5} → {after:<8} mana {mana:>2}: {}",
            it.shape,
            it.frame,
            it.quality,
            said.join(" | ")
        );
        let vm = g.vm().unwrap();
        assert_eq!((vm.errors, vm.missing.len()), (0, 0), "shape {} frame {}: {:?}", it.shape, it.frame, vm.missing);
        g.stats.get_mut(AVATAR).unwrap().mana = 50; // (each from full)
    }
}

#[test]
#[ignore]
fn the_kit_goes_everywhere_the_recall_does() {
    let Some(mut g) = game(40) else { return };
    for (place, map) in u8engine::kit::PLACES {
        assert!(g.go_to(place), "{place}");
        for _ in 0..30 {
            g.update(1.0 / 30.0, &Input::default());
        }
        assert_eq!(g.map(), map, "{place}");
        let vm = g.vm().unwrap();
        assert_eq!((vm.errors, vm.missing.len()), (0, 0), "{place}: {:?}", vm.missing);
    }
}

/// Grant Peace (ScummVM's GrantPeaceProcess): cast at one of the undead, every undead nearby is
/// struck with its hit points, with lightning and thunder; the living are left alone.
#[test]
#[ignore]
fn grant_peace_lays_the_undead_to_rest() {
    let dir = PathBuf::from(std::env::var("U8_DATA").unwrap());
    let Ok(mut g) = Game::open(&dir, Some(0), Some(3)) else { return };
    g.update(1.0 / 30.0, &Input::default());
    let a = g.world.get(AVATAR).unwrap();
    let mut spots = (-5..=5).flat_map(|x| (-5..=5).map(move |y| (x * 64, y * 64))).filter(|&p| p != (0, 0));
    let mut hatch = |g: &mut Game, shape: u32| {
        spots.find_map(|(x, y)| {
            let egg = u8data::map::Item {
                x: a.x + x,
                y: a.y + y,
                z: a.z,
                quality: shape as u16,
                mapnum: 2 << 3 | 2,
                ..Default::default()
            };
            g.hatch_monster(egg)
        })
    };
    let (bones, ghoul, torax) = (hatch(&mut g, 119).unwrap(), hatch(&mut g, 142).unwrap(), hatch(&mut g, 120).unwrap());
    let hp = |g: &Game, id| g.stats.get(id).unwrap().hp;
    let (b0, gh0, t0) = (hp(&g, bones), hp(&g, ghoul), hp(&g, torax));
    g.take_events();
    assert_eq!(g.call_intrinsic(0xB8, &[]), 0);
    let (mut picked, mut lightning, mut thunder) = (false, false, false);
    for _ in 0..30 * 6 {
        g.update(1.0 / 30.0, &Input::default());
        for e in g.take_events() {
            match e {
                UiEvent::Target { pid } => {
                    picked = true;
                    g.vm().unwrap().finish(pid, bones as u32);
                }
                UiEvent::Sfx { sfx: 91 | 94 | 96, item: AVATAR, .. } => thunder = true,
                _ => {}
            }
        }
        let fade = &g.vm().unwrap().fade;
        lightning |= fade.matrix() != fade.settled();
    }
    assert!(picked, "the target cursor comes up");
    assert!(g.stats.get(bones).unwrap().dead, "struck with its own hit points");
    assert!(hp(&g, ghoul) <= (gh0 - b0).max(0) || g.stats.get(ghoul).unwrap().dead, "every undead near takes as much");
    assert_eq!(hp(&g, torax), t0, "the living aren't touched");
    assert!(thunder && lightning);
}

/// Casting closes every item window, as in DOS: the spell's own script does it (FREE @0x36f3:
/// closeGump on everything nearby with a window open, `use` on a person's to put the
/// paperdoll away, then the Avatar's), before the spell goes on.
#[test]
#[ignore]
fn casting_closes_every_item_window() {
    let Some(mut g) = game(40) else { return };
    let bag = g.give_test_kit().expect("a bag");
    let pack = g.backpack().unwrap();
    for id in [AVATAR, pack, bag] {
        g.trigger(id, event::USE);
        for _ in 0..5 {
            g.update(1.0 / 30.0, &Input::default());
        }
    }
    let opened: Vec<_> = g
        .take_events()
        .into_iter()
        .filter_map(|e| match e {
            UiEvent::OpenGump { item, .. } => Some(item),
            _ => None,
        })
        .collect();
    assert_eq!(opened, [AVATAR, pack, bag]);
    let divination =
        g.world.contents(bag).into_iter().find(|&i| g.world.get(i).is_some_and(|it| (it.shape, it.frame) == (396, 8)));
    let said = use_and_watch(&mut g, divination.expect("the Divination focus"), 5.0);
    assert!(said.iter().any(|s| s.contains("The focus reads")), "{said:?}");
    let open = |g: &Game, id| g.world.get(id).unwrap().flags & u8engine::world::item_flag::GUMP_OPEN != 0;
    assert!(![AVATAR, pack, bag].iter().any(|&id| open(&g, id)), "every window closed");
}

/// Fire Shield's ring of flames (Sorcery's focus at spell 6) where the original puts them: the
/// DOS game's flames (x, y, z, read from its saves after the same cast; Claude outputs/
/// magic-overlap). Its script tries each spot at the height of the last, down by 8 to 24 under
/// the feet, then up to 24 over, and canExistAt(…, 1, 0, 0) wants something solid with land to
/// stand on as well as room: none under a floor, hanging in the air or on the plaza's pool.
#[test]
#[ignore]
fn fire_shield_flames_land_where_the_originals_do() {
    // (place, where the Avatar stands: x, y, z, map, and the original's flames)
    type Spot = (&'static str, (i32, i32, i32, u16), &'static str);
    const DOS: [Spot; 4] = [
        ("castle", (12774, 9010, 48, 40), "12614,8850,48 12646,8818,48 12678,8818,48 12678,9170,48 12710,8786,48 12742,8786,48 12774,8786,48 12806,8786,48 12838,8818,48 12838,9170,48 12870,8818,48 12902,8850,48 12934,8882,48 12934,8914,48 12934,9074,48 12934,9106,48 12966,8946,48 12966,8978,48 12966,9010,48 12966,9042,48"),
        ("docks", (11728, 6288, 48, 3), "11600,6096,48 11600,6448,48 11632,6096,48 11632,6448,48 11664,6064,48 11664,6480,48 11696,6064,48 11696,6480,48 11728,6064,48 11728,6480,48 11760,6064,48 11760,6480,48 11792,6096,48 11792,6448,48 11824,6096,48 11824,6448,48 11856,6128,48 11856,6416,48 11888,6160,48 11888,6192,48 11888,6352,48 11888,6384,48 11920,6224,48 11920,6256,48 11920,6288,48 11920,6320,48 11536,6160,72 11536,6192,72 11536,6352,72 11536,6384,72 11568,6128,72 11568,6416,72"),
        ("plaza", (20396, 14716, 48, 41), "20172,14652,48 20172,14684,48 20172,14716,48 20172,14748,48 20204,14588,48 20204,14620,48 20204,14780,48 20204,14812,48 20236,14556,48 20268,14524,48 20300,14524,48 20332,14492,48 20364,14492,48 20396,14492,48 20428,14492,48 20460,14524,48 20492,14524,48 20524,14556,48 20556,14588,48 20556,14620,48 20556,14780,48 20556,14812,48 20588,14652,48 20588,14684,48 20588,14716,48 20588,14748,48 20236,14844,64 20268,14876,64 20300,14876,64 20460,14876,64"),
        ("rug", (13044, 8820, 48, 40), "12820,8756,48 12820,8788,48 12820,8820,48 12820,8852,48 12852,8692,48 12852,8724,48 12852,8884,48 12852,8916,48 12884,8660,48 12884,8948,48 12916,8628,48 12916,8980,48 12948,8628,48 12948,8980,48 12980,8596,48 12980,9012,48 13012,8596,48 13012,9012,48 13044,8596,48 13044,9012,48 13076,8596,48 13076,9012,48 13108,8628,48 13108,8980,48 13140,8628,48 13140,8980,48 13172,8660,48 13172,8948,48 13204,8692,48 13204,8724,48 13204,8884,48 13204,8916,48 13236,8756,48 13236,8788,48 13236,8820,48 13236,8852,48"),
    ];
    for (place, (x, y, z, map), dos) in DOS {
        let Some(mut g) = saved(2) else { return };
        g.put_avatar(map, (x, y, z));
        let shield = sorcery(&mut g, 3, 6);
        use_and_watch(&mut g, shield, 9.0);
        let mut ours: Vec<(i32, i32, i32)> = (0..g.world.len())
            .map(|i| g.world.id(i))
            .filter_map(|o| g.world.get(o).filter(|i| i.shape == 569 && !g.world.is_void(o)))
            .map(|i| (i.x, i.y, i.z))
            .collect();
        let mut want: Vec<(i32, i32, i32)> = dos
            .split(' ')
            .map(|p| p.split(',').map(|v| v.parse().unwrap()).collect::<Vec<i32>>())
            .map(|v| (v[0], v[1], v[2]))
            .collect();
        ours.sort();
        want.sort();
        assert_eq!(ours, want, "{place}");
    }
}

/// canExistAt's flags and mover (MOTRAK.C): water bears only what may stand on water, a land
/// surface refuses what may not, flags 4 needs nothing underneath, and the mover doesn't count
/// as in its own way.
#[test]
#[ignore]
fn can_exist_at_follows_its_flags() {
    use u8engine::world::exist::{FLOATS, LAND, SEA};
    let Some(mut g) = saved(2) else { return };
    g.put_avatar(41, (20396, 14716, 48));
    let (w, gd) = (&g.world, &g.data);
    let pool = (20364, 14908, 48);
    assert!(!w.can_exist_at(gd, 398, pool, LAND, None), "the pool's water bears nothing walking");
    assert!(w.can_exist_at(gd, 398, pool, SEA, None), "but bears what may stand on water");
    assert!(w.can_exist_at(gd, 398, (20300, 14524, 48), LAND, None), "the plaza's paving");
    assert!(!w.can_exist_at(gd, 398, (20300, 14524, 48), SEA, None), "land refuses a swimmer");
    assert!(!w.can_exist_at(gd, 398, (20300, 14524, 80), LAND, None), "in the air");
    assert!(w.can_exist_at(gd, 398, (20300, 14524, 80), FLOATS, None), "unless it floats");
    let a = w.get(AVATAR).unwrap();
    assert!(!w.can_exist_at(gd, a.shape, (a.x, a.y, a.z), LAND, None), "the Avatar is in its own way");
    assert!(w.can_exist_at(gd, a.shape, (a.x, a.y, a.z), LAND, Some(AVATAR)), "unless it's the mover");
}

/// A scene that takes the camera off the Avatar and scrolls it over another building leaves the
/// roof cut as it was over the Avatar (CAMERA.C); centred on him again, it follows him as before.
#[test]
#[ignore]
fn the_roof_stays_while_a_scene_has_the_camera() {
    let Some(mut g) = saved(2) else { return };
    let roof = g.roof();
    assert!(roof.is_some(), "indoors");
    // Someone under another roof than the Avatar's: the camera goes over them.
    let other = (2..256).find_map(|n| {
        let r = g.world.ceiling(n)?;
        (Some(r - 1) != roof).then(|| g.world.get(n)).flatten()
    });
    let other = other.expect("someone under another roof");
    // (A scroll being read keeps a script running, so the camera isn't handed back meanwhile.)
    let bag = g.give_test_kit().expect("a bag");
    let scroll = g.world.contents(bag).into_iter().find(|&o| g.world.get(o).is_some_and(|i| i.shape == 827));
    g.trigger(scroll.expect("a scroll"), event::USE);
    let tick = |g: &mut Game| {
        g.update(1.0 / 30.0, &Input::default());
        g.take_events();
    };
    (0..10).for_each(|_| tick(&mut g));
    g.call_intrinsic(0xBE, &[0, 0]); // setCenterOn(0)
    let [x0, x1] = (other.x as u16).to_le_bytes();
    let [y0, y1] = (other.y as u16).to_le_bytes();
    g.call_intrinsic(0xBF, &[x0, x1, y0, y1, other.z as u8 + 20, 0, 40, 0]); // moveTo
    for _ in 0..30 {
        tick(&mut g);
        assert_eq!(g.camera(), Some((other.x, other.y, other.z + 20)), "the scene has the camera");
        assert_eq!(g.roof(), roof, "the cut stays over the Avatar");
    }
    g.call_intrinsic(0xBE, &[1, 0]); // back on the Avatar
    g.put_avatar(40, (other.x, other.y + 64, other.z));
    tick(&mut g);
    assert_eq!(g.roof(), g.world.ceiling(AVATAR).map(|z| z - 1), "and follows him again");
}

/// Flash parks the Avatar at the map's corner (0, 0, 0) for a moment before it lands. The view
/// stays on the room, and so does the roof cut (DOS keeps the room in view throughout): its
/// script takes the camera off the Avatar first, and the original only looks the roof up again
/// while the camera is centred on him.
#[test]
#[ignore]
fn flash_keeps_the_roof_cut_while_the_avatar_is_away() {
    let Some(mut g) = saved(2) else { return };
    let flash = sorcery(&mut g, 0, 3);
    let a = g.world.get(AVATAR).unwrap();
    let (px, py) = (a.x + 200, a.y - 100);
    let floor = (0..g.world.len()).map(|i| g.world.id(i)).find(|&c| {
        g.world.abs_box(c).is_some_and(|b| b.z1 == a.z && b.x0 < px && px <= b.x1 && b.y0 < py && py <= b.y1)
    });
    let roof = g.roof();
    assert!(roof.is_some(), "indoors");
    g.trigger(flash, event::USE);
    let mut away = 0;
    for t in 0..8 * 30 {
        g.update(1.0 / 30.0, &Input::default());
        for e in g.take_events() {
            match e {
                UiEvent::Target { pid } => g.vm().unwrap().finish(pid, floor.expect("a floor spot") as u32),
                UiEvent::Bark { pid, .. } => g.vm().unwrap().finish(pid, 0),
                _ => {}
            }
        }
        away += g.world.get(AVATAR).is_some_and(|a| (a.x, a.y, a.z) == (0, 0, 0)) as u32;
        assert_eq!(g.roof(), roof, "tick {t}");
    }
    assert!(away > 10, "the Avatar was away {away} ticks");
    assert!(g.world.get(AVATAR).is_some_and(|b| (b.x, b.y) != (a.x, a.y)), "and landed elsewhere");
}
