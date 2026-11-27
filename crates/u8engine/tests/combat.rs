//! Fighting: the Avatar's stance, swings, kicks and blocks; people and monsters fighting back,
//! dying, loot and the risen.
//! Run against a real install: U8_DATA=/path/to/U8 cargo test -- --ignored
mod common;
use common::*;

#[test]
#[ignore]
fn the_avatar_draws_its_weapon_and_fights() {
    use u8data::anim::action;
    let Some(mut g) = game(40) else { return };
    let step = |g: &mut Game, input: Input, secs: f32| {
        for _ in 0..(secs * 30.0) as usize {
            g.update(1.0 / 30.0, &input);
        }
    };
    let frame = |g: &Game, act: usize| {
        let a = g.world.get(AVATAR).unwrap();
        let dir = g.avatar.as_ref().unwrap().dir;
        u8engine::avatar::anim_frame(&g.data, a.shape, act, dir, 0).map(|f| f.0) == Some(a.frame)
    };
    assert!(!g.in_combat() && g.world.overlay(AVATAR).is_none(), "at ease the dagger is put away");
    step(&mut g, Input { combat: true, ..Default::default() }, 1.0 / 30.0);
    assert!(g.in_combat());
    step(&mut g, Input::default(), 1.5);
    assert!(frame(&g, action::COMBAT_STAND), "readied, in combat stance");
    let o = g.world.overlay(AVATAR).expect("the dagger drawn over the Avatar");
    assert_eq!(o.shape, 245);
    // A swing, then back in stance.
    step(&mut g, Input { attack: true, ..Default::default() }, 1.0 / 30.0);
    assert!(g.avatar.as_ref().unwrap().busy());
    step(&mut g, Input::default(), 1.0);
    assert!(frame(&g, action::COMBAT_STAND));
    // Stepping forward keeps the stance; running puts the weapon away.
    let dir = g.avatar.as_ref().unwrap().dir;
    step(&mut g, Input { dir: Some(dir), ..Default::default() }, 0.3);
    assert!(g.in_combat());
    step(&mut g, Input { dir: Some(dir), run: true, ..Default::default() }, 0.6);
    assert!(!g.in_combat() && g.world.overlay(AVATAR).is_none());
    // Scripts can start and end it too (conversations do).
    step(&mut g, Input::default(), 1.0);
    g.world.set_in_combat(AVATAR, true);
    step(&mut g, Input::default(), 0.1);
    assert!(frame(&g, action::COMBAT_STAND) && g.world.overlay(AVATAR).is_some());
    // A frame set by a script isn't the stance: no dagger floating beside it.
    let mut a = g.world.get(AVATAR).unwrap();
    a.frame += 1;
    g.world.set(&g.data, AVATAR, a);
    step(&mut g, Input::default(), 0.1);
    assert!(g.world.overlay(AVATAR).is_none());
}

