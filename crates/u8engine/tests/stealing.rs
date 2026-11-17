//! Taking what isn't the Avatar's, as the original answers it (claude/steal-compare.md): when the
//! player lets go of it, everyone about is told (event 19), and their scripts decide.
mod common;
use common::*;
use u8engine::usecode::UiEvent;
use u8engine::Place;

/// Run `secs`, showing each line of speech for as long as the client would. Returns what was said.
fn play(g: &mut Game, secs: u32) -> Vec<(u16, String)> {
    let (mut said, mut up) = (vec![], vec![]);
    for _ in 0..secs * 30 {
        g.update(1.0 / 30.0, &Input::default());
        for e in g.take_events() {
            if let UiEvent::Bark { pid, item, text } = e {
                up.push((pid, 60 + 2 * text.len() as u32));
                said.push((item, text.trim().to_string()));
            }
        }
        for (pid, left) in &mut up {
            *left -= 1;
            if *left == 0 {
                g.vm().unwrap().finish(*pid, 0);
            }
        }
        up.retain(|b| b.1 > 0);
    }
    said
}

/// Slot 1 at 10:00 on `map`, the Avatar at `at`.
fn at(map: u16, at: (i32, i32, i32)) -> Option<Game> {
    let mut g = saved(1)?;
    g.put_avatar(map, (0, 0, 0));
    g.set_minutes(600);
    g.put_avatar(map, at);
    Some(g)
}

fn says(said: &[(u16, String)], who: u16, line: &str) -> bool {
    said.iter().any(|(i, t)| *i == who && t.contains(line))
}

/// Korick's hammer dragged onto the Avatar, whose pack (as slot 1 has it) is full: the drop is
/// refused, but letting go of it in front of him is theft all the same, as in DOS: "Stop!
/// Thief!" and Beren comes. The hammer stays where it was.
#[test]
#[ignore]
fn a_refused_take_is_still_theft() {
    let Some(mut g) = at(39, (12313, 16463, 48)) else { return };
    let hammer = 30506;
    let was = g.world.get(hammer).map(|it| (it.x, it.y, it.z));
    let pack = g.backpack().unwrap();
    assert_eq!(g.move_item(hammer, Place::Into { container: pack, at: (20, 20) }), Err("There's no room for that."));
    assert_eq!(g.world.get(hammer).map(|it| (it.x, it.y, it.z)), was, "left where it was");
    let said = play(&mut g, 8);
    assert!(says(&said, 9, "Stop! Thief!") && says(&said, 32, "What seems to be the problem?"), "{said:?}");
}

/// Things of no weight (Devon's table, beds, doors, thrones) can't be moved at all, so nobody
/// minds: DOS leaves the table where it is and Devon says nothing.
#[test]
#[ignore]
fn tables_cant_be_dragged() {
    let Some(mut g) = at(39, (10367, 8511, 48)) else { return };
    let table = 30098;
    assert_eq!(g.can_pick_up(table), Err("You can't move that."));
    assert_eq!(g.move_item(table, Place::Map((10303, 8511, 48))), Err("You can't move that."));
    assert!(!play(&mut g, 3).iter().any(|(i, _)| *i == 2), "Devon says nothing");
}

/// Beren's things moved about his house four times: three warnings, then the verdict. His script
/// passes the variable of a search that has just run out; the original leaves 0 there (DOS:
/// "guilty of assault"), and so does our VM now (it left the last thing looked at: "theft").
#[test]
#[ignore]
fn berens_fourth_warning_is_assault() {
    let Some(mut g) = at(41, (19199, 6351, 48)) else { return };
    let thing = 31934;
    let mut said = vec![];
    for (k, to) in [(19119, 6447, 64), (19071, 6399, 64), (19119, 6447, 64), (19071, 6399, 64)].into_iter().enumerate()
    {
        g.move_item(thing, Place::Map(to)).unwrap_or_else(|e| panic!("move {k}: {e}"));
        said.extend(play(&mut g, 10));
    }
    for line in [
        "I represent the entire Cabal here.",
        "Do you know who I am?",
        "I will kill you if you diturb my things again.",
    ] {
        assert!(says(&said, 32, line), "{line}: {said:?}");
    }
    assert!(says(&said, 32, "Knave, I find you guilty of assault."), "{said:?}");
    assert!(!says(&said, 32, "theft"), "{said:?}");
}

/// The accuser answers Beren "That person is a thief!", as DOS does every time; class 1419 says
/// "This person" only for an accuser whose last action is standing. Korick (his hammer put in
/// the pack) talked last: the talk stays his last action, as the original's saves keep 53.
/// Corinth (a candle brought into his house and taken) was walking his round: the walk stays.
#[test]
#[ignore]
fn the_accuser_says_that_person() {
    let cases = [(39, 9, (12313, 16463, 48), 30506, None), (7, 15, (12709, 21721, 8), 24484, Some((12799, 21631, 32)))];
    for (map, who, stand, thing, bring) in cases {
        let Some(mut g) = at(map, stand) else { return };
        if let Some(p) = bring {
            g.world.move_to_map(&g.data, thing, p);
        }
        make_room(&mut g);
        let pack = g.backpack().unwrap();
        g.move_item(thing, Place::Into { container: pack, at: (20, 20) }).unwrap();
        let said = play(&mut g, 16);
        assert!(says(&said, who, "Stop! Thief!") && says(&said, 32, "What seems to be the problem?"), "{said:?}");
        assert!(says(&said, who, "That person is a thief!") && !says(&said, who, "This person"), "{said:?}");
    }
}

/// Part of a pile: who saw is told as the player lets go, before the count is asked (ScummVM
/// tells them before the slider opens), so taking none is noticed all the same; the take itself
/// doesn't tell them again. Coins put among Beren's things.
#[test]
#[ignore]
fn a_pile_is_noticed_before_the_count() {
    let Some(mut g) = at(41, (19199, 6351, 48)) else { return };
    let coins =
        (1..1024u32).find(|&s| g.data.typeflags.get(s).family == 2 && g.data.typeflags.get(s).weight > 0).unwrap();
    let it = u8data::map::Item { shape: coins, quality: 10, x: 19119, y: 6447, z: 64, ..Default::default() };
    let pile = g.world.add(&g.data, it, true);
    g.let_go(pile);
    assert_eq!(g.split_to(pile, 0, Place::Map((19071, 6399, 64))), Ok(()));
    let said = play(&mut g, 10);
    assert!(says(&said, 32, "I represent the entire Cabal here."), "{said:?}");
    let spot = [(19071, 6399, 64), (19087, 6447, 64), (19119, 6415, 64), (19151, 6447, 64), (19119, 6479, 64)]
        .into_iter()
        .find(|&p| g.world.fits(&g.data, pile, p))
        .expect("room by the pile");
    assert_eq!(g.split_to(pile, 5, Place::Map(spot)), Ok(()));
    let said = play(&mut g, 10);
    assert!(!said.iter().any(|(i, _)| *i == 32), "told once: {said:?}");
}
