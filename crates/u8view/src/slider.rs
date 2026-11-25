//! "How many?" — the original's slider (U8GUMPS 41 window, 42 OK, 43/44 less/more, 45 the knob,
//! as ScummVM's SliderGump lays them out), asked when the player drops part of a pile, or by a
//! script (getSliderInput: a range and a step). Arrows or the buttons step, the knob drags (and
//! snaps to the step), Enter or OK takes it, Esc or a right click leaves the pile where it was
//! (a script gets the value shown, as ScummVM's).
use u8data::font::Fonts;
use u8data::palette::Palette;
use u8engine::{ObjId, Place};
use u8render::{text, Canvas};
use winit::keyboard::KeyCode;

use crate::art::GumpArt;

const WINDOW: u32 = 41;
const OK: u32 = 42;
const LESS: u32 = 43;
const MORE: u32 = 44;
const KNOB: u32 = 45;
const KNOB_Y: i32 = 17;
const TRACK: (i32, i32) = (55, 140);
const BUTTONS: [(u32, i32); 3] = [(OK, 158), (LESS, 36), (MORE, 141)];
const LABEL: (i32, i32) = (17, 25);
const LABEL_FONT: usize = 0;

/// Where the part goes once the player says how much.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dest {
    /// Onto a pile it merges with.
    Pile(ObjId),
    To(Place),
    /// Into the backpack (dropped on the paperdoll).
    Stow,
    /// A script's getSliderInput: its process ends with the value.
    Script(u16),
}

/// What the player decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Take(u16),
    Cancel,
}

pub struct Slider {
    pub item: ObjId,
    pub dest: Dest,
    min: i16,
    max: i16,
    step: i16,
    pub value: i16,
    /// Where it's drawn (top-left), the knob's width, and the knob being dragged.
    at: (i32, i32),
    knob_w: i32,
    dragging: bool,
    pressed: Option<u32>,
}

impl Slider {
    /// Ask how many of the `of` in `item` to move (all, to begin with).
    pub fn new(item: ObjId, of: u16, dest: Dest) -> Self {
        let of = of.min(i16::MAX as u16) as i16;
        Slider {
            item,
            dest,
            min: 0,
            max: of,
            step: 1,
            value: of,
            at: (0, 0),
            knob_w: 0,
            dragging: false,
            pressed: None,
        }
    }

    /// A script's question: `min` to `max` by `step`, starting at `min`.
    pub fn for_script(pid: u16, min: i16, max: i16, step: i16) -> Self {
        let dest = Dest::Script(pid);
        let (max, step) = (max.max(min), step.max(1));
        Slider { item: 0, dest, min, max, step, value: min, at: (0, 0), knob_w: 0, dragging: false, pressed: None }
    }

    fn knob_x(&self) -> i32 {
        let span = TRACK.1 - TRACK.0 - self.knob_w;
        TRACK.0 + span * (self.value - self.min) as i32 / (self.max - self.min).max(1) as i32
    }

    fn set_from_x(&mut self, x: i32) {
        let span = (TRACK.1 - TRACK.0 - self.knob_w).max(1);
        let range = (self.max - self.min) as i32;
        let v = self.min as i32 + (x - self.at.0 - TRACK.0 - self.knob_w / 2) * range / span;
        let v = v.clamp(self.min as i32, self.max as i32);
        // Snapped to the step as ScummVM does it (from 0, not from the minimum).
        self.value = (self.min as i32 + self.step as i32 * (v / self.step as i32)).min(self.max as i32) as i16;
    }

    fn step(&mut self, by: i32) {
        let v = self.value as i32 + by * self.step as i32;
        self.value = v.clamp(self.min as i32, self.max as i32) as i16;
    }

    /// What it's set to.
    pub fn amount(&self) -> u16 {
        self.value as u16
    }

    pub fn draw(
        &mut self,
        canvas: &mut Canvas,
        art: &mut GumpArt,
        fonts: Option<(&Fonts, &Palette)>,
        ss: u32,
        (vw, vh): (u32, u32),
    ) {
        let (w, h) = art.size(WINDOW);
        self.at = ((vw as i32 - w) / 2, (vh as i32 - h) / 2);
        self.knob_w = art.size(KNOB).0;
        let (x, y) = self.at;
        art.draw(canvas, WINDOW, 0, x, y);
        for (shape, bx) in BUTTONS {
            art.draw(canvas, shape, (self.pressed == Some(shape)) as u32, x + bx, y + KNOB_Y);
        }
        art.draw(canvas, KNOB, 0, x + self.knob_x(), y + KNOB_Y);
        if let Some((f, pal)) = fonts {
            // Each letter's hotspot on the baseline from LABEL, as the original prints (SLIDER.C).
            let label = f.render_set(LABEL_FONT, &[self.value.to_string()], pal, 0, false);
            text::draw_plain(canvas, ss, &label, x + LABEL.0, y + LABEL.1 - f.baseline(LABEL_FONT));
        }
    }

    /// Is (gx, gy) on `shape`'s button?
    fn on(&self, art: &GumpArt, shape: u32, bx: i32, (gx, gy): (i32, i32)) -> bool {
        let (w, h) = art.size(shape);
        let (x, y) = (self.at.0 + bx, self.at.1 + KNOB_Y);
        (x..x + w).contains(&gx) && (y..y + h).contains(&gy)
    }