#[test]
#[ignore]
fn swings_hurt_people_until_they_die() {
    use u8data::anim::action;
    let Some(mut g) = game(40) else { return };
    let step = |g: &mut Game, input: Input, secs: f32| {
        for _ in 0..(secs * 30.0).ceil() as usize {
            g.update(1.0 / 30.0, &input);
        }
    };
    // Stand the Avatar right behind Mordea (NPC 7), facing her (+x is facing 2). She's one of
    // those the game keeps alive (NPCDATA's immortal flag); a script could lift that.
    let m = 7;
    assert!(g.stats.get(m).unwrap().immortal);
    g.receive_hit(m, 0, 2, 999, u8engine::combat::dmg::PIERCE);
    assert!(!g.stats.get(m).unwrap().dead, "immortal");
    g.stats.get_mut(m).unwrap().immortal = false;
    let her = g.world.get(m).unwrap();
    let mut a = g.world.get(AVATAR).unwrap();
    (a.x, a.y, a.z) = (her.x - 64, her.y, her.z);
    g.world.set(&g.data, AVATAR, a);
    g.avatar.as_mut().unwrap().dir = 2;
    assert_eq!(g.swing_target(AVATAR, 2, 1), Some(m), "she's within a step");
    step(&mut g, Input { combat: true, ..Default::default() }, 1.0 / 30.0);
    step(&mut g, Input::default(), 1.5);
    let full = g.stats.get(m).unwrap().hp;
    for _ in 0..60 {
        step(&mut g, Input { attack: true, ..Default::default() }, 1.0 / 30.0);
        step(&mut g, Input::default(), 0.6);
        if g.stats.get(m).unwrap().dead {
            break;
        }
    }
    let s = g.stats.get(m).unwrap();
    assert!(s.dead && s.hp == 0, "dead after enough swings (had {full} hit points)");
    step(&mut g, Input::default(), 2.0);
    let dir = 6; // facing the Avatar (facing 2) back
    let last = g.data.anims.get(her.shape, action::DIE).unwrap().frames[dir].len() - 1;
    let fallen = u8engine::avatar::anim_frame(&g.data, her.shape, action::DIE, dir, last).unwrap().0;
    assert_eq!(g.world.get(m).unwrap().frame, fallen, "she lies where she fell");
    assert_eq!(g.swing_target(AVATAR, 2, 1), None, "the dead aren't struck again");
    // Saved: no hit points left.
    let save = g.snapshot();
    let f = u8data::flex::Flex::parse(save.get("NPCDATA.DAT").unwrap().to_vec()).unwrap();
    assert_eq!(u8data::map::npc_stats(f.get(0).unwrap(), m as usize).unwrap().hit_points, 0);
}

#[test]
#[ignore]
fn the_avatar_staggers_and_can_die() {
    let Some(mut g) = game(40) else { return };
    let hp = g.stats.get(AVATAR).unwrap().hp;
    // A trap's piercing blow: no armour, no dodging.
    use u8engine::combat::dmg;
    g.receive_hit(AVATAR, 0, 2, 5, dmg::PIERCE);
    assert_eq!(g.stats.get(AVATAR).unwrap().hp, hp - 5);
    assert!(g.avatar.as_ref().unwrap().busy(), "staggering");
    // A plain one: armour class 7 takes 21% off, the fraction kept by chance.
    g.receive_hit(AVATAR, 0, 2, 10, dmg::NORMAL);
    let took = hp - 5 - g.stats.get(AVATAR).unwrap().hp;
    assert!(took == 7 || took == 8, "{took}");
    g.receive_hit(AVATAR, 0, 2, 999, dmg::PIERCE);
    assert!(g.avatar_dead());
    (0..60).for_each(|_| _ = g.update(1.0 / 30.0, &Input::default())); // falling back
    let at = g.world.get(AVATAR).map(|a| (a.x, a.y));
    for _ in 0..30 {
        g.update(1.0 / 30.0, &Input { dir: Some(2), ..Default::default() });
    }
    assert_eq!(g.world.get(AVATAR).map(|a| (a.x, a.y)), at, "the dead don't walk");
}

