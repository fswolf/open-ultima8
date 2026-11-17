//! Intrinsics against ScummVM's behaviour, called directly on a real world.
//! Run against a real install: U8_DATA=/path/to/U8 cargo test -- --ignored
mod common;
use common::*;
use u8engine::usecode::{intrinsics, UiEvent};
use u8engine::world::item_flag::CONTAINED;
use u8engine::ObjId;

/// A script's pointer to an object (what member intrinsics get as their first argument).
fn this(id: ObjId) -> Vec<u8> {
    (0x8000_0000u32 | id as u32).to_le_bytes().to_vec()
}
fn args(id: ObjId, rest: &[u16]) -> Vec<u8> {
    let mut a = this(id);
    rest.iter().for_each(|v| a.extend(v.to_le_bytes()));
    a
}
fn call(g: &mut Game, num: u16, name: &str, a: &[u8]) -> u32 {
    assert_eq!(intrinsics::name(num), Some(name), "intrinsic {num:#x}");
    g.call_intrinsic(num, a)
}
fn backpack(g: &Game) -> ObjId {
    *g.world.contents(AVATAR).iter().find(|&&id| g.world.get(id).unwrap().shape == 529).expect("a backpack")
}

#[test]
#[ignore]
fn destroy_spills_what_was_inside_and_spares_the_avatar() {
    let Some(mut g) = game(40) else { return };
    let pack = backpack(&g);
    let inside = g.world.contents(pack);
    assert!(!inside.is_empty());
    // A carried bag: its things go to whoever carried it.
    call(&mut g, 0x3A, "Item::destroy", &this(pack));
    assert!(g.world.get(pack).is_none());
    assert!(inside.iter().all(|&i| g.world.container(i) == Some(AVATAR)), "spilled into the Avatar");
    // A bag on the floor: its things land where it stood.
    let av = g.world.get(AVATAR).unwrap();
    let bag = g.world.add(
        &g.data,
        u8data::map::Item { shape: 529, x: av.x + 64, y: av.y, z: av.z, ..Default::default() },
        true,
    );
    let coin = g.world.add_inside(&g.data, u8data::map::Item { shape: 143, quality: 5, ..Default::default() }, bag);
    call(&mut g, 0x3A, "Item::destroy", &this(bag));
    let c = g.world.get(coin).expect("the coins are still about");
    assert_eq!((g.world.container(coin), c.x, c.y, c.z), (None, av.x + 64, av.y, av.z));
    assert_eq!(c.flags & CONTAINED, 0);
    // Never the Avatar.
    call(&mut g, 0x3A, "Item::destroy", &this(AVATAR));
    assert!(g.world.get(AVATAR).is_some());
}

#[test]
#[ignore]
fn destroy_contents_empties_all_the_way_down() {
    let Some(mut g) = game(40) else { return };
    let pack = backpack(&g);
    let inner = g.world.add_inside(&g.data, u8data::map::Item { shape: 529, ..Default::default() }, pack);
    let deep = g.world.add_inside(&g.data, u8data::map::Item { shape: 143, quality: 1, ..Default::default() }, inner);
    let all = g.world.contents(pack);
    call(&mut g, 0x3C, "Container::destroyContents", &this(pack));
    assert!(g.world.get(pack).is_some(), "the bag itself stays");
    assert!(g.world.contents(pack).is_empty());
    assert!(all.iter().chain([&deep]).all(|&i| g.world.get(i).is_none()), "nothing spills anywhere");
}

#[test]
#[ignore]
fn move_takes_things_out_of_bags_and_positions_ask_the_outermost() {
    let Some(mut g) = game(40) else { return };
    let pack = backpack(&g);
    let thing = g.world.contents(pack)[0];
    let av = g.world.get(AVATAR).unwrap();
    // Where is a thing in a bag? Where the one carrying it is (getLocationAbsolute).
    assert_eq!(call(&mut g, 0x03, "Item::getX", &this(thing)), av.x as u32);
    // move(x, y, z): onto the map, out of the bag.
    call(&mut g, 0x3E, "Item::move", &args(thing, &[av.x as u16 + 96, av.y as u16, av.z as u16]));
    let it = g.world.get(thing).unwrap();
    assert_eq!((g.world.container(thing), it.x, it.y, it.z), (None, av.x + 96, av.y, av.z));
    assert_eq!(it.flags & CONTAINED, 0);
    assert!(g.world.entry(g.world.index(thing).unwrap()).present(), "on the map");
}

