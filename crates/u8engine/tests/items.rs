//! Carrying, bags, wearing and wielding, weights, armour and the weapon table.
//! Run against a real install: U8_DATA=/path/to/U8 cargo test -- --ignored
mod common;
use common::*;
use u8data::map::Item;
use u8engine::usecode::{event, ReadKind, UiEvent};

#[test]
#[ignore]
fn the_avatar_carries_things_and_bags_open() {
    use u8engine::usecode::{event, UiEvent};
    let Some(mut g) = game(40) else { return };
    let carried = g.world.contents(AVATAR);
    assert_eq!(carried.len(), 5, "backpack and four worn things in the save");
    let pack = *carried.iter().find(|&&id| g.world.get(id).unwrap().shape == 529).expect("a backpack");
    assert_eq!(g.world.contents(pack).len(), 7);
    assert!(g.world.get(pack).is_some() && !g.world.entry(g.world.index(pack).unwrap()).present(), "not on the map");
    // Using the backpack opens its window.
    assert!(g.trigger(pack, event::USE));
    g.update(1.0 / 30.0, &Input::default());
    let opened = g.take_events().into_iter().any(|e| matches!(e, UiEvent::OpenGump { item, .. } if item == pack));
    assert!(opened, "the backpack's script opens its window");
    // Belongings come along through a teleporter.
    let mut av = g.world.get(AVATAR).unwrap();
    (av.x, av.y, av.z) = (13048, 15400, 48);
    g.world.set(&g.data, AVATAR, av);
    assert!(g.update(1.0 / 30.0, &Input::default()).map_changed);
    assert_eq!(g.world.contents(AVATAR).len(), 5);
    // Windows close on the way: using the backpack opens it again (it doesn't think it's open).
    g.take_events();
    let pack = *g.world.contents(AVATAR).iter().find(|&&id| g.world.get(id).unwrap().shape == 529).unwrap();
    assert!(g.trigger(pack, event::USE));
    g.update(1.0 / 30.0, &Input::default());
    assert!(g.take_events().into_iter().any(|e| matches!(e, UiEvent::OpenGump { item, .. } if item == pack)));
}