#[test]
#[ignore]
fn people_fight_back() {
    let Some(mut g) = game(40) else { return };
    let tick = |g: &mut Game, secs: f32| {
        for _ in 0..(secs * 30.0) as usize {
            g.update(1.0 / 30.0, &Input::default());
        }
    };
    // A castle guard (NPC 50, a few steps off) struck by the Avatar comes after it.
    let guard = 50;
    let far = |g: &Game| {
        let (a, b) = (g.world.get(AVATAR).unwrap(), g.world.get(guard).unwrap());
        (a.x - b.x).abs().max((a.y - b.y).abs())
    };
    // The double door between them shuts as the map starts (its LOCKDOOR egg); open it again.
    tick(&mut g, 4.0);
    let door = g.world.objects().find(|&id| g.world.get(id).is_some_and(|it| it.shape == 69 && it.y == 9215));
    assert!(g.trigger(door.expect("the shut door"), u8engine::usecode::event::USE));
    tick(&mut g, 2.0);
    let start = far(&g);
    assert!(start > 200, "starts {start} away");
    // A fall of its own blames nobody: no fight, no call for help.
    g.receive_hit(guard, 0, 0, 1, u8engine::combat::dmg::FALLING);
    assert!(!g.world.in_combat(guard) && !g.world.in_combat(51), "a fall starts no fight");
    g.receive_hit(guard, AVATAR, 0, 1, u8engine::combat::dmg::PIERCE);
    assert_eq!(g.world.target(guard).now, AVATAR);
    assert!(g.world.in_combat(guard));
    // Struck, he cries "Guardsmen, to arms!" (his going-into-combat script, as DOS shows it),
    // once, and the guards nearby come: the next one (NPC 51) is running in within a second or
    // two in DOS.
    g.receive_hit(guard, AVATAR, 0, 1, u8engine::combat::dmg::PIERCE);
    g.update(1.0 / 30.0, &Input::default());
    let barks = format!("{:?}", g.vm().unwrap().events.drain(..).collect::<Vec<_>>());
    assert_eq!(barks.matches("to arms").count(), 3, "50 once, and 51 and 61 called in: {barks}");
    assert!(g.world.in_combat(51) && g.world.target(51).now == AVATAR, "the next guard comes too");
    assert!(!g.world.in_combat(7), "Mordea (no quarrel with the Avatar) stays out of it");
    let d51 = |g: &Game| {
        let (a, b) = (g.world.get(AVATAR).unwrap(), g.world.get(51).unwrap());
        (a.x - b.x).abs().max((a.y - b.y).abs())
    };
    let from = d51(&g);
    let hp = g.stats.get(AVATAR).unwrap().hp;
    for _ in 0..40 {
        tick(&mut g, 0.5);
        if g.stats.get(AVATAR).unwrap().hp < hp {
            break;
        }
    }
    assert!(far(&g) < start, "closed in ({} from {start})", far(&g));
    assert!(d51(&g) < from, "NPC 51 closed in too ({} from {from})", d51(&g));
    let left = g.stats.get(AVATAR).unwrap().hp;
    assert!(left < hp, "a guard's blade gets through armour class 7 ({left} of {hp})");
    // Out of combat, it stands down.
    g.world.set_in_combat(guard, false);
    tick(&mut g, 1.0);
    assert!(!g.world.in_combat(guard));
}

#[test]
#[ignore]
fn monsters_hit_by_their_kind() {
    use u8engine::combat::dmg;
    let m = u8engine::monsters::get(574).unwrap();
    assert_eq!((m.name, m.damage, m.armour, m.damage_type), ("guard", (8, 15), 25, dmg::BLADE));
    let Some(mut g) = game(40) else { return };
    // A guard's armour 25 takes 75% off a plain blow; piercing gets through whole.
    let hp = g.stats.get(50).unwrap().hp;
    g.receive_hit(50, 0, 0, 8, dmg::NORMAL);
    assert_eq!(g.stats.get(50).unwrap().hp, hp - 2);
    // Alignments: guards (side 1) count the Avatar (2) an enemy; it isn't theirs.
    let (guard, av) = (g.stats.get(50).unwrap(), g.stats.get(AVATAR).unwrap());
    assert!(guard.is_enemy(&av) && !av.is_enemy(&guard));
}

