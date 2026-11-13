//! The films: the intro (before a new game, and from the menu) and the ending (when the game is
//! won, and from the menu), full screen over the paused game, with their music, effects, the
//! Guardian's lines and subtitles (red, centred 200 wide from x 60, as ScummVM shows them).
//! Any key or click skips.
use super::*;
use u8data::font::TextImage;
#[cfg(feature = "audio")]
use u8engine::film::Cue;
use u8engine::film::{Film, HEIGHT, WIDTH};

const SUB_FONT: usize = 6;
const SUB_X: i32 = 60;
const SUB_W: u32 = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    Intro,
    Endgame,
}

impl Which {
    fn file(self) -> &'static str {
        match self {
            Which::Intro => "STATIC/EINTRO.SKF",
            Which::Endgame => "STATIC/ENDGAME.SKF",
        }
    }
}

/// What comes after the film.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum After {
    /// A new game: give the Avatar a name.
    Name,
    Menu,
}

pub struct Showing {
    film: Film,
    after: After,
    /// The song to go back to.
    #[cfg_attr(not(feature = "audio"), allow(dead_code))]
    back: usize,
    sub: Option<(String, TextImage)>,
}

impl App {
    /// Start a film; false (with a notice) if its file isn't there.
    pub(super) fn start_film(&mut self, which: Which, after: After) -> bool {
        let skf = u8data::find_file(&self.root, which.file())
            .ok()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|d| u8data::skf::Skf::parse(d).ok());
        let Some(skf) = skf else {
            self.notice("The film isn't in this copy of the game (STATIC/*.SKF).");
            return false;
        };
        if let Some(m) = &mut self.menu {
            m.close();
        }
        #[allow(unused_mut)]
        let mut back = 0;
        #[cfg(feature = "audio")]
        if let Some(a) = &mut self.audio {
            back = a.wanted_track();
            a.hush();
        }
        self.film = Some(Showing { film: Film::new(skf), after, back, sub: None });
        self.dirty = true;
        true
    }

    /// The film is over (or skipped): its sounds stop, the game's music comes back, then what
    /// was to come after.
    pub(super) fn end_film(&mut self) {
        let Some(s) = self.film.take() else { return };
        #[cfg(feature = "audio")]
        if let Some(a) = &mut self.audio {
            a.hush();
            a.set_music_level(1.0);
            a.play_track(s.back);
        }
        match s.after {
            After::Name => self.open_menu(menu::Page::Name),
            After::Menu => self.open_menu(menu::Page::Main),
        }
        self.dirty = true;
    }

    /// Play on for `dt` seconds.
    pub(super) fn tick_film(&mut self, dt: f32) {
        let Some(s) = &mut self.film else { return };
        #[cfg_attr(not(feature = "audio"), allow(unused_variables))]
        let cues = s.film.tick(dt);
        #[cfg(feature = "audio")]
        if let Some(a) = &mut self.audio {
            for cue in cues {
                match cue {
                    Cue::Music(n) => a.play_track(n as usize),
                    Cue::MusicLevel(l) => a.set_music_level(l),
                    Cue::Sfx(n) => a.sfx(n, 0, false),
                    Cue::StopSfx(n) => a.stop_sfx(n, None),
                    Cue::Line(l) => a.say(&l),
                }
            }
        }
        // The subtitle, rendered when it changes.
        let want = s.film.subtitle().map(|(_, t)| t.as_str());
        if want != s.sub.as_ref().map(|(t, _)| t.as_str()) {
            s.sub = match (want, &self.menu) {
                (Some(t), Some(m)) => {
                    let (fonts, pal) = m.fonts();
                    Some((t.to_string(), fonts.render(SUB_FONT, &fonts.wrap(SUB_FONT, t, SUB_W), pal, None)))
                }
                _ => None,
            };
        }
        self.dirty = true;
        if s.film.done() {
            self.end_film();
        }
    }
}

impl Showing {
    /// Draw the film centred in the view, black around it.
    pub fn draw(&self, canvas: &mut Canvas, ss: u32, (vw, vh): (u32, u32)) {
        let s = self;
        canvas.fill([0, 0, 0, 255]);
        let (ox, oy) = ((vw as i32 - WIDTH as i32) / 2, (vh as i32 - HEIGHT as i32) / 2);
        let (pal, (grey, fade)) = (s.film.palette(), s.film.fade());
        let mix = |c: u8| (c as f32 * (1.0 - fade) + grey as f32 * fade) as u8;
        let rgba = s.film.screen().iter().flat_map(|&p| pal.0[p as usize].map(mix)).collect::<Vec<_>>();
        let mut img = TextImage { w: WIDTH as u32, h: HEIGHT as u32, rgba };
        img.rgba.chunks_exact_mut(4).for_each(|c| c[3] = 255);
        text::draw_plain(canvas, ss, &img, ox, oy);
        if let (Some((_, sub)), Some((y, _))) = (&s.sub, s.film.subtitle()) {
            text::draw(canvas, ss, sub, ox + SUB_X + (SUB_W as i32 - sub.w as i32) / 2, oy + y);
        }
    }
}
