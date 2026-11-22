//! People's routines: scripted walks, schedules, routes, sleeping, made actors.
//! Run against a real install: U8_DATA=/path/to/U8 cargo test -- --ignored
mod common;
use common::*;

#[test]
#[ignore]
fn scripted_walks_arrive_or_give_up() {
    use u8engine::npc::Walk;
    let Some(mut g) = game(40) else { return };
    let start = g.world.get(AVATAR).unwrap();
    // Somewhere the Avatar can reach (south-east, as in the walking test).
    let mut w = Walk::new(1, AVATAR, (start.x + 120, start.y + 90, start.z), 0);
    let done = (0..600).find_map(|_| {
        let r = w.step(&g.data, &mut g.world);
        g.world.tick_acting(&g.data);
        r
    });
    assert_eq!(done, Some(true));
    let end = g.world.get(AVATAR).unwrap();
    assert!((end.x - (start.x + 120)).abs() <= 48 && (end.y - (start.y + 90)).abs() <= 48);
    // Into a wall: gives up instead of walking forever.
    let mut w = Walk::new(2, AVATAR, (end.x - 4000, end.y - 4000, end.z), 0);
    assert_eq!(
        (0..2000).find_map(|_| {
            let r = w.step(&g.data, &mut g.world);
            g.world.tick_acting(&g.data);
            r
        }),
        Some(false)
    );
}

#[test]
#[ignore]
fn schedules_move_people_through_the_day() {
    let Some(mut g) = game(40) else { return };
    let mordea = g.world.get(7).expect("Mordea on map 40");
    let still = Input::default();
    // 09:00 → past 20:00 at 30 ticks per second.
    let ticks = (u8engine::game::DAY_SECONDS as f32 * 30.0 * 11.5 / 24.0) as usize;
    for _ in 0..ticks {
        g.update(1.0 / 30.0, &still);
    }
    let later = g.world.get(7).unwrap();
    assert_ne!((mordea.x, mordea.y), (later.x, later.y), "Mordea keeps a different place in the evening");
    assert_eq!(g.scripts.as_ref().unwrap().vm.errors, 0);
}

#[test]
#[ignore]
fn walkers_route_around_walls() {
    use u8data::anim::action;
    use u8engine::{npc::Walk, path, Game};
    let Some(mut g) = game(40) else { return };
    let s = g.world.get(AVATAR).unwrap();
    // Find a spot a straight walk can't reach but a route can (the other side of a wall).
    let straight = |g: &mut Game, to: (i32, i32)| {
        let home = g.world.get(AVATAR).unwrap();
        let mut w = home;
        for _ in 0..200 {
            let (dx, dy) = ((to.0 - w.x).clamp(-8, 8), (to.1 - w.y).clamp(-8, 8));
            match g.world.try_move(AVATAR, dx, dy) {
                Some(p) => {
                    (w.x, w.y, w.z) = p;
                    g.world.set(&g.data, AVATAR, w);
                }
                None => break,
            }
        }
        g.world.set(&g.data, AVATAR, home);
        (w.x - to.0).abs().max((w.y - to.1).abs()) <= 48
    };
    let mut found = None;
    'search: for r in [4, 6, 8, 10, 12] {
        for (dx, dy) in (-r..=r).flat_map(|x| [(x, -r), (x, r), (-r, x), (r, x)]) {
            let to = (s.x + dx * 64, s.y + dy * 64);
            // A route nobody's standing in.
            let route = path::find(&g.world, &g.data, AVATAR, action::WALK, to, 48, true);
            let ends_there = route.is_some_and(|r| (r.end.0 - to.0).abs().max((r.end.1 - to.1).abs()) <= 48);
            if ends_there && !straight(&mut g, to) {
                found = Some(to);
                break 'search;
            }
        }
    }
    let to = found.expect("a spot behind a wall near the start");
    let mut w = Walk::new(1, AVATAR, (to.0, to.1, s.z), 0);
    assert_eq!(
        (0..3000).find_map(|_| {
            let r = w.step(&g.data, &mut g.world);
            g.world.tick_acting(&g.data);
            r
        }),
        Some(true),
        "walked round to {to:?}"
    );
}

