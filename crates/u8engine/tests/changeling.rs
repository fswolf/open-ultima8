//! The changeling (shape 357) as the original's combat brain has it (COMBATBR.C): it hides as a
//! tree when the Avatar is far, and takes the Avatar's shape to fight.
mod common;
use common::*;

const CHANGELING: u32 = 357;
const INTO_TREE: usize = 20;
const OUT_OF_TREE: usize = 21;
const INTO_AVATAR: usize = 22;
const OUT_OF_AVATAR: usize = 23;

/// Slot 1 with the Avatar on `map` at `at` and a changeling hatched from the egg at `egg` (its
/// monster egg: shape 500): its id.
fn hatched(map: u16, at: (i32, i32, i32), egg: (i32, i32)) -> Option<(Game, u16)> {
    let mut g = saved(1)?;
    g.put_avatar(map, at);
    let egg = g.world.objects().find(|&id| g.world.get(id).is_some_and(|it| it.shape == 500 && (it.x, it.y) == egg))?;
    let before: Vec<u16> = (2..256).filter(|&id| g.world.get(id).is_some_and(|it| it.shape == CHANGELING)).collect();
    g.call_intrinsic(0x79, &egg.to_le_bytes());
    tick(&mut g, 1);
    let c = (2..256).find(|&id| !before.contains(&id) && g.world.get(id).is_some_and(|it| it.shape == CHANGELING))?;
    Some((g, c))
}

fn tick(g: &mut Game, n: usize) {
    for _ in 0..n {
        g.update(1.0 / 30.0, &Input::default());
        answer(g);
    }
}

/// Play on until `what` holds (up to `secs`); the ticks it took.
fn until(g: &mut Game, secs: usize, what: impl Fn(&Game) -> bool) -> Option<usize> {
    (0..secs * 30).find(|_| {
        tick(g, 1);
        what(g)
    })
}

fn shape(g: &Game, id: u16) -> u32 {
    g.world.get(id).map_or(0, |it| it.shape)
}

/// Put the Avatar `d` from the changeling along x (and half of it along y, keeping it in the
/// original's fast area at 2048), on the floor there.
fn avatar_off(g: &mut Game, c: u16, d: i32) {
    let it = g.world.get(c).unwrap();
    let map = g.map();
    let z = (0..=120)
        .step_by(8)
        .find(|&z| g.put_avatar(map, (it.x + d, it.y + d / 2, z)) && g.world.floor_under(AVATAR) == Some(z));
    assert!(z.is_some(), "a floor {d} off");
}

/// Near the Avatar with no quarry, it becomes him at once; it fights in his shape; when its
/// time is up it turns back. Near with a quarry, about 1 in 8 of its thoughts become him.
#[test]
#[ignore]
fn near_the_avatar_it_takes_his_shape() {
    let Some((mut g, c)) = hatched(43, (15487, 16127, 16), (15487, 15999)) else { return };
    // (No quarry: hatched fighting, but it never looks for one in its own shape.)
    g.world.restore_target(c, Default::default());
    g.world.set_in_combat(c, true);
    let into = until(&mut g, 3, |g| g.world.last_anim(c) == INTO_AVATAR);
    assert!(into.is_some(), "into the Avatar");
    let became = until(&mut g, 5, |g| shape(g, c) == 1);
    assert!(became.is_some(), "the Avatar's shape");
    let it = g.world.get(c).unwrap();
    assert_eq!((it.frame, g.world.last_dir(c)), (0, Some(0)), "first frame, facing north");
    assert_eq!(g.world.get(AVATAR).unwrap().shape, 1);
    // As him it finds the Avatar (within 600) and fights.
    let quarry = until(&mut g, 5, |g| g.world.target(c).now == AVATAR);
    assert!(quarry.is_some(), "after the Avatar");
    // 5 to 25 seconds, then back.
    let back = until(&mut g, 30, |g| g.world.last_anim(c) == OUT_OF_AVATAR);
    let back = back.expect("back into itself") + into.unwrap() + became.unwrap();
    assert!((5 * 30..=26 * 30).contains(&back), "{back} ticks");
    assert_eq!(shape(&g, c), CHANGELING);
}

