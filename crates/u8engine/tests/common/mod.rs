//! Shared by the behaviour tests (each file is its own test crate).
#![allow(dead_code)]
pub use std::path::PathBuf;
pub use u8data::GameData;
pub use u8engine::{avatar::Input, usecode::Scripts, world::AVATAR, Game};

pub fn game(map: u16) -> Option<Game> {
    let dir = PathBuf::from(std::env::var("U8_DATA").ok()?);
    let gd = GameData::load(&dir).unwrap();
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
