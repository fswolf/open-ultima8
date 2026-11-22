//! MUSIC.FLX: entry 0 is a text index ("tenebrae.xmi 8 51 1" — the 2nd field's ASCII code is the
//! song's Flex index); songs are XMIDI (Miles AIL) files. XMIDI → timed MIDI messages.
use crate::flex::Flex;

/// XMIDI timing: AIL plays at a fixed 120 ticks per second.
pub const TICKS_PER_SEC: u32 = 120;

/// One short MIDI message (status, data1, data2) at a tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub tick: u32,
    pub status: u8,
    pub d1: u8,
    pub d2: u8,
}

#[derive(Clone, Debug)]
pub struct Song {
    pub name: String,
    pub index: usize,
}

/// Song list from the index text, in file order.
pub fn songs(f: &Flex) -> Vec<Song> {
    let Some(txt) = f.get(0) else { return vec![] };
    String::from_utf8_lossy(txt)
        .lines()
        .take_while(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            let name = w.next()?.trim_end_matches(".xmi").to_string();
            let key = w.next()?.bytes().next()? as usize;
            f.get(key).is_some().then_some(Song { name, index: key })
        })
        .collect()
}

fn be32(d: &[u8], o: usize) -> Option<usize> {
    d.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
}

/// Find the `n`th EVNT chunk (one per XMID sequence; files may hold several in a CAT).
fn evnt_chunk(d: &[u8], n: usize) -> Option<&[u8]> {
    let mut found = 0;
    let mut o = 0;
    while o + 8 <= d.len() {
        let id = &d[o..o + 4];
        let len = be32(d, o + 4)?;
        match id {
            // Containers: FORM/CAT have a 4-byte type after the length; descend into them.
            b"FORM" | b"CAT " => o += 12,
            b"EVNT" => {
                if found == n {
                    return d.get(o + 8..o + 8 + len);
                }
                found += 1;
                o += 8 + len + (len & 1);
            }
            _ => o += 8 + len + (len & 1),
        }
    }
    None
}

fn vlq(d: &[u8], p: &mut usize) -> Option<u32> {
    let mut v = 0u32;
    for _ in 0..4 {
        let b = *d.get(*p)?;
        *p += 1;
        v = (v << 7) | (b & 0x7F) as u32;
        if b & 0x80 == 0 {
            return Some(v);
        }
    }
    Some(v)
}

/// Decode sequence `n` of an XMIDI file into time-ordered MIDI messages.
/// XMIDI differences from SMF: delays are sums of bytes < 0x80, and note-ons carry a duration
/// (we synthesise the note-off). Sysex and meta events are skipped (end-of-track stops).
pub fn parse_xmidi(d: &[u8], n: usize) -> Option<Vec<Event>> {
    let ev = evnt_chunk(d, n)?;
    let (mut out, mut p, mut tick) = (Vec::new(), 0usize, 0u32);
    while p < ev.len() {
        let b = ev[p];
        if b < 0x80 {
            tick += b as u32;
            p += 1;
            continue;
        }
        p += 1;
        let ch = b & 0x0F;
        match b & 0xF0 {
            0x90 => {
                let (note, vel) = (*ev.get(p)?, *ev.get(p + 1)?);
                p += 2;
                let dur = vlq(ev, &mut p)?;
                out.push(Event { tick, status: b, d1: note, d2: vel });
                out.push(Event { tick: tick + dur, status: 0x80 | ch, d1: note, d2: 0 });
            }
            0x80 | 0xA0 | 0xB0 | 0xE0 => {
                out.push(Event { tick, status: b, d1: *ev.get(p)?, d2: *ev.get(p + 1)? });
                p += 2;
            }
            0xC0 | 0xD0 => {
                out.push(Event { tick, status: b, d1: *ev.get(p)?, d2: 0 });
                p += 1;
            }
            _ => match b {
                0xFF => {
                    let ty = *ev.get(p)?;
                    p += 1;
                    let len = vlq(ev, &mut p)? as usize;
                    p += len;
                    if ty == 0x2F {
                        break;
                    }
                }
                0xF0 | 0xF7 => {
                    let len = vlq(ev, &mut p)? as usize;
                    p += len;
                }
                _ => return None,
            },
        }
    }
    // Stable sort keeps note-offs after note-ons at the same tick only if they came later; put
    // offs first at equal ticks so a re-struck note isn't cut immediately.
    out.sort_by_key(|e| (e.tick, (e.status & 0xF0 != 0x80) as u8));
    Some(out)
}