#[test]
#[ignore]
fn items_move_between_bags_and_the_floor_and_saves_keep_it() {
    use u8data::save::Save;
    use u8engine::Place;
    let Some(mut g) = game(40) else { return };
    let dir = PathBuf::from(std::env::var("U8_DATA").unwrap());
    let pack = *g.world.contents(AVATAR).iter().find(|&&id| g.world.get(id).unwrap().shape == 529).unwrap();
    let dagger = *g.world.contents(pack).iter().find(|&&id| g.world.get(id).unwrap().shape == 418).expect("a dagger");
    // Out of the backpack onto the floor next to the Avatar.
    let av = g.world.get(AVATAR).unwrap();
    let spot = (av.x + 96, av.y, av.z);
    assert_eq!(g.move_item(dagger, Place::Map(spot)), Ok(()));
    assert!(g.world.container(dagger).is_none() && g.world.entry(g.world.index(dagger).unwrap()).shown());
    assert_eq!(g.world.contents(pack).len(), 6);
    // Scenery and people can't be picked up; things out of reach can't either.
    let fixed = g
        .world
        .objects()
        .find(|&id| g.world.entry(g.world.index(id).unwrap()).origin == u8engine::world::Origin::Fixed)
        .unwrap();
    assert_eq!(g.can_pick_up(fixed), Err("You can't move that."));
    assert_eq!(g.can_pick_up(AVATAR), Err("You can't move that."));
    let far = g.world.objects().find(|&id| {
        let e = g.world.entry(g.world.index(id).unwrap());
        e.origin == u8engine::world::Origin::Movable
            && e.present()
            && !e.traits.egg
            && (e.it.x - av.x).abs() > 2000
            && !g.data.typeflags.get(e.it.shape).has(u8data::typeflag::flag::FIXED)
            && g.data.typeflags.get(e.it.shape).weight != 0 // (no weight: never movable)
    });
    if let Some(far) = far {
        assert_eq!(g.can_pick_up(far), Err("You can't reach that."));
    }
    // A bag can't go inside itself. (Too far to put down is a throw: see things_out_of_reach_are_thrown.)
    assert!(g.move_item(pack, Place::Into { container: pack, at: (10, 10) }).is_err());
    // A save keeps it on the floor and out of the backpack.
    let back = Save::parse(&g.snapshot().to_bytes()).unwrap();
    let g2 = Game::new(GameData::load_save(&dir, Some(&back)).unwrap(), None, None).unwrap();
    let pack2 = *g2.world.contents(AVATAR).iter().find(|&&id| g2.world.get(id).unwrap().shape == 529).unwrap();
    assert_eq!(g2.world.contents(pack2).len(), 6);
    let on_floor =
        g2.world.objects().filter_map(|id| g2.world.get(id)).any(|it| it.shape == 418 && (it.x, it.y, it.z) == spot);
    assert!(on_floor, "the dagger is where it was dropped");
    // And back into the backpack.
    let mut g = g2;
    let d = g
        .world
        .objects()
        .find(|&id| g.world.get(id).is_some_and(|it| it.shape == 418 && (it.x, it.y, it.z) == spot))
        .unwrap();
    // The pack's over full (a bag inside, 188 of 175): no room, as DOS refuses it, until the bag
    // comes out.
    let into = Place::Into { container: pack2, at: (30, 20) };
    assert_eq!(g.move_item(d, into), Err("There's no room for that."));
    let bag = *g.world.contents(pack2).iter().find(|&&id| g.world.get(id).unwrap().shape == 529).unwrap();
    assert_eq!(g.move_item(bag, Place::Map((spot.0, spot.1 + 96, spot.2))), Ok(()));
    assert_eq!(g.move_item(d, into), Ok(()));
    let back = Save::parse(&g.snapshot().to_bytes()).unwrap();
    let g3 = Game::new(GameData::load_save(&dir, Some(&back)).unwrap(), None, None).unwrap();
    let pack3 = *g3.world.contents(AVATAR).iter().find(|&&id| g3.world.get(id).unwrap().shape == 529).unwrap();
    let inside: Vec<_> = g3.world.contents(pack3).into_iter().filter_map(|id| g3.world.get(id)).collect();
    assert_eq!(inside.len(), 6);
    assert!(inside.iter().any(|it| it.shape == 418 && (it.x, it.y) == (30, 20)));
}

#[test]
#[ignore]
fn the_avatar_wears_and_wields_things() {
    use u8engine::equip::{worn, Slot};
    let Some(mut g) = game(40) else { return };
    let (carried, most) = g.load();
    println!("carrying {carried} of {most}");
    assert!(carried <= most, "the saved game's load fits ({carried} of {most})");
    let held = worn(&g.data, &g.world, AVATAR, Slot::Weapon).expect("a weapon in hand");
    assert!(worn(&g.data, &g.world, AVATAR, Slot::Head).is_some(), "a helmet");
    let pack = g.backpack().unwrap();
    let spare =
        *g.world.contents(pack).iter().find(|&&id| g.world.get(id).unwrap().shape == 418).expect("a spare dagger");
    assert_eq!(g.equip(spare), Ok(()));
    assert_eq!(worn(&g.data, &g.world, AVATAR, Slot::Weapon), Some(spare));
    assert_eq!(g.world.container(held), Some(pack), "the old one went into the backpack");
    assert_eq!(g.world.get(spare).map(|it| (it.x, it.y)), Some(Slot::Weapon.spot()));
    // Saved, NPCDATA lists the new weapon (by the object number the save gave it).
    let npcdata = |g: &Game| {
        let save = g.snapshot();
        let f = u8data::flex::Flex::parse(save.get("NPCDATA.DAT").unwrap().to_vec()).unwrap();
        u8data::map::npc_equipment(f.get(0).unwrap(), 1).unwrap()
    };
    let before = npcdata(&g);
    assert!(before[5] != 0 && before[2] != 0, "a weapon and a helmet listed: {before:?}");
    assert_eq!(g.world.get(spare).unwrap().objid, before[5]);
    let keys = *g.world.contents(pack).iter().find(|&&id| g.world.get(id).unwrap().shape == 79).unwrap();
    assert_eq!(g.equip(keys), Err("You can't wear that."));
    // The backpack itself: dropped on the paperdoll it stays; on the floor and back, it's worn again.
    assert_eq!(g.stow(pack), Ok(()));
    let a = g.world.get(AVATAR).unwrap();
    assert_eq!(g.move_item(pack, u8engine::Place::Map((a.x + 96, a.y, a.z))), Ok(()));
    assert_eq!(g.backpack(), None);
    assert_eq!(g.stow(pack), Ok(()));
    assert_eq!(g.backpack(), Some(pack));
    assert!(g.world.contents(pack).len() > 1, "with its contents");
}