#[test]
#[ignore]
fn monster_eggs_hatch_into_free_npc_slots() {
    use u8engine::world::item_flag::{ETHEREAL, FAST_ONLY};
    let dir = PathBuf::from(std::env::var("U8_DATA").unwrap());
    let Ok(mut g) = Game::open(&dir, Some(0), Some(3)) else { return };
    let tick = |g: &mut Game, secs: f32| {
        for _ in 0..(secs * 30.0) as usize {
            g.update(1.0 / 30.0, &Input::default());
        }
    };
    let place = |g: &mut Game, (x, y, z): (i32, i32, i32)| {
        let mut a = g.world.get(AVATAR).unwrap();
        (a.x, a.y, a.z) = (x, y, z);
        g.world.set(&g.data, AVATAR, a);
    };
    tick(&mut g, 0.2);
    // Into the range of a MONEGG usecode egg (id 1) on the shore: its monster eggs hatch a child
    // and a torax into the first free NPC slots (62 on, as the original's free list has them).
    place(&mut g, (14207, 2783, 48));
    tick(&mut g, 1.0);
    let hatched: Vec<u16> =
        (2..256u16).filter(|&id| g.world.get(id).is_some_and(|it| it.flags & FAST_ONLY != 0)).collect();
    assert_eq!(hatched, [62, 63]);
    let torax = g.world.get(63).unwrap();
    assert_eq!(torax.shape, 120);
    let s = g.stats.get(63).unwrap();
    assert!((40..=55).contains(&s.hp) && (8..=12).contains(&s.dex), "rolled from the torax's ranges: {s:?}");
    assert_eq!((s.alignment, s.enemies, s.str), (4, 2, 0));
    // Loitering (activity 0): it wanders off within a few seconds (near the Avatar).
    place(&mut g, (torax.x - 600, torax.y, torax.z));
    tick(&mut g, 10.0);
    let moved = g.world.get(63).unwrap();
    assert_ne!((moved.x, moved.y), (torax.x, torax.y), "loitering");
    // Out of the fast area: put away, the slot free again; saved as the original does.
    place(&mut g, (11494, 2330 + 4000, 48));
    tick(&mut g, 1.0);
    assert!(g.world.is_void(63));
    let save = g.snapshot();
    let f = u8data::flex::Flex::parse(save.get("ITEMCACH.DAT").unwrap().to_vec()).unwrap();
    let (_, row) = u8data::map::actor_row(f.get(0).unwrap(), 63).unwrap();
    assert_eq!((row.shape, row.flags & (ETHEREAL | FAST_ONLY)), (120, ETHEREAL | FAST_ONLY));
}

#[test]
#[ignore]
fn the_dead_leave_loot_and_skeletons_rise() {
    use u8engine::combat::dmg;
    use u8engine::usecode::UiEvent;
    let dir = PathBuf::from(std::env::var("U8_DATA").unwrap());
    let Ok(mut g) = Game::open(&dir, Some(0), Some(3)) else { return };
    let tick = |g: &mut Game, secs: f32| {
        for _ in 0..(secs * 30.0) as usize {
            g.update(1.0 / 30.0, &Input::default());
        }
    };
    tick(&mut g, 0.2);
    let a = g.world.get(AVATAR).unwrap();
    let egg = |shape: u32, x: i32, y: i32| u8data::map::Item {
        x: a.x + x,
        y: a.y + y,
        z: a.z,
        quality: shape as u16,
        mapnum: 2 << 3 | 2,
        ..Default::default()
    };
    // Somewhere with room, a few steps off.
    let hatch = |g: &mut Game, shape: u32| {
        let near = (-6..=6).flat_map(|x| (-6..=6).map(move |y| (x * 64, y * 64))).filter(|&(x, y)| (x, y) != (0, 0));
        near.into_iter().find_map(|(x, y)| g.hatch_monster(egg(shape, x, y)))
    };
    // A torax always leaves meat; its body opens for searching.
    let torax = hatch(&mut g, 120).expect("room by the Avatar");
    g.receive_hit(torax, AVATAR, 2, 999, dmg::PIERCE);
    assert!(g.stats.get(torax).unwrap().dead);
    let loot: Vec<u32> = g.world.contents(torax).iter().filter_map(|&i| g.world.get(i)).map(|it| it.shape).collect();
    assert_eq!(loot, [537]);
    g.vm().unwrap().events.clear();
    assert!(g.use_item(torax));
    let ev: Vec<UiEvent> = g.vm().unwrap().events.drain(..).collect();
    assert!(ev.iter().any(|e| matches!(e, UiEvent::OpenGump { gump: 12, .. })), "{ev:?}");
    // A skeleton gets up again within 30 seconds, fresh and fighting; burnt, it stays down.
    let bones = hatch(&mut g, 119).expect("room by the Avatar");
    g.receive_hit(bones, AVATAR, 2, 999, dmg::PIERCE);
    assert!(g.stats.get(bones).unwrap().dead);
    // (As it gets up: after that the fight wears it down.)
    for _ in 0..31 * 30 {
        if !g.stats.get(bones).unwrap().dead {
            break;
        }
        tick(&mut g, 1.0 / 30.0);
    }
    let s = g.stats.get(bones).unwrap();
    assert!(!s.dead && (35..=45).contains(&s.hp), "risen: {s:?}");
    assert!(g.world.in_combat(bones));
}

