//! The original's own Birthplace of Moriens, and "Restored content" (`u8engine::restore`): each
//! entry's hook fires once, its flag or thing is there and stays over a save and load.
mod common;
use common::*;
use u8engine::usecode::{event, UiEvent};

/// Story flags (global bits; FLAGS.unk's order, 14 on).
const LOTHIAN_DEAD: usize = 0xBF;
const LOTH_INTERRED: usize = 0xD3;
const IS_APPRENTICE: usize = 0xD7;
const OPEN_BIRTH: usize = 0xDE;
const SAID_BIRTH: usize = 0xDF;

fn find(g: &Game, shape: u32, near: (i32, i32), r: i32) -> Vec<u16> {
    g.world
        .objects()
        .filter(|&id| {
            g.world.get(id).is_some_and(|it| it.shape == shape && (it.x - near.0).abs().max((it.y - near.1).abs()) <= r)
        })
        .collect()
}

/// Finish what the scripts show: speech read at once, the targeting cursor on `pick`, questions
/// answered by the first option starting with `say` (else the last); every question's options.
fn run(g: &mut Game, secs: u32, pick: u16, say: &str) -> Vec<Vec<String>> {
    let mut asked = vec![];
    for _ in 0..secs * 30 {
        g.update(1.0 / 30.0, &Input::default());
        for e in g.take_events() {
            let vm = g.vm().unwrap();
            match e {
                UiEvent::Bark { pid, .. } => vm.finish(pid, 0),
                UiEvent::Target { pid } => vm.finish(pid, pick as u32),
                UiEvent::Read { pid, .. } => vm.finish(pid, 0),
                UiEvent::Ask { pid, options, .. } => {
                    let i =
                        options.iter().position(|o| !say.is_empty() && o.starts_with(say)).unwrap_or(options.len() - 1);
                    vm.answer(pid, &options, i);
                    asked.push(options);
                }
                _ => {}
            }
        }
    }
    asked
}

/// The Birthplace of Moriens, as shipped (option off): with Lothian interred the Key of the
/// Scion (232 frame 3) opens the tomb's door (lock 72, KEYRING); the teleporter inside leads to
/// the chamber with the altar; a ghoul comes; the chamber's plaque (quality 69), read after
/// Vividos's "Oh yes, my pilgrimage", sets openBirth, and Vividos then hears "I have seen the
/// Birthplace of Moriens."
#[test]
#[ignore]
fn the_birthplace_of_moriens_works_as_shipped() {
    let Some(mut g) = saved(1) else { return };
    g.put_avatar(50, (0, 0, 0));
    let door = find(&g, 68, (10271, 2879), 0)[0];
    assert_eq!(g.world.get(door).unwrap().quality & 0x4FF, 0x400 | 72, "locked, lock 72");
    // Outside, in front of the door, in reach of it.
    g.put_avatar(50, (10239, 2943, 48));
    assert!(g.world.can_reach(AVATAR, door, 128));
    let pack = g.backpack().unwrap_or(AVATAR);
    let key = g.world.add_inside(
        &g.data,
        u8data::map::Item { shape: 232, frame: 3, x: 20, y: 20, ..Default::default() },
        pack,
    );
    g.vm().unwrap().set_global_bits(LOTH_INTERRED, 1, 1);
    g.trigger(key, event::USE);
    run(&mut g, 2, door, "");
    assert!(g.world.get(door).unwrap().quality & 0x400 == 0, "unlocked");
    g.trigger(door, event::USE);
    run(&mut g, 3, 0, "");
    // In from the east through the open door, west to the teleporter.
    g.put_avatar(50, (10367, 2815, 48));
    let west = Input { dir: Some(6), ..Default::default() };
    for _ in 0..90 {
        g.update(1.0 / 30.0, &west);
    }
    run(&mut g, 2, 0, "");
    let a = g.world.get(AVATAR).unwrap();
    assert!(
        (a.x - 14624).abs().max((a.y - 13480).abs()) < 512 && g.map() == 50,
        "in the chamber: {:?}",
        (a.x, a.y, g.map())
    );
    let ghouls = |g: &Game| {
        (2..256u16).filter(|&id| g.world.get(id).is_some_and(|it| it.shape == 142 && !g.world.is_void(id))).count()
    };
    run(&mut g, 10, 0, "");
    assert!(ghouls(&g) > 0, "a ghoul comes");
    // The plaque, once Vividos has told of the pilgrimage.
    let plaque = find(&g, 614, (14207, 12735), 0)[0];
    g.trigger(plaque, event::USE);
    run(&mut g, 1, 0, "");
    assert_eq!(g.vm().unwrap().global_bits(OPEN_BIRTH, 1), 0, "not before the pilgrimage was told");
    g.vm().unwrap().set_global_bits(SAID_BIRTH, 1, 1);
    g.trigger(plaque, event::USE);
    run(&mut g, 1, 0, "");
    assert_eq!(g.vm().unwrap().global_bits(OPEN_BIRTH, 1), 1, "openBirth");
    // Vividos, to an apprentice (Lothian dead).
    for f in [LOTHIAN_DEAD, IS_APPRENTICE] {
        g.vm().unwrap().set_global_bits(f, 1, 1);
    }
    let vividos = 21;
    g.set_minutes(600);
    g.put_avatar(6, (0, 0, 0));
    let v = g.world.get(vividos).expect("Vividos in the cemetery at ten");
    g.put_avatar(6, (v.x + 96, v.y, v.z));
    g.trigger(vividos, event::USE);
    let asked = run(&mut g, 20, 0, "I have seen");
    assert!(asked.iter().flatten().any(|o| o.starts_with("I have seen the Birthplace of Moriens.")), "{asked:?}");
}

