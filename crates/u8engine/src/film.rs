//! Playing an SKF film (the intro, the ending): pictures painted one over another onto a 320×200
//! screen, events at frame numbers for music, effects, recorded lines with subtitles, waits and
//! fades. As ScummVM's SKFPlayer, except for the pace: a picture stays `speed`/70 s (the
//! SetSpeed events, which ScummVM ignores; VGA's 70 Hz, as FLI films count) — the pace at which
//! the Guardian's lines don't run into each other. Waits count fifteenths of a second.
use u8data::palette::Palette;
use u8data::skf::{action, Object, Skf};
use u8data::sound::Sample;

pub const WIDTH: usize = 320;
pub const HEIGHT: usize = 200;
/// A picture's time before any SetSpeed, in 70ths of a second.
const SPEED: u32 = 12;
const WAIT_TICK: f32 = 1.0 / 15.0;
/// Fades to and from black or white take this long; music fades out over this.
const FADE_SECS: f32 = 16.0 / 60.0;
const MUSIC_FADE_SECS: f32 = 1.5;

/// What the film asks of the host as it plays.
#[derive(Clone, Debug)]
pub enum Cue {
    /// MUSIC.FLX song (0: stop).
    Music(u16),
    /// The music's loudness (1 = as set), while it fades out.
    MusicLevel(f32),
    Sfx(u16),
    StopSfx(u16),
    /// A recorded line.
    Line(Sample),
}

pub struct Film {
    skf: Skf,
    screen: Vec<u8>,
    palette: Palette,
    /// The next object to look at, and frames shown.
    object: usize,
    frame: u16,
    event: usize,
    speed: u32,
    /// Seconds until the next step; fifteenths still to wait.
    clock: f32,
    wait: u16,
    /// Fade: colour, how far (0 clear … 1 all colour), and where it's going.
    fade: (u8, f32, f32),
    music_fade: Option<f32>,
    subtitle: Option<(i32, String)>,
    done: bool,
}

impl Film {
    pub fn new(skf: Skf) -> Self {
        Self {
            skf,
            screen: vec![0; WIDTH * HEIGHT],
            palette: Palette([[0, 0, 0, 255]; 256]),
            object: 0,
            frame: 0,
            event: 0,
            speed: SPEED,
            clock: 0.0,
            wait: 0,
            fade: (0, 0.0, 0.0),
            music_fade: None,
            subtitle: None,
            done: false,
        }
    }

    /// The picture: palette indices, `WIDTH` × `HEIGHT`.
    pub fn screen(&self) -> &[u8] {
        &self.screen
    }
    pub fn palette(&self) -> &Palette {
        &self.palette
    }
    /// The fade over it: grey level of the colour (0 black, 255 white) and how much (0–1).
    pub fn fade(&self) -> (u8, f32) {
        (self.fade.0, self.fade.1)
    }
    /// The subtitle shown: its y on the screen and text.
    pub fn subtitle(&self) -> Option<&(i32, String)> {
        self.subtitle.as_ref()
    }
    pub fn done(&self) -> bool {
        self.done
    }

    /// Play on for `dt` seconds.
    pub fn tick(&mut self, dt: f32) -> Vec<Cue> {
        let mut cues = vec![];
        let (_, level, to) = &mut self.fade;
        let step = dt / FADE_SECS;
        *level = if *to > *level { (*level + step).min(*to) } else { (*level - step).max(*to) };
        if let Some(left) = &mut self.music_fade {
            // Nothing else happens till the music has gone.
            *left -= dt;
            if *left > 0.0 {
                cues.push(Cue::MusicLevel(*left / MUSIC_FADE_SECS));
                return cues;
            }
            self.music_fade = None;
            cues.extend([Cue::Music(0), Cue::MusicLevel(1.0)]);
        }
        self.clock -= dt;
        while self.clock <= 0.0 && !self.done && self.music_fade.is_none() {
            self.clock += self.step(&mut cues);
        }
        cues
    }