#[test]
#[ignore]
fn armour_adds_up() {
    use u8engine::equip::armour_value;
    let Some(g) = game(40) else { return };
    // Ryan's castle game: open-faced helm 2 + cuirbolli 2 + plate leggings 3, a dagger 0 (the
    // original shows 7, and 4 with the leggings off).
    assert_eq!(g.armour_class(), 7);
    assert_eq!(g.load(), (35, 60), "the original shows WGHT 35; four times strength (patched)");
    assert_eq!((armour_value(523, 0), armour_value(523, 1), armour_value(539, 2), armour_value(841, 1)), (4, 4, 3, 8));
    let Ok(new) = Game::open(&PathBuf::from(std::env::var("U8_DATA").unwrap()), Some(0), None) else { return };
    assert_eq!(new.armour_class(), 0, "a new game shows ARMR 0");
}

/// A new game's Avatar wears nothing (DOS: ARMR 0, WGHT 1, nothing drawn) and his pack is his
/// backpack: armour found in a chest goes on, with no magic armour turning up in the pack, and
/// the magic armour is armour like any when found.
#[test]
#[ignore]
fn a_new_games_avatar_wears_nothing_but_his_pack() {
    use u8engine::world::item_flag::EQUIPPED;
    let Some(mut g) = saved(0) else { return };
    let mine = |g: &Game| {
        let pack = g.backpack().unwrap();
        let all = [g.world.contents(AVATAR), g.world.contents(pack)].concat();
        all.into_iter()
            .filter_map(|id| g.world.get(id))
            .map(|it| (it.shape, it.flags & EQUIPPED != 0))
            .collect::<Vec<_>>()
    };
    assert_eq!(mine(&g), [(529, true)]);
    assert_eq!((g.armour_class(), g.load().0), (0, 1));
    let a = g.world.get(AVATAR).unwrap();
    let armour = g.world.add(&g.data, Item { shape: 523, x: a.x + 32, y: a.y, z: a.z, ..Default::default() }, true);
    assert_eq!(g.equip(armour), Ok(()));
    assert_eq!(mine(&g), [(529, true), (523, true)]);
    assert_eq!(g.armour_class(), 4);
    assert_eq!(u8engine::equip::armour_value(64, 0), 6, "the magic armour found later");
}

#[test]
#[ignore]
fn the_weapon_table_comes_from_u8_exe() {
    let Some(g) = game(40) else { return };
    if g.data.weapons.list.is_empty() {
        return; // no U8.EXE in this install
    }
    let dagger = u8engine::equip::weapon(&g.data, 418).unwrap();
    assert_eq!((dagger.hold, dagger.base, dagger.modifier), (3, 3, 7));
    assert_eq!(u8engine::equip::weapon_overlay(&g.data, 418), Some((245, 3)));
    let protector = u8engine::equip::weapon(&g.data, 818).unwrap();
    assert_eq!((protector.dex_defend, protector.armour), (4, 5));
}

