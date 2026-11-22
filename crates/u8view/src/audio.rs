//! Audio: music (MUSIC.FLX through a SoundFont), sound effects (SOUND.FLX) and speech
//! (SOUND/E<shape>.FLX), mixed into one cpal stream. Effects can be placed at an item: the
//! viewer updates their volume and pan as the camera moves.
use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use u8data::{flex::Flex, music, sound};
use u8engine::ObjId;

/// What a voice is playing, so it can be found again to stop or move it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Sfx(u16),
    Speech,
}

struct Voice {
    kind: Kind,
    /// Item it comes from (0 = everywhere).
    item: ObjId,
    pcm: Arc<Vec<f32>>,
    pos: f64,
    /// Source samples per output sample.
    step: f64,
    looping: bool,
    /// Script-set volume (0–1) and placement gains (left, right).
    volume: f32,
    gain: [f32; 2],
}

#[derive(Default)]
struct Mixer {
    seq: Option<music::Sequencer>,
    /// A song to go on to when the one playing (not looping) ends: after the fanfare.
    next: Option<Vec<music::Event>>,
    voices: Vec<Voice>,
    sfx_gain: f32,
}

impl Mixer {
    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        match &mut self.seq {
            Some(s) => {
                s.render(l, r);
                if s.finished() {
                    if let Some(next) = self.next.take() {
                        s.play(next);
                        s.looping = true;
                    }
                }
            }
            None => l.iter_mut().chain(r.iter_mut()).for_each(|v| *v = 0.0),
        }
        let g = self.sfx_gain;
        self.voices.retain_mut(|v| {
            let n = v.pcm.len();
            for (a, b) in l.iter_mut().zip(r.iter_mut()) {
                if v.pos >= n as f64 {
                    if !v.looping || n == 0 {
                        return false;
                    }
                    v.pos -= n as f64;
                }
                let i = v.pos as usize;
                let f = (v.pos - i as f64) as f32;
                let s = v.pcm[i] * (1.0 - f) + v.pcm[(i + 1) % n] * f;
                let s = s * v.volume * g;
                *a += s * v.gain[0];
                *b += s * v.gain[1];
                v.pos += v.step;
            }
            true
        });
    }
}

/// Decoded samples and their rate.
type Clip = (Arc<Vec<f32>>, u32);

/// Decoded clips, loaded on first use.
struct Bank {
    flex: Flex,
    clips: HashMap<usize, Option<Clip>>,
}

impl Bank {
    fn open(path: Option<PathBuf>) -> Option<Self> {
        let flex = Flex::parse(std::fs::read(path?).ok()?).ok()?;
        Some(Self { flex, clips: HashMap::new() })
    }
    fn clip(&mut self, i: usize) -> Option<Clip> {
        let flex = &self.flex;
        self.clips
            .entry(i)
            .or_insert_with(|| flex.get(i).and_then(sound::decode).map(|s| (Arc::new(s.to_f32()), s.rate)))
            .clone()
    }
}

/// MUSIC.FLX songs: fighting, an enemy falling, the Avatar dying (ScummVM's constants).
/// Background sounds: looping effects (fireplaces, fountains: the scripts' playAmbientSFX) and
/// the ones that repeat on their own ([`QUIETER`]), at a quarter of other effects, times the
/// player's "Background sounds" setting. The original plays its loops at full volume and other
/// effects at half (ScummVM's AudioProcess: 0xff against 0x80), so a fireplace drowns out
/// everything near it; Ryan finds them far too loud there and here.
const AMBIENT: f32 = 0.25;

/// Effects that repeat on their own and grate: background sounds too.
const QUIETER: [u16; 2] = [
    0x7a, // the hourglass's tick (HOURGLAS, as it turns; nothing else plays it)
    0xa3, // dripping water (DRIPS, DRIPS2: each drip as it animates)
];

/// Left and right gains for a sound at `vol` (0–1) panned `pan` (−1 left … 1 right).
fn gain(vol: f32, pan: f32) -> [f32; 2] {
    [vol * (1.0 - pan.max(0.0) * 0.7), vol * (1.0 + pan.min(0.0) * 0.7)]
}

/// Is `sfx` a background sound?
fn background(sfx: u16, looping: bool) -> bool {
    looping || QUIETER.contains(&sfx)
}

/// Effect `sfx`'s volume: what the script set (1 if nothing), less for a background sound
/// (`ambience`: the player's setting for those, 0 to 1).
fn sfx_level(sfx: u16, script: Option<f32>, looping: bool, ambience: f32) -> f32 {
    script.unwrap_or(1.0) * if background(sfx, looping) { AMBIENT * ambience } else { 1.0 }
}
const COMBAT_SONG: usize = 98;
const FANFARE_SONG: usize = 109;
const DEATH_SONG: usize = 44;

