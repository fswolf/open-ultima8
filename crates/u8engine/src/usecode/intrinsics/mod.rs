//! Native functions scripts call with `calli`. [`TABLE`] is the single list of every intrinsic we
//! know — number, name and handler — used for dispatch, disassembly and diagnostics.
//!
//! To add one: write `fn(&mut Ctx) -> u32` in the module for its domain and point its `TABLE`
//! row at it. Rows with no handler are known by name only; calling them returns 0 and is
//! counted in [`Vm::missing`](super::Vm::missing).
//!
//! Numbers are checked against argument sizes in the game's code; names follow ScummVM.
mod actor;
mod audio;
mod camera;
mod egg;
mod item;
mod misc;
mod palette;
mod ui;

use super::{
    host::Host,
    stack::{le16, Stack},
    vm::{Pid, Vm},
};
use crate::world::ObjId;
use u8data::{map::Item, usecode::Usecode};

/// What a handler gets: the VM, the calling process and its stack, raw argument bytes, and the world.
pub struct Ctx<'a> {
    pub(crate) vm: &'a mut Vm,
    pub(crate) pid: Pid,
    pub(crate) stack: &'a mut Stack,
    pub args: &'a [u8],
    pub usecode: &'a Usecode,
    pub host: &'a mut dyn Host,
}