/// With U8_DOS_OUT set: the Birthplace door's start state for the original (the Avatar in front
/// of it, the Key of the Scion on the floor by him, Lothian interred), and where to click.
#[test]
#[ignore]
fn birthplace_door_for_dos() {
    let (Some(mut g), Ok(out)) = (saved(1), std::env::var("U8_DOS_OUT")) else { return };
    g.put_avatar(50, (0, 0, 0));
    g.put_avatar(50, (10367, 2815, 48));
    let key = g.world.add(
        &g.data,
        u8data::map::Item { shape: 232, frame: 3, x: 10399, y: 2879, z: 48, ..Default::default() },
        true,
    );
    g.vm().unwrap().set_global_bits(LOTH_INTERRED, 1, 1);
    run(&mut g, 1, 0, "");
    std::fs::create_dir_all(&out).unwrap();
    for (name, d) in &g.snapshot().files {
        std::fs::write(PathBuf::from(&out).join(name), d).unwrap();
    }
    let scr = |x: i32, y: i32, z: i32| (x / 4 - y / 4, x / 8 + y / 8 - z);
    let a = scr(10367, 2815, 48 + 20);
    for (what, p) in [("key", scr(10399 - 8, 2879 - 8, 48)), ("door", scr(10271 - 16, 2815, 48 + 30))] {
        println!("{what} {} {} (key id {key})", p.0 - a.0, p.1 - a.1);
    }
}

/// Slot 1 with the option on (or off), the Avatar on `map` at `at` (or wherever it arrives).
fn on_map(map: u16, at: (i32, i32, i32), on: bool) -> Option<Game> {
    let mut g = saved(1)?;
    g.restored.on = on;
    g.put_avatar(map, (0, 0, 0));
    g.put_avatar(map, at);
    run(&mut g, 2, 0, "");
    Some(g)
}

/// Things of `shape` and `frame` at `at`.
fn things(g: &Game, shape: u32, frame: u32, at: (i32, i32, i32)) -> usize {
    g.world
        .objects()
        .filter(|&id| {
            g.world.get(id).is_some_and(|it| (it.shape, it.frame, it.x, it.y, it.z) == (shape, frame, at.0, at.1, at.2))
        })
        .count()
}

/// A save and load of `g` (its side file too); the option on in the loaded one.
fn reload(g: &Game) -> Game {
    reload_as(g, Some(1))
}

/// The same, the side file read as going with a save of `print` (1 is this one: another, a save
/// written over since by the original), or no side file.
fn reload_as(g: &Game, print: Option<u64>) -> Game {
    let root = PathBuf::from(std::env::var("U8_DATA").unwrap());
    let gd = GameData::load_save(&root, Some(&g.snapshot())).unwrap();
    let scripts = Scripts::load(&root, &gd).ok();
    let mut h = Game::new(gd, scripts, Some(g.map())).unwrap();
    if let Some(p) = print {
        assert_eq!(h.apply_extra(&g.extra(1), p), p == 1);
    }
    h.restored.on = true;
    run(&mut h, 2, 0, "");
    h
}

