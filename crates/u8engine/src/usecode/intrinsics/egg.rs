//! `Egg::` intrinsics. Eggs keep their trigger range in the record's npc byte (x << 4 | y, in
//! 32-unit cells) and their id in its map byte.
use super::Ctx;

pub fn x_range(c: &mut Ctx) -> u32 {
    c.item().map_or(0, |i| (i.npcnum >> 4) as u32)
}
pub fn y_range(c: &mut Ctx) -> u32 {
    c.item().map_or(0, |i| (i.npcnum & 0xF) as u32)
}
pub fn set_x_range(c: &mut Ctx) -> u32 {
    let r = (c.arg2(4) & 0xF) as u8;
    c.update(|i| i.npcnum = (i.npcnum & 0x0F) | r << 4);
    0
}
pub fn set_y_range(c: &mut Ctx) -> u32 {
    let r = (c.arg2(4) & 0xF) as u8;
    c.update(|i| i.npcnum = (i.npcnum & 0xF0) | r);
    0
}
pub fn id(c: &mut Ctx) -> u32 {
    c.item().map_or(0, |i| i.mapnum as u32)
}
pub fn set_id(c: &mut Ctx) -> u32 {
    let v = c.arg2(4) as u8;
    c.update(|i| i.mapnum = v);
    0
}
/// hatch: the monster egg's creature appears (if there's room). Scripts don't use the answer.
pub fn monster_hatch(c: &mut Ctx) -> u32 {
    if let Some(egg) = c.item() {
        c.vm.requests.push(crate::usecode::Request::Hatch(egg));
    }
    0
}

/// Monster eggs: the low 3 bits of the map byte are the spawned monster's activity, the rest an id.
pub fn monster_id(c: &mut Ctx) -> u32 {
    c.item().map_or(0, |i| (i.mapnum >> 3) as u32)
}