#[test]
#[ignore]
fn route_planning_is_cheap_even_when_hopeless() {
    let Some(g) = game(40) else { return };
    let s = g.world.get(AVATAR).unwrap();
    let t = std::time::Instant::now();
    let _ = u8engine::path::find(&g.world, &g.data, AVATAR, 0, (s.x - 6000, s.y - 6000), 48, false);
    let ms = t.elapsed().as_millis();
    eprintln!("hopeless route search: {ms} ms");
    let limit = if cfg!(debug_assertions) { 1500 } else { 60 };
    assert!(ms < limit, "took {ms} ms");
}

#[test]
#[ignore]
fn mordeas_bed_shows_her_only_while_she_sleeps() {
    // Her schedule moves her once the Avatar is 700 away (still in the fast area); her bed
    // (MORDEABE, 567) must follow, not keep its old look until the Avatar leaves altogether.
    let Some(mut g) = game(40) else { return };
    let bed = g.world.objects().find(|&id| g.world.get(id).is_some_and(|it| it.shape == 567)).unwrap();
    let b = g.world.get(bed).unwrap();
    let run = |g: &mut Game, s: f32| (0..(s * 30.0) as usize).for_each(|_| _ = g.update(1.0 / 30.0, &Input::default()));
    let mut a = g.world.get(AVATAR).unwrap();
    (a.x, a.y) = (b.x + 900, b.y);
    g.world.set(&g.data, AVATAR, a);
    for (hour, asleep) in [(11, false), (2, true), (11, false)] {
        let day = g.minutes() / 1440;
        g.set_minutes((day + 1) * 1440 + hour * 60);
        run(&mut g, 3.0);
        let m = g.world.get(7).unwrap();
        let in_bed = (m.x, m.y, m.z) == (b.x, b.y, 0);
        assert_eq!(in_bed, asleep, "at {hour}:00 she's at ({}, {}, {})", m.x, m.y, m.z);
        assert_eq!(g.world.get(bed).unwrap().frame, if asleep { 10 } else { 11 }, "the bed at {hour}:00");
    }
}

#[test]
#[ignore]
fn the_ethereal_stack_and_made_actors() {
    use u8engine::usecode::{Host, Pop, WorldHost};
    let Some(mut g) = game(40) else { return };
    let pack = g.backpack().unwrap();
    let mut h = WorldHost { world: &mut g.world, data: &g.data, stats: &mut g.stats, others: None };
    // Made in the void, a script pops it into a bag: the top of the stack goes, whatever's named.
    let coin = h.create(u8data::map::Item { shape: 143, quality: 5, ..Default::default() }, false);
    assert_eq!(h.ethereal_top(), Some(coin));
    assert_eq!(h.pop_ethereal(Pop::Into(pack)), Some(coin));
    assert_eq!(h.ethereal_top(), None);
    assert_eq!(h.container(coin), Some(pack));
    // createActor: a person in slot 62 (the castle's first free one), off the map till popped.
    assert!(h.create_actor(62, 574, 0));
    assert_eq!(h.ethereal_top(), Some(62));
    assert!(h.world.is_void(62));
    let s = h.stats(62).unwrap();
    assert!((35..=65).contains(&s.hp) && (s.alignment, s.enemies) == (1, 0xE), "a guard's numbers: {s:?}");
    assert_eq!(h.pop_ethereal(Pop::At((12800, 9300, 48))), Some(62));
    assert!(!h.world.is_void(62));
    assert_eq!(g.world.get(62).map(|it| (it.x, it.y)), Some((12800, 9300)));
}