/// Entries 3, 4, 5 and 7: each thing is put once where the table says, on arriving on its map,
/// and a save and load keeps the one without putting another; with the option off, nothing.
#[test]
#[ignore]
fn things_are_put_once() {
    use u8engine::restore::{Act, Hook, ENTRIES};
    let placed = ENTRIES.iter().filter_map(|e| match (e.hook, e.act) {
        (Hook::Map(m), Act::Place(shape, frame, q, at)) => Some((e.name, m, shape, frame, q, at)),
        _ => None,
    });
    for (name, map, shape, frame, quality, at) in placed {
        let Some(g) = on_map(map, (0, 0, 0), false) else { return };
        assert_eq!(things(&g, shape, frame, at), 0, "{name}: nothing with the option off");
        let mut g = on_map(map, (0, 0, 0), true).unwrap();
        assert_eq!(things(&g, shape, frame, at), 1, "{name} put");
        let id = g
            .world
            .objects()
            .find(|&id| g.world.get(id).is_some_and(|it| it.shape == shape && (it.x, it.y, it.z) == at))
            .unwrap();
        assert_eq!(g.world.get(id).unwrap().quality, quality);
        // (On something whose top is where it lies: the ground, a table, a shelf board.)
        let b = g.world.entry(g.world.index(id).unwrap()).bounds();
        let rests = g.world.area(at.0, at.1, 256).into_iter().filter(|&t| t != id).any(|t| {
            let o = g.world.entry(g.world.index(t).unwrap()).bounds();
            o.z1 == at.2 && o.x0 < b.x1 && b.x0 < o.x1 && o.y0 < b.y1 && b.y0 < o.y1
        });
        assert!(rests, "{name} lies on something");
        let reach = (-6..=6).flat_map(|i| (-6..=6).map(move |j| (at.0 + 32 * i, at.1 + 32 * j))).find(|&(x, y)| {
            [at.2 - 64, at.2 - 48, at.2 - 16, at.2].into_iter().any(|z| {
                g.world.fits(&g.data, AVATAR, (x, y, z)) && {
                    g.put_avatar(map, (x, y, z));
                    g.world.floor_under(AVATAR) == Some(z) && g.world.can_reach(AVATAR, id, 128)
                }
            })
        });
        assert!(reach.is_some(), "{name} can be reached");
        assert!(g.restored.done.iter().any(|d| d.0 == name));
        run(&mut g, 3, 0, "");
        assert_eq!(things(&g, shape, frame, at), 1, "{name}: once");
        let h = reload(&g);
        assert_eq!(things(&h, shape, frame, at), 1, "{name}: kept, not put again");
        // Written over by the original since, or the side file lost: still one.
        for print in [Some(2), None] {
            assert_eq!(things(&reload_as(&g, print), shape, frame, at), 1, "{name}: {print:?}, one");
        }
        // Taken away and the map left and entered again: not put back.
        let mut h = h;
        let it = h
            .world
            .objects()
            .find(|&id| h.world.get(id).is_some_and(|it| it.shape == shape && (it.x, it.y, it.z) == at));
        h.world.remove(it.unwrap());
        h.put_avatar(if map == 6 { 39 } else { 6 }, (0, 0, 0));
        run(&mut h, 1, 0, "");
        h.put_avatar(map, (0, 0, 0));
        run(&mut h, 2, 0, "");
        assert_eq!(things(&h, shape, frame, at), 0, "{name}: never twice");
    }
}

/// Put the Avatar standing somewhere within reach of `id` (near `at`, on `map`).
fn stand_by(g: &mut Game, map: u16, at: (i32, i32, i32), id: u16) -> bool {
    (-6..=6).flat_map(|i| (-6..=6).map(move |j| (at.0 + 32 * i, at.1 + 32 * j))).any(|(x, y)| {
        [at.2 - 64, at.2 - 48, at.2 - 16, at.2].into_iter().any(|z| {
            g.world.fits(&g.data, AVATAR, (x, y, z)) && {
                g.put_avatar(map, (x, y, z));
                g.world.floor_under(AVATAR) == Some(z) && g.world.can_reach(AVATAR, id, 128)
            }
        })
    })
}