impl Ctx<'_> {
    pub fn arg1(&self, o: usize) -> u8 {
        self.args.get(o).copied().unwrap_or(0)
    }
    pub fn arg2(&self, o: usize) -> u16 {
        self.args.get(o..o + 2).map_or(0, le16)
    }
    pub fn arg4(&self, o: usize) -> u32 {
        self.args.get(o..o + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    /// Read `n` bytes through a script pointer.
    pub fn read(&self, ptr: u32, n: usize) -> Vec<u8> {
        self.vm.read_ptr(self.pid, self.stack, ptr, n)
    }
    /// Write through a script pointer (used for out-parameters).
    pub fn write(&mut self, ptr: u32, v: &[u8]) {
        self.vm.write_ptr(self.pid, self.stack, ptr, v)
    }
    /// The object a member intrinsic was called on: the first argument points at its id.
    pub fn this(&self) -> ObjId {
        le16(&self.read(self.arg4(0), 2))
    }
    pub fn item(&self) -> Option<Item> {
        self.host.item(self.this())
    }
    /// The item whose id is the argument at `o`.
    pub fn item_arg(&self, o: usize) -> Option<Item> {
        self.host.item(self.arg2(o))
    }
    /// Change `this` (no-op if it doesn't exist).
    pub fn update(&mut self, f: impl FnOnce(&mut Item)) {
        let id = self.this();
        if let Some(mut it) = self.host.item(id) {
            f(&mut it);
            self.host.set_item(id, it);
        }
    }
    /// Footprint of an item in world units (x, y, z); a flipped one's x and y swap (ScummVM's
    /// getFootpadWorld).
    pub fn dims(&self, it: &Item) -> (i32, i32, i32) {
        let (x, y, z) = self.cells(it);
        (x * 32, y * 32, z * 8)
    }
    /// Footprint in TYPEFLAG cells (getFootpadData), flipped as [`dims`](Self::dims).
    pub fn cells(&self, it: &Item) -> (i32, i32, i32) {
        let tf = self.host.typeflag(it.shape);
        let (x, y, z) = ((tf[2] >> 4) as i32, (tf[3] & 0xF) as i32, (tf[3] >> 4) as i32);
        match it.flags & crate::world::item_flag::FLIPPED != 0 {
            true => (y, x, z),
            false => (x, y, z),
        }
    }
    pub fn family(&self, it: &Item) -> u8 {
        self.host.typeflag(it.shape)[1] >> 4
    }
    /// Where an item is on the map: a thing in a bag (or carried) is where its outermost
    /// container is (ScummVM's getLocationAbsolute).
    pub fn abs(&self, id: ObjId) -> Option<Item> {
        let mut id = id;
        for _ in 0..32 {
            match self.host.container(id) {
                Some(p) => id = p,
                None => break,
            }
        }
        self.host.item(id)
    }
    pub fn string(&self, id: u16) -> String {
        self.vm.heap.string(id)
    }
}

pub type Handler = fn(&mut Ctx) -> u32;

pub struct Intrinsic {
    pub num: u16,
    pub name: &'static str,
    pub run: Option<Handler>,
}

const fn known(num: u16, name: &'static str) -> Intrinsic {
    Intrinsic { num, name, run: None }
}
const fn imp(num: u16, name: &'static str, run: Handler) -> Intrinsic {
    Intrinsic { num, name, run: Some(run) }
}

/// Every intrinsic we know, sorted by number (numbers and names as ScummVM's u8_intrinsics.h).
pub static TABLE: &[Intrinsic] = &[
    imp(0x00, "TargetGump::target", misc::target),
    imp(0x02, "Item::touch", misc::no),
    imp(0x03, "Item::getX", item::get_x),
    imp(0x04, "Item::getY", item::get_y),
    imp(0x05, "Item::getZ", item::get_z),
    imp(0x06, "Item::getCX", item::get_cx),
    imp(0x07, "Item::getCY", item::get_cy),
    imp(0x08, "Item::getCZ", item::get_cz),
    imp(0x0C, "Item::getPoint", item::get_point),
    imp(0x0D, "Item::getShape", item::get_shape),
    imp(0x0E, "Item::setShape", item::set_shape),
    imp(0x0F, "Item::getFrame", item::get_frame),
    imp(0x10, "Item::setFrame", item::set_frame),
    imp(0x11, "Item::getQuality", item::get_quality),
    imp(0x12, "Item::getUnkEggType", item::get_unk_egg_type),
    imp(0x13, "Item::getQuantity", item::get_quantity),
    imp(0x15, "Item::getContainer", item::get_container),
    imp(0x16, "Item::getRootContainer", item::get_root_container),
    known(0x17, "Item::getGlobNum"),
    known(0x18, "Item::setGlobNum"),
    imp(0x19, "Item::getQ", item::get_q),
    imp(0x1A, "Item::setQ", item::set_q),
    imp(0x1B, "Item::setQuality", item::set_quality),
    known(0x1C, "Item::setUnkEggType"),
    imp(0x1D, "Item::setQuantity", item::set_quantity),
    imp(0x1E, "Item::getFamily", item::get_family),
    imp(0x1F, "Item::getTypeFlag", item::get_type_flag),
    imp(0x20, "Item::getStatus", item::get_status),
    imp(0x21, "Item::orStatus", item::or_status),
    imp(0x22, "Item::andStatus", item::and_status),
    imp(0x23, "Item::getFootpad", item::get_footpad),
    imp(0x25, "Item::overlaps", item::overlaps),
    imp(0x27, "Item::isOn", item::is_on),
    imp(0x2B, "Item::ascend", item::ascend),
    imp(0x2C, "Item::getWeight", item::get_weight),
    imp(0x2D, "Item::getWeightIncludingContents", item::get_total_weight),
    imp(0x2E, "Item::getSurfaceWeight", item::get_surface_weight),
    imp(0x31, "Item::legalCreateAtCoords", item::legal_create_at_coords),
    imp(0x32, "Item::create", item::create),
    imp(0x33, "Item::legalCreateAtPoint", item::legal_create_at_point),
    imp(0x34, "Item::legalCreateInCont", item::legal_create_in_cont),
    imp(0x35, "Item::push", item::push),
    imp(0x36, "Item::popToCoords", item::pop_to_coords),
    imp(0x37, "Item::popToContainer", item::pop_to_container),
    imp(0x38, "Item::pop", item::pop),
    imp(0x39, "Item::popToEnd", item::pop_to_container),
    imp(0x3A, "Item::destroy", item::destroy),
    known(0x3B, "Container::removeContents"),
    imp(0x3C, "Container::destroyContents", item::destroy_contents),
    imp(0x3D, "Item::isExplosive", item::is_explosive),
    imp(0x3E, "Item::move", item::move_to),
    imp(0x40, "Item::legalMoveToPoint", item::legal_move_to_point),
    imp(0x41, "Item::legalMoveToContainer", item::legal_move_to_container),
    imp(0x42, "Actor::isNPC", actor::is_npc),
    imp(0x44, "Item::hurl", item::hurl),
    imp(0x45, "Item::shoot", item::shoot),
    imp(0x46, "Item::fall", item::fall),
    imp(0x47, "Item::grab", item::grab),
    imp(0x49, "Item::bark", ui::bark),
    imp(0x4A, "Item::ask", ui::ask),
    imp(0x4B, "Item::getSliderInput", ui::slider),
    imp(0x4C, "Item::openGump", item::open_gump),
    imp(0x4D, "Item::closeGump", item::close_gump),
    imp(0x50, "Item::getMapArray", item::get_map_array),
    imp(0x51, "Item::getNpcArray", item::get_npc_array),
    imp(0x52, "Item::setMapArray", item::set_map_array),
    imp(0x53, "Item::receiveHit", item::receive_hit),
    imp(0x54, "Item::explode", item::explode),
    imp(0x55, "Item::canReach", item::can_reach),
    imp(0x56, "Item::getRange", item::get_range),
    imp(0x58, "Item::getDirToCoords", item::dir_to_coords),
    imp(0x59, "Item::getDirFromCoords", item::dir_from_coords),
    imp(0x5A, "Item::getDirToItem", item::dir_to_item),
    imp(0x5B, "Item::getDirFromItem", item::dir_from_item),
    imp(0x5C, "Item::look", item::look),
    imp(0x5D, "Item::use", item::use_item),
    imp(0x61, "Item::gotHit", item::got_hit),
    imp(0x67, "Item::enterFastArea", item::enter_fast_area),
    imp(0x6D, "Item::guardianBark", item::guardian_bark),
    imp(0x6E, "Book::read", ui::read_book),
    imp(0x6F, "Scroll::read", ui::read_scroll),
    imp(0x70, "Grave::read", ui::read_grave),
    imp(0x71, "Plaque::read", ui::read_plaque),
    imp(0x72, "Egg::getEggXRange", egg::x_range),
    imp(0x73, "Egg::getEggYRange", egg::y_range),
    imp(0x74, "Egg::setEggXRange", egg::set_x_range),
    imp(0x75, "Egg::setEggYRange", egg::set_y_range),
    imp(0x76, "Egg::getEggId", egg::id),
    imp(0x77, "Egg::setEggId", egg::set_id),
    imp(0x79, "MonsterEgg::hatch", egg::monster_hatch),
    imp(0x7A, "MonsterEgg::getMonId", egg::monster_id),
    imp(0x80, "Actor::isBusy", actor::is_busy),
    imp(0x81, "Actor::areEnemiesNear", actor::are_enemies_near),
    imp(0x82, "Actor::isInCombat", actor::is_in_combat),
    imp(0x83, "Actor::setInCombat", actor::set_in_combat),
    imp(0x84, "Actor::clrInCombat", actor::clr_in_combat),
    imp(0x85, "Actor::setTarget", actor::set_target),
    imp(0x86, "Actor::getTarget", actor::get_target),
    imp(0x87, "Actor::setAlignment", actor::set_alignment),
    imp(0x88, "Actor::getAlignment", actor::get_alignment),
    imp(0x89, "Actor::setEnemyAlignment", actor::set_enemy_alignment),
    imp(0x8A, "Actor::getEnemyAlignment", actor::get_enemy_alignment),
    imp(0x8B, "Actor::isEnemy", actor::is_enemy),
    imp(0x8C, "Actor::isDead", actor::is_dead),
    imp(0x8D, "Actor::setDead", actor::set_dead),
    imp(0x8E, "Actor::clrDead", actor::clr_dead),
    imp(0x8F, "Actor::isImmortal", actor::is_immortal),
    imp(0x90, "Actor::setImmortal", actor::set_immortal),
    imp(0x91, "Actor::clrImmortal", actor::clr_immortal),
    imp(0x92, "Actor::isWithstandDeath", actor::is_withstand_death),
    imp(0x93, "Actor::setWithstandDeath", actor::set_withstand_death),
    imp(0x94, "Actor::clrWithstandDeath", actor::clr_withstand_death),
    imp(0x95, "Actor::isFeignDeath", actor::is_feign_death),
    imp(0x96, "Actor::setFeignDeath", actor::set_feign_death),
    imp(0x97, "Actor::clrFeignDeath", actor::clr_feign_death),
    imp(0x9C, "Actor::getDir", actor::get_dir),
    imp(0x9D, "Actor::getMap", actor::get_map),
    imp(0x9E, "Actor::teleport", actor::teleport),
    imp(0x9F, "Actor::doAnim", actor::do_anim),
    imp(0xA0, "Actor::getLastAnimSet", actor::get_last_anim_set),
    imp(0xA1, "Actor::pathfindToPoint", actor::pathfind_to_point),
    imp(0xA2, "Actor::pathfindToItem", actor::pathfind_to_item),
    // 0xA3–0xAC: stats; setters confirmed by AVATAR's cheat setting STR/INT/DEX 21, HP/MANA 42.
    imp(0xA3, "Actor::getStr", actor::get_str),
    imp(0xA4, "Actor::getInt", actor::get_int),
    imp(0xA5, "Actor::getDex", actor::get_dex),
    imp(0xA6, "Actor::getHp", actor::get_hp),
    imp(0xA7, "Actor::getMana", actor::get_mana),
    imp(0xA8, "Actor::setStr", actor::set_str),
    imp(0xA9, "Actor::setInt", actor::set_int),
    imp(0xAA, "Actor::setDex", actor::set_dex),
    imp(0xAB, "Actor::setHp", actor::set_hp),
    imp(0xAC, "Actor::setMana", actor::set_mana),
    imp(0xAD, "Actor::createActor", actor::create_actor),
    imp(0xAE, "Actor::setActivity", actor::set_activity),
    imp(0xAF, "Actor::setAirWalkEnabled", actor::set_air_walk),
    imp(0xB0, "Actor::getAirWalkEnabled", actor::get_air_walk),
    imp(0xB1, "Actor::schedule", actor::schedule),
    imp(0xB2, "Actor::getEquip", actor::get_equip),
    imp(0xB3, "Actor::setEquip", actor::set_equip),
    imp(0xB4, "Ultima8Engine::closeItemGumps", misc::close_item_gumps),
    imp(0xB5, "Camera::scrollTo", camera::scroll_to),
    imp(0xB6, "random", misc::random),
    imp(0xB7, "rndRange", misc::rnd_range),
    imp(0xB8, "GrantPeaceProcess::castGrantPeace", misc::cast_grant_peace),
    imp(0xB9, "numToStr", misc::num_to_str),
    imp(0xBB, "MusicProcess::playMusic", misc::play_music),
    imp(0xBC, "MainActor::getName", misc::avatar_name),
    imp(0xBD, "Item::igniteChaos", item::ignite_chaos),
    imp(0xBE, "Camera::setCenterOn", camera::set_center_on),
    imp(0xBF, "Camera::moveTo", camera::move_to),
    imp(0xC6, "Camera::startQuake", misc::start_quake),
    imp(0xC7, "Camera::stopQuake", misc::stop_quake),
    imp(0xC8, "InverterProcess::invertScreen", palette::invert_screen),
    imp(0xCA, "Kernel::getNumProcesses", misc::num_processes),
    imp(0xCB, "Kernel::resetRef", misc::reset_ref),
    imp(0xCC, "MainActor::teleportToEgg", misc::teleport_to_egg),
    imp(0xCD, "Kernel::resetRef", misc::reset_ref),
    imp(0xCF, "getAvatarInStasis", misc::get_stasis),
    imp(0xD0, "setAvatarInStasis", misc::set_stasis),
    imp(0xD1, "Item::getEtherealTop", item::ethereal_top),
    imp(0xD2, "getCurrentTimerTick", misc::timer_tick),
    imp(0xD4, "CurrentMap::canExistAt", misc::can_exist_at),
    imp(0xD5, "SpriteProcess::createSprite", misc::create_sprite),
    imp(0xD6, "SpriteProcess::createSprite(repeats)", misc::create_sprite),
    imp(0xD7, "getFamilyOfType", misc::family_of_type),
    imp(0xD8, "getGameHours", misc::game_hours),
    imp(0xD9, "getTimeInMinutes", misc::time_in_minutes),
    imp(0xDA, "getTimeInSeconds", misc::time_in_seconds),
    imp(0xDB, "setTimeInGameHours", misc::set_time_in_game_hours),
    imp(0xDE, "PaletteFader::fadeToBlack", palette::to_black),
    imp(0xDF, "PaletteFader::fadeFromBlack", palette::from_black),
    imp(0xE0, "PaletteFader::fadeToPaletteTransform", palette::to_transform),
    imp(0xE1, "PaletteFader::lightningBolt", palette::lightning),
    imp(0xE2, "PaletteFader::fadeToWhite", palette::to_white),
    imp(0xE3, "PaletteFader::fadeFromWhite", palette::from_white),
    imp(0xE4, "Game::playEndgame", misc::play_endgame),
    imp(0xE5, "HealProcess::feedAvatar", misc::feed_avatar),
    imp(0xE6, "MainActor::accumulateStrength", misc::grow_str),
    imp(0xE7, "MainActor::accumulateIntelligence", misc::grow_int),
    imp(0xE8, "MainActor::accumulateDexterity", misc::grow_dex),
    imp(0xE9, "MainActor::clrAvatarInCombat", misc::clr_avatar_in_combat),
    imp(0xEA, "MainActor::setAvatarInCombat", misc::set_avatar_in_combat),
    imp(0xEB, "MainActor::isAvatarInCombat", misc::is_avatar_in_combat),
    imp(0xEC, "AudioProcess::playSFX", audio::play),
    imp(0xED, "AudioProcess::playSFX(priority)", audio::play),
    imp(0xEE, "AudioProcess::playSFX(item)", audio::play_at),
    imp(0xF1, "AudioProcess::playAmbientSFX", audio::play_ambient),
    imp(0xF2, "AudioProcess::isSFXPlaying", audio::is_playing),
    imp(0xF3, "AudioProcess::setVolumeSFX", audio::set_volume),
    imp(0xF4, "AudioProcess::stopSFX", audio::stop),
    imp(0xF5, "AudioProcess::stopSFX(item)", audio::stop_at),
    imp(0xF8, "MusicProcess::stopMusic", misc::stop_music),
    imp(0xFB, "FireballProcess::TonysBalls", misc::tonys_balls),
    imp(0xFC, "avatarCanCheat", misc::can_cheat),
    imp(0xFD, "makeAvatarACheater", misc::no), // (the original only remembers it)
    imp(0xFE, "true", misc::yes),
];

pub fn lookup(num: u16) -> Option<&'static Intrinsic> {
    TABLE.binary_search_by_key(&num, |i| i.num).ok().map(|k| &TABLE[k])
}

pub fn name(num: u16) -> Option<&'static str> {
    lookup(num).map(|i| i.name)
}

/// Dispatch a `calli`.
pub(crate) fn call(
    vm: &mut Vm,
    pid: Pid,
    stack: &mut Stack,
    num: u16,
    args: &[u8],
    usecode: &Usecode,
    host: &mut dyn Host,
) -> u32 {
    match lookup(num).and_then(|i| i.run) {
        Some(run) => run(&mut Ctx { vm, pid, stack, args, usecode, host }),
        None => {
            *vm.missing.entry(num).or_default() += 1;
            0
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn table_is_sorted_and_unique() {
        assert!(super::TABLE.windows(2).all(|w| w[0].num < w[1].num));
    }
}