#[test]
#[ignore]
fn actions_carry_people_along() {
    // Scripts walk people with doAnim(walk): each frame steps them 4 × its forward amount, as
    // ScummVM's AnimationTracker does (the crowd leaving the execution walks off this way).
    use u8data::anim::action;
    let Some(mut g) = game(40) else { return };
    let guard = 50;
    let before = g.world.get(guard).unwrap();
    // A way that's open for a while.
    let dirs = [(0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1)];
    let dir = (0..8)
        .find(|&d| {
            (1..6).all(|k| {
                g.world
                    .step_from(
                        guard,
                        (before.x + dirs[d].0 * 32 * (k - 1), before.y + dirs[d].1 * 32 * (k - 1), before.z),
                        dirs[d].0 * 32,
                        dirs[d].1 * 32,
                        true,
                        (u8engine::world::WALK_STEP, u8engine::world::WALK_STEP),
                    )
                    .is_some()
            })
        })
        .expect("room to walk");
    let fwd: i32 =
        g.data.anims.get(before.shape, action::WALK).unwrap().frames[dir].iter().map(|f| 4 * f.ddir as i32).sum();
    assert!(fwd > 0, "a walk goes somewhere");
    g.scripts = None; // (his own routine would have him stand)
    g.world.play(&g.data, guard, action::WALK, dir);
    (0..20).for_each(|_| _ = g.update(1.0 / 30.0, &Input::default()));
    let after = g.world.get(guard).unwrap();
    let moved = (after.x - before.x).abs().max((after.y - before.y).abs());
    assert!(moved > 0 && moved <= fwd, "one walk cycle along facing {dir}: {moved} of {fwd}");
}

#[test]
#[ignore]
fn an_action_ending_in_mid_air_falls() {
    // Jumps (the Crag's acolytes) can end over lower ground: they come down (ScummVM falls an
    // actor whose animation leaves it unsupported).
    use u8data::anim::action;
    let Some(mut g) = game(40) else { return };
    g.scripts = None;
    // Someone with open air above.
    let (id, floor) = (2..256)
        .filter_map(|id| {
            let mut it = g.world.get(id).filter(|_| !g.world.is_void(id))?;
            let floor = it.z;
            it.z += 32;
            g.world.set(&g.data, id, it);
            let clear = g.world.floor_under(id) == Some(floor);
            it.z = floor;
            g.world.set(&g.data, id, it);
            clear.then_some((id, floor))
        })
        .next()
        .expect("someone under the sky");
    let mut it = g.world.get(id).unwrap();
    it.z += 32;
    g.world.set(&g.data, id, it);
    g.world.play(&g.data, id, action::STAND, 0);
    for _ in 0..90 {
        g.update(1.0 / 30.0, &Input::default());
    }
    assert_eq!(g.world.get(id).unwrap().z, floor);
}

#[test]
#[ignore]
fn the_avatar_doesnt_walk_up_onto_tables() {
    // Ryan: the Avatar could stand on a table by walking into it (it climbed 16 a step; ScummVM's
    // walk finds footing within 8). A table in Tenebrae (map 12), walked into from the east.
    let Some(mut g) = game(40) else { return };
    assert!(g.put_avatar(12, (16599 + 64, 15615 - 32, 40)));
    let t = g
        .world
        .objects()
        .find(|&id| g.world.get(id).is_some_and(|it| it.shape == 263 && (it.x, it.y) == (16543, 15647)))
        .unwrap();
    let top = g.world.index(t).map(|i| g.world.entry(i).z1).unwrap();
    for _ in 0..40 {
        if let Some((x, y, z)) = g.world.try_move(AVATAR, -8, 0) {
            let mut a = g.world.get(AVATAR).unwrap();
            (a.x, a.y, a.z) = (x, y, z);
            g.world.set(&g.data, AVATAR, a);
        }
    }
    assert!(g.world.get(AVATAR).unwrap().z < top, "still on the floor");
}