#[test]
#[ignore]
fn kicks_land_and_blocks_soften_blows() {
    use u8engine::combat::dmg;
    let Some(mut g) = game(40) else { return };
    let step = |g: &mut Game, input: Input, secs: f32| {
        for _ in 0..(secs * 30.0).ceil() as usize {
            g.update(1.0 / 30.0, &input);
        }
    };
    let hp = |g: &Game, id| g.stats.get(id).unwrap().hp;
    // A blow taken standing, then the same one caught while blocking.
    let before = hp(&g, AVATAR);
    g.receive_hit(AVATAR, 0, 6, 20, dmg::BLUNT);
    let open = before - hp(&g, AVATAR);
    step(&mut g, Input::default(), 1.0);
    step(&mut g, Input { combat: true, ..Default::default() }, 1.0 / 30.0);
    step(&mut g, Input::default(), 1.5);
    step(&mut g, Input { block: true, ..Default::default() }, 0.5);
    assert!(g.avatar.as_ref().unwrap().blocking());
    let before = hp(&g, AVATAR);
    g.receive_hit(AVATAR, 0, 6, 20, dmg::BLUNT);
    let blocked = before - hp(&g, AVATAR);
    assert!(blocked < open, "blocking took some off ({blocked} vs {open})");
    step(&mut g, Input { block: true, dir: Some(2), ..Default::default() }, 0.5);
    assert!(!g.avatar.as_ref().unwrap().blocking(), "stepping lets go");
    step(&mut g, Input { block: true, ..Default::default() }, 0.5);
    step(&mut g, Input::default(), 0.5);
    assert!(!g.avatar.as_ref().unwrap().blocking(), "let go");
    // Kicks at Mordea (NPC 7) right in front.
    let m = 7;
    g.stats.get_mut(m).unwrap().immortal = false;
    let her = g.world.get(m).unwrap();
    let mut a = g.world.get(AVATAR).unwrap();
    (a.x, a.y, a.z) = (her.x - 64, her.y, her.z);
    g.world.set(&g.data, AVATAR, a);
    g.avatar.as_mut().unwrap().dir = 2;
    let full = hp(&g, m);
    for _ in 0..30 {
        step(&mut g, Input { kick: true, ..Default::default() }, 1.0 / 30.0);
        step(&mut g, Input::default(), 1.0);
        if hp(&g, m) < full {
            break;
        }
    }
    assert!(hp(&g, m) < full, "a kick landed");
    let str = g.stats.get(AVATAR).unwrap().str as i16;
    assert!(full - hp(&g, m) <= str / 2 + 4, "kicks do 4 to 4 + half strength (no magic leggings here)");
    assert!(g.stats.get(m).unwrap().stunned > 0, "and stun");
    // Stunned, every blow lands.
    for _ in 0..20 {
        let before = hp(&g, m);
        g.stats.get_mut(m).unwrap().stunned = 10;
        g.stats.get_mut(m).unwrap().dex = 25;
        g.stats.get_mut(AVATAR).unwrap().dex = 1;
        g.receive_hit(m, AVATAR, 6, 1, dmg::NORMAL);
        assert_eq!(hp(&g, m), before - 1);
    }
    g.stats.get_mut(m).unwrap().stunned = 0;
    step(&mut g, Input::default(), 0.1);
}

#[test]
#[ignore]
fn blows_land_as_often_as_the_originals() {
    // ITEM.C receiveHit: hit when 0..attack + 3 beats 0..defend; at 10 against 10 that's 72%.
    use u8engine::combat::dmg;
    let Some(mut g) = game(40) else { return };
    let m = 7;
    g.stats.get_mut(m).unwrap().immortal = false;
    let mut hits = 0;
    for _ in 0..2000 {
        g.stats.get_mut(m).unwrap().hp = 100;
        g.stats.get_mut(m).unwrap().dex = 10;
        g.stats.get_mut(AVATAR).unwrap().dex = 10;
        g.receive_hit(m, AVATAR, 6, 1, dmg::NORMAL);
        hits += (g.stats.get(m).unwrap().hp < 100) as u32;
    }
    assert!((1340..1540).contains(&hits), "{hits} of 2000");
}