#[test]
#[ignore]
fn quality_answers_by_family() {
    let Some(mut g) = game(40) else { return };
    let gold = g.world.add(&g.data, u8data::map::Item { shape: 143, quality: 37, ..Default::default() }, true);
    assert_eq!(call(&mut g, 0x13, "Item::getQuantity", &this(gold)), 37);
    assert_eq!(call(&mut g, 0x11, "Item::getQuality", &this(gold)), 0, "coins have a quantity, not a quality");
    assert_eq!(call(&mut g, 0x19, "Item::getQ", &this(gold)), 37, "getQ is raw");
    // Gold weighs a tenth each, rounded up (getWeight answers in tenths).
    let w = g.data.typeflags.get(143).weight as u32;
    assert_eq!(call(&mut g, 0x2C, "Item::getWeight", &this(gold)), (w * 37).div_ceil(10));
}

#[test]
#[ignore]
fn is_dead_counts_the_missing_as_dead() {
    let Some(mut g) = game(40) else { return };
    assert_eq!(call(&mut g, 0x8C, "Actor::isDead", &this(AVATAR)), 0);
    let rock = g.world.add(&g.data, u8data::map::Item { shape: 143, ..Default::default() }, true);
    assert_eq!(call(&mut g, 0x8C, "Actor::isDead", &this(rock)), 1, "not a person at all");
}

#[test]
#[ignore]
fn create_answers_one() {
    let Some(mut g) = game(40) else { return };
    let mut a = 0u32.to_le_bytes().to_vec(); // (out-pointer: nowhere)
    a.extend([143u16, 0].iter().flat_map(|v| v.to_le_bytes()));
    assert_eq!(call(&mut g, 0x32, "Item::create", &a), 1);
}

#[test]
#[ignore]
fn sound_and_music_calls() {
    let Some(mut g) = game(40) else { return };
    g.take_events();
    call(&mut g, 0xEC, "AudioProcess::playSFX", &5u16.to_le_bytes());
    call(&mut g, 0xF4, "AudioProcess::stopSFX", &5u16.to_le_bytes());
    call(&mut g, 0xBB, "MusicProcess::playMusic", &[0x85, 0]);
    call(&mut g, 0xF8, "MusicProcess::stopMusic", &[]);
    let ev: Vec<_> = g.take_events();
    assert!(matches!(ev[0], UiEvent::Sfx { sfx: 5, item: 0, looping: false }));
    assert!(matches!(ev[1], UiEvent::StopSfx { sfx: 5, item: Some(0) }), "only the one played without an item");
    assert!(matches!(ev[2], UiEvent::Music(5)), "the top bit isn't part of the track");
    assert!(matches!(ev[3], UiEvent::Music(0)));
}

#[test]
#[ignore]
fn do_anim_direction_8_keeps_the_facing() {
    use u8data::anim::action;
    let Some(mut g) = game(40) else { return };
    let face = |g: &Game| u8engine::avatar::facing(&g.data, &g.world.get(AVATAR).unwrap());
    let before = face(&g);
    let pid = call(&mut g, 0x9F, "Actor::doAnim", &args(AVATAR, &[action::STAND as u16, 8, 0, 0]));
    assert_ne!(pid, 0);
    assert_eq!(face(&g), before);
}

#[test]
#[ignore]
fn pathfind_to_point_walks_there() {
    let Some(mut g) = game(40) else { return };
    let av = g.world.get(AVATAR).unwrap();
    let to = (av.x + 128, av.y, av.z);
    let pid = call(&mut g, 0xA1, "Actor::pathfindToPoint", &args(AVATAR, &[to.0 as u16, to.1 as u16, to.2 as u16, 0]));
    assert_ne!(pid, 0);
    for _ in 0..30 * 10 {
        g.update(1.0 / 30.0, &Input::default());
        if !g.vm().unwrap().alive(pid as u16) {
            break;
        }
    }
    let now = g.world.get(AVATAR).unwrap();
    assert!(!g.vm().unwrap().alive(pid as u16), "the walk ends");
    assert!((now.x - to.0).abs() <= 64 && (now.y - to.1).abs() <= 64, "near {to:?}: at {:?}", (now.x, now.y));
}

