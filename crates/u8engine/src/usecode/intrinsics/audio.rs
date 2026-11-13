//! Sound intrinsics. The VM only queues [`UiEvent`]s for the host to play, and remembers what's
//! playing (from the effects' lengths) so scripts can wait for a sound to end.
use super::Ctx;
use crate::usecode::vm::UiEvent;
use crate::world::ObjId;

fn start(c: &mut Ctx, sfx: u16, item: ObjId, looping: bool) -> u32 {
    let len = c.vm.sfx_ticks.get(sfx as usize).copied().unwrap_or(0);
    let until = if looping { u32::MAX } else { c.vm.ticks.saturating_add(len) };
    c.vm.sounds.retain(|&(s, i, _)| (s, i) != (sfx, item));
    c.vm.sounds.push((sfx, item, until));
    c.vm.events.push_back(UiEvent::Sfx { sfx, item, looping });
    0
}

/// playSFX(sfx[, priority])
pub fn play(c: &mut Ctx) -> u32 {
    start(c, c.arg2(0), 0, false)
}
/// playSFX(sfx, priority, item): positioned at the item.
pub fn play_at(c: &mut Ctx) -> u32 {
    start(c, c.arg2(0), c.arg2(4), false)
}
/// playAmbientSFX(sfx, priority, item): loops at the item until stopped.
pub fn play_ambient(c: &mut Ctx) -> u32 {
    start(c, c.arg2(0), c.arg2(4), true)
}
/// isSFXPlaying(sfx)
pub fn is_playing(c: &mut Ctx) -> u32 {
    let (sfx, now) = (c.arg2(0), c.vm.ticks);
    c.vm.sounds.retain(|&(_, _, until)| until > now);
    c.vm.sounds.iter().any(|&(s, _, _)| s == sfx) as u32
}
/// setVolumeSFX(sfx, volume)
pub fn set_volume(c: &mut Ctx) -> u32 {
    c.vm.events.push_back(UiEvent::SfxVolume { sfx: c.arg2(0), volume: c.arg1(2) });
    0
}
/// stopSFX(sfx): the one played without an item (ScummVM: object 0).
pub fn stop(c: &mut Ctx) -> u32 {
    let sfx = c.arg2(0);
    c.vm.sounds.retain(|&(s, i, _)| (s, i) != (sfx, 0));
    c.vm.events.push_back(UiEvent::StopSfx { sfx, item: Some(0) });
    0
}
/// stopSFX(sfx, item)
pub fn stop_at(c: &mut Ctx) -> u32 {
    let (sfx, item) = (c.arg2(0), c.arg2(2));
    c.vm.sounds.retain(|&(s, i, _)| (s, i) != (sfx, item));
    c.vm.events.push_back(UiEvent::StopSfx { sfx, item: Some(item) });
    0
}
