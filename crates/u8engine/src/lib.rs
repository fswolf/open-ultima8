//! Game state and rules for Ultima 8, independent of any window, renderer or audio device.
//!
//! - [`world`]: the live map — items with world boxes, object ids, area queries, collision.
//! - [`avatar`]: player movement (walk, run, jump) driven by an [`avatar::Input`].
//! - [`eggs`]: invisible triggers — usecode events and teleporters — fired by the Avatar's position.
//! - [`fast_area`]: the live neighbourhood of the Avatar; items entering/leaving it get events.
//! - [`npc`]: NPCs walking where their scripts send them.
//! - [`stats`]: people's strength, hit points and the rest (NPCDATA); [`combat`]: hitting them.
//! - [`usecode`]: the VM that runs the game's own scripts, and its intrinsics.
//! - [`film`]: the intro and ending films (SKF).
//! - [`game`]: a session tying them together, advanced with [`game::Game::update`].
pub mod avatar;
pub mod camera;
pub mod combat;
pub mod eggs;
pub mod equip;
mod extra;
pub mod fade;
pub mod fast_area;
mod fight;
pub mod film;
mod fireball;
pub mod game;
pub mod geom;
mod gravity;
mod heal;
pub mod kit;
pub mod monsters;
pub mod npc;
pub mod path;
mod peace;
pub mod persist;
pub(crate) mod spawn;
mod sprites;
pub mod stats;
mod touch;
pub mod usecode;
pub mod world;

pub use game::{Game, Place};
pub use u8data;
pub use world::{ObjId, World};