#[test]
#[ignore]
fn coins_dropped_on_coins_make_one_pile() {
    let Some(mut g) = game(40) else { return };
    let a = g.world.get(AVATAR).unwrap();
    // The 40 obsidian coins in the pouch (DOSBox showed them).
    let mut all = vec![];
    let mut q = vec![AVATAR];
    while let Some(c) = q.pop() {
        let inside = g.world.contents(c);
        q.extend(&inside);
        all.extend(inside);
    }
    let pile =
        all.into_iter().find(|&id| g.world.get(id).is_some_and(|it| it.shape == 143)).expect("the Avatar's coins");
    let shape = g.world.get(pile).unwrap().shape;
    let had = g.world.get(pile).unwrap().quality;
    let coin = |g: &mut Game, q: u16| {
        let it = u8data::map::Item { shape, quality: q, x: a.x + 64, y: a.y, z: a.z, ..Default::default() };
        g.world.add(&g.data, it, true)
    };
    let c = coin(&mut g, 10);
    assert!(g.can_merge(c, pile) && !g.can_merge(c, c));
    g.merge(c, pile).unwrap();
    assert_eq!(g.world.get(pile).unwrap().quality, had + 10);
    assert!(g.world.get(c).is_none(), "one pile now");
    // A pile holds 666 at most; the rest stays where it was.
    let mut p = g.world.get(pile).unwrap();
    p.quality = 660;
    g.world.set(&g.data, pile, p);
    let c = coin(&mut g, 10);
    g.merge(c, pile).unwrap();
    assert_eq!((g.world.get(pile).unwrap().quality, g.world.get(c).unwrap().quality), (666, 4));
}

#[test]
#[ignore]
fn part_of_a_pile_can_be_taken() {
    let Some(mut g) = game(40) else { return };
    let mut all = vec![];
    let mut q = vec![AVATAR];
    while let Some(c) = q.pop() {
        let inside = g.world.contents(c);
        q.extend(&inside);
        all.extend(inside);
    }
    let pouch_coins = all.into_iter().find(|&id| g.world.get(id).is_some_and(|it| it.shape == 143)).unwrap();
    assert!(g.is_pile(pouch_coins));
    let a = g.world.get(AVATAR).unwrap();
    // 15 of the 40 put down beside the Avatar: a new pile there, 25 left in the pouch.
    let spots = [(64, 0), (0, 64), (-64, 0), (0, -64), (96, 96)].map(|(dx, dy)| (a.x + dx, a.y + dy, a.z));
    let spot = spots.into_iter().find(|&at| g.world.fits(&g.data, pouch_coins, at)).expect("room");
    g.split_to(pouch_coins, 15, u8engine::Place::Map(spot)).unwrap();
    assert_eq!(g.world.get(pouch_coins).unwrap().quality, 25);
    let put = g
        .world
        .area(spot.0, spot.1, 8)
        .into_iter()
        .find(|&id| g.world.get(id).is_some_and(|it| it.shape == 143))
        .unwrap();
    assert_eq!(g.world.get(put).unwrap().quality, 15);
    // 5 of those back onto the pouch's.
    g.split_onto(put, 5, pouch_coins).unwrap();
    assert_eq!((g.world.get(pouch_coins).unwrap().quality, g.world.get(put).unwrap().quality), (30, 10));
    // All of it: the pile itself moves (merges).
    g.split_onto(put, 10, pouch_coins).unwrap();
    assert!(g.world.get(put).is_none() && g.world.get(pouch_coins).unwrap().quality == 40);
}

#[test]
#[ignore]
fn things_out_of_reach_are_thrown() {
    let Some(mut g) = game(40) else { return };
    let a = g.world.get(AVATAR).unwrap();
    let coin = u8data::map::Item { shape: 143, quality: 1, x: a.x + 64, y: a.y, z: a.z, ..Default::default() };
    let coin = g.world.add(&g.data, coin, true);
    // A spot far off down the hall, on the floor.
    let far = [(600, 0), (0, 600), (-600, 0), (0, -600)]
        .map(|(dx, dy)| (a.x + dx, a.y + dy, a.z))
        .into_iter()
        .find(|&at| g.world.fits(&g.data, coin, at))
        .expect("somewhere far");
    g.move_item(coin, u8engine::Place::Map(far)).unwrap();
    for _ in 0..150 {
        g.update(1.0 / 30.0, &Input::default());
    }
    let c = g.world.get(coin).unwrap();
    let went = (c.x - a.x).abs().max((c.y - a.y).abs());
    assert!(went > 300, "it flew ({went} units)");
}