/// Roland MT-32 → General MIDI program map (U8's music targets the MT-32).
pub const MT32_TO_GM: [u8; 128] = [
    0, 1, 0, 2, 4, 4, 5, 3, 16, 17, 18, 16, 16, 19, 20, 21, //
    6, 6, 6, 7, 7, 7, 8, 112, 62, 62, 63, 63, 38, 38, 39, 39, //
    88, 95, 52, 98, 97, 99, 14, 54, 102, 96, 53, 102, 81, 100, 14, 80, //
    48, 48, 49, 45, 41, 40, 42, 42, 43, 46, 45, 24, 25, 28, 27, 104, //
    32, 32, 34, 33, 36, 37, 35, 35, 79, 73, 72, 72, 74, 75, 64, 65, //
    66, 67, 71, 71, 68, 69, 70, 22, 56, 59, 57, 57, 60, 60, 58, 61, //
    61, 11, 11, 98, 14, 9, 14, 13, 12, 107, 107, 77, 78, 78, 76, 76, //
    47, 117, 127, 118, 118, 116, 115, 119, 115, 112, 55, 124, 123, 0, 14, 117, //
];

/// Plays decoded XMIDI through a SoundFont synth (pure Rust, no system audio needed).
#[cfg(feature = "synth")]
pub struct Sequencer {
    synth: rustysynth::Synthesizer,
    events: Vec<Event>,
    pos: usize,
    /// Current song time in ticks (fractional).
    tick: f64,
    rate: u32,
    pub looping: bool,
}

#[cfg(feature = "synth")]
impl Sequencer {
    /// Load a .sf2 SoundFont.
    pub fn new(sf2: &std::path::Path, rate: u32) -> crate::Result<Self> {
        let bad = |e: &dyn std::fmt::Display| crate::Error::Bad(format!("{}: {e}", sf2.display()));
        let sf =
            rustysynth::SoundFont::new(&mut std::io::BufReader::new(std::fs::File::open(sf2)?)).map_err(|e| bad(&e))?;
        let synth =
            rustysynth::Synthesizer::new(&std::sync::Arc::new(sf), &rustysynth::SynthesizerSettings::new(rate as i32))
                .map_err(|e| bad(&e))?;
        Ok(Self { synth, events: vec![], pos: 0, tick: 0.0, rate, looping: true })
    }

    /// Start a song from the beginning (empty = silence).
    pub fn play(&mut self, events: Vec<Event>) {
        self.synth.reset();
        self.events = events;
        self.pos = 0;
        self.tick = 0.0;
    }

    pub fn set_volume(&mut self, v: f32) {
        self.synth.set_master_volume(v);
    }

    pub fn finished(&self) -> bool {
        self.pos >= self.events.len()
    }

    fn send(&mut self, e: Event) {
        let (cmd, ch) = ((e.status & 0xF0) as i32, (e.status & 0x0F) as i32);
        // MT-32 programs → GM (channel 10 is percussion on both).
        let d1 = if cmd == 0xC0 && ch != 9 { MT32_TO_GM[(e.d1 & 0x7F) as usize] } else { e.d1 };
        self.synth.process_midi_message(ch, cmd, d1 as i32, e.d2 as i32);
    }

    /// Fill stereo buffers, firing events on time; loops the song if `looping`.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        const BLOCK: usize = 64;
        let ticks_per_sample = TICKS_PER_SEC as f64 / self.rate as f64;
        let mut i = 0;
        while i < left.len() {
            while self.pos < self.events.len() && self.events[self.pos].tick as f64 <= self.tick {
                self.send(self.events[self.pos]);
                self.pos += 1;
            }
            if self.looping && !self.events.is_empty() && self.finished() {
                self.synth.note_off_all(false);
                self.pos = 0;
                self.tick = 0.0;
            }
            let n = BLOCK.min(left.len() - i);
            self.synth.render(&mut left[i..i + n], &mut right[i..i + n]);
            self.tick += n as f64 * ticks_per_sample;
            i += n;
        }
    }
}

/// Find a General MIDI SoundFont: `$U8_SOUNDFONT`, any .sf2 in the data dir (or its SOUND),
/// then in `extra` (the program's own folders, the settings folder…), then common system paths:
/// Linux's, Homebrew's, and macOS's own place for sound banks.
pub fn find_soundfont(data_dir: &std::path::Path, extra: &[std::path::PathBuf]) -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    if let Ok(p) = std::env::var("U8_SOUNDFONT") {
        return Some(PathBuf::from(p));
    }
    let sf2_in = |d: &std::path::Path| -> Option<PathBuf> {
        std::fs::read_dir(d)
            .ok()?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("sf2")))
    };
    let banks = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Audio/Sounds/Banks"));
    [data_dir.to_path_buf(), data_dir.join("SOUND")]
        .into_iter()
        .chain(extra.iter().cloned())
        .chain(banks)
        .chain([
            PathBuf::from("/usr/share/soundfonts"),
            PathBuf::from("/usr/share/sounds/sf2"),
            PathBuf::from("/usr/share/sounds/sf3"),
            PathBuf::from("/usr/local/share/soundfonts"),
            PathBuf::from("/opt/homebrew/share/soundfonts"),
        ])
        .find_map(|d| sf2_in(&d))
}
