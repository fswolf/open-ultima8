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
    step(&mut g, Input { dir: Some(dir), run: true, ..Default::default() }, 0.3);
    assert!(!g.in_combat() && g.world.overlay(AVATAR).is_none());
    // Scripts can start and end it too (conversations do).
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
    // A plain one: armour class 7 takes 21% off.
    g.receive_hit(AVATAR, 0, 2, 10, dmg::NORMAL);
    assert_eq!(g.stats.get(AVATAR).unwrap().hp, hp - 5 - 7);
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
    // Struck, he doesn't cry "Guardsmen, to arms!" (DOS doesn't), but the guards nearby come:
    // the next one (NPC 51) is running in within a second or two in DOS.
    g.update(1.0 / 30.0, &Input::default());
    let barks: Vec<_> = g.vm().unwrap().events.drain(..).collect();
    assert!(!format!("{barks:?}").contains("to arms"), "{barks:?}");
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
    assert!(full - hp(&g, m) <= str / 2, "kicks do up to half strength (no leggings of power here)");
}

#[test]
#[ignore]
fn blood_fanfares_and_bursting_seekers() {
    use u8engine::combat::dmg;
    use u8engine::usecode::UiEvent;
    let Some(mut g) = game(40) else { return };
    let shapes =
        |g: &Game, s: u32| g.world.objects().filter(|&id| g.world.get(id).is_some_and(|it| it.shape == s)).count();
    // A real blow bleeds.
    g.receive_hit(AVATAR, 50, 2, 6, dmg::PIERCE);
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

#[test]
#[ignore]
fn changelings_wait_as_trees() {
    let Some(mut g) = game(40) else { return };
    let c = 61;
    let mut it = g.world.get(c).unwrap();
    it.shape = 357; // a changeling
    g.world.set(&g.data, c, it);
    let avatar_side = g.stats.get(AVATAR).unwrap().alignment;
    g.stats.get_mut(c).unwrap().enemies |= avatar_side;
    g.world.set_in_combat(c, true);
    // The Avatar far off: nobody to fight, so before long it turns into a tree.
    let mut a = g.world.get(AVATAR).unwrap();
    (a.x, a.y) = (it.x + 900, it.y);
    g.world.set(&g.data, AVATAR, a);
    let tick = |g: &mut Game, n: usize| (0..n).for_each(|_| _ = g.update(1.0 / 30.0, &Input::default()));
    let mut hid = false;
    for _ in 0..40 {
        tick(&mut g, 30);
        if g.world.last_anim(c) == 20 {
            hid = true;
            break;
        }
    }
    assert!(hid, "a tree");
    // The Avatar comes close: it springs back.
    (a.x, a.y, a.z) = (it.x + 128, it.y, it.z);
    g.world.set(&g.data, AVATAR, a);
    // (Once it's done turning into one: frames stay up their repeat + 1 ticks.)
    let back = (0..20).any(|_| {
        tick(&mut g, 10);
        g.world.last_anim(c) == 21
    });
    assert!(back, "back into shape");
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
    let starts: Vec<i32> = rings.values().copied().collect();
    assert_eq!(starts.len(), 4, "{starts:?}");
    assert!(starts.windows(2).all(|w| (25..=27).contains(&(w[1] - w[0]))), "{starts:?}");
    assert!(g.avatar_dead(), "drowned");
    assert_eq!(g.world.get(AVATAR).map(|a| (a.x, a.y, a.z)), Some((0, 0, 0)), "the body put away");
    let (at, was) = (g.camera().unwrap(), last.unwrap());
    assert!((at.0 - was.0).abs() < 64 && (at.1 - was.1).abs() < 64, "the view stays: {at:?} vs {was:?}");
}