    /// A left press at (gx, gy).
    pub fn press(&mut self, art: &GumpArt, at: (i32, i32)) -> Option<Answer> {
        let hit = BUTTONS.iter().find(|&&(s, bx)| self.on(art, s, bx, at)).map(|&(s, _)| s);
        self.pressed = hit;
        match hit {
            Some(OK) => return Some(Answer::Take(self.amount())),
            Some(LESS) => self.step(-1),
            Some(MORE) => self.step(1),
            _ => {
                let (x, y) = (at.0 - self.at.0, at.1 - self.at.1);
                if (TRACK.0..TRACK.1).contains(&x) && (KNOB_Y..KNOB_Y + art.size(KNOB).1.max(8)).contains(&y) {
                    self.dragging = true;
                    self.set_from_x(at.0);
                }
            }
        }
        None
    }

    pub fn release(&mut self) {
        (self.dragging, self.pressed) = (false, None);
    }

    /// The pointer moved to game x `gx` (drags the knob while held).
    pub fn drag(&mut self, gx: i32) -> bool {
        if self.dragging {
            self.set_from_x(gx);
        }
        self.dragging
    }

    pub fn key(&mut self, k: KeyCode) -> Option<Answer> {
        match k {
            KeyCode::ArrowLeft | KeyCode::KeyA => self.step(-1),
            KeyCode::ArrowRight | KeyCode::KeyD => self.step(1),
            KeyCode::PageDown | KeyCode::Home => self.value = self.min,
            KeyCode::PageUp | KeyCode::End => self.value = self.max,
            KeyCode::Enter | KeyCode::NumpadEnter => return Some(Answer::Take(self.amount())),
            KeyCode::Escape => return Some(Answer::Cancel),
            _ => {}
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A script's slider: from its minimum, by its step, within its range; Esc still answers.
    #[test]
    fn a_scripts_slider_steps_by_its_step() {
        let mut s = Slider::for_script(5, 2, 10, 3);
        assert_eq!((s.amount(), s.dest), (2, Dest::Script(5)));
        s.key(KeyCode::ArrowRight);
        assert_eq!(s.amount(), 5);
        (0..3).for_each(|_| _ = s.key(KeyCode::ArrowRight));
        assert_eq!(s.amount(), 10, "no further than the most");
        s.key(KeyCode::Home);
        assert_eq!(s.key(KeyCode::Enter), Some(Answer::Take(2)));
    }

    /// Drawn from the game's art; the buttons step, the knob drags, OK answers. With `U8_SHOTS`
    /// set, the picture is written there as PPM.
    #[test]
    #[ignore]
    fn asks_how_many() {
        let Ok(dir) = std::env::var("U8_DATA") else { return };
        let root = std::path::PathBuf::from(dir);
        let read = |p: &str| std::fs::read(u8data::find_file(&root, p).unwrap()).unwrap();
        let pal = Palette::parse(&read("STATIC/U8PAL.PAL")).unwrap();
        let fonts = Fonts::parse(&u8data::flex::Flex::parse(read("STATIC/U8FONTS.FLX")).unwrap());
        let mut art = GumpArt::load(&root, &pal).unwrap();
        let mut s = Slider::new(300, 40, Dest::Stow);
        let mut px = vec![0u8; 320 * 200 * 4];
        s.draw(&mut Canvas::new(&mut px, 320, 200), &mut art, Some((&fonts, &pal)), 1, (320, 200));
        let (x, y) = s.at;
        assert!(art.size(WINDOW).0 > 100, "the window is there");
        s.press(&art, (x + 36 + 2, y + KNOB_Y + 2));
        s.release();
        assert_eq!(s.value, 39, "less");
        s.press(&art, (x + TRACK.0 + 1, y + KNOB_Y + 2));
        assert!(s.drag(x + TRACK.0 - 50) && s.value == 0, "dragged all the way down");
        s.drag(x + TRACK.1 + 50);
        assert_eq!(s.value, 40);
        s.release();
        s.value = 25;
        let mut px = vec![0u8; 320 * 200 * 4];
        s.draw(&mut Canvas::new(&mut px, 320, 200), &mut art, Some((&fonts, &pal)), 1, (320, 200));
        // The number sits in its box as the original prints it: letters' feet on y + 25.
        let lit: Vec<i32> = (y + 12..y + 30)
            .filter(|&r| (x + 12..x + 40).any(|c| px[((r * 320 + c) * 4) as usize..][..3].iter().all(|&v| v > 200)))
            .collect();
        assert_eq!((lit[0], lit[lit.len() - 1]), (y + 19, y + 24), "in the box");
        assert_eq!(s.press(&art, (x + 158 + 2, y + KNOB_Y + 2)), Some(Answer::Take(25)));
        assert_eq!(s.key(KeyCode::Escape), Some(Answer::Cancel));
        if let Ok(out) = std::env::var("U8_SHOTS") {
            let mut ppm = b"P6 320 200 255\n".to_vec();
            px.chunks(4).for_each(|p| ppm.extend(&p[..3]));
            std::fs::write(format!("{out}/slider.ppm"), ppm).unwrap();
        }
    }
}
