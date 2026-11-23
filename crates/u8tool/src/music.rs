//! Audio exports: songs through a SoundFont, effects and speech decoded from Sonarc.
use crate::cli::Args;
use anyhow::{anyhow, Result};
use u8data::{flex::Flex, music, sound};

pub fn run(a: &Args) -> Result<()> {
    match a.get(1, "").as_str() {
        "sfx" => sfx(a),
        "speech" => speech(a),
        _ => music(a),
    }
}

/// `sfx list` or `sfx <n|name> [out.wav]`.
fn sfx(a: &Args) -> Result<()> {
    let flex = Flex::parse(std::fs::read(u8data::find_file(a.data(), "SOUND/SOUND.FLX")?)?)?;
    let names = sound::names(&flex);
    let want = a.get(2, "list");
    if want == "list" {
        for i in 1..flex.len() {
            if let Some(s) = flex.get(i).and_then(sound::decode) {
                say!("{i:3} {:8} {:5.2}s {} Hz", names.get(i).map_or("", String::as_str), s.seconds(), s.rate);
            }
        }
        return Ok(());
    }
    let i = want.parse().ok().or_else(|| names.iter().position(|n| n.eq_ignore_ascii_case(&want)));
    let i = i.ok_or_else(|| anyhow!("no effect {want:?}; try `sfx list`"))?;
    let s = flex.get(i).and_then(sound::decode).ok_or_else(|| anyhow!("effect {i} is empty or can't be decoded"))?;
    let out = a.get(3, "sfx.wav");
    std::fs::write(&out, wav8(&s))?;
    say!("wrote {out}: effect {i} {}, {:.2}s at {} Hz", names.get(i).map_or("", String::as_str), s.seconds(), s.rate);
    Ok(())
}

/// `speech <shape> list` or `speech <shape> <n> [out.wav]` from SOUND/E<shape>.FLX (or the
/// language played's).
fn speech(a: &Args) -> Result<()> {
    let shape: u32 = a.num(2, "44", "shape")?;
    let flex = Flex::parse(std::fs::read(u8data::lang::file(a.data(), &format!("SOUND/{{}}{shape}.FLX"))?)?)?;
    let phrases = sound::phrases(&flex);
    let want = a.get(3, "list");
    if want == "list" {
        for (n, p) in phrases.iter().enumerate() {
            let secs = flex.get(n + 1).and_then(sound::decode).map_or(0.0, |s| s.seconds());
            say!("{n:3} {secs:5.2}s {p}");
        }
        return Ok(());
    }
    let n: usize = crate::cli::num(&want, "phrase")?;
    let s = flex.get(n + 1).and_then(sound::decode).ok_or_else(|| anyhow!("phrase {n} has no audio"))?;
    let out = a.get(4, "speech.wav");
    std::fs::write(&out, wav8(&s))?;
    say!("wrote {out}: {:.2}s \"{}\"", s.seconds(), phrases.get(n).map_or("", String::as_str));
    Ok(())
}

/// 8-bit mono WAV.
fn wav8(s: &sound::Sample) -> Vec<u8> {
    let mut w = Vec::with_capacity(44 + s.pcm.len());
    w.extend(b"RIFF");
    w.extend((36 + s.pcm.len() as u32).to_le_bytes());
    w.extend(b"WAVEfmt ");
    for v in [16u32, 0x0001_0001, s.rate, s.rate, 0x0008_0001] {
        w.extend(v.to_le_bytes());
    }
    w.extend(b"data");
    w.extend((s.pcm.len() as u32).to_le_bytes());
    w.extend(&s.pcm);
    w
}

fn music(a: &Args) -> Result<()> {
    let flex = Flex::parse(std::fs::read(u8data::find_file(a.data(), "SOUND/MUSIC.FLX")?)?)?;
    let songs = music::songs(&flex);
    let want = a.get(2, "list");
    if want == "list" {
        for s in &songs {
            say!("{:10} (#{})", s.name, s.index);
        }
        return Ok(());
    }
    let song = songs.iter().find(|s| s.name == want).ok_or_else(|| anyhow!("no song {want:?}; try `music list`"))?;
    let events =
        music::parse_xmidi(flex.get(song.index).unwrap_or(&[]), 0).ok_or_else(|| anyhow!("couldn't decode {want}"))?;
    let sf = music::find_soundfont(a.data(), &[])
        .ok_or_else(|| anyhow!("no SoundFont found; set U8_SOUNDFONT=/path/to/GM.sf2"))?;
    let rate = 44100u32;
    let mut seq = music::Sequencer::new(&sf, rate)?;
    seq.looping = false;
    seq.play(events);
    let secs: u32 = a.num(4, "30", "seconds")?;
    let n = (rate * secs) as usize;
    let (mut l, mut r) = (vec![0f32; n], vec![0f32; n]);
    seq.render(&mut l, &mut r);
    let out = a.get(3, "music.wav");
    std::fs::write(&out, wav(&l, &r, rate))?;
    let peak = l.iter().chain(&r).fold(0f32, |m, v| m.max(v.abs()));
    say!("wrote {out}: {} {secs}s with {} (peak {peak:.2})", song.name, sf.display());
    Ok(())
}

/// 16-bit stereo PCM WAV.
fn wav(l: &[f32], r: &[f32], rate: u32) -> Vec<u8> {
    let pcm: Vec<u8> = l
        .iter()
        .zip(r)
        .flat_map(|(a, b)| [*a, *b])
        .flat_map(|v| ((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())
        .collect();
    let mut w = Vec::with_capacity(44 + pcm.len());
    w.extend(b"RIFF");
    w.extend((36 + pcm.len() as u32).to_le_bytes());
    w.extend(b"WAVEfmt ");
    for v in [16u32, 0x0002_0001, rate, rate * 4, 0x0010_0004] {
        w.extend(v.to_le_bytes());
    }
    w.extend(b"data");
    w.extend((pcm.len() as u32).to_le_bytes());
    w.extend(pcm);
    w
}
