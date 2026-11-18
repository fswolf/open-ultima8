//! Striking people, as the original answers it (claude/attack-compare.md): each one struck, and
//! each of the same alignment nearby, runs its going-into-combat script.
mod common;
use common::*;
use u8engine::combat::dmg;

/// Slot 1 at 10:00, the Avatar beside `npc` on `map` with 200 hit points.
fn beside(map: u16, npc: u16) -> Option<Game> {
    let mut g = saved(1)?;
    g.put_avatar(map, (0, 0, 0));
    g.set_minutes(600);
    g.put_avatar(map, (0, 0, 0));
    let n = g.world.get(npc)?;
    let at = [96, 112, 128, 160]
        .iter()
        .flat_map(|r| [(0, 1), (1, 0), (0, -1), (-1, 0)].map(|(dx, dy)| (n.x + dx * r, n.y + dy * r, n.z)))
        .find(|&p| g.world.fits(&g.data, AVATAR, p))?;
    g.put_avatar(map, at);
    g.stats.get_mut(AVATAR)?.hp = 200;
    Some(g)
}

fn at(g: &Game, id: u16) -> (i32, i32) {
    g.world.get(id).map(|it| (it.x, it.y)).unwrap()
}

/// Jenna struck at the tavern: she cries for Beren, Orlok at the bar cries out too (but doesn't
/// come: the Avatar's on the far side of the counter from him, and nobody walks part way), Beren
/// comes, questions her, finds the Avatar guilty of assault and burns it. Salkind's scene too:
/// there the arm that ends it lands on a leg it lay beside when thrown, after a bounce off the
/// ceiling (each tick of a flight is its own move, as ScummVM's GravityProcess has it).
#[test]
#[ignore]
fn struck_townsfolk_bring_beren() {
    for (map, npc, also) in [(39, 24, Some(10)), (39, 10, Some(24)), (41, 3, None)] {
        let Some(mut g) = beside(map, npc) else { return };
        let other = also.map(|o| at(&g, o));
        g.receive_hit(npc, AVATAR, 0, 1, dmg::NORMAL);
        let mut said = play(&mut g, 12);
        // (While the Avatar lives: once it's dead there's nobody to fight, and they wander.)
        let moved = also.zip(other).map(|(o, was)| {
            let now = at(&g, o);
            (now.0 - was.0).abs().max((now.1 - was.1).abs())
        });
        said.extend(play(&mut g, 60));
        let by = |who: u16, line: &str| said.iter().any(|(i, t)| *i == who && t.contains(line));
        assert!(by(npc, "Beren") || by(npc, "Sorcerer") || by(npc, "hurt me"), "{npc} cries out: {said:?}");
        assert!(by(32, "What seems to be the problem?") && by(npc, "attacked me!"), "{npc}'s complaint: {said:?}");
        assert!(by(32, "Knave, I find you guilty of assault."), "{said:?}");
        assert!(g.avatar_dead(), "map {map}: burned");
        if let (Some(o), Some(d)) = (also, moved) {
            assert!(by(o, "Beren") || by(o, "Sorcerer"), "{o} standing by cries out too: {said:?}");
            assert!(d < 32, "{o} stays put (moved {d})");
        }
    }
}

/// A guard struck shouts "Guardsmen, to arms!" and fights; a hatched peasant 640 off, on the
/// guards' side but with other enemies (1/2 against the guard's 1/e), isn't called in (DOS: no
/// scene, the guard fights on alone).
#[test]
#[ignore]
fn only_their_own_kind_come() {
    let Some(mut g) = beside(41, 65) else { return };
    g.receive_hit(65, AVATAR, 0, 1, dmg::NORMAL);
    let said = play(&mut g, 4);
    assert!(said.iter().any(|(i, t)| *i == 65 && t.contains("Guardsmen, to arms!")), "{said:?}");
    assert!(g.world.in_combat(65) && !g.world.in_combat(66), "66 stays out");
    assert!(!said.iter().any(|(i, _)| *i == 66 || *i == 32), "{said:?}");
}