    /// One step: a fifteenth waited, or this frame's events and then the next picture. Returns
    /// how long until the next step.
    fn step(&mut self, cues: &mut Vec<Cue>) -> f32 {
        if self.wait > 0 {
            self.wait -= 1;
            return WAIT_TICK;
        }
        while let Some(e) = self.skf.events.get(self.event).copied().filter(|e| e.frame <= self.frame) {
            self.event += 1;
            match e.action {
                action::FADE_OUT => self.fade = (0, 0.0, 1.0),
                action::FADE_WHITE => self.fade = (255, 0.0, 1.0),
                action::FADE_IN => self.fade = (self.fade.0, 1.0, 0.0),
                action::WAIT => {
                    self.wait = e.data;
                    return WAIT_TICK;
                }
                action::PLAY_MUSIC => cues.push(Cue::Music(e.data)),
                action::SLOW_STOP_MUSIC => {
                    self.music_fade = Some(MUSIC_FADE_SECS);
                    return 0.0;
                }
                action::PLAY_SFX => cues.push(Cue::Sfx(e.data)),
                action::STOP_SFX => cues.push(Cue::StopSfx(e.data)),
                action::SET_SPEED => self.speed = (e.data as u32).max(1),
                action::PLAY_SOUND => {
                    if let Object::Line(s) = self.skf.object(e.data as usize) {
                        cues.push(Cue::Line(s));
                    }
                    if let Object::Subtitle { y, text } = self.skf.object(e.data as usize - 1) {
                        self.subtitle = (!text.is_empty()).then_some((y, text));
                    }
                }
                action::CLEAR_SUBS => self.subtitle = None,
                _ => {}
            }
        }
        self.frame += 1;
        // On to the next picture (palettes on the way take effect).
        loop {
            self.object += 1;
            if self.object >= self.skf.len() {
                self.done = true;
                return 0.0;
            }
            match self.skf.object(self.object) {
                Object::Palette(p) => self.palette = *p,
                Object::Picture(f) => {
                    self.paint(&f);
                    break;
                }
                _ => {}
            }
        }
        self.speed as f32 / 70.0
    }

    fn paint(&mut self, f: &u8data::shape::Frame) {
        let (x0, y0) = (-f.xoff, -f.yoff);
        for y in 0..f.height as i32 {
            let sy = y0 + y;
            if !(0..HEIGHT as i32).contains(&sy) {
                continue;
            }
            for x in 0..f.width as i32 {
                let (sx, k) = (x0 + x, (y * f.width as i32 + x) as usize);
                if (0..WIDTH as i32).contains(&sx) && f.mask[k] {
                    self.screen[sy as usize * WIDTH + sx as usize] = f.pixels[k];
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The intro plays through: the Guardian's lines with subtitles, music, then it ends.
    #[test]
    #[ignore]
    fn the_intro_plays_through() {
        let Ok(dir) = std::env::var("U8_DATA") else { return };
        let Ok(p) = u8data::find_file(std::path::Path::new(&dir), "STATIC/EINTRO.SKF") else { return };
        let mut film = Film::new(Skf::parse(std::fs::read(p).unwrap()).unwrap());
        let (mut lines, mut songs, mut secs, mut subs) = (0, vec![], 0.0, vec![]);
        while !film.done() && secs < 300.0 {
            for c in film.tick(0.05) {
                match c {
                    Cue::Line(s) => lines += (s.seconds() > 1.0) as u32,
                    Cue::Music(n) => songs.push(n),
                    _ => {}
                }
            }
            if let Some((_, t)) = film.subtitle().filter(|s| subs.last() != Some(&s.1)) {
                subs.push(t.clone());
            }
            secs += 0.05;
        }
        assert!(film.done(), "ended");
        assert_eq!(lines, 8, "the Guardian's eight lines");
        assert!(subs.iter().any(|s| s.contains("world of Pagan")), "{subs:?}");
        assert_eq!(songs.first(), Some(&49));
        assert!((80.0..100.0).contains(&secs), "about a minute and a half: {secs:.0} s");
        assert!(film.screen().iter().any(|&p| p != 0), "pictures were painted");
    }
}