/// A script's pointer to a WorldPoint (x, y, z byte) — on the stack of nobody, so a global's.
fn point_arg(g: &mut Game, (x, y, z): (i32, i32, i32)) -> Vec<u8> {
    let vm = g.vm().unwrap();
    let at = 0x100; // (spare global bytes)
    vm.globals[at..at + 5]
        .copy_from_slice(&[(x as u16).to_le_bytes(), (y as u16).to_le_bytes(), [z as u8, 0]].concat()[..5]);
    (0x8001_0000u32 | at as u32).to_le_bytes().to_vec()
}

#[test]
#[ignore]
fn legal_move_to_point_stops_at_walls_and_strikes_what_it_lands_in() {
    use u8data::map::Item;
    let Some(mut g) = game(40) else { return };
    let av = g.world.get(AVATAR).unwrap();
    let up = av.z + 120; // (clear air, nothing about but what we put there)
    assert!(g.data.typeflags.get(115).has(u8data::typeflag::flag::SOLID));
    g.world.add(&g.data, Item { shape: 115, x: av.x + 320, y: av.y, z: up, ..Default::default() }, true);
    let gold = g.world.add(&g.data, Item { shape: 143, x: av.x + 64, y: av.y, z: up, ..Default::default() }, true);
    let mv = |g: &mut Game, to: (i32, i32, i32), abort: u16| {
        let mut a = this(gold);
        a.extend(point_arg(g, to));
        a.extend([abort.to_le_bytes(), [0, 0]].concat());
        call(g, 0x40, "Item::legalMoveToPoint", &a)
    };
    let at = |g: &Game| g.world.get(gold).map(|i| (i.x, i.y, i.z)).unwrap();
    assert_eq!(mv(&mut g, (av.x + 128, av.y, up), 0), 1, "clear");
    assert_eq!(at(&g), (av.x + 128, av.y, up));
    assert_eq!(mv(&mut g, (av.x + 512, av.y, up), 1), 0, "blocked, and told to give up");
    assert_eq!(at(&g), (av.x + 128, av.y, up), "stays put");
    assert_eq!(mv(&mut g, (av.x + 512, av.y, up), 0), 0, "blocked");
    let x = at(&g).0;
    assert!(x > av.x + 128 && x <= av.x + 320 - 64, "stops short of the barrel: {}", x - av.x);
    // A fire missile popped onto the Avatar (MISSTRAP's way) strikes it where it lies.
    let hp = g.stats.get(AVATAR).unwrap().hp;
    let fire = g.world.add(&g.data, Item { shape: 697, x: av.x, y: av.y, z: av.z + 8, ..Default::default() }, true);
    let mut a = this(fire);
    a.extend(point_arg(&mut g, (av.x, av.y, av.z + 8)));
    a.extend([0u8; 4]);
    call(&mut g, 0x40, "Item::legalMoveToPoint", &a);
    for _ in 0..10 {
        g.update(1.0 / 30.0, &Input::default());
    }
    assert!(g.stats.get(AVATAR).unwrap().hp < hp, "burnt");
}

