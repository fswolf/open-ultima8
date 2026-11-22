//! Key bindings: physical keys → commands, and held keys → a movement direction.
//! Game keys follow the original (C combat, I backpack, Z stats, Backspace close, Alt-X quit,
//! Alt-H swap mouse buttons, F7/F8 quick save/load…); the viewer's own switches live on the F-keys.
use std::collections::HashSet;
use winit::keyboard::KeyCode;

/// Everything a key press can do. Bindings live in [`bind`] only.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    Quit,
    /// Backspace: put away pages and speech (the original's "close all displays").
    Close,
    /// Esc: put away pages and speech, or (if there are none) open the game menu.
    Menu,
    /// J: the diary (save and load).
    Diary,
    /// I: the backpack.
    Backpack,
    /// Z: the Avatar's paperdoll (what it wears and carries).
    Paperdoll,
    /// C: weapon out (combat stance) or away.
    Combat,
    SwapButtons,
    QuickSave,
    QuickLoad,
    Fullscreen,
    CycleUpscaler,
    ToggleRoofs,
    ToggleEditor,
    ToggleCutaway,
    ToggleFollow,
    /// F1: the settings panel.
    Settings,
    ZoomIn,
    ZoomOut,
    PrevMap,
    NextMap,
    Recentre,
    StopScripts,
    Jump,
    ToggleColourFix,
    ToggleStats,
    GammaDown,
    GammaUp,
    /// Pick conversation option n (0-based).
    Answer(usize),
    MusicToggle,
    SfxToggle,
    MusicNext,
    MusicPrev,
    VolumeDown,
    VolumeUp,
}

#[derive(Clone, Copy, Default)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl From<winit::keyboard::ModifiersState> for Mods {
    fn from(m: winit::keyboard::ModifiersState) -> Self {
        Self { shift: m.shift_key(), ctrl: m.control_key(), alt: m.alt_key() }
    }
}

pub fn bind(k: KeyCode, m: Mods) -> Option<Command> {
    use Command::*;
    Some(match k {
        KeyCode::KeyX if m.alt => Quit,
        KeyCode::KeyQ if m.ctrl => Quit,
        KeyCode::KeyH if m.alt => SwapButtons,
        KeyCode::Escape => Menu,
        KeyCode::Backspace => Close,
        KeyCode::KeyJ => Diary,
        KeyCode::KeyI => Backpack,
        KeyCode::KeyZ => Paperdoll,
        KeyCode::KeyC => Combat,
        KeyCode::Space => Jump,
        KeyCode::F1 => Settings,
        KeyCode::F2 => CycleUpscaler,
        KeyCode::F3 => ToggleCutaway,
        KeyCode::F4 => ToggleRoofs,
        KeyCode::F5 => ToggleEditor,
        KeyCode::F6 => ToggleFollow,
        KeyCode::F7 => QuickSave,
        KeyCode::F8 => QuickLoad,
        KeyCode::F9 => StopScripts,
        KeyCode::F10 => ToggleColourFix,
        KeyCode::F11 => Fullscreen,
        KeyCode::F12 => ToggleStats,
        KeyCode::Equal | KeyCode::NumpadAdd => ZoomIn,
        KeyCode::Minus | KeyCode::NumpadSubtract => ZoomOut,
        KeyCode::PageUp => PrevMap,
        KeyCode::PageDown => NextMap,
        KeyCode::Home => Recentre,
        KeyCode::BracketLeft => GammaDown,
        KeyCode::BracketRight => GammaUp,
        KeyCode::KeyP if m.shift => SfxToggle,
        KeyCode::KeyP => MusicToggle,
        KeyCode::KeyN if m.shift => MusicPrev,
        KeyCode::KeyN => MusicNext,
        KeyCode::Digit1 => Answer(0),
        KeyCode::Digit2 => Answer(1),
        KeyCode::Digit3 => Answer(2),
        KeyCode::Digit4 => Answer(3),
        KeyCode::Digit5 => Answer(4),
        KeyCode::Digit6 => Answer(5),
        KeyCode::Digit7 => Answer(6),
        KeyCode::Digit8 => Answer(7),
        // 9 is both "option 9" and "volume down": the viewer tries the answer first.
        KeyCode::Digit9 => Answer(8),
        KeyCode::Digit0 => VolumeUp,
        _ => return None,
    })
}

/// Keys currently held.
#[derive(Default)]
pub struct Held(HashSet<KeyCode>);

impl Held {
    pub fn press(&mut self, k: KeyCode) {
        self.0.insert(k);
    }
    pub fn release(&mut self, k: KeyCode) {
        self.0.remove(&k);
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
    fn any(&self, a: KeyCode, b: KeyCode) -> bool {
        self.0.contains(&a) || self.0.contains(&b)
    }
    pub fn shift(&self) -> bool {
        self.any(KeyCode::ShiftLeft, KeyCode::ShiftRight)
    }
    /// Arrow/WASD state as screen axes (-1, 0 or 1 each).
    pub fn axes(&self) -> (i32, i32) {
        let ax = |neg: bool, pos: bool| pos as i32 - neg as i32;
        (
            ax(self.any(KeyCode::ArrowLeft, KeyCode::KeyA), self.any(KeyCode::ArrowRight, KeyCode::KeyD)),
            ax(self.any(KeyCode::ArrowUp, KeyCode::KeyW), self.any(KeyCode::ArrowDown, KeyCode::KeyS)),
        )
    }
    /// U8 facing for the held keys.
    pub fn dir(&self) -> Option<usize> {
        facing_of(self.axes())
    }
}

/// U8 facing for screen axes (-1, 0 or 1 each): up = 7 (NW), right = 1 (NE), down = 3 (SE),
/// left = 5 (SW). The pad's stick and D-pad turn the same way.
pub fn facing_of(axes: (i32, i32)) -> Option<usize> {
    Some(match axes {
        (0, -1) => 7,
        (1, -1) => 0,
        (1, 0) => 1,
        (1, 1) => 2,
        (0, 1) => 3,
        (-1, 1) => 4,
        (-1, 0) => 5,
        (-1, -1) => 6,
        _ => return None,
    })
}
