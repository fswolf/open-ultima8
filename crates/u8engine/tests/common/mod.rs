//! Shared by the behaviour tests (each file is its own test crate).
#![allow(dead_code)]
pub use std::path::PathBuf;
pub use u8data::GameData;
pub use u8engine::{avatar::Input, usecode::Scripts, world::AVATAR, Game};

pub fn game(map: u16) -> Option<Game> {
    let dir = PathBuf::from(std::env::var("U8_DATA").ok()?);
    let gd = GameData::load_fixed(&dir).unwrap();
    let scripts = Scripts::load(&dir, &gd).ok();
    Some(Game::new(gd, scripts, Some(map)).unwrap())
}

/// Play along with the scripts: speech is read at once, questions get their last answer ("Goodbye.").
pub fn answer(g: &mut Game) {
    use u8engine::usecode::UiEvent;
    for e in g.take_events() {
        let vm = g.vm().unwrap();
        match e {
            UiEvent::Bark { pid, .. } => vm.finish(pid, 0),
            UiEvent::Ask { pid, options, .. } => vm.answer(pid, &options, options.len() - 1),
            _ => {}
        }
    }
}

/// The test saves' backpack is over full (a bag inside it: 188 of 175, so the original, and we,
/// won't take more): put that bag down by the Avatar to make room. Returns it.
pub fn make_room(g: &mut Game) -> Option<u16> {
    let pack = *g.world.contents(AVATAR).iter().find(|&&id| g.world.get(id).is_some_and(|it| it.shape == 529))?;
    let bag = *g.world.contents(pack).iter().find(|&&id| g.world.get(id).is_some_and(|it| it.shape == 529))?;
    let a = g.world.get(AVATAR)?;
    g.world.move_to_map(&g.data, bag, (a.x, a.y + 64, a.z));
    Some(bag)
}

/// Saved game `slot` (the scripts loaded).
pub fn saved(slot: u32) -> Option<Game> {
    let dir = PathBuf::from(std::env::var("U8_DATA").ok()?);
    let mut g = Game::open(&dir, Some(slot), None).ok()?;
    if g.scripts.is_none() {
        g.scripts = Scripts::load(&dir, &g.data).ok();
    }
    Some(g)
}

/// Run `secs`, showing each line of speech for as long as the client would. Returns what was said.
pub fn play(g: &mut Game, secs: u32) -> Vec<(u16, String)> {
    let (mut said, mut up) = (vec![], vec![]);
    for _ in 0..secs * 30 {
        g.update(1.0 / 30.0, &Input::default());
        for e in g.take_events() {
            if let u8engine::usecode::UiEvent::Bark { pid, item, text } = e {
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