#[test]
#[ignore]
fn chest_traps_go_off_and_lock_markers_dont() {
    // A chest's TRAP marker with quality 0 blows up when it's opened (FIRESPEL's blast, and the
    // Guardian's "Ouch!"); one with 0x1nn is the chest's lock (nn = the key), and does nothing on
    // opening. The castle save's chest by the south huts (once locked, key 0x23) has the latter.
    use u8engine::usecode::UiEvent;
    let open = |explosive: bool| {
        let mut g = game(40).unwrap();
        let chest = g
            .world
            .objects()
            .find(|&id| g.world.get(id).is_some_and(|it| (it.x, it.y, it.shape) == (12566, 13538, 78)))
            .unwrap();
        let c = g.world.get(chest).unwrap();
        let mut a = g.world.get(AVATAR).unwrap();
        (a.x, a.y, a.z) = (c.x + 64, c.y, c.z);
        g.world.set(&g.data, AVATAR, a);
        let trap = *g.world.contents(chest).iter().find(|&&t| g.world.get(t).unwrap().shape == 756).unwrap();
        assert_eq!(g.world.get(trap).unwrap().quality, 0x123, "the lock's marker");
        if explosive {
            let mut t = g.world.get(trap).unwrap();
            t.quality = 0;
            g.world.set(&g.data, trap, t);
        }
        let hp = g.stats.get(AVATAR).unwrap().hp;
        assert!(g.use_item(chest));
        let mut said = vec![];
        for _ in 0..90 {
            g.update(1.0 / 30.0, &Input::default());
            for e in g.take_events() {
                if let UiEvent::Bark { pid, text, .. } = e {
                    said.push(text);
                    g.vm().unwrap().finish(pid, 0);
                }
            }
        }
        (hp - g.stats.get(AVATAR).unwrap().hp, said.iter().any(|t| t.contains("Ouch")))
    };
    assert_eq!(open(false), (0, false), "the lock marker does nothing");
    let (hurt, ouch) = open(true);
    assert!(hurt > 0 && ouch, "a real trap blows up");
}

/// What using `id` puts up to read: (kind, gump, text).
fn read_on_use(g: &mut Game, id: u16) -> Option<(ReadKind, u16, String)> {
    g.trigger(id, event::USE);
    for _ in 0..60 {
        g.update(1.0 / 30.0, &Input::default());
        for e in g.take_events() {
            if let UiEvent::Read { kind, gump, text, .. } = e {
                return Some((kind, gump, text));
            }
        }
    }
    None
}

/// The Grimoire's Spell of Resurrection (quality 0x66) reads as itself, not the book its
/// script's first check for 0x66 shows (ScummVM bug #12503); other books are as they were.
#[test]
#[ignore]
fn the_spell_of_resurrection_reads_as_itself() {
    let Some(mut g) = game(40) else { return };
    let me = g.world.get(AVATAR).unwrap();
    let book = |q| u8data::map::Item { shape: 0x120, quality: q, x: me.x + 64, y: me.y, z: me.z, ..Default::default() };
    let id = g.world.add(&g.data, book(0x66), true);
    let (kind, _, text) = read_on_use(&mut g, id).expect("a book to read");
    assert_eq!(kind, ReadKind::Book);
    assert!(text.starts_with("The Spell of Resurrection~~As told to Cyrrus"), "{text:.60}");
    let id = g.world.add(&g.data, book(0x67), true);
    let (_, _, text) = read_on_use(&mut g, id).expect("a book to read");
    assert!(!text.starts_with("The Spell of Resurrection"));
}

/// A gravestone shows on its own gump (the script says which), its lines broken by `*`.
#[test]
#[ignore]
fn graves_say_which_stone() {
    let Some(mut g) = game(40) else { return };
    let me = g.world.get(AVATAR).unwrap();
    let grave = u8data::map::Item { shape: 197, quality: 1, x: me.x + 64, y: me.y, z: me.z, ..Default::default() };
    let id = g.world.add(&g.data, grave, true);
    let (kind, gump, text) = read_on_use(&mut g, id).expect("words on the stone");
    assert_eq!((kind, gump), (ReadKind::Grave, 0x1b));
    assert_eq!(text, "SIVLE*THE MINSTREL*WANDERS NO MORE");
}