#[test]
#[ignore]
fn walkers_step_as_their_walk_animates() {
    // Ryan: the guards' walk looked off. A scripted walk is whole walk steps (ScummVM's
    // pathfinder: one doAnim a step, the animation moving them): the looping cycle's frame 0
    // (the start from standing) never shows on the way, each frame stays up its action's repeat +
    // 1 ticks, and they only move as a frame says.
    use u8data::anim::action;
    use u8engine::npc::Walk;
    let Some(mut g) = game(40) else { return };
    g.scripts = None;
    let mut walked = 0;
    for id in 2..256u16 {
        let Some(it) = g.world.get(id).filter(|_| !g.world.is_void(id)) else { continue };
        let Some(walk) = g.data.anims.get(it.shape, action::WALK) else { continue };
        let Some(to) = (0..8).find_map(|d| {
            let (dx, dy) = u8engine::geom::DIRS[d];
            let to = (it.x + dx * 256, it.y + dy * 256, it.z);
            u8engine::path::find(&g.world, &g.data, id, action::WALK, (to.0, to.1), 48, true)
                .filter(|r| (r.end.0 - to.0).abs().max((r.end.1 - to.1).abs()) <= 48)
                .map(|_| to)
        }) else {
            continue;
        };
        let mut w = Walk::new(0, id, to, 0);
        let (mut last, mut held, mut holds) = (None, 0, vec![]);
        let done = (0..900).find_map(|_| {
            let r = w.step(&g.data, &mut g.world);
            if let Some((act, _, k)) = g.world.acting(id) {
                assert!(act != action::WALK || k != 0, "shape {} showed frame 0 mid-walk", it.shape);
            }
            g.world.tick_acting(&g.data);
            let it = g.world.get(id).unwrap();
            if r.is_none() {
                if Some((it.frame, it.x, it.y)) == last {
                    held += 1;
                } else {
                    holds.push(held);
                    held = 1;
                }
                last = Some((it.frame, it.x, it.y));
            }
            r
        });
        assert_eq!(done, Some(true), "NPC {id} (shape {}) walked to {to:?}", it.shape);
        let per = walk.repeat as usize + 1;
        // (Frames, bar the first and the turn about, are up the whole time.)
        let typical = holds.iter().skip(1).filter(|&&h| h == per).count();
        assert!(typical * 2 >= holds.len().saturating_sub(1), "frames held {per} ticks: {holds:?}");
        walked += 1;
        if walked == 3 {
            break;
        }
    }
    assert!(walked > 0, "somebody to walk");
}

#[test]
#[ignore]
fn a_roof_keeps_floors_apart() {
    // Ryan: on the castle roof he could read the Queen's speech from the hall below and hear its
    // fire and clock. Things are on the same floor unless a roof lies between them.
    let Some(mut g) = game(40) else { return };
    let (below, roof) = g
        .world
        .objects()
        .filter(|&id| id != AVATAR && g.world.container(id).is_none())
        .find_map(|id| Some((id, g.world.ceiling(id)?)))
        .expect("something under a roof");
    let it = g.world.get(below).unwrap();
    let at = |z| u8data::map::Item { shape: 1, x: it.x, y: it.y, z, ..Default::default() };
    let up = g.world.add(&g.data, at(roof + 8), true);
    let beside = g.world.add(&g.data, at(it.z), true);
    assert!(!g.world.same_floor(below, up) && !g.world.same_floor(up, below), "the roof's between");
    assert!(g.world.same_floor(below, beside), "the same room");
}