/// A Theurgist struck calls on Stratos and is gone: his script sends him to a random spot on map
/// 0 (DOS: (7325, 552) each time, its random numbers starting alike after a load), taken for
/// dead, hit points untouched, as all three end in DOS.
#[test]
#[ignore]
fn theurgists_flee() {
    let Some(mut g) = beside(37, 22) else { return };
    let hp = g.stats.get(22).unwrap().hp;
    g.receive_hit(22, AVATAR, 0, 0, dmg::NORMAL);
    let said = play(&mut g, 20);
    assert!(said.iter().any(|(i, t)| *i == 22 && t.contains("Stratos")), "{said:?}");
    let s = g.stats.get(22).unwrap();
    assert!(g.world.is_void(22) && s.dead && s.hp == hp, "gone, dead, unhurt");
}

/// Each tick of a flight is its own move (ScummVM's GravityProcess: a collideMove a tick): a
/// rock lying on a rat, thrown up, comes back down on it and squashes it. Its landing was only
/// matched against what it touched when thrown, so this hit was lost (and with it the arm that
/// ends Beren's burning when it fell on a leg it lay by).
#[test]
#[ignore]
fn what_comes_down_on_what_it_left_hits_it() {
    let Some(mut g) = saved(3) else { return };
    g.put_avatar(3, (9808, 2760, 48));
    let a = g.world.get(AVATAR).unwrap();
    let at = (a.x + 192, a.y, a.z);
    let egg = u8data::map::Item { x: at.0, y: at.1, z: at.2, quality: 806, mapnum: 2, ..Default::default() };
    let rat = g.hatch_monster(egg).expect("a rat");
    let r = g.world.get(rat).unwrap();
    let top = r.z + 8;
    let shape = (1..1024u32)
        .find(|&s| {
            let t = g.data.typeflags.get(s);
            t.weight > 3 && t.family == 0 && !t.has(u8data::typeflag::flag::FIXED) && t.x <= 2 && t.y <= 2 && t.z > 0
        })
        .unwrap();
    let rock = g.world.add(&g.data, u8data::map::Item { shape, x: r.x, y: r.y, z: top, ..Default::default() }, true);
    assert!(g.world.contacts(rock).contains(&rat), "lying on the rat");
    g.hurl(rock, (0, 0, 8), 4, None);
    for _ in 0..60 {
        g.update(1.0 / 30.0, &Input::default());
    }
    assert!(g.stats.get(rat).is_some_and(|s| s.dead), "squashed by what came down on it");
}

/// Mythran struck speaks the death spell: "Kal Vas Grav Corp!" strikes the Avatar for all its
/// hit points (receiveHit, then setDead in the same turn), and it dies.
#[test]
#[ignore]
fn mythrans_death_spell_kills() {
    let Some(mut g) = beside(8, 33) else { return };
    g.receive_hit(33, AVATAR, 0, 1, dmg::NORMAL);
    let said = play(&mut g, 20);
    assert!(said.iter().any(|(i, t)| *i == 33 && t.contains("Kal Vas Grav Corp!")), "{said:?}");
    assert!(g.avatar_dead(), "the Avatar dies of it");
}

/// Going into combat runs the script once: a guard struck twice shouts once; saved and loaded,
/// he fights on without shouting again (nor on coming back to the map, the fight over).
#[test]
#[ignore]
fn the_shout_comes_once() {
    let Some(mut g) = beside(41, 65) else { return };
    let shouts = |said: &[(u16, String)]| said.iter().filter(|(i, t)| *i == 65 && t.contains("to arms")).count();
    g.receive_hit(65, AVATAR, 0, 1, dmg::NORMAL);
    g.receive_hit(65, AVATAR, 0, 1, dmg::NORMAL);
    assert_eq!(shouts(&play(&mut g, 3)), 1);
    let dir = PathBuf::from(std::env::var("U8_DATA").unwrap());
    let gd = GameData::load_save(&dir, Some(&g.snapshot())).unwrap();
    let scripts = Scripts::load(&dir, &gd).ok();
    let mut back = Game::new(gd, scripts, None).unwrap();
    assert!(back.world.in_combat(65), "still fighting after a load");
    assert_eq!(shouts(&play(&mut back, 3)), 0, "no second shout on loading");
    back.load_map(40);
    back.load_map(41);
    assert_eq!(shouts(&play(&mut back, 3)), 0, "nor on coming back");
}
