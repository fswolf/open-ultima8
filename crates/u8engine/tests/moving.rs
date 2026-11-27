//! The Avatar moved as the original moves him (WORLD.C, ANIM.C, ANIMPRIM.C): every move an
//! animation at its own pace, so a walk covers about 390 units a second and a run about 680;
//! turning about looks over the shoulder; a run into a wall stops short, a leap into one hurts one
//! time in three; moves follow each other on time at any frame rate; a running
//! leap is long; a standing jump goes to the pointer when it can.
//! Run against a real install: U8_DATA=/path/to/U8 cargo test -- --ignored
mod common;
use common::*;
use u8data::anim::action;
use u8data::{map::Item, typeflag::flag};
use u8engine::avatar::Aim;

const EAST: usize = 2;

/// The plateau (map 8), with 1900 units clear to the east.
fn plateau(facing: usize) -> Option<Game> {
    let mut g = saved(10)?;
    g.put_avatar(8, (109, 895, 0));
    g.avatar.as_mut()?.dir = facing;
    Some(g)
}

fn at(g: &Game) -> (i32, i32, i32) {
    g.world.get(AVATAR).map(|a| (a.x, a.y, a.z)).unwrap()
}

/// Play `ticks` with `input`: the (action, facing) shown each tick.
fn play(g: &mut Game, input: Input, ticks: usize) -> Vec<(usize, usize)> {
    (0..ticks)
        .map(|_| {
            g.update(1.0 / 30.0, &input);
            let a = g.avatar.as_ref().unwrap();
            (a.action(), a.dir)
        })
        .collect()
}

/// The footsteps heard since last asked.
/// (Footsteps come apart from other sounds: the host plays them quieter, or not at all.)
fn steps(g: &mut Game) -> Vec<u16> {
    let steps = g.take_events().into_iter().filter_map(|e| match e {
        u8engine::usecode::UiEvent::Step { sfx, item: AVATAR, run } => Some((sfx, run)),
        u8engine::usecode::UiEvent::Sfx { sfx, .. } if [0x90, 0x97].contains(&sfx) => panic!("a step as an effect"),
        _ => None,
    });
    steps
        .map(|(s, run)| {
            assert_eq!(run, s == 0x90, "running or walking");
            s
        })
        .collect()
}

#[test]
#[ignore]
fn walking_and_running_go_at_the_originals_pace() {
    let Some(mut g) = plateau(EAST) else { return };
    let x = at(&g).0;
    play(&mut g, Input { dir: Some(EAST), ..Input::default() }, 60);
    let walked = at(&g).0 - x;
    assert!((740..=820).contains(&walked), "a walk: 208 every 16 ticks ({walked} in 2 s)");
    // Footsteps (ANIMCALL.C): on the plateau's ground, a walk's; then a run's.
    let walk = steps(&mut g);
    assert!((7..=8).contains(&walk.len()) && walk.iter().all(|&s| s == 0x97), "two a cycle: {walk:?}");
    // A run (after a walk's first step): 408 every 18 ticks.
    let Some(mut g) = plateau(EAST) else { return };
    let run = Input { dir: Some(EAST), run: true, ..Input::default() };
    let acts = play(&mut g, run, 16);
    assert_eq!((acts[0].0, acts[15].0), (action::WALK, action::RUN), "walks off, then runs");
    let x = at(&g).0;
    play(&mut g, run, 36);
    let ran = at(&g).0 - x;
    assert!((760..=870).contains(&ran), "two runs' cycles, 9 frames each: {ran} in 36 ticks");
    assert!(steps(&mut g).ends_with(&[0x90; 4]));
    // Let go: a walking step, then he stands.
    let stop = play(&mut g, Input::default(), 30);
    let first = stop.iter().find(|a| a.0 != action::RUN).unwrap().0;
    assert_eq!((first, stop[29].0), (action::WALK, action::STAND));
}

#[test]
#[ignore]
fn turning_about_looks_over_the_shoulder() {
    let Some(mut g) = plateau(6) else { return };
    let here = at(&g);
    let acts = play(&mut g, Input { dir: Some(EAST), ..Input::default() }, 40);
    // West to east, clockwise: a look right facing 6, 7, 0 and 1 (3 ticks each), standing east.
    let mut shown: Vec<_> = acts.clone();
    shown.dedup();
    assert_eq!(shown[..5], [(33, 6), (33, 7), (33, 0), (33, 1), (action::STAND, EAST)], "looking round: {shown:?}");
    let moved = acts.iter().position(|a| a.0 == action::WALK).unwrap();
    assert!(moved >= 12, "no step till turned, four looks of three ticks ({moved})");
    assert_ne!(at(&g), here);
    // Moving, one way round is no turn: the step goes that way at once.
    let acts = play(&mut g, Input { dir: Some(3), ..Input::default() }, 10);
    assert!(acts.iter().all(|a| a.0 == action::WALK), "{acts:?}");
    // The combat stance turns about in the stance.
    g.world.set_in_combat(AVATAR, true);
    play(&mut g, Input::default(), 30);
    let facing = g.avatar.as_ref().unwrap().dir;
    let acts = play(&mut g, Input { dir: Some((facing + 2) % 8), ..Input::default() }, 20);
    assert!(acts[..6].iter().all(|a| a.0 == action::COMBAT_STAND), "{acts:?}");
    assert_eq!(acts[19].0, action::ADVANCE);
    // Pointing behind, fighting, he backs off.
    let acts = play(&mut g, Input { dir: Some((facing + 6) % 8), ..Input::default() }, 10);
    assert!(acts.iter().any(|a| a.0 == action::RETREAT), "{acts:?}");
}