/// With the Avatar its quarry and near, it fights in its own shape and only some of the time
/// turns into him: about 1 in 8 of its turns to think.
#[test]
#[ignore]
fn with_a_quarry_it_shifts_one_time_in_eight() {
    let (mut shifted, mut runs) = (0, 0);
    for run in 0..16 {
        let Some((mut g, c)) = hatched(43, (15487, 16127, 16), (15487, 15999)) else { return };
        (0..run * 5).for_each(|_| _ = g.world.roll(2)); // (another throw of the dice each time)
        g.world.set_target(c, AVATAR);
        runs += 1;
        // (A few turns: a turn is a swing and its pause, a walk, a turn about.)
        if until(&mut g, 4, |g| g.world.last_anim(c) == INTO_AVATAR).is_some() {
            shifted += 1;
        }
    }
    assert!((1..runs).contains(&shifted), "{shifted} of {runs}");
}

/// Far from the Avatar (2048, not the pentagram): a tree, dead to the world, till a victim
/// comes within 160 or its time runs out; out again and alive. On the pentagram never a tree.
#[test]
#[ignore]
fn far_from_the_avatar_it_hides_as_a_tree() {
    let Some((mut g, c)) = hatched(5, (8447, 15359, 48), (8447, 15231)) else { return };
    g.world.restore_target(c, Default::default());
    avatar_off(&mut g, c, 2048);
    tick(&mut g, 12);
    assert!(g.world.is_fast(c), "still in the fast area");
    until(&mut g, 3, |g| g.world.last_anim(c) == INTO_TREE).expect("into a tree");
    assert!(g.stats.get(c).unwrap().dead, "dead while a tree");
    // Struck, nothing; double-clicked, searched like a body.
    let hp = g.stats.get(c).unwrap().hp;
    g.receive_hit(c, AVATAR, 0, 10, 1);
    assert_eq!(g.stats.get(c).unwrap().hp, hp);
    tick(&mut g, 60);
    assert_eq!(g.world.last_anim(c), INTO_TREE, "waits");
    // The Avatar comes within 160: out it comes, alive, after him.
    let it = g.world.get(c).unwrap();
    let map = g.map();
    assert!((0..=96)
        .step_by(8)
        .any(|z| g.put_avatar(map, (it.x + 150, it.y, z)) && g.world.floor_under(AVATAR) == Some(z)));
    until(&mut g, 2, |g| g.world.last_anim(c) == OUT_OF_TREE).expect("out of the tree");
    assert!(!g.stats.get(c).unwrap().dead);
    assert_eq!(g.world.target(c).now, AVATAR);
    // Its time runs out (25 to 45 s) with nobody about: out too.
    let Some((mut g, c)) = hatched(5, (8447, 15359, 48), (8447, 15231)) else { return };
    g.world.restore_target(c, Default::default());
    avatar_off(&mut g, c, 2048);
    let into = until(&mut g, 3, |g| g.world.last_anim(c) == INTO_TREE).unwrap();
    let out = until(&mut g, 50, |g| g.world.last_anim(c) == OUT_OF_TREE).expect("out on time") + into;
    assert!((25 * 30..=46 * 30).contains(&out), "{out}");
    assert!(!g.stats.get(c).unwrap().dead);
    // The pentagram: never a tree, the Avatar there or far.
    let Some((mut g, c)) = hatched(43, (15487, 16127, 16), (15487, 15999)) else { return };
    g.world.restore_target(c, Default::default());
    avatar_off(&mut g, c, 2048);
    let trees = (0..20 * 30).filter(|_| {
        tick(&mut g, 1);
        g.world.last_anim(c) == INTO_TREE
    });
    assert_eq!(trees.count(), 0, "no tree at the pentagram");
}