/// What the placed things are for: the book reads, the trowel digs dirt (395 frame 9) out of the ground (shapes 3
/// and 4). The torch is meant to light wood (395 frame 19) into a campfire (267), but its script
/// changes a thing's shape only within the same family (FREE legal_setType), and the campfire is
/// family 0 against the wood's 9 (reagent), so in the original too the wood stays wood.
#[test]
#[ignore]
fn what_the_placed_things_do() {
    use u8data::map::Item;
    let Some(mut g) = on_map(6, (0, 0, 0), true) else { return };
    let trowel = find(&g, 834, (7903, 25823), 0)[0];
    assert!(stand_by(&mut g, 6, (7903, 25823, 8), trowel));
    let a = g.world.get(AVATAR).unwrap();
    let ground =
        g.world.area(a.x, a.y, 160).into_iter().find(|&id| {
            g.world.get(id).is_some_and(|it| [3, 4].contains(&it.shape)) && g.world.can_reach(AVATAR, id, 128)
        });
    let ground = ground.expect("ground to dig");
    let dirt =
        |g: &Game| find(g, 395, (a.x, a.y), 512).into_iter().filter(|&d| g.world.get(d).unwrap().frame == 9).count();
    let before = dirt(&g);
    g.trigger(trowel, event::USE);
    run(&mut g, 2, ground, "");
    assert_eq!(dirt(&g), before + 1, "a handful of dirt");
    // The Mysteries of Lithos reads as itself.
    let book = find(&g, 613, (8159, 24863), 0).into_iter().find(|&b| g.world.get(b).unwrap().quality == 110).unwrap();
    assert!(stand_by(&mut g, 6, (8159, 24863, 80), book));
    g.trigger(book, event::USE);
    let mut text = String::new();
    for _ in 0..60 {
        g.update(1.0 / 30.0, &Input::default());
        for e in g.take_events() {
            if let UiEvent::Read { pid, text: t, .. } = e {
                text += &t;
                g.vm().unwrap().finish(pid, 0);
            }
        }
    }
    assert!(text.contains("Lithos"), "{text}");
    let Some(mut g) = on_map(41, (0, 0, 0), true) else { return };
    let torch = find(&g, 568, (15295, 1951), 0)[0];
    assert!(stand_by(&mut g, 41, (15295, 1951, 96), torch));
    let a = g.world.get(AVATAR).unwrap();
    let wood = (1..4)
        .flat_map(|k| [(k, 0), (-k, 0), (0, k), (0, -k)])
        .map(|(i, j)| (a.x + 64 * i, a.y + 64 * j, a.z))
        .find(|&p| g.world.fits(&g.data, AVATAR, p))
        .map(|(x, y, z)| g.world.add(&g.data, Item { shape: 395, frame: 19, x, y, z, ..Default::default() }, true))
        .unwrap();
    g.trigger(torch, event::USE);
    run(&mut g, 2, wood, "");
    assert_eq!(g.world.get(wood).map(|it| it.shape), Some(395), "no campfire, as in the original");
}