pub struct Audio {
    _stream: cpal::Stream,
    mix: Arc<Mutex<Mixer>>,
    rate: u32,
    data: PathBuf,
    music: Option<Flex>,
    pub songs: Vec<music::Song>,
    pub current: Option<usize>,
    pub volume: f32,
    /// Music switched off by the player.
    off: bool,
    /// The song scripts last asked for (resumed when music is switched back on).
    wanted: Option<usize>,
    effects: Option<Bank>,
    pub sfx_on: bool,
    /// A SoundFont was found (music can play).
    pub music_ready: bool,
    /// Fighting (or dead): the combat music holds, the game's songs wait.
    combat: bool,
    /// Script-set volume per effect.
    sfx_volume: HashMap<u16, f32>,
    /// The player's level for background sounds (fires, drips, fountains), 0–1.
    pub ambience: f32,
    speech: HashMap<u32, Option<(Bank, Vec<String>)>>,
}

impl Audio {
    /// Opens the output device. Music needs a SoundFont; effects and speech work without one.
    pub fn new(data: &Path) -> Result<Self> {
        let device = cpal::default_host().default_output_device().ok_or_else(|| anyhow!("no audio output device"))?;
        let cfg = device.default_output_config()?;
        let rate = cfg.sample_rate();
        let mix = Arc::new(Mutex::new(Mixer { sfx_gain: 0.8, ..Default::default() }));
        let music =
            u8data::find_file(data, "SOUND/MUSIC.FLX").ok().and_then(|p| Flex::parse(std::fs::read(p).ok()?).ok());
        let songs = music.as_ref().map(music::songs).unwrap_or_default();
        // (Also beside the program, a few folders up as the game is looked for, and with the settings.)
        let mut near: Vec<PathBuf> = std::env::current_exe()
            .map(|e| e.ancestors().skip(1).take(4).map(PathBuf::from).collect())
            .unwrap_or_default();
        near.extend(crate::settings::dir());
        match music::find_soundfont(data, &near) {
            Some(sf) => match music::Sequencer::new(&sf, rate) {
                Ok(seq) => {
                    eprintln!("music: {} songs, SoundFont {}, {rate} Hz", songs.len(), sf.display());
                    mix.lock().unwrap().seq = Some(seq);
                }
                Err(e) => eprintln!("music disabled: {e}"),
            },
            None => eprintln!(
                "music disabled: no SoundFont: put a General MIDI .sf2 in the game folder, next to the program or in {} (Fedora: `sudo dnf install fluid-soundfont-gm`), or set U8_SOUNDFONT=/path/to/GM.sf2",
                crate::settings::dir().map_or("the settings folder".into(), |d| d.display().to_string())
            ),
        }
        let effects = Bank::open(u8data::find_file(data, "SOUND/SOUND.FLX").ok());
        if effects.is_none() {
            eprintln!("sound effects disabled: no SOUND/SOUND.FLX");
        }
        let stream = match cfg.sample_format() {
            cpal::SampleFormat::F32 => build::<f32>(&device, cfg.into(), mix.clone())?,
            cpal::SampleFormat::I16 => build::<i16>(&device, cfg.into(), mix.clone())?,
            cpal::SampleFormat::I32 => build::<i32>(&device, cfg.into(), mix.clone())?,
            cpal::SampleFormat::U16 => build::<u16>(&device, cfg.into(), mix.clone())?,
            f => return Err(anyhow!("unsupported audio sample format {f}")),
        };
        stream.play()?;
        let music_ready = mix.lock().unwrap().seq.is_some();
        Ok(Self {
            music_ready,
            _stream: stream,
            mix,
            rate,
            data: data.to_path_buf(),
            music,
            songs,
            current: None,
            volume: 0.5,
            off: false,
            wanted: None,
            combat: false,
            effects,
            sfx_on: true,
            sfx_volume: HashMap::new(),
            ambience: 1.0,
            speech: HashMap::new(),
        })
    }