#[test]
#[ignore]
fn things_go_into_containers_by_room_and_weight() {
    use u8data::map::Item;
    use u8engine::world::item_flag::{EQUIPPED, OWNED};
    let Some(mut g) = game(40) else { return };
    let pack = backpack(&g);
    let av = g.world.get(AVATAR).unwrap();
    let new_in = |g: &mut Game, shape: u16, into: ObjId| {
        let mut a = 0u32.to_le_bytes().to_vec(); // (no out-pointer)
        a.extend([shape, 0, into, 0].iter().flat_map(|v| v.to_le_bytes()));
        call(g, 0x34, "Item::legalCreateInCont", &a)
    };
    // legalCreateInCont: straight in (the Avatar's now), or not at all.
    let before = g.world.contents(pack).len();
    assert_eq!(new_in(&mut g, 143, pack), 1);
    let coin = *g.world.contents(pack).iter().find(|&&i| g.world.get(i).unwrap().shape == 143).unwrap();
    assert_eq!(g.world.contents(pack).len(), before + 1);
    assert_ne!(g.world.get(coin).unwrap().flags & OWNED, 0, "the Avatar's own");
    assert_eq!(new_in(&mut g, 143, coin), 0, "coins hold nothing");
    // legalMoveToContainer: a barrel (volume 201) won't go in a chest (200); a coin will.
    let on_map = |g: &mut Game, shape: u32, dx: i32| {
        g.world.add(&g.data, Item { shape, x: av.x + dx, y: av.y, z: av.z, ..Default::default() }, true)
    };
    let (chest, barrel) = (on_map(&mut g, 78, 256), on_map(&mut g, 115, 512));
    let into = |g: &mut Game, id: ObjId, c: ObjId| {
        let mut a = this(id);
        a.extend(this(c));
        a.extend([0, 0]);
        call(g, 0x41, "Item::legalMoveToContainer", &a)
    };
    assert_eq!(into(&mut g, barrel, chest), 0, "no room");
    assert_eq!(into(&mut g, coin, chest), 1);
    assert_eq!(g.world.container(coin), Some(chest));
    // …and the Avatar can only carry so much: a barrel fits a backpack (ScummVM's allowance)
    // but not on a weakling's back.
    g.stats.get_mut(AVATAR).unwrap().str = 1;
    assert_eq!(into(&mut g, barrel, pack), 0, "too heavy");
    g.stats.get_mut(AVATAR).unwrap().str = 25;
    assert_eq!(into(&mut g, barrel, pack), 1);
    // setEquip: only into a free slot.
    let weapon = (1..1024).find(|&s| g.data.typeflags.get(s).equip == 6).unwrap();
    let held = u8engine::equip::worn(&g.data, &g.world, AVATAR, u8engine::equip::Slot::Weapon);
    let sword = on_map(&mut g, weapon, 768);
    let equip = |g: &mut Game, id: ObjId| call(g, 0xB3, "Actor::setEquip", &args(AVATAR, &[5, id]));
    if let Some(h) = held {
        assert_eq!(equip(&mut g, sword), 0, "already holding one");
        call(&mut g, 0x3A, "Item::destroy", &this(h));
    }
    assert_eq!(equip(&mut g, sword), 1);
    let s = g.world.get(sword).unwrap();
    assert_eq!((g.world.container(sword), s.flags & EQUIPPED != 0), (Some(AVATAR), true));
    assert_eq!(call(&mut g, 0xB2, "Actor::getEquip", &args(AVATAR, &[5])), sword as u32);
    // Loose things don't go on the Avatar itself (one of each worn thing and a pack): popped
    // there, a coin stays in the void.
    let loose = g.world.add(&g.data, Item { shape: 143, quality: 1, ..Default::default() }, false);
    g.world.push_ethereal(loose);
    call(&mut g, 0x37, "Item::popToContainer", &args(0, &[AVATAR]));
    assert_eq!(g.world.container(loose), None);
    assert!(g.world.is_void(loose));
    // closeItemGumps
    g.take_events();
    call(&mut g, 0xB4, "Ultima8Engine::closeItemGumps", &[]);
    assert!(matches!(g.take_events()[..], [UiEvent::CloseItemGumps]));
}