/// Running into a wall (ANIMPRIM.C's initialize): a run that can't go is tried as a walk, then a
/// careful step, each a little to either side and a way to either side; none can, so a careful
/// step that fails, and held on, he just stands: no hurt. A running leap into it hurts one time
/// in three. A careful step at the wall the same: no move, and again, only standing.
#[test]
#[ignore]
fn running_into_a_wall_doesnt_hurt_but_leaping_into_it_does() {
    // Mythran's house, north of the plateau: its wall at y 2132.
    let Some(mut g) = saved(10) else { return };
    let (mut hurt, trials) = (0, 18);
    for leap in [false, true] {
        for k in 0..trials {
            g.put_avatar(8, (2805, 2420, 0));
            g.avatar.as_mut().unwrap().dir = 0;
            let hp = g.stats.get(AVATAR).unwrap().hp;
            let run = Input { dir: Some(0), run: true, ..Input::default() };
            let acts: Vec<_> =
                (0..60).flat_map(|t| play(&mut g, Input { jump: leap && t >= 8 + k % 4, ..run }, 1)).collect();
            assert!((2100..=2132).contains(&at(&g).1), "at the wall: {}", at(&g).1);
            if !leap {
                assert_eq!(acts[59].0, action::STAND, "held on, he stands");
            }
            hurt += (g.stats.get(AVATAR).unwrap().hp < hp) as usize;
            g.stats.get_mut(AVATAR).unwrap().hp = hp;
            play(&mut g, Input::default(), 30);
        }
        if !leap {
            assert_eq!(hurt, 0, "running into it never hurts");
        }
    }
    assert!((2..=12).contains(&hurt), "leaping into it: {hurt} of {trials}");
    // A careful step where the run stopped: nowhere, and held on, standing.
    g.put_avatar(8, (2805, 2420, 0));
    g.avatar.as_mut().unwrap().dir = 0;
    play(&mut g, Input { dir: Some(0), run: true, ..Input::default() }, 60);
    play(&mut g, Input::default(), 30);
    let here = at(&g);
    let acts = play(&mut g, Input { dir: Some(0), careful: true, ..Input::default() }, 60);
    assert_eq!(at(&g), here);
    assert!(acts[30..].iter().all(|a| a.0 == action::STAND), "{acts:?}");
}

/// Held, one move follows another on time, whatever the frame rate: a run's frames come every
/// two ticks (its first frame, standing still, as in the original), the same at 45, 60 or 144
/// updates a second.
#[test]
#[ignore]
fn moves_follow_on_time_at_any_frame_rate() {
    for fps in [45.0f32, 60.0, 144.0] {
        let Some(mut g) = plateau(EAST) else { return };
        let run = Input { dir: Some(EAST), run: true, ..Input::default() };
        let (mut last, mut was, mut gaps) = (0.0, at(&g).0, vec![]);
        for n in 1..(fps * 3.0) as usize {
            g.update(1.0 / fps, &run);
            let now = n as f32 / fps;
            if at(&g).0 != was {
                (gaps, last, was) = ([gaps, vec![now - last]].concat(), now, at(&g).0);
            }
        }
        let gaps = &gaps[gaps.len() - 16..];
        assert!(gaps.iter().all(|&t| (0.06..0.073).contains(&t) || (0.125..0.14).contains(&t)), "{fps}: {gaps:?}");
    }
}

