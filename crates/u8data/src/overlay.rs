//! WPNOVLAY.DAT: where a held weapon is drawn over the Avatar's animations. A Flex with one
//! entry per Avatar action; each holds, for every overlay type (the way a weapon is held), 8
//! directions × the action's frames of (x offset i8, y offset i8, overlay frame u16).
use crate::{anim::AnimDat, flex::Flex, u16le};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OverlayFrame {
    /// Screen offset from the actor's origin.
    pub dx: i32,
    pub dy: i32,
    /// Frame of the weapon's overlay shape.
    pub frame: u32,
}

#[derive(Default)]
pub struct WeaponOverlays {
    /// [action] → (frames per direction, raw entry).
    actions: Vec<Option<(usize, Vec<u8>)>>,
}

/// The Avatar's shape, whose animations the table follows.
const AVATAR_SHAPE: u32 = 1;

impl WeaponOverlays {
    pub fn parse(f: &Flex, anims: &AnimDat) -> Self {
        let actions = (0..f.len())
            .map(|a| {
                let d = f.get(a).filter(|d| !d.is_empty())?;
                let len = anims.get(AVATAR_SHAPE, a)?.frames.first()?.len();
                (len > 0).then(|| (len, d.to_vec()))
            })
            .collect();
        Self { actions }
    }

    /// Is a weapon drawn at all in `action` (not in walking or standing at ease)?
    pub fn has(&self, action: usize) -> bool {
        self.actions.get(action).is_some_and(Option::is_some)
    }

    /// The weapon's place in frame `step` of `action` facing `dir`, held the `kind` way; None
    /// where nothing is drawn.
    pub fn get(&self, action: usize, kind: usize, dir: usize, step: usize) -> Option<OverlayFrame> {
        let (len, d) = self.actions.get(action)?.as_ref()?;
        let o = ((kind * 8 + dir) * len + step % len) * 4;
        let r = d.get(o..o + 4)?;
        let f = OverlayFrame { dx: r[0] as i8 as i32, dy: r[1] as i8 as i32, frame: u16le(r, 2) as u32 };
        (f != OverlayFrame::default()).then_some(f)
    }
}