/// A pile growing in a bag takes more room: dropping more onto a pile in a full bag is refused
/// as anything else would be, and goes once there's room.
#[test]
#[ignore]
fn piles_in_a_full_bag_dont_grow() {
    let Some(mut g) = game(40) else { return };
    let a = g.world.get(AVATAR).unwrap();
    let pack = *g.world.contents(AVATAR).iter().find(|&&id| g.world.get(id).unwrap().shape == 529).unwrap();
    let coins = |g: &mut Game, q: u16| {
        let it = u8data::map::Item { shape: 143, quality: q, x: a.x + 64, y: a.y, z: a.z, ..Default::default() };
        g.world.add(&g.data, it, true)
    };
    // 99 coins in the (over full) pack, 50 on the floor: one pile of 149 takes more room.
    let pile = coins(&mut g, 99);
    g.world.move_inside(&g.data, pile, pack, (20, 20));
    let more = coins(&mut g, 50);
    assert_eq!(g.merge(more, pile), Err("There's no room for that."), "the pack's over full");
    make_room(&mut g);
    assert_eq!(g.merge(more, pile), Ok(()));
    assert_eq!(g.world.get(pile).unwrap().quality, 149);
}

/// Reach as in DOS: the bedroll taken at an offset of (156, -196) from the Avatar, refused at
/// (196, -236): box to box with nothing in between (ScummVM's canReach), 160 across. Using
/// things checks it too (a bed out of reach shows the red cross), people excepted.
#[test]
#[ignore]
fn the_avatar_reaches_as_far_as_in_dos() {
    let Some(mut g) = game(40) else { return };
    let a = g.world.get(AVATAR).unwrap();
    let put = |g: &mut Game, (dx, dy): (i32, i32)| {
        let it = u8data::map::Item { shape: 579, x: a.x + dx, y: a.y + dy, z: a.z, ..Default::default() };
        g.world.add(&g.data, it, true)
    };
    // A way with nothing in it (the castle has walls about): the same offsets turned about.
    let signs = [(1, -1), (1, 1), (-1, 1), (-1, -1)];
    let (sx, sy) = *signs
        .iter()
        .find(|&&(sx, sy)| {
            let id = put(&mut g, (156 * sx, 196 * sy));
            let ok = g.can_pick_up(id).is_ok();
            g.world.remove(id);
            ok
        })
        .expect("room round the Avatar");
    let near = put(&mut g, (156 * sx, 196 * sy));
    assert_eq!(g.can_pick_up(near), Ok(()));
    assert!(g.can_use(near));
    g.world.remove(near);
    let far = put(&mut g, (196 * sx, 236 * sy));
    assert_eq!(g.can_pick_up(far), Err("You can't reach that."));
    assert!(!g.can_use(far), "out of reach: the red cross");
    // People can be talked to from further.
    let guard = g.world.objects().find(|&id| id != AVATAR && (2..256).contains(&id)).unwrap();
    assert!(g.can_use(guard));
}