#[test]
#[ignore]
fn blood_fanfares_and_bursting_seekers() {
    use u8engine::combat::dmg;
    use u8engine::usecode::UiEvent;
    let Some(mut g) = game(40) else { return };
    let shapes =
        |g: &Game, s: u32| g.world.objects().filter(|&id| g.world.get(id).is_some_and(|it| it.shape == s)).count();
    // A real blow bleeds (a blade's, not only piercing: ITEM.C's blockable).
    g.receive_hit(AVATAR, 50, 2, 6, dmg::PIERCE);
    assert_eq!(shapes(&g, 620), 0, "no blood");
    g.receive_hit(AVATAR, 50, 2, 6, dmg::PIERCE | dmg::BLADE);
    assert_eq!(shapes(&g, 620), 1, "blood");
    // A guard who's the Avatar's enemy falls while it fights: the fanfare.
    let guard = 50;
    let avatar_side = g.stats.get(AVATAR).unwrap().alignment;
    g.stats.get_mut(guard).unwrap().enemies |= avatar_side;
    g.world.set_in_combat(AVATAR, true);
    g.take_events();
    g.receive_hit(guard, AVATAR, 2, 999, dmg::PIERCE);
    assert!(g.take_events().iter().any(|e| matches!(e, UiEvent::Victory)), "fanfare");
    // A seeker blows apart into five pieces flung about.
    let other = 61;
    let mut s = g.world.get(other).unwrap();
    s.shape = 509;
    g.world.set(&g.data, other, s);
    g.stats.get_mut(other).unwrap().immortal = false;
    g.receive_hit(other, AVATAR, 2, 999, dmg::PIERCE);
    assert_eq!(shapes(&g, 458), 5, "pieces");
}

/// Walking off the end of the Docks' pier: deep water drowns the Avatar (the water's script puts
/// the body away at 0,0,0) and the view stays over the water, as the DOS game's, instead of
/// following the body to the map's corner.
#[test]
#[ignore]
fn drowning_leaves_the_view_over_the_water() {
    let Some(dir) = std::env::var_os("U8_DATA") else { return };
    let mut g = Game::open(std::path::Path::new(&dir), Some(0), None).unwrap(); // the Docks
    g.vm().unwrap().kill_all(); // no opening scene: the view follows the Avatar
    g.follow_avatar();
    let mut a = g.world.get(AVATAR).unwrap();
    (a.x, a.y, a.z) = (11720, 6450, 48); // on the pier, near its open end
    g.world.set(&g.data, AVATAR, a);
    let (mut last, mut rings) = (None, std::collections::BTreeMap::new());
    for t in 0..240 {
        g.follow_avatar(); // (whatever the opening scene's left running says)
        g.update(1.0 / 30.0, &Input { dir: Some(5), ..Default::default() });
        if !g.avatar_dead() {
            last = g.camera();
        }
        for id in g.world.objects().filter(|&id| g.world.get(id).is_some_and(|it| it.shape == 476)) {
            rings.entry(id).or_insert(t);
        }
    }
    // Four ripples spread out over the body, one every 5/6 of a second (50 ticks of the scripts'
    // 60 Hz timer; the first with the splash).
    // (Landing across several water tiles, each runs its script: the same ripples, stacked.)
    let mut starts: Vec<i32> = rings.values().copied().collect();
    starts.dedup();
    assert_eq!(starts.len(), 4, "{starts:?}");
    assert!(starts.windows(2).all(|w| (25..=27).contains(&(w[1] - w[0]))), "{starts:?}");
    assert!(g.avatar_dead(), "drowned");
    assert_eq!(g.world.get(AVATAR).map(|a| (a.x, a.y, a.z)), Some((0, 0, 0)), "the body put away");
    let (at, was) = (g.camera().unwrap(), last.unwrap());
    assert!((at.0 - was.0).abs() < 64 && (at.1 - was.1).abs() < 64, "the view stays: {at:?} vs {was:?}");
}