/// `loop` searches as ScummVM's CurrentMap: anchors in range, chunk by chunk, newly placed things
/// first in their chunk; surfaces by what sits on what.
#[test]
#[ignore]
fn searches_find_what_scummvm_finds_in_its_order() {
    use u8data::map::Item;
    let Some(mut g) = game(40) else { return };
    let av = g.world.get(AVATAR).unwrap();
    let (x, y) = (av.x - av.x.rem_euclid(512) + 256, av.y - av.y.rem_euclid(512) + 256); // a chunk's middle
    let up = av.z + 160;
    let put = |g: &mut Game, shape: u32, x: i32, y: i32, z: i32| {
        g.world.add(&g.data, Item { shape, x, y, z, quality: 1, ..Default::default() }, true)
    };
    // In range by anchor only: a chest (footprint reaching back 64+) anchored just past the
    // range isn't found though its back edge is within it.
    let chest = put(&mut g, 78, x + 300 + 1, y, up);
    let near = put(&mut g, 143, x + 300, y, up);
    let found = g.world.search_area((x, y), 300, (0, 0), false);
    assert!(found.contains(&near) && !found.contains(&chest));
    // Order: by chunk (rows, then columns); in a chunk, what was put there last comes first.
    let a = put(&mut g, 143, x + 10, y + 512, up); // the chunk below
    let b = put(&mut g, 143, x + 512, y + 10, up); // the chunk to the right
    let c = put(&mut g, 143, x + 20, y + 20, up);
    let d = put(&mut g, 143, x + 30, y + 30, up);
    let order: Vec<ObjId> =
        g.world.search_area((x, y), 600, (0, 0), false).into_iter().filter(|o| [a, b, c, d].contains(o)).collect();
    assert_eq!(order, [d, c, b, a]);
    // Recursive: what's inside straight after its container.
    let bag = put(&mut g, 529, x - 40, y - 40, up);
    let inside = g.world.add_inside(&g.data, Item { shape: 143, quality: 1, ..Default::default() }, bag);
    let all = g.world.search_area((x, y), 100, (0, 0), true);
    let at = all.iter().position(|&o| o == bag).unwrap();
    assert_eq!(all.get(at + 1), Some(&inside));
    assert!(!g.world.search_area((x, y), 100, (0, 0), false).contains(&inside));
    // Surfaces: a coin on a chest is above it; the chest is below the coin; a coin beside isn't.
    let top = g.world.index(chest).map(|i| g.world.entry(i).z1).unwrap();
    let on = put(&mut g, 143, x + 300, y, top);
    let beside = put(&mut g, 143, x + 300 + 200, y, top);
    assert_eq!(g.world.search_surface(chest, true, false, false), [on]);
    assert!(g.world.search_surface(on, false, true, false).contains(&chest));
    assert!(!g.world.search_surface(chest, true, false, false).contains(&beside));
    assert_eq!(
        g.world.search_surface(chest, false, true, false),
        [near],
        "only the flat coin it overlaps (top = its bottom)"
    );
}

/// The Avatar's justMoved runs on every move (ScummVM's Item::move): with magic armour on
/// (global avatarArmor) its glow (shape 400) keeps up with it.
#[test]
#[ignore]
fn the_armour_glow_follows_the_avatar() {
    use u8data::map::Item;
    let Some(mut g) = game(40) else { return };
    g.update(1.0 / 30.0, &Input::default());
    let av = g.world.get(AVATAR).unwrap();
    let glow =
        g.world.add(&g.data, Item { shape: 400, quality: 1, x: av.x, y: av.y, z: av.z, ..Default::default() }, true);
    g.vm().unwrap().set_global_bits(0x29, 1, 1); // avatarArmor
    let mut a = av;
    a.x += 32;
    g.world.set(&g.data, AVATAR, a);
    for _ in 0..3 {
        g.update(1.0 / 30.0, &Input::default());
    }
    let it = g.world.get(glow).unwrap();
    assert_eq!((it.x, it.y), (a.x, a.y), "moved along with the Avatar");
}