#[test]
#[ignore]
fn the_avatar_turns_to_what_it_puts_down() {
    // DOS: the plate dragged from the table to the floor, the Avatar has turned to face it.
    use u8engine::{avatar::facing, Place};
    let Some(mut g) = game(40) else { return };
    let a = g.world.get(AVATAR).unwrap();
    let coin = u8data::map::Item { shape: 143, quality: 1, x: a.x + 64, y: a.y, z: a.z, ..Default::default() };
    let coin = g.world.add(&g.data, coin, true);
    let face = |g: &Game| facing(&g.data, &g.world.get(AVATAR).unwrap());
    // (A step first: done walking, it turns as well.)
    let step = Input { dir: face(&g), ..Input::default() };
    g.update(1.0 / 30.0, &step);
    for _ in 0..10 {
        g.update(1.0 / 30.0, &Input::default());
    }
    let a = g.world.get(AVATAR).unwrap();
    let mut turned = 0;
    for (d, (dx, dy)) in [(2, (128, 0)), (4, (0, 128)), (6, (-96, 0)), (0, (0, -96)), (3, (128, 128))] {
        let at = (a.x + dx, a.y + dy, a.z);
        if !g.world.fits(&g.data, coin, at) {
            continue;
        }
        g.move_item(coin, Place::Map(at)).unwrap();
        g.update(1.0 / 30.0, &Input::default());
        assert_eq!(face(&g), Some(d), "dropped at ({dx}, {dy})");
        turned += 1;
    }
    assert!(turned >= 3, "only {turned} spots free");
    // Dropped on a pile on the floor (one pile then), it turns to the pile.
    let (d, at) = [(2, (128, 0)), (4, (0, 128)), (6, (-96, 0)), (0, (0, -96))]
        .map(|(d, (dx, dy))| (d, (a.x + dx, a.y + dy, a.z)))
        .into_iter()
        .find(|&(d, at)| Some(d) != face(&g) && g.world.fits(&g.data, coin, at))
        .expect("a spot for a pile");
    let pile = u8data::map::Item { shape: 143, quality: 5, x: at.0, y: at.1, z: at.2, ..Default::default() };
    let pile = g.world.add(&g.data, pile, true);
    g.merge(coin, pile).unwrap();
    g.update(1.0 / 30.0, &Input::default());
    assert_eq!((face(&g), g.world.get(pile).unwrap().quality), (Some(d), 6), "merged");
    let coin = pile;
    // Thrown, it turns the way it threw.
    let far = [(2, (600, 0)), (4, (0, 600)), (6, (-600, 0)), (0, (0, -600))]
        .into_iter()
        .find(|&(d, (dx, dy))| Some(d) != face(&g) && g.world.fits(&g.data, coin, (a.x + dx, a.y + dy, a.z)))
        .expect("somewhere far");
    g.move_item(coin, Place::Map((a.x + far.1 .0, a.y + far.1 .1, a.z))).unwrap();
    g.update(1.0 / 30.0, &Input::default());
    assert_eq!(face(&g), Some(far.0), "thrown");
}

/// Worn things keep their slot in z, as the DOS saves have it (shield 0, arms 1, head 2, body 3,
/// legs 4, weapon 5, pack 6): take everything off, put it on again, save, and each is back at
/// the z the DOS save gave it.
#[test]
#[ignore]
fn worn_things_keep_their_slot_number() {
    use u8engine::{world::item_flag::EQUIPPED, Place};
    let Some(mut g) = saved(2) else { return };
    let worn = |g: &Game| {
        let mut v: Vec<_> = g
            .world
            .contents(AVATAR)
            .into_iter()
            .filter_map(|id| g.world.get(id).filter(|it| it.flags & EQUIPPED != 0).map(|it| (id, it.shape, it.z)))
            .collect();
        v.sort();
        v
    };
    let dos = worn(&g);
    assert!(dos.len() >= 4 && dos.iter().all(|&(_, s, z)| z == u8engine::equip::worn_z(&g.data, s)), "{dos:?}");
    let pack = g.backpack();
    let a = g.world.get(AVATAR).unwrap();
    for &(id, _, _) in dos.iter().filter(|(id, _, _)| Some(*id) != pack) {
        let spot = [(96, 0), (0, 96), (-96, 0), (0, -96), (96, 96), (-96, 96)]
            .map(|(dx, dy)| (a.x + dx, a.y + dy, a.z))
            .into_iter()
            .find(|&at| g.world.fits(&g.data, id, at))
            .expect("a spot");
        g.move_item(id, Place::Map(spot)).unwrap();
    }
    for &(id, _, _) in dos.iter().filter(|(id, _, _)| Some(*id) != pack) {
        g.equip(id).unwrap();
    }
    let dir = std::env::var_os("U8_DATA").unwrap();
    let s = g.snapshot();
    let back =
        Game::new(u8data::GameData::load_save(std::path::Path::new(&dir), Some(&s)).unwrap(), None, None).unwrap();
    assert_eq!(worn(&back), dos);
}