/// Guards give chase round the castle: struck from the open ground south of the archway (slot
/// 2), with the Avatar running off south, east or west, guards 50 and 51 follow and close in.
#[test]
#[ignore]
fn guards_give_chase() {
    for dir in [4, 2, 5] {
        let Some(mut g) = saved(2) else { return };
        g.put_avatar(40, (12799, 9400, 48));
        g.stats.get_mut(AVATAR).unwrap().hp = 250;
        let tick = |g: &mut Game, input: Input, n: usize| {
            for _ in 0..n {
                g.update(1.0 / 30.0, &input);
            }
        };
        let (gd, w) = (&g.data, &mut g.world);
        g.avatar.as_mut().unwrap().turn(gd, w, 0);
        tick(&mut g, Input { combat: true, ..Input::default() }, 1);
        tick(&mut g, Input::default(), 30);
        for _ in 0..3 {
            tick(&mut g, Input { dir: Some(0), attack: true, ..Input::default() }, 1);
            tick(&mut g, Input::default(), 25);
        }
        tick(&mut g, Input { combat: true, ..Input::default() }, 1);
        tick(&mut g, Input::default(), 30);
        tick(&mut g, Input { dir: Some(dir), run: true, ..Input::default() }, 90); // (staying in the fast area)
        tick(&mut g, Input::default(), 900);
        let a = g.world.get(AVATAR).unwrap();
        for guard in [50, 51] {
            let it = g.world.get(guard).unwrap();
            let d = (it.x - a.x).abs().max((it.y - a.y).abs());
            assert!(
                g.world.in_combat(guard) && d < 320,
                "running {dir}: guard {guard} {d} away, at ({}, {}), avatar ({}, {}), target {:?}",
                it.x,
                it.y,
                a.x,
                a.y,
                g.world.target(guard)
            );
        }
    }
}

/// Rats (806) are low and flagged as land: the Avatar walks onto one rather than into it, and
/// standing on it runs its gotHit, which finds the Avatar on top and squashes it (as in DOS).
/// Outdoors, by the docks' rat egg (indoors a ceiling at 88 leaves no room to step up 8).
#[test]
#[ignore]
fn walking_onto_a_rat_squashes_it() {
    let Some(mut g) = saved(3) else { return };
    g.put_avatar(3, (9808, 2760, 48));
    let a = g.world.get(AVATAR).unwrap();
    let (dir, at) = [(2usize, (96, 0)), (4, (0, 96)), (6, (-96, 0)), (0, (0, -96))]
        .into_iter()
        .map(|(d, (dx, dy))| (d, (a.x + dx, a.y + dy, a.z)))
        .find(|&(_, at)| g.world.clear_at(&g.data, 806, at))
        .expect("room for a rat");
    // (activity 2: it stands where it is)
    let egg = u8data::map::Item { x: at.0, y: at.1, z: at.2, quality: 806, mapnum: 2, ..Default::default() };
    let rat = g.hatch_monster(egg).expect("a rat");
    let mut squish = false;
    for _ in 0..60 {
        g.update(1.0 / 30.0, &Input { dir: Some(dir), ..Input::default() });
        for e in g.take_events() {
            squish |= matches!(e, u8engine::usecode::UiEvent::Sfx { sfx: 0x55, .. });
        }
        if g.stats.get(rat).is_some_and(|s| s.dead) {
            break;
        }
    }
    let av = g.world.get(AVATAR).unwrap();
    assert!(g.stats.get(rat).is_some_and(|s| s.dead), "squashed underfoot (avatar at z {})", av.z);
    assert!(squish, "with its squish");
}

