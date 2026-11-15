//! The original's mouse: the pointer is an arrow out from the Avatar. Its direction is where to go,
//! its length how: short = careful step, medium = walk, long = run (both buttons: jump). In combat
//! stance the arrows are red: towards where the Avatar faces advances, away retreats.
use std::path::Path;
use u8data::{palette::Palette, shape::Shape};
use u8render::upscale;
use winit::{
    event_loop::ActiveEventLoop,
    window::{Cursor, CursorIcon, CustomCursor, Window},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reach {
    Short,
    Medium,
    Long,
}

/// Game pixels from the Avatar's feet the short arrow reaches at least (past its body).
const SHORT_AT_LEAST: f32 = 28.0;
/// Where the arrow starts: this far (world z) above the Avatar's base, its feet on screen.
pub const FEET_UP: i32 = 6;

/// The pointer at (dx, dy) game pixels from the Avatar in a vw×vh view: (screen sector 0–7,
/// clockwise from up, reach). Vertical distance counts double, as the isometric view squashes it.
pub fn aim(dx: i32, dy: i32, (vw, vh): (u32, u32)) -> (usize, Reach) {
    let (x, y) = (dx as f32, dy as f32 * 2.0);
    let sector = ((x.atan2(-y).to_degrees() + 360.0 + 22.5) / 45.0) as usize % 8;
    // As in the original at 320×200: short within ~33 px, medium within ~80 px, scaled to the view.
    let (vw, vh) = (vw as f32, vh as f32);
    // (Zoomed right in, short still reaches past the Avatar's figure.)
    let short = (vw / 8.0).min(vh / 6.0).max(SHORT_AT_LEAST);
    let medium = (vw * 0.4).min(vh * 0.4).max(short * 2.4);
    let d = (dx as f32).hypot(dy as f32);
    (
        sector,
        if d < short {
            Reach::Short
        } else if d < medium {
            Reach::Medium
        } else {
            Reach::Long
        },
    )
}

/// U8 facing (0 = NE … 7 = N) for a screen sector.
pub fn facing(sector: usize) -> usize {
    (sector + 7) % 8
}

/// U8MOUSE.SHP: 0–23 blue arrows (8 per reach, by sector), then dots, combat arrows, hand, quill….
pub fn arrow(sector: usize, reach: Reach) -> usize {
    reach as usize * 8 + sector
}

/// U8MOUSE.SHP 25–32: the red combat arrows, one per sector.
/// The targeting cursor (scripts asking the player to pick something).
pub const TARGET: usize = 34;
/// The red cross: out of reach.
pub const CROSS: usize = 40;

pub fn combat_arrow(sector: usize) -> usize {
    25 + sector
}

/// The game's cursors as OS cursors (drawn by the compositor, so moving the mouse never redraws).
pub struct Cursors {
    shape: Shape,
    pal: Palette,
    made: Vec<Option<CustomCursor>>,
    style: (u32, bool),
    shown: Option<Option<usize>>,
}

impl Cursors {
    pub fn load(data: &Path, pal: &Palette) -> Option<Self> {
        let d = std::fs::read(u8data::find_file(data, "STATIC/U8MOUSE.SHP").ok()?).ok()?;
        let shape = u8data::shape::parse_shape(&d).map_err(|e| eprintln!("no game cursors: {e}")).ok()?;
        Some(Self { made: vec![None; shape.len()], shape, pal: pal.clone(), style: (0, false), shown: None })
    }

    /// Show cursor `frame` (None: the system arrow) at `scale`, smoothed with xBRZ or not.
    pub fn show(&mut self, el: &ActiveEventLoop, win: &Window, frame: Option<usize>, scale: u32, smooth: bool) {
        if self.style != (scale, smooth) {
            self.style = (scale, smooth);
            self.made.iter_mut().for_each(|c| *c = None);
            self.shown = None;
        }
        if self.shown == Some(frame) {
            return;
        }
        self.shown = Some(frame);
        let cursor = frame.and_then(|i| {
            if self.made.get(i)?.is_none() {
                let f = self.shape.get(i).filter(|f| f.width > 0 && f.height > 0)?;
                let rgba = f.to_rgba(&self.pal, None);
                let up = if smooth { upscale::xbrz } else { upscale::nearest };
                let s = scale.clamp(1, 6);
                let src = CustomCursor::from_rgba(
                    up(&rgba, f.width, f.height, s),
                    (f.width * s) as u16,
                    (f.height * s) as u16,
                    (f.xoff.max(0) as u32 * s) as u16,
                    (f.yoff.max(0) as u32 * s) as u16,
                )
                .ok()?;
                self.made[i] = Some(el.create_custom_cursor(src));
            }
            self.made[i].clone()
        });
        win.set_cursor(cursor.map_or(Cursor::Icon(CursorIcon::Default), Cursor::Custom));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aiming_matches_the_arrow_frames() {
        let v = (320, 200);
        assert_eq!(aim(0, -60, v), (0, Reach::Medium)); // up
        assert_eq!(aim(60, -30, v), (1, Reach::Medium)); // 2:1 up-right is the diagonal
        assert_eq!(aim(60, 0, v), (2, Reach::Medium));
        assert_eq!(aim(0, 60, v), (4, Reach::Medium));
        assert_eq!(aim(-60, 0, v), (6, Reach::Medium));
        assert_eq!(aim(5, 5, v).1, Reach::Short);
        assert_eq!(aim(150, 0, v).1, Reach::Long);
        // Screen right walks like the right arrow key (facing 1), up like the up key (7).
        assert_eq!((facing(2), facing(0)), (1, 7));
        assert_eq!(arrow(2, Reach::Long), 18);
    }
}