#[test]
#[ignore]
fn reach_range_surfaces_and_the_fast_area() {
    use u8data::map::Item;
    let Some(mut g) = game(40) else { return };
    let av = g.world.get(AVATAR).unwrap();
    let up = av.z + 160; // (clear air)
    let put = |g: &mut Game, shape: u32, x: i32, y: i32, z: i32, q: u16| {
        g.world.add(&g.data, Item { shape, x, y, z, quality: q, ..Default::default() }, true)
    };
    let a = put(&mut g, 78, av.x, av.y, up, 0);
    let b = put(&mut g, 143, av.x + 300, av.y, up, 1);
    let bdx = g.data.typeflags.get(143).dims().0;
    // getRange: the gap between the boxes.
    assert_eq!(call(&mut g, 0x56, "Item::getRange", &args(a, &[b])), (300 - bdx) as u32);
    // canReach: near enough and in sight; not past a barrel, nor out of range.
    let reach = |g: &mut Game, r: u16| call(g, 0x55, "Item::canReach", &args(a, &[b, r]));
    assert_eq!(reach(&mut g, 400), 1);
    assert_eq!(reach(&mut g, 100), 0, "too far");
    put(&mut g, 115, av.x + 150, av.y + 32, up, 0);
    assert_eq!(reach(&mut g, 400), 0, "a barrel in the way");
    // getSurfaceWeight: what rests on it, piles on piles.
    let top = g.world.index(a).map(|i| g.world.entry(i).z1).unwrap();
    let c2 = put(&mut g, 78, av.x - 8, av.y - 8, top, 0);
    let top2 = g.world.index(c2).map(|i| g.world.entry(i).z1).unwrap();
    put(&mut g, 143, av.x - 16, av.y - 16, top2, 100);
    let w = |g: &Game, id| u8engine::equip::weight(&g.data, &g.world, id) as u32;
    let expect = w(&g, c2) + 10;
    assert_eq!(call(&mut g, 0x2E, "Item::getSurfaceWeight", &this(a)), expect);
    // grab: lift the lower chest away and what was on it falls.
    call(&mut g, 0x47, "Item::grab", &this(a));
    let mut it = g.world.get(a).unwrap();
    it.x += 1000;
    g.world.set(&g.data, a, it);
    for _ in 0..30 {
        g.update(1.0 / 30.0, &Input::default());
    }
    assert!(g.world.get(c2).unwrap().z < top, "the chest on it fell");
    // enterFastArea: once, and then it's in.
    let shape = (1..1024).find(|&s| g.scripts.as_ref().unwrap().usecode.event(s, 15).is_some()).unwrap();
    let far = put(&mut g, shape, av.x + 4000, av.y + 4000, av.z, 0);
    assert!(!g.world.is_fast(far));
    assert_ne!(call(&mut g, 0x67, "Item::enterFastArea", &this(far)), 0, "its event runs");
    assert!(g.world.is_fast(far));
    assert_eq!(call(&mut g, 0x67, "Item::enterFastArea", &this(far)), 0, "only once");
}

/// ascend (ScummVM's): a lift (PLATFORM, map 54) rises with whoever stands on it, as far as
/// it can.
#[test]
#[ignore]
fn ascend_carries_what_rests_on_it() {
    use u8data::map::Item;
    let Some(mut g) = game(40) else { return };
    assert!(g.put_avatar(54, (12799 - 64, 7423 - 64, 16)), "to map 54");
    let p = g.world.objects().find(|&id| g.world.get(id).is_some_and(|it| it.shape == 549 && it.z == 16)).unwrap();
    let top = |g: &Game, id| g.world.index(id).map(|i| g.world.entry(i).z1).unwrap();
    let mut a = g.world.get(AVATAR).unwrap();
    (a.x, a.y, a.z) = (12799 - 32, 7423 - 32, top(&g, p));
    g.world.set(&g.data, AVATAR, a);
    let up = |g: &mut Game, dz: i16| call(g, 0x2B, "Item::ascend", &args(p, &[dz as u16]));
    assert_eq!(up(&mut g, 8), 1);
    assert_eq!((g.world.get(p).unwrap().z, g.world.get(AVATAR).unwrap().z), (24, top(&g, p)), "both up 8");
    // Something solid over the lift stops it short (but not what rides on it).
    let (ax, ay) = (12799 - 160, 7423 - 160);
    g.world.add(&g.data, Item { shape: 115, x: ax, y: ay, z: top(&g, p) + 16, ..Default::default() }, true);
    assert_eq!(up(&mut g, 40), 0, "blocked");
    assert_eq!(g.world.get(p).unwrap().z, 24 + 16, "as far as it went");
    assert_eq!(g.world.get(AVATAR).unwrap().z, top(&g, p), "still riding it");
    assert_eq!(up(&mut g, -16), 1);
    assert_eq!(g.world.get(AVATAR).unwrap().z, top(&g, p), "and down again");
}

/// invertScreen (ScummVM's InverterProcess): over in about two seconds, then back the same way
/// round; its process ends when it gets there.
#[test]
#[ignore]
fn the_screen_turns_upside_down_and_back() {
    use u8engine::fade::Inverter;
    let Some(mut g) = game(40) else { return };
    let pid = call(&mut g, 0xC8, "InverterProcess::invertScreen", &[]) as u16;
    let mut seen = vec![];
    for _ in 0..90 {
        g.update(1.0 / 30.0, &Input::default());
        seen.push(g.vm().unwrap().inverter.state);
    }
    assert_eq!(g.vm().unwrap().inverter.state, 0x8000, "upside down");
    assert!(!g.vm().unwrap().alive(pid));
    assert!(seen.windows(2).all(|w| w[0] <= w[1]), "one way round");
    call(&mut g, 0xC8, "InverterProcess::invertScreen", &[]);
    (0..90).for_each(|_| _ = g.update(1.0 / 30.0, &Input::default()));
    assert_eq!(g.vm().unwrap().inverter.state, 0, "upright again (through 0x10000)");
    assert_eq!((Inverter::row(0, 5, 100), Inverter::row(0x8000, 5, 100)), (5, 94));
    let mid: std::collections::HashSet<i32> = (0..100).map(|i| Inverter::row(0x4000, i, 100)).collect();
    assert_eq!(mid.len(), 100, "every row shown once while turning");
}

