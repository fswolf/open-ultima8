//! The tester's kit (`Game::give_test_kit`), and every school's magic used from it: no script
//! errors or missing intrinsics, and what each item says or does (printed with --nocapture).
//! Run against a real install: U8_DATA=/path/to/U8 cargo test -- --ignored
mod common;
use common::*;
use u8engine::usecode::{event, UiEvent};

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