#[test]
#[ignore]
fn people_stay_put_until_the_hour_turns() {
    // Ryan: Corinth (NPC 15, map 7) wasn't home. Arriving from a loaded game ran everyone's
    // schedule at once, so at 9 o'clock he was sent to his daytime spot; the original leaves
    // people where the save had them until the hour turns. A map whose people missed an hourly
    // run while we were away catches it up on arrival.
    let Ok(dir) = std::env::var("U8_DATA") else { return };
    let mut g = Game::open(std::path::Path::new(&dir), Some(2), None).expect("save 2");
    let corinth = 15;
    let home = (12671, 21791);
    let tick = |g: &mut Game, n: usize| (0..n).for_each(|_| _ = g.update(1.0 / 30.0, &Input::default()));
    let at = |g: &Game| g.world.get(corinth).map(|c| (c.x, c.y));
    let hour = g.minutes() / 60 % 24;
    assert_eq!(hour, 3, "the save is at 03:02 (KERNEL.DAT's clock)");
    // Somewhere on his map far (over 700) from both his home and his daytime spot.
    let far = [(14000, 20000), (10000, 20000), (14000, 23000), (10000, 22000), (15000, 21791)];
    let spot = |g: &mut Game| far.into_iter().find(|&(x, y)| g.put_avatar(7, (x, y, 8)) || g.put_avatar(7, (x, y, 0)));
    let s = spot(&mut g).expect("room for the Avatar on map 7");
    tick(&mut g, 30);
    assert_eq!(at(&g), Some(home), "still at home, as saved (Avatar at {s:?})");
    // Away until morning: on coming back, his schedule catches up.
    g.put_avatar(40, (12774, 9010, 48));
    g.set_minutes(g.minutes() / 1440 * 1440 + 9 * 60);
    tick(&mut g, 30);
    spot(&mut g);
    tick(&mut g, 30);
    assert_eq!(at(&g), Some((12255, 24831)), "off to his daytime spot");
}

#[test]
#[ignore]
fn loaded_people_are_where_the_save_put_them() {
    // Every NPC of saves 1 and 2, on every map: a few seconds after the game is loaded there
    // (the Avatar beside the first of them), each is still near where the save had it —
    // nobody sent off by a schedule run that the original wouldn't have made yet.
    let Ok(dir) = std::env::var("U8_DATA") else { return };
    let root = std::path::Path::new(&dir);
    for slot in [1, 2] {
        let save = u8data::save::Save::load(root, slot).unwrap();
        let gd = GameData::load_save(root, Some(&save)).unwrap();
        let ic = gd.itemcach.clone().unwrap();
        let mut maps: std::collections::BTreeMap<usize, Vec<(u16, u8data::map::Item)>> = Default::default();
        for n in 2..256 {
            // (Parked monster slots, z 254, wait off the map for eggs to hatch them.)
            if let Some((m, it)) = u8data::map::load_actor(&ic, n).filter(|(m, it)| *m > 0 && it.z < 250) {
                maps.entry(m).or_default().push((n as u16, it));
            }
        }
        for (&m, npcs) in &maps {
            let mut g = Game::open(root, Some(slot), Some(m as u16)).unwrap();
            let p = npcs[0].1;
            [(64, 0), (0, 64), (-64, 0), (0, -64)]
                .iter()
                .any(|&(dx, dy)| g.put_avatar(m as u16, (p.x + dx, p.y + dy, p.z)));
            (0..120).for_each(|_| _ = g.update(1.0 / 30.0, &Input::default()));
            for &(n, s) in npcs {
                let now = g.world.get(n).filter(|_| !g.world.is_void(n));
                let off = now.map(|i| (i.x - s.x).abs().max((i.y - s.y).abs()));
                // (Someone already walking can get a few steps on.)
                assert!(
                    off.is_some_and(|d| d <= 320),
                    "save {slot} map {m}: NPC {n} saved at {:?}, now {:?}",
                    (s.x, s.y, s.z),
                    now.map(|i| (i.x, i.y, i.z))
                );
            }
        }
    }
}