/// Where a jump from the plateau lands (how far east, how high), `run_up` ticks of running before
/// it (0: standing); from a box to the top of another `aim` units east if any, aimed at its
/// middle. And whether he shrugged first.
fn jump(run_up: usize, aim: Option<i32>, strength: Option<u8>) -> Option<(i32, i32, bool)> {
    let mut g = plateau(EAST)?;
    if let Some(s) = strength {
        g.stats.get_mut(AVATAR)?.str = s;
    }
    let (x, y, _) = at(&g);
    let aim = aim.map(|d| {
        let shape = (0..2048)
            .find(|&s| {
                let t = g.data.typeflags.get(s);
                t.has(flag::SOLID) && t.has(flag::LAND) && t.dims() == (64, 64, 24)
            })
            .expect("a box");
        let put = |g: &mut Game, x| g.world.add(&g.data, Item { shape, x, y: y + 16, z: 0, ..Item::default() }, true);
        put(&mut g, x + 16);
        let on = put(&mut g, x + d + 32);
        g.put_avatar(8, (x, y, 24));
        Aim { at: (x + d, y - 16, 24), on }
    });
    let run = Input { dir: Some(EAST), run: true, ..Input::default() };
    play(&mut g, run, run_up);
    let (x, _, _) = at(&g);
    let acts = play(&mut g, Input { jump: true, aim, ..run }, 1);
    let acts = [acts, play(&mut g, Input::default(), 90)].concat();
    let j = if run_up > 0 { action::RUNNING_JUMP } else { action::JUMP };
    let shrug = acts.iter().any(|a| a.0 == 32);
    assert!(shrug || acts.iter().any(|a| a.0 == j), "jumped");
    let (to, _, z) = at(&g);
    Some((to - x, z, shrug))
}

#[test]
#[ignore]
fn a_running_leap_is_long_and_a_jump_goes_to_the_pointer() {
    // A running leap: half of ANIM 10, 648 forward, on from the run's step it's queued behind.
    let Some((leap, ..)) = jump(30, None, None) else { return };
    assert!((648..=1100).contains(&leap), "{leap}");
    // Standing: 384 forward; to the pointer, on something to land on, if he's strong enough (32
    // a point of STR); else only a shrug.
    assert_eq!(jump(0, None, None), Some((384, 0, false)));
    // (Each frame's share rounded down, as the original's: a little short.)
    let (to, z, shrug) = jump(0, Some(200), None).unwrap();
    assert!((192..=200).contains(&to) && z == 24 && !shrug, "box to box: {to}, {z}");
    // Too weak for it: only a shrug (ANIM.C: the jump's doAnim finds him busy shrugging).
    assert_eq!(jump(0, Some(200), Some(3)), Some((0, 24, true)), "too weak for it");
}

#[test]
#[ignore]
fn swings_queue_two_at_most_and_a_blow_in_the_air_brings_him_down() {
    let Some(mut g) = plateau(EAST) else { return };
    // Five clicks at once in the stance: the swing playing and one more (ANIM.C avatarSwing).
    g.world.set_in_combat(AVATAR, true);
    play(&mut g, Input::default(), 40);
    let mut acts = vec![];
    for _ in 0..5 {
        acts.extend(play(&mut g, Input { attack: true, ..Input::default() }, 1));
    }
    acts.extend(play(&mut g, Input::default(), 120));
    let mut runs: Vec<_> = acts.iter().map(|a| a.0).collect();
    runs.dedup();
    let swings = runs.iter().filter(|&&a| a == action::ATTACK).count();
    assert_eq!(swings, 2, "{acts:?}");
    // Struck hard at the top of a jump straight up: no stagger, and down he comes.
    let Some(mut g) = plateau(EAST) else { return };
    play(&mut g, Input { jump: true, ..Input::default() }, 1);
    play(&mut g, Input::default(), 14);
    assert!(at(&g).2 > 8, "up");
    g.receive_hit(AVATAR, 0, 0, 8, 0x80);
    play(&mut g, Input::default(), 60);
    assert_eq!(at(&g).2, 0, "back down");
}

/// His steady pace, for a view that keeps its own: a held run east is the whole cycle's 408
/// units (both steps and the still frame) over its 18 ticks, a walk 208 over 16; none standing,
/// turning about, or once the button's let go.
#[test]
#[ignore]
fn the_steady_pace_of_a_walk_and_a_run() {
    let Some(mut g) = plateau(EAST) else { return };
    let pace = |g: &Game| g.avatar.as_ref().unwrap().pace(&g.data, &g.world);
    assert_eq!(pace(&g), None, "standing");
    play(&mut g, Input { dir: Some(EAST), run: true, ..Default::default() }, 20);
    let ((vx, vy), cycle) = pace(&g).expect("running");
    assert!((vx - 680.0).abs() < 0.5 && vy == 0.0 && (cycle - 0.6).abs() < 1e-4, "{vx} {vy} {cycle}");
    play(&mut g, Input::default(), 30);
    assert_eq!(pace(&g), None, "let go");
    play(&mut g, Input { dir: Some(EAST), ..Default::default() }, 12);
    let ((vx, _), cycle) = pace(&g).expect("walking");
    assert!((vx - 390.0).abs() < 0.5 && (cycle - 16.0 / 30.0).abs() < 1e-4, "{vx} {cycle}");
    play(&mut g, Input::default(), 30);
    let turns = play(&mut g, Input { dir: Some(6), ..Default::default() }, 2);
    assert!(turns.iter().all(|&(a, _)| a != action::WALK), "turning about first: {turns:?}");
    assert_eq!(pace(&g), None, "turning");
}