#[test]
#[ignore]
fn shot_things_land_where_aimed_and_fireballs_find_the_avatar() {
    use u8data::map::Item;
    let Some(mut g) = game(40) else { return };
    g.update(1.0 / 30.0, &Input::default());
    let av = g.world.get(AVATAR).unwrap();
    // shoot (a dart's way: speed 0x40, gravity 1), in clear air: it passes through the point.
    let up = av.z + 200;
    let dart =
        g.world.add(&g.data, Item { shape: 143, quality: 1, x: av.x, y: av.y, z: up, ..Default::default() }, true);
    let to = (av.x + 256, av.y, up - 16);
    let mut a = this(dart);
    a.extend(point_arg(&mut g, to));
    a.extend([0x40u16.to_le_bytes(), 1u16.to_le_bytes()].concat());
    call(&mut g, 0x45, "Item::shoot", &a);
    let mut path = vec![];
    for _ in 0..8 {
        g.update(1.0 / 30.0, &Input::default());
        let d = g.world.get(dart).unwrap();
        path.push((d.x, d.y, d.z));
    }
    assert!(path.iter().any(|&(x, y, z)| (x, y) == (to.0, to.1) && (z - to.2).abs() <= 2), "through {to:?}: {path:?}");
    // TonysBalls: a fireball a few steps off comes for the Avatar and burns it (from whichever
    // side has a clear way: walls turn it back).
    let hp = g.stats.get(AVATAR).unwrap().hp;
    let burnt = u8engine::geom::DIRS.iter().any(|&(dx, dy)| {
        let at = [0u16, 0, (av.x + 192 * dx) as u16, (av.y + 192 * dy) as u16, (av.z + 8) as u16];
        call(&mut g, 0xFB, "FireballProcess::TonysBalls", &at.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>());
        (0..60).for_each(|_| _ = g.update(1.0 / 30.0, &Input::default()));
        g.stats.get(AVATAR).unwrap().hp < hp
    });
    assert!(burnt, "burnt");
    (0..700).for_each(|_| _ = g.update(1.0 / 30.0, &Input::default())); // (the rest go out in time)
    assert!(!g.world.objects().any(|id| g.world.get(id).is_some_and(|it| it.shape == 260)), "no fireballs left");
    assert!(!g.world.objects().any(|id| g.world.get(id).is_some_and(|it| it.shape == 261)), "its flames gone too");
}

/// receiveHit lands after the script's turn; a setDead or clrDead after it in the same turn
/// waits behind it, in the script's order (Mythran's death spell strikes, then marks dead: the
/// blow must find the Avatar alive). With nothing queued, they act at once.
#[test]
#[ignore]
fn set_dead_waits_behind_a_blow_in_the_same_turn() {
    let Some(mut g) = game(40) else { return };
    let hp = g.stats.get(50).unwrap().hp;
    call(&mut g, 0x8D, "Actor::setDead", &this(51));
    assert!(g.stats.get(51).unwrap().dead, "at once with nothing queued");
    // A 1-point piercing blow (it always lands, through any armour), marked dead, up again.
    call(&mut g, 0x53, "Item::receiveHit", &args(50, &[AVATAR, 0, 1, 0x80]));
    call(&mut g, 0x8D, "Actor::setDead", &this(50));
    assert!(!g.stats.get(50).unwrap().dead, "not before the blow");
    call(&mut g, 0x8E, "Actor::clrDead", &this(50));
    g.update(1.0 / 30.0, &Input::default());
    let s = g.stats.get(50).unwrap();
    assert_eq!((s.hp, s.dead), (hp - 1, false), "the blow found him alive; dead, then not, in that order");
}