/// Orlok's `use` (class 1034) is over 32K: twenty of its relative jumps reach the far end by
/// wrapping round 16 bits. Asked his name, his list of replies comes back; and twenty talks,
/// each taking the replies taken least so far (goodbye after 30), end without a script error.
#[test]
#[ignore]
fn orloks_whole_conversation_runs() {
    use std::collections::HashMap;
    use u8engine::usecode::UiEvent;
    let mut taken: HashMap<(Vec<String>, usize), u32> = HashMap::new();
    let mut talk = |first_then_bye: bool| {
        let mut g = saved(1)?;
        g.set_minutes(600);
        g.put_avatar(39, (16095, 12575, 48));
        for _ in 0..30 {
            g.update(1.0 / 30.0, &Input::default());
        }
        assert!(g.use_item(10));
        let (mut asks, mut said) = (Vec::<String>::new(), vec![]);
        for _ in 0..60000 {
            g.update(1.0 / 30.0, &Input::default());
            for e in g.take_events() {
                let vm = g.vm().unwrap();
                match e {
                    UiEvent::Bark { pid, text, .. } => {
                        said.push(text);
                        vm.finish(pid, 0)
                    }
                    UiEvent::Ask { pid, options, .. } => {
                        let last = options.len() - 1;
                        let k = if first_then_bye {
                            if asks.is_empty() {
                                0
                            } else {
                                last
                            }
                        } else if asks.len() >= 30 || last == 0 {
                            last
                        } else {
                            let n = |k: usize| taken.get(&(options.clone(), k)).copied().unwrap_or(0);
                            (0..last).min_by_key(|&k| n(k)).unwrap()
                        };
                        *taken.entry((options.clone(), k)).or_default() += 1;
                        asks.push(options[k].clone());
                        vm.answer(pid, &options, k)
                    }
                    _ => {}
                }
            }
            if !g.in_stasis() && asks.last().is_some_and(|a| a.starts_with("Goodbye")) {
                break;
            }
        }
        let vm = g.vm().unwrap();
        assert_eq!((vm.errors, vm.missing.len()), (0, 0), "after {asks:?}");
        assert!(!g.in_stasis(), "the conversation ended: {asks:?}");
        Some((asks, said))
    };
    // His name, then the replies again.
    let Some((asks, said)) = talk(true) else { return };
    assert!(said.iter().any(|s| s.starts_with("Me name's Orlok")), "{said:?}");
    assert!(asks.len() >= 2, "the list came back: {asks:?}");
    // Everything he can be asked.
    for _ in 0..20 {
        talk(false).unwrap();
    }
    let lists: std::collections::HashSet<_> = taken.keys().map(|(o, _)| o.clone()).collect();
    println!("{} replies in {} lists", taken.len(), lists.len());
    assert!(taken.len() > 15, "{}", taken.len());
}

/// Woken (her bed used, at night in slot 2), Mordea gets up, zaps the Avatar and goes back to
/// bed. While she's up the bed shows itself empty (frame 11), never her asleep in it as well;
/// back in bed, it shows her again (10) and she's out of sight.
#[test]
#[ignore]
fn mordea_woken_isnt_in_her_bed_too() {
    let Some(mut g) = saved(2) else { return };
    g.put_avatar(40, (13151, 8100, 48));
    let bed = g.world.objects().find(|&id| g.world.get(id).is_some_and(|it| it.shape == 567)).unwrap();
    assert_eq!(g.world.get(bed).unwrap().frame, 10, "asleep");
    assert!(g.use_item(bed));
    let mut up = 0;
    for t in 0..900 {
        g.update(1.0 / 30.0, &Input::default());
        answer(&mut g);
        if g.world.get(7).unwrap().z == 48 {
            up += 1;
            assert_ne!(g.world.get(bed).unwrap().frame, 10, "tick {t}: she's up, and asleep in her bed too");
        }
    }
    assert!(up > 30, "she got up ({up} ticks)");
    assert_eq!((g.world.get(bed).unwrap().frame, g.world.get(7).unwrap().z), (10, 0), "back in bed");
}

/// An actor keeps the way its last action faced, even on a frame no standing or walking has:
/// Mordea, woken, lowers her hands facing east (CAST2, dir 2) before she's shown, and her turn
/// to the Avatar starts from there (not from south, which made her spin round through her back).
#[test]
#[ignore]
fn a_spell_pose_keeps_its_facing() {
    use u8data::anim::action::CAST2;
    let Some(mut g) = game(40) else { return };
    g.world.play(&g.data, 7, CAST2, 2).expect("Mordea has the pose");
    let it = g.world.get(7).unwrap();
    assert_eq!(u8engine::avatar::facing(&g.data, &it), None, "(a frame the guess doesn't know)");
    assert_eq!(g.world.facing(&g.data, 7), Some(2));
}