/// The other creatures that squash: a spider (807, 8 high, like a skellot) walked onto, and an
/// insect (824, 16 high: too high to step onto, and the clue book doesn't list it as squished
/// underfoot) only when landed on.
#[test]
#[ignore]
fn spiders_and_insects_squash_too() {
    // (walk: walked into; else dropped on from above)
    let squashed = |shape: u16, walk: bool| {
        let mut g = saved(3)?;
        g.put_avatar(3, (9808, 2760, 48));
        let a = g.world.get(AVATAR).unwrap();
        let at = (a.x + 96, a.y, a.z);
        let egg = u8data::map::Item { x: at.0, y: at.1, z: at.2, quality: shape, mapnum: 2, ..Default::default() };
        let id = g.hatch_monster(egg).expect("a creature");
        g.stats.get_mut(id).unwrap().dex = 0; // (the squash is a blow it can dodge)
        if !walk {
            g.put_avatar(3, (at.0 + 16, at.1 + 16, at.2 + 60));
            g.fall(AVATAR);
        }
        g.avatar.as_mut().unwrap().dir = 2;
        for _ in 0..60 {
            let dir = walk.then_some(2);
            g.update(1.0 / 30.0, &Input { dir, ..Input::default() });
            if g.stats.get(id).is_some_and(|s| s.dead) {
                break;
            }
        }
        Some(g.stats.get(id).is_some_and(|s| s.dead))
    };
    let Some(spider) = squashed(807, true) else { return };
    assert!(spider, "a spider walked onto");
    assert_eq!(squashed(824, true), Some(false), "an insect isn't: too high a step");
    assert_eq!(squashed(824, false), Some(true), "an insect landed on");
}

/// Hurt at ease, the Avatar staggers with nothing in his hand (the original draws the weapon only
/// in combat, or while putting it away: DLITEM.C); in combat stance, with it.
#[test]
#[ignore]
fn the_weapon_shows_only_in_combat() {
    use u8data::anim::action;
    let Some(mut g) = game(40) else { return };
    for combat in [false, true] {
        g.world.set_in_combat(AVATAR, combat);
        (0..45).for_each(|_| _ = g.update(1.0 / 30.0, &Input::default()));
        g.receive_hit(AVATAR, 0, 0, 2, u8engine::combat::dmg::PIERCE);
        let mut seen = vec![];
        for _ in 0..20 {
            g.update(1.0 / 30.0, &Input::default());
            if g.avatar.as_ref().unwrap().action() == action::STUMBLE {
                seen.push(g.world.overlay(AVATAR).is_some());
            }
        }
        assert!(!seen.is_empty() && seen.iter().all(|&o| o == combat), "combat {combat}: {seen:?}");
    }
}

/// A fighter's pause between blows (3 ticks for each point of dex under 25) runs while it
/// swings, as COMBATBR.C attackTarget waits on both: at dex 10, a blow every 45 ticks.
#[test]
#[ignore]
fn the_pause_between_blows_runs_during_the_swing() {
    let Some(mut g) = game(40) else { return };
    let guard = 50;
    let a = g.world.get(AVATAR).unwrap();
    let mut it = g.world.get(guard).unwrap();
    (it.x, it.y, it.z) = (a.x + 64, a.y, a.z);
    g.world.set(&g.data, guard, it);
    g.stats.get_mut(AVATAR).unwrap().invincible = true;
    g.stats.get_mut(guard).unwrap().dex = 10;
    g.world.set_target(guard, AVATAR);
    let (mut starts, mut swing, mut was) = (vec![], 0, false);
    for t in 0..600 {
        g.update(1.0 / 30.0, &Input::default());
        let now = g.world.acting(guard).is_some_and(|(act, ..)| act == u8data::anim::action::ATTACK);
        if now && !was {
            starts.push(t);
        }
        swing += now as u32;
        was = now;
    }
    assert!(starts.len() > 4, "swung {starts:?}");
    let swing = swing / starts.len() as u32;
    let gaps: Vec<_> = starts.windows(2).map(|w| w[1] - w[0]).skip(1).collect();
    let most = gaps.iter().max().copied().unwrap_or(0) as u32;
    assert!(most <= swing.max(45) + 3, "a blow every {gaps:?} ticks, a swing {swing} long");
}