/// Wielding another weapon puts the one in hand into the pack, but only if there's room: the
/// castle save's pack is over full, so with the new one off the floor the swap is refused and
/// nothing moves; out of the pack itself (it leaves room) or with the bag taken out, it goes
/// through.
#[test]
#[ignore]
fn a_weapon_swap_needs_room_in_the_pack() {
    use u8engine::equip::{worn, Slot};
    let Some(mut g) = saved(2) else { return };
    let (Some(pack), Some(dagger)) = (g.backpack(), worn(&g.data, &g.world, AVATAR, Slot::Weapon)) else { return };
    let other = 19532; // (a weapon carried in the pack)
    assert_eq!(Slot::of(&g.data, g.world.get(other).unwrap().shape), Some(Slot::Weapon));
    // Swapped for one out of the pack itself: the pack's no fuller after.
    {
        let Some(mut h) = saved(2) else { return };
        assert_eq!(h.world.container(other), Some(pack));
        assert_eq!(h.equip(other), Ok(()));
        assert_eq!(h.world.container(dagger), Some(pack));
    }
    let a = g.world.get(AVATAR).unwrap();
    g.world.move_to_map(&g.data, other, (a.x + 96, a.y, a.z));
    let before = g.world.contents(pack);
    assert_eq!(g.equip(other), Err("There's no room for that."));
    assert_eq!(worn(&g.data, &g.world, AVATAR, Slot::Weapon), Some(dagger));
    assert_eq!(g.world.contents(pack), before, "nothing went in");
    assert!(g.world.container(other).is_none(), "the new one still on the floor");
    make_room(&mut g);
    assert_eq!(g.equip(other), Ok(()));
    assert_eq!(worn(&g.data, &g.world, AVATAR, Slot::Weapon), Some(other));
    assert_eq!(g.world.container(dagger), Some(pack));
}

/// What a drop would do, shown while dragging (WORLD.C dragItem): put down near the Avatar, thrown
/// out of reach, refused where it won't go (a bag into itself, farther than it can be thrown).
#[test]
#[ignore]
fn a_drop_says_whether_it_puts_throws_or_wont() {
    use u8engine::{DropKind, Place};
    let Some(g) = game(40) else { return };
    let pack = *g.world.contents(AVATAR).iter().find(|&&id| g.world.get(id).unwrap().shape == 529).unwrap();
    let dagger = *g.world.contents(pack).iter().find(|&&id| g.world.get(id).unwrap().shape == 418).unwrap();
    let av = g.world.get(AVATAR).unwrap();
    assert_eq!(g.drop_kind(dagger, Place::Map((av.x + 96, av.y, av.z))), DropKind::Put);
    assert_eq!(g.drop_kind(dagger, Place::Map((av.x + 600, av.y, av.z))), DropKind::Throw);
    // (Too far to throw: speed × 2 × speed / gravity, speed 64 − weight + STR; MISSILE.C.)
    assert_eq!(g.drop_kind(dagger, Place::Map((av.x + 5000, av.y, av.z))), DropKind::Refused);
    assert_eq!(g.drop_kind(pack, Place::Into { container: pack, at: (10, 10) }), DropKind::Refused);
}

#[test]
#[ignore]
fn closing_a_bag_closes_the_bags_inside() {
    use u8data::typeflag::family;
    let Some(mut g) = game(40) else { return };
    let pack = *g.world.contents(AVATAR).iter().find(|&&id| g.world.get(id).unwrap().shape == 529).unwrap();
    let bag = g
        .world
        .contents(pack)
        .into_iter()
        .find(|&id| g.data.typeflags.get(g.world.get(id).unwrap().shape).family == family::CONTAINER);
    let Some(bag) = bag else {
        panic!(
            "no bag in the backpack: {:?}",
            g.world.contents(pack).iter().map(|&i| g.world.get(i).unwrap().shape).collect::<Vec<_>>()
        )
    };
    let open = |g: &Game, id| g.world.get(id).unwrap().flags & u8engine::world::item_flag::GUMP_OPEN != 0;
    for id in [pack, bag] {
        assert!(g.trigger(id, event::USE));
        g.update(1.0 / 30.0, &Input::default());
    }
    assert!(open(&g, pack) && open(&g, bag));
    g.take_events();
    // Shutting the backpack shuts the bag in it too (ITEM.C Item::closeGump).
    assert!(g.use_item(pack));
    g.update(1.0 / 30.0, &Input::default());
    let shut: Vec<_> = g
        .take_events()
        .into_iter()
        .filter_map(|e| if let UiEvent::CloseGump { item } = e { Some(item) } else { None })
        .collect();
    assert!(shut.contains(&pack) && shut.contains(&bag), "{shut:?}");
    assert!(!open(&g, pack) && !open(&g, bag));
}