    fn mixer(&self) -> std::sync::MutexGuard<'_, Mixer> {
        self.mix.lock().unwrap_or_else(|e| e.into_inner())
    }

    // --- music ---

    /// Play song `i` of `songs` (None = stop).
    pub fn play(&mut self, i: Option<usize>) {
        let events = i
            .and_then(|i| self.songs.get(i))
            .and_then(|s| music::parse_xmidi(self.music.as_ref()?.get(s.index)?, 0))
            .unwrap_or_default();
        let volume = self.volume;
        let mut mix = self.mixer();
        mix.next = None;
        let Some(seq) = &mut mix.seq else { return };
        seq.play(events.clone());
        seq.looping = true;
        seq.set_volume(volume);
        drop(mix);
        self.current = i.filter(|_| !events.is_empty());
    }

    /// Play MUSIC.FLX entry `index` (what scripts ask for; 0 stops). Leaves it alone if it's already
    /// playing or the player switched music off.
    pub fn play_track(&mut self, index: usize) {
        let i = self.songs.iter().position(|s| s.index == index);
        self.wanted = i;
        if !self.off && !self.combat && self.current != i && (i.is_some() || index == 0) {
            self.play(i);
        }
    }

    /// The Avatar fights (combat music, MUSIC.FLX 98, as ScummVM's MainActor::setInCombat) or
    /// stops (back to what the game last asked for).
    pub fn set_combat(&mut self, on: bool) {
        if on == self.combat {
            return;
        }
        self.combat = on;
        if !self.off {
            let song = if on { self.songs.iter().position(|s| s.index == COMBAT_SONG) } else { self.wanted };
            self.play(song);
        }
    }

    /// An enemy fell: the fanfare (109) once, then the combat music again.
    pub fn fanfare(&mut self) {
        let song = |n: usize| self.songs.iter().position(|s| s.index == n);
        let (Some(f), Some(c)) = (song(FANFARE_SONG), song(COMBAT_SONG)) else { return };
        if self.off || !self.combat {
            return;
        }
        self.play(Some(f));
        let back =
            music::parse_xmidi(self.music.as_ref().and_then(|m| m.get(self.songs[c].index)).unwrap_or_default(), 0);
        let mut mix = self.mixer();
        if let Some(seq) = &mut mix.seq {
            seq.looping = false;
        }
        mix.next = back;
    }

    /// The Avatar died: its lament (44), and the game's songs wait.
    pub fn death(&mut self) {
        self.combat = true;
        if !self.off {
            self.play(self.songs.iter().position(|s| s.index == DEATH_SONG));
        }
    }

    /// The MUSIC.FLX entry the game last asked for (to come back to after the credits).
    pub fn wanted_track(&self) -> usize {
        self.wanted.and_then(|i| self.songs.get(i)).map_or(0, |s| s.index)
    }

    pub fn next(&mut self, step: isize) {
        if self.songs.is_empty() {
            return;
        }
        let n = self.songs.len() as isize;
        let i = self.current.map_or(0, |c| (c as isize + step).rem_euclid(n) as usize);
        self.off = false;
        self.play(Some(i));
    }

    /// Music off, or back on with the song the map wants (else `fallback`).
    pub fn toggle(&mut self, fallback: usize) {
        self.off = !self.off;
        self.play(if self.off { None } else { Some(self.wanted.unwrap_or(fallback)) });
    }

    pub fn music_on(&self) -> bool {
        !self.off
    }

    /// Music off until switched back on (scripts' songs are ignored meanwhile).
    pub fn music_off(&mut self) {
        self.off = true;
        self.play(None);
    }

    pub fn change_volume(&mut self, d: f32) {
        self.volume = (self.volume + d).clamp(0.0, 1.0);
        let v = self.volume;
        if let Some(s) = &mut self.mixer().seq {
            s.set_volume(v);
        }
    }

    /// The music at `level` (0–1) of its volume, while a film fades it out.
    pub fn set_music_level(&mut self, level: f32) {
        let v = self.volume * level.clamp(0.0, 1.0);
        if let Some(s) = &mut self.mixer().seq {
            s.set_volume(v);
        }
    }

    // --- effects ---

    /// Start effect `sfx` (placed at `item` unless 0, at `at`: (volume, pan) where it is now, if
    /// known); a looping one replaces itself.
    pub fn sfx(&mut self, sfx: u16, item: ObjId, looping: bool, at: Option<(f32, f32)>) {
        if !self.sfx_on {
            return;
        }
        let Some((pcm, rate)) = self.effects.as_mut().and_then(|b| b.clip(sfx as usize)) else { return };
        let volume = sfx_level(sfx, self.sfx_volume.get(&sfx).copied(), looping, self.ambience);
        let step = rate as f64 / self.rate as f64;
        let mut mix = self.mixer();
        if looping {
            mix.voices.retain(|v| (v.kind, v.item) != (Kind::Sfx(sfx), item));
        }
        // (Placed from the start: a far drip doesn't tick at full volume for a frame.)
        let gain = at.map_or([1.0; 2], |(vol, pan)| gain(vol, pan));
        mix.voices.push(Voice { kind: Kind::Sfx(sfx), item, pcm, pos: 0.0, step, looping, volume, gain });
    }

    pub fn stop_sfx(&mut self, sfx: u16, item: Option<ObjId>) {
        self.mixer().voices.retain(|v| !(v.kind == Kind::Sfx(sfx) && item.is_none_or(|i| i == v.item)));
    }

    pub fn set_sfx_volume(&mut self, sfx: u16, volume: u8) {
        let v = volume as f32 / 255.0;
        self.sfx_volume.insert(sfx, v);
        let mut mix = self.mixer();
        for x in mix.voices.iter_mut().filter(|x| x.kind == Kind::Sfx(sfx)) {
            x.volume = sfx_level(sfx, Some(v), x.looping, self.ambience);
        }
    }

    /// The player's level for background sounds (0–1): those playing follow at once.
    pub fn set_ambience(&mut self, level: f32) {
        self.ambience = level.clamp(0.0, 1.0);
        let mut mix = self.mixer();
        for x in mix.voices.iter_mut() {
            if let Kind::Sfx(sfx) = x.kind {
                x.volume = sfx_level(sfx, self.sfx_volume.get(&sfx).copied(), x.looping, self.ambience);
            }
        }
    }

    pub fn toggle_sfx(&mut self) {
        self.sfx_on = !self.sfx_on;
        if !self.sfx_on {
            self.mixer().voices.clear();
        }
    }

    /// Re-place every voice tied to an item: `place(item)` gives (volume, pan −1…1), or None to
    /// leave it where it was.
    pub fn place(&mut self, place: impl Fn(ObjId) -> Option<(f32, f32)>) {
        for v in self.mixer().voices.iter_mut().filter(|v| v.item != 0) {
            if let Some((vol, pan)) = place(v.item) {
                v.gain = gain(vol, pan);
            }
        }
    }

    // --- speech ---

    /// Speak a bark if its speaker (`shape`) has recorded lines matching the text. Returns how
    /// long it lasts.
    pub fn speak(&mut self, shape: u32, text: &str, item: ObjId) -> Option<f32> {
        let data = self.data.clone();
        let (bank, phrases) = self
            .speech
            .entry(shape)
            .or_insert_with(|| {
                let bank = Bank::open(u8data::find_file(&data, &format!("SOUND/E{shape}.FLX")).ok())?;
                let phrases = sound::phrases(&bank.flex);
                Some((bank, phrases))
            })
            .as_mut()?;
        let lines = match_phrases(phrases, text);
        if lines.is_empty() {
            return None;
        }
        let (mut pcm, mut rate) = (vec![], 11111);
        for n in lines {
            if let Some((clip, r)) = bank.clip(n + 1) {
                pcm.extend(clip.iter());
                pcm.extend(std::iter::repeat_n(0.0, r as usize / 8)); // a breath between lines
                rate = r;
            }
        }
        let secs = pcm.len() as f32 / rate as f32;
        let step = rate as f64 / self.rate as f64;
        let mut mix = self.mixer();
        mix.voices.retain(|v| v.kind != Kind::Speech);
        let voice = Voice {
            kind: Kind::Speech,
            item,
            pcm: Arc::new(pcm),
            pos: 0.0,
            step,
            looping: false,
            volume: 1.2,
            gain: [1.0; 2],
        };
        mix.voices.push(voice);
        Some(secs)
    }

    /// Play a recorded line as it is (the films' speech).
    pub fn say(&mut self, s: &sound::Sample) {
        let step = s.rate as f64 / self.rate as f64;
        let pcm = Arc::new(s.to_f32());
        let mut mix = self.mixer();
        mix.voices.retain(|v| v.kind != Kind::Speech);
        let gain = [1.0; 2];
        mix.voices.push(Voice { kind: Kind::Speech, item: 0, pcm, pos: 0.0, step, looping: false, volume: 1.2, gain });
    }

    pub fn stop_speech(&mut self) {
        self.mixer().voices.retain(|v| v.kind != Kind::Speech);
    }

    /// Stop every effect and line (e.g. leaving a map).
    pub fn hush(&mut self) {
        self.mixer().voices.clear();
    }
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// The recorded phrases a bark consists of, in order: greedily the longest phrase the remaining
/// text starts with. Empty unless the whole text is covered.
fn match_phrases(phrases: &[String], text: &str) -> Vec<usize> {
    let norm_phrases: Vec<String> = phrases.iter().map(|p| norm(p)).collect();
    let mut rest = norm(text);
    let mut out = vec![];
    while !rest.is_empty() {
        let best = norm_phrases
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.is_empty() && rest.starts_with(p.as_str()))
            .max_by_key(|(_, p)| p.len());
        let Some((n, p)) = best else { return vec![] };
        out.push(n);
        rest = rest[p.len()..].trim_start().to_string();
    }
    out
}

fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    cfg: cpal::StreamConfig,
    mix: Arc<Mutex<Mixer>>,
) -> Result<cpal::Stream> {
    let ch = cfg.channels as usize;
    let (mut l, mut r) = (Vec::<f32>::new(), Vec::<f32>::new());
    Ok(device.build_output_stream(
        cfg,
        move |out: &mut [T], _| {
            let frames = out.len() / ch.max(1);
            l.resize(frames, 0.0);
            r.resize(frames, 0.0);
            match mix.lock() {
                Ok(mut m) => m.render(&mut l, &mut r),
                Err(_) => l.iter_mut().chain(r.iter_mut()).for_each(|v| *v = 0.0),
            }
            for (i, frame) in out.chunks_mut(ch).enumerate() {
                for (c, v) in frame.iter_mut().enumerate() {
                    let s = if c % 2 == 0 { l[i] } else { r[i] };
                    *v = T::from_sample(s.clamp(-1.0, 1.0));
                }
            }
        },
        |e| eprintln!("audio: {e}"),
        None,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(pcm: Vec<f32>, step: f64, looping: bool) -> Voice {
        Voice {
            kind: Kind::Sfx(1),
            item: 0,
            pcm: Arc::new(pcm),
            pos: 0.0,
            step,
            looping,
            volume: 1.0,
            gain: [1.0, 0.5],
        }
    }

    #[test]
    fn mixer_resamples_pans_and_retires_voices() {
        let mut m = Mixer { sfx_gain: 1.0, ..Default::default() };
        m.voices.push(voice(vec![0.0, 1.0], 0.5, false));
        let (mut l, mut r) = (vec![0.0; 6], vec![0.0; 6]);
        m.render(&mut l, &mut r);
        assert_eq!(&l[..4], &[0.0, 0.5, 1.0, 0.5]); // linear interpolation at half speed
        assert_eq!(r[2], 0.5); // right gain
        assert!(m.voices.is_empty(), "a one-shot ends");
        m.voices.push(voice(vec![0.25], 1.0, true));
        m.render(&mut l, &mut r);
        assert!(l.iter().all(|&v| v == 0.25) && m.voices.len() == 1, "a loop keeps going");
    }

    #[test]
    fn background_sounds_are_quieter() {
        // Ryan: fireplaces (playAmbientSFX's loops) were far too loud next to everything else.
        assert_eq!(sfx_level(1, None, false, 0.0), 1.0, "other effects aren't background");
        assert_eq!(sfx_level(0x86, None, true, 1.0), 0.25);
        assert_eq!(sfx_level(0x86, Some(0.5), true, 1.0), 0.125, "a script's own volume still applies");
        assert_eq!(sfx_level(0x86, None, true, 0.5), 0.125, "and the player's background level");
        // And the dripping (one drip at a time) as a background sound too.
        assert_eq!(sfx_level(0xa3, None, false, 1.0), 0.25);
        // And the hourglass's tick.
        assert_eq!(sfx_level(0x7a, None, false, 1.0), 0.25);
    }

    /// The Guardian's lines (barked from object 666) are recorded in SOUND/E666.FLX.
    #[test]
    #[ignore]
    fn the_guardian_has_a_voice() {
        let Ok(dir) = std::env::var("U8_DATA") else { return };
        let f = u8data::find_file(Path::new(&dir), &format!("SOUND/E{}.FLX", crate::ui::GUARDIAN)).unwrap();
        let p = sound::phrases(&Flex::parse(std::fs::read(f).unwrap()).unwrap());
        assert!(!match_phrases(&p, "I -really- do appreciate your plight, Avatar. ").is_empty(), "{p:?}");
    }

    #[test]
    fn barks_split_into_recorded_phrases() {
        let p =
            vec!["Who dares?".to_string(), "Indeed, piteous mortal,".into(), "Indeed,".into(), "I am Hydros.".into()];
        assert_eq!(match_phrases(&p, "Indeed, piteous  mortal, I am Hydros."), vec![1, 3]);
        assert_eq!(match_phrases(&p, "who dares?"), vec![0]);
        assert!(match_phrases(&p, "Who dares? Nobody.").is_empty());
    }
}