/// Entry 7: the Key of the Necromancer lies on the Birthplace's altar, is called so, and makes
/// a spell from a bag of reagents as the Caretaker's key does (blood moss and bone, frames 0 and
/// 6 of 395: Death Speak, 746 frame 0).
#[test]
#[ignore]
fn the_key_of_the_necromancer_lies_on_the_altar() {
    use u8data::map::Item;
    let Some(mut g) = on_map(50, (14400, 12700, 48), true) else { return };
    let on_altar =
        |g: &Game| g.world.objects().find(|&id| g.world.get(id).is_some_and(|it| (it.shape, it.frame) == (232, 4)));
    let key = on_altar(&g).expect("the key");
    let (k, altar) = (g.world.get(key).unwrap(), find(&g, 550, (14212, 12732), 0)[0]);
    let a = g.world.entry(g.world.index(altar).unwrap()).bounds();
    assert!(k.z == a.z1 && k.x > a.x0 && k.y > a.y0 && k.x - 64 < a.x1 && k.y - 32 < a.y1, "on the altar: {k:?} {a:?}");
    for frame in [0, 4] {
        let key = if frame == 4 {
            key
        } else {
            g.world.add_inside(&g.data, Item { shape: 232, x: 20, y: 20, ..Default::default() }, AVATAR)
        };
        g.world.move_inside(&g.data, key, AVATAR, (30, 30));
        g.trigger(key, event::LOOK);
        g.update(1.0 / 30.0, &Input::default());
        let looked: Vec<String> = g
            .take_events()
            .into_iter()
            .filter_map(|e| match e {
                UiEvent::Bark { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        let name = if frame == 4 { "Key of the Necromancer" } else { "Key of the Caretaker" };
        assert!(looked.iter().any(|t| t.starts_with(name)), "{looked:?}");
        let bag = g.world.add_inside(&g.data, Item { shape: 637, x: 40, y: 40, ..Default::default() }, AVATAR);
        for f in [0, 6] {
            g.world.add_inside(
                &g.data,
                Item { shape: 395, frame: f, quality: 1, x: 20, y: 20, ..Default::default() },
                bag,
            );
        }
        g.trigger(key, event::USE);
        run(&mut g, 3, bag, "");
        let made = g
            .world
            .contents(bag)
            .into_iter()
            .filter(|&t| g.world.get(t).is_some_and(|it| (it.shape, it.frame) == (746, 0)))
            .count();
        assert_eq!(made, 1, "key frame {frame} makes Death Speak");
    }
    // Option off: no key there.
    let g = on_map(50, (14400, 12700, 48), false).unwrap();
    assert!(on_altar(&g).is_none());
}

/// Slot 1 at ten, beside `npc` on `map`, with these story flags set.
fn beside(map: u16, npc: u16, flags: &[usize]) -> Option<Game> {
    let mut g = saved(1)?;
    g.put_avatar(map, (0, 0, 0));
    g.set_minutes(600);
    g.put_avatar(map, (0, 0, 0));
    let n = g.world.get(npc)?;
    let at = [96, 128, 160, 64]
        .iter()
        .flat_map(|r| {
            [(0, 1), (1, 0), (0, -1), (-1, 0), (1, 1), (-1, -1)].map(|(dx, dy)| (n.x + dx * r, n.y + dy * r, n.z))
        })
        .find(|&p| g.world.fits(&g.data, AVATAR, p))?;
    g.put_avatar(map, at);
    for &f in flags {
        g.vm().unwrap().set_global_bits(f, 1, 1);
    }
    run(&mut g, 1, 0, "");
    Some(g)
}

/// Talk to `npc` again and again, each time taking replies not taken before (leaving by the
/// way out only when nothing new is left), till `line` is said or nothing new comes up. What
/// was said, in order.
fn talk_until(g: &mut Game, npc: u16, line: &str) -> Vec<String> {
    let exits = ["Goodbye", "Farewell", "bye", "Bye"];
    let (mut said, mut taken) = (vec![], std::collections::HashSet::new());
    for _ in 0..12 {
        let before = taken.len();
        g.trigger(npc, event::USE);
        let mut quiet = 0;
        for _ in 0..30 * 120 {
            g.update(1.0 / 30.0, &Input::default());
            let mut talking = false;
            for e in g.take_events() {
                let vm = g.vm().unwrap();
                match e {
                    UiEvent::Bark { pid, text, .. } => {
                        said.push(text);
                        vm.finish(pid, 0);
                        talking = true;
                    }
                    UiEvent::Ask { pid, options, .. } => {
                        let new =
                            options.iter().position(|o| !taken.contains(o) && !exits.iter().any(|x| o.starts_with(x)));
                        let i = new
                            .or_else(|| options.iter().position(|o| exits.iter().any(|x| o.starts_with(x))))
                            .unwrap_or(0);
                        taken.insert(options[i].clone());
                        said.push(format!("? {}", options.join(" | ")));
                        said.push(format!("> {}", options[i]));
                        vm.answer(pid, &options, i);
                        talking = true;
                    }
                    UiEvent::Read { pid, .. } => vm.finish(pid, 0),
                    _ => {}
                }
            }
            if said.iter().any(|t| t.contains(line)) {
                return said;
            }
            quiet = if talking { 0 } else { quiet + 1 };
            if quiet > 150 {
                break;
            }
        }
        if taken.len() == before {
            break;
        }
    }
    said
}

const HAS_HEART: usize = u8engine::restore::flag::HAS_HEART;
const MORDEA_DEAD: usize = u8engine::restore::flag::MORDEA_DEAD;

/// Entry 2: mordeaDead set once Mordea dies (only with the option on), kept over a save and
/// load; Gwillim, Aramina (looking for the dagger), Kilandra and Vividos then have their lines.
/// (Torwin's sits behind his foci story, read in the script only.)
#[test]
#[ignore]
fn mordea_dead_is_noted() {
    let flag = |g: &mut Game| g.vm().unwrap().global_bits(MORDEA_DEAD, 1);
    for on in [false, true] {
        let Some(mut g) = on_map(40, (0, 0, 0), on) else { return };
        assert_eq!(flag(&mut g), 0);
        g.stats.get_mut(7).unwrap().dead = true;
        run(&mut g, 2, 0, "");
        assert_eq!(flag(&mut g), on as u32, "option {on}");
        if on {
            assert_eq!(g.restored.done.iter().filter(|d| d.0 == "mordea-dead").count(), 1);
            let mut h = reload(&g);
            assert_eq!(flag(&mut h), 1, "kept");
            assert_eq!(h.restored.done.iter().filter(|d| d.0 == "mordea-dead").count(), 1, "not again");
        }
    }
    const TOLD_DAGGER: usize = 0xD6;
    for (map, npc, flags, line) in [
        (7, 14, &[MORDEA_DEAD][..], "Gwillim, Mordea's dead."),
        (41, 13, &[MORDEA_DEAD, TOLD_DAGGER][..], "Mordea kept it in a chest"),
        (4, 16, &[MORDEA_DEAD][..], "Lady Mordea is dead!"),
        (6, 21, &[MORDEA_DEAD][..], "Mordea is dead."),
    ] {
        let mut g = beside(map, npc, flags).unwrap();
        let said = talk_until(&mut g, npc, line);
        assert!(said.iter().any(|t| t.contains(line)), "{npc}: {said:?}");
    }
}

/// hasHeart's lines: who says what once the Heart is taken (map and person at ten o'clock).
const DEAD_WALK: [(u16, u16, &str); 17] = [
    (41, 13, "'Tis another dead one"),
    (39, 5, "The dead are walking the streets"),
    (39, 2, "one of the ghouls who seem"),
    (40, 11, "the dead rise"),
    (40, 45, "beset by the dead"),
    (40, 46, "walking dead overrun the city"),
    (40, 47, "The walking dead march upon the city"),
    (40, 48, "spits up the dead"),
    (40, 49, "Find shelter before the quakes"),
    (40, 50, "the death that walks"),
    (40, 51, "You are not undead"),
    (5, 59, "the dead have come"),
    (39, 24, "The dead have left their graves"),
    (8, 33, "I assume the quakes are your responsibility"),
    (39, 19, "the living dead walk the streets"),
    (41, 8, "the walking dead march upon the city"),
    (6, 21, "You stole the Heart of Earth"),
];

/// Entry 1: holding the glowing Heart of Earth (EBROCK frame 1) sets hasHeart, once, with the
/// option on; a game day later it clears and the people it silenced talk again. Each of the 17
/// gives the line written for it; Devon and Mythran, whom the story needs later, talk on as
/// before (Vividos talks his talk through, then sends the Avatar away).
#[test]
#[ignore]
fn the_heart_wakes_the_dead_for_a_day() {
    use u8data::map::Item;
    let flag = |g: &mut Game| g.vm().unwrap().global_bits(HAS_HEART, 1);
    for on in [false, true] {
        let Some(mut g) = on_map(41, (0, 0, 0), on) else { return };
        g.world.add_inside(&g.data, Item { shape: 793, frame: 2, x: 20, y: 20, ..Default::default() }, AVATAR);
        run(&mut g, 2, 0, "");
        assert_eq!(flag(&mut g), 0, "the Tear of the Seas isn't the Heart");
        g.world.add_inside(&g.data, Item { shape: 793, frame: 1, x: 30, y: 20, ..Default::default() }, AVATAR);
        run(&mut g, 2, 0, "");
        assert_eq!(flag(&mut g), on as u32, "option {on}");
        if !on {
            continue;
        }
        // Turned off within the day, the day still ends it.
        let mut h = reload(&g);
        h.restored.on = false;
        h.set_minutes(h.minutes() + 24 * 60 + 5);
        run(&mut h, 2, 0, "");
        assert_eq!(flag(&mut h), 0, "off, the dead still go back");
        let mut h = reload(&g);
        assert_eq!(flag(&mut h), 1, "kept");
        let now = h.minutes();
        h.set_minutes(now + 23 * 60);
        run(&mut h, 2, 0, "");
        assert_eq!(flag(&mut h), 1, "still the next morning");
        h.set_minutes(now + 24 * 60 + 5);
        run(&mut h, 2, 0, "");
        assert_eq!(flag(&mut h), 0, "a day on, the dead go back");
        // Still holding the Heart: not set again.
        run(&mut h, 3, 0, "");
        assert_eq!(flag(&mut h), 0);
    }
    for (map, npc, line) in DEAD_WALK {
        // (Mythran says his after "Things are well.", once met and past the Blackrock lore.)
        let flags = if npc == 33 { &[HAS_HEART, 0x43, 0x120][..] } else { &[HAS_HEART][..] };
        let mut g = beside(map, npc, flags).unwrap();
        let said = talk_until(&mut g, npc, line);
        assert!(said.iter().any(|t| t.contains(line)), "{npc}: {said:?}");
        if [2, 33].contains(&npc) {
            // The story's talks go on past it.
            let at = said.iter().position(|t| t.contains(line)).unwrap();
            let more = talk_until(&mut g, npc, "\u{0}");
            assert!(more.iter().filter(|t| t.starts_with("? ")).count() > 2, "{npc} talks on after {at}: {more:?}");
        }
    }
    // Tarna talks again once it clears.
    let mut g = beside(41, 8, &[]).unwrap();
    let said = talk_until(&mut g, 8, "?");
    assert!(said.iter().any(|t| t.starts_with("? ")), "Tarna asks again: {said:?}");
}

/// While hasHeart is set the towns' guard posts (GUARDMEG eggs, entering the fast area) raise a
/// ghoul (142) now and then instead of a guard; once it clears, guards again.
#[test]
#[ignore]
fn guard_posts_raise_the_dead_while_the_heart_is_out() {
    let posts = |g: &Game| -> Vec<(i32, i32, i32)> {
        g.world
            .objects()
            .filter_map(|id| g.world.get(id))
            .filter(|it| it.shape == 73 && it.quality == 0x34)
            .map(|it| (it.x, it.y, it.z))
            .collect()
    };
    let count = |g: &Game, shape: u32| {
        g.world.objects().filter(|&id| g.world.get(id).is_some_and(|it| it.shape == shape && it.npc > 0)).count()
    };
    for heart in [1, 0] {
        let Some(mut g) = on_map(39, (0, 0, 0), true) else { return };
        g.vm().unwrap().set_global_bits(HAS_HEART, 1, heart);
        let at = posts(&g);
        assert!(!at.is_empty(), "guard posts on map 39");
        let (mut ghouls, mut guards) = (0, 0);
        for _ in 0..8 {
            for &(x, y, z) in &at {
                g.put_avatar(39, (x + 64, y + 64, z));
                run(&mut g, 1, 0, "");
                (ghouls, guards) = (ghouls.max(count(&g, 142)), guards.max(count(&g, 574)));
            }
        }
        if heart == 1 {
            assert!(ghouls > 0, "the dead walk");
        } else {
            assert_eq!(ghouls, 0, "no dead without the Heart");
            assert!(guards > 0, "guards at their posts");
        }
    }
}

/// Ticks of `secs` seconds (speech finished at once): how many had the ground shaking, and the
/// furthest the view was shaken.
fn shaking(g: &mut Game, secs: u32) -> (u32, i8) {
    let (mut n, mut most) = (0, 0);
    for _ in 0..secs * 30 {
        run(g, 0, 0, "");
        g.update(1.0 / 30.0, &Input::default());
        for e in g.take_events() {
            if let UiEvent::Bark { pid, .. } = e {
                g.vm().unwrap().finish(pid, 0);
            }
        }
        if g.vm().unwrap().quake > 0 {
            n += 1;
        }
        most = most.max(g.shake.at.0.abs()).max(g.shake.at.1.abs());
    }
    (n, most)
}

/// The quake: the first time in Tenebrae while the dead walk, the Mountain King's own (strength
/// 1, 120 ticks), once; never over a talk; not with the option off; none once the dead are gone.
#[test]
#[ignore]
fn the_ground_shakes_once_when_the_dead_come() {
    const TALKING: usize = u8engine::restore::flag::SOMEBODY_TALKING;
    let Some(mut g) = on_map(39, (0, 0, 0), false) else { return };
    g.vm().unwrap().set_global_bits(HAS_HEART, 1, 1);
    assert_eq!(shaking(&mut g, 3).0, 0, "option off: nothing");
    for map in [39, 40, 41] {
        let mut g = on_map(map, (0, 0, 0), true).unwrap();
        assert_eq!(shaking(&mut g, 2).0, 0, "no dead, no quake");
        g.vm().unwrap().set_global_bits(TALKING, 1, 1);
        g.vm().unwrap().set_global_bits(HAS_HEART, 1, 1);
        assert_eq!(shaking(&mut g, 3).0, 0, "not over a talk");
        g.vm().unwrap().set_global_bits(TALKING, 1, 0);
        let (n, most) = shaking(&mut g, 6);
        assert_eq!(n, 120, "map {map}: four seconds of it");
        assert!((1..=4).contains(&most), "a mild shake: {most}");
        assert_eq!((g.vm().unwrap().quake, g.shake.at), (0, (0, 0)), "over, the view still");
        assert_eq!(shaking(&mut g, 3).0, 0, "once");
        g.put_avatar(if map == 39 { 40 } else { 39 }, (0, 0, 0));
        assert_eq!(shaking(&mut g, 2).0, 0, "once in the whole town");
        let mut h = reload(&g);
        assert_eq!(shaking(&mut h, 3).0, 0, "not again after a load");
    }
    // In a real conversation (Tarna's): it waits until the talk is over.
    let mut g = beside(41, 8, &[]).unwrap();
    g.restored.on = true;
    g.trigger(8, event::USE);
    let mut asked = None;
    for _ in 0..90 {
        g.update(1.0 / 30.0, &Input::default());
        for e in g.take_events() {
            match e {
                UiEvent::Bark { pid, .. } => g.vm().unwrap().finish(pid, 0),
                UiEvent::Ask { pid, options, .. } => asked = Some((pid, options)),
                _ => {}
            }
        }
        if asked.is_some() {
            break;
        }
    }
    let (pid, options) = asked.expect("Tarna asks");
    g.vm().unwrap().set_global_bits(HAS_HEART, 1, 1);
    let mut quiet = 0;
    for _ in 0..60 {
        g.update(1.0 / 30.0, &Input::default());
        quiet += (g.vm().unwrap().quake == 0) as u32;
    }
    assert_eq!(quiet, 60, "not while she asks");
    let bye = options.iter().position(|o| o.starts_with("Bye")).unwrap_or(options.len() - 1);
    g.vm().unwrap().answer(pid, &options, bye);
    assert_eq!(shaking(&mut g, 8).0, 120, "after the talk");
    // The Heart taken away from town, and the day over before the Avatar comes: no quake.
    let mut g = on_map(6, (0, 0, 0), true).unwrap();
    g.world.add_inside(&g.data, u8data::map::Item { shape: 793, frame: 1, x: 30, y: 20, ..Default::default() }, AVATAR);
    run(&mut g, 2, 0, "");
    assert_eq!(g.vm().unwrap().global_bits(HAS_HEART, 1), 1);
    let now = g.minutes();
    g.set_minutes(now + 24 * 60 + 5);
    run(&mut g, 2, 0, "");
    g.put_avatar(40, (0, 0, 0));
    assert_eq!(shaking(&mut g, 3).0, 0, "the dead gone back: no quake");
}

/// With U8_DOS_OUT set: a save made with the option on for the original (by the Birthplace's
/// altar with the Key of the Necromancer on it, the Heart held: hasHeart set).
#[test]
#[ignore]
fn restored_save_for_dos() {
    let Ok(out) = std::env::var("U8_DOS_OUT") else { return };
    let mut g = on_map(50, (14400, 12700, 48), true).unwrap();
    g.world.add_inside(&g.data, u8data::map::Item { shape: 793, frame: 1, x: 30, y: 20, ..Default::default() }, AVATAR);
    run(&mut g, 2, 0, "");
    assert_eq!(g.vm().unwrap().global_bits(HAS_HEART, 1), 1);
    if std::env::var("U8_DOS_AT").is_ok_and(|m| m == "40") {
        // (Or in the castle, after the quake.)
        g.put_avatar(40, (0, 0, 0));
        assert!((0..120)
            .step_by(8)
            .any(|z| g.put_avatar(40, (12799, 9400, z)) && g.world.floor_under(AVATAR) == Some(z)));
        assert_eq!(shaking(&mut g, 6).0, 120);
    }
    std::fs::create_dir_all(&out).unwrap();
    for (name, d) in &g.snapshot().files {
        if name != u8data::save::EXTRA {
            std::fs::write(PathBuf::from(&out).join(name), d).unwrap();
        }
    }
}
