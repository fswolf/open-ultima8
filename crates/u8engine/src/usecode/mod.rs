//! Usecode: U8's compiled game logic, run as cooperative processes.
//!
//! - [`vm`]: process scheduling and the opcode interpreter.
//! - [`intrinsics`]: the native functions scripts call, one table, grouped by domain.
//! - [`host`]: what scripts can see of the game ([`Host`]), and its implementation over a [`World`].
//! - `stack` / `heap` / `search`: process stacks, the string/list heap, and `loop` item searches.
//!
//! The bytecode format itself (classes, events, operand lengths, disassembly) is in
//! [`u8data::usecode`].
//!
//! [`World`]: crate::world::World
pub mod host;
pub mod intrinsics;
pub mod vm;

mod heap;
mod search;
mod stack;

pub use host::{Host, Pop, WorldHost};
pub use u8data::usecode::{event, Usecode};
pub use vm::{event_class, Pid, ReadKind, Request, UiEvent, Vm};

/// The game's scripts and the VM running them.
pub struct Scripts {
    pub usecode: Usecode,
    pub vm: Vm,
}

impl Scripts {
    /// Load EUSECODE.FLX from an install, with the global flags and names of `gd`'s game.
    pub fn load(root: &std::path::Path, gd: &u8data::GameData) -> u8data::Result<Self> {
        let (usecode, _) = u8data::load_usecode(root)?;
        let mut vm = Vm::new(&usecode, gd.flags.as_deref());
        if let Some(n) = &gd.avatar_name {
            vm.avatar_name = n.clone();
        }
        // Effect lengths, so scripts waiting for a sound to finish wait about the right time.
        if let Some(f) = u8data::find_file(root, "SOUND/SOUND.FLX").ok().and_then(|p| std::fs::read(p).ok()) {
            let f = u8data::flex::Flex::parse(f)?;
            vm.sfx_ticks = (0..f.len())
                .map(|i| {
                    f.get(i)
                        .and_then(u8data::sound::length)
                        .map_or(0, |(n, r)| (n as u64 * 30 / r as u64).min(30 * 60) as u32)
                })
                .collect();
        }
        Ok(Self { usecode, vm })
    }
}