/// Killed as itself it leaves the original's treasure (coins, oil, gems: its wealth); killed
/// as the Avatar, nothing (no case for shape 1).
#[test]
#[ignore]
fn what_it_leaves() {
    let near = |g: &Game, c: u16| {
        let it = g.world.get(c).unwrap();
        g.world
            .area(it.x, it.y, 256)
            .into_iter()
            .filter(|&t| {
                t != c && g.world.get(t).is_some_and(|i| [143, 592, 579].contains(&i.shape)) && !g.world.is_void(t)
            })
            .count()
            + g.world.contents(c).len()
    };
    let (mut itself, mut clone) = (0, 0);
    for _ in 0..12 {
        let Some((mut g, c)) = hatched(43, (15487, 16127, 16), (15487, 15999)) else { return };
        g.world.restore_target(c, Default::default());
        let before = near(&g, c);
        g.receive_hit(c, AVATAR, 0, 999, 0x80);
        tick(&mut g, 60);
        itself += near(&g, c) - before;
        let Some((mut g, c)) = hatched(43, (15487, 16127, 16), (15487, 15999)) else { return };
        g.world.restore_target(c, Default::default());
        until(&mut g, 5, |g| shape(g, c) == 1).unwrap();
        let before = near(&g, c);
        g.receive_hit(c, AVATAR, 0, 999, 0x80);
        tick(&mut g, 60);
        clone += near(&g, c) - before;
    }
    assert!(itself > 0, "treasure");
    assert_eq!(clone, 0, "nothing from the clone");
}

/// A save and load keeps where it is in its changes: itself, the Avatar (and its time left), a
/// tree (dead, and its time), and the middle of a change (the change completes).
#[test]
#[ignore]
fn saves_keep_its_changes() {
    let reload = |g: &Game| {
        let root = PathBuf::from(std::env::var("U8_DATA").unwrap());
        let gd = GameData::load_save(&root, Some(&g.snapshot())).unwrap();
        let scripts = Scripts::load(&root, &gd).ok();
        let mut h = Game::new(gd, scripts, Some(g.map())).unwrap();
        assert!(h.apply_extra(&g.extra(1), 1));
        h
    };
    // The Avatar's shape, its time running: still him, then back on time.
    let Some((mut g, c)) = hatched(43, (15487, 16127, 16), (15487, 15999)) else { return };
    g.world.restore_target(c, Default::default());
    until(&mut g, 2, |g| g.world.last_anim(c) == INTO_AVATAR).unwrap();
    let mut h = reload(&g); // mid-change
    until(&mut h, 3, |h| shape(h, c) == 1).expect("the change completes");
    let mut k = reload(&h);
    assert_eq!(shape(&k, c), 1);
    until(&mut k, 30, |k| k.world.last_anim(c) == OUT_OF_AVATAR).expect("back on time");
    // A tree: dead and waiting, then out.
    let Some((mut g, c)) = hatched(5, (8447, 15359, 48), (8447, 15231)) else { return };
    g.world.restore_target(c, Default::default());
    avatar_off(&mut g, c, 2048);
    until(&mut g, 3, |g| g.world.last_anim(c) == INTO_TREE).unwrap();
    tick(&mut g, 60);
    let mut h = reload(&g);
    assert!(h.stats.get(c).unwrap().dead);
    until(&mut h, 50, |h| h.world.last_anim(c) == OUT_OF_TREE).expect("out of the tree after a load");
    assert!(!h.stats.get(c).unwrap().dead);
}

/// The East Road's changeling-shaped props (map 5: trees and rocks) are only things.
#[test]
#[ignore]
fn the_east_road_props_stay_props() {
    let Some(mut g) = saved(1) else { return };
    g.put_avatar(5, (11300, 12900, 8));
    let props = [20745, 20755, 20756, 20757, 20763];
    let was: Vec<_> = props.iter().map(|&id| g.world.get(id)).collect();
    tick(&mut g, 300);
    for (i, &id) in props.iter().enumerate() {
        assert_eq!(
            g.world.get(id).map(|it| (it.shape, it.frame, it.x, it.y)),
            was[i].map(|it| (it.shape, it.frame, it.x, it.y))
        );
        assert!(!g.world.in_combat(id));
    }
    g.trigger(20745, u8engine::usecode::event::LOOK);
    tick(&mut g, 1);
}
