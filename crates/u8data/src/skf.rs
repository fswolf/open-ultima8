//! SKF films (STATIC/EINTRO.SKF, ENDGAME.SKF): a flex whose entry 0 is the event list and the
//! rest objects tagged by a u16 type — 1 a palette (768 × 6-bit RGB), 2 a picture (a shape frame
//! painted over the last one), 13 a recorded line (u16 rate at 6, channels at 8, signed 8-bit PCM
//! from 34), 17 its subtitle (u16 y at 4, text from 6). Pictures play in order, one per frame;
//! events fire at frame numbers (ScummVM's SKFPlayer).
use crate::{flex::Flex, palette::Palette, shape::Frame, sound::Sample, u16le, Result};

/// What an event does.
pub mod action {
    pub const PLAY_MUSIC: u16 = 3;
    pub const SLOW_STOP_MUSIC: u16 = 4;
    pub const PLAY_SFX: u16 = 5;
    pub const STOP_SFX: u16 = 6;
    pub const SET_SPEED: u16 = 7;
    pub const FADE_OUT: u16 = 8;
    pub const FADE_IN: u16 = 9;
    pub const WAIT: u16 = 12;
    pub const PLAY_SOUND: u16 = 14;
    pub const FADE_WHITE: u16 = 15;
    pub const CLEAR_SUBS: u16 = 18;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub frame: u16,
    pub action: u16,
    pub data: u16,
}

pub enum Object {
    Palette(Box<Palette>),
    Picture(Frame),
    Line(Sample),
    Subtitle { y: i32, text: String },
    Other,
}

pub struct Skf {
    flex: Flex,
    pub events: Vec<Event>,
}

impl Skf {
    pub fn parse(d: Vec<u8>) -> Result<Self> {
        let flex = Flex::parse(d)?;
        let list = flex.get(0).unwrap_or_default();
        let events = list
            .as_chunks::<6>()
            .0
            .iter()
            .map(|c| Event { frame: u16le(c, 0), action: u16le(c, 2), data: u16le(c, 4) })
            .take_while(|e| e.frame != 0xFFFF)
            .collect();
        Ok(Self { flex, events })
    }

    /// Objects after the event list.
    pub fn len(&self) -> usize {
        self.flex.len()
    }
    pub fn is_empty(&self) -> bool {
        self.flex.len() < 2
    }

    pub fn object(&self, i: usize) -> Object {
        let Some(d) = self.flex.get(i).filter(|d| d.len() > 2) else { return Object::Other };
        match u16le(d, 0) {
            1 if d.len() >= 770 => {
                let mut p = [[0u8, 0, 0, 255]; 256];
                for (k, c) in p.iter_mut().enumerate() {
                    for j in 0..3 {
                        let v = d[2 + k * 3 + j] & 63;
                        c[j] = (v << 2) | (v >> 4);
                    }
                }
                Object::Palette(Box::new(Palette(p)))
            }
            2 => crate::shape::frame_at(d, 2).map_or(Object::Other, Object::Picture),
            13 if d.len() > 34 => {
                let rate = u16le(d, 6) as u32;
                let stereo = d[8] == 2;
                // Signed 8-bit to our unsigned; stereo mixed down.
                let pcm: Vec<u8> = if stereo {
                    d[34..]
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|s| ((s[0] as i8 as i16 + s[1] as i8 as i16) / 2 + 128) as u8)
                        .collect()
                } else {
                    d[34..].iter().map(|&s| (s as i8 as i16 + 128) as u8).collect()
                };
                Object::Line(Sample { rate: rate.max(1), pcm })
            }
            17 if d.len() > 6 => {
                let text = d[6..].split(|&b| b == 0).next().unwrap_or_default();
                Object::Subtitle { y: u16le(d, 4) as i32, text: String::from_utf8_lossy(text).into_owned() }
            }
            _ => Object::Other,
        }
    }
}
