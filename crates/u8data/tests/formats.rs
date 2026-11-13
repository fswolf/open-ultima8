use u8data::{flex::Flex, shape::parse_shape};

fn flex(entries: &[&[u8]]) -> Vec<u8> {
    let mut d = vec![0x1a; 0x80];
    d[0x54..0x58].copy_from_slice(&(entries.len() as u32).to_le_bytes());
    let mut off = 0x80 + entries.len() * 8;
    let mut idx = vec![];
    for e in entries {
        idx.extend((off as u32).to_le_bytes());
        idx.extend((e.len() as u32).to_le_bytes());
        off += e.len();
    }
    d.extend(idx);
    for e in entries {
        d.extend(*e);
    }
    d
}

#[test]
fn flex_roundtrip() {
    let f = Flex::parse(flex(&[b"abc", b"", b"hello"])).unwrap();
    assert_eq!(f.len(), 3);
    assert_eq!(f.get(0), Some(&b"abc"[..]));
    assert_eq!(f.get(1), None);
    assert_eq!(f.get(2), Some(&b"hello"[..]));
}

#[test]
fn shape_rle() {
    // 1 frame, 3×1, compressed: skip 0, fill run of 2 × colour 7, then 1 literal colour 9.
    let mut s = vec![0, 0, 0, 0, 1, 0, 12, 0, 0, 0, 0, 0];
    let rle = [0u8, (2 << 1) | 1, 7, 0, 1 << 1, 9];
    s.extend([0u8; 8]);
    for v in [1i16, 3, 1, 0, 0] {
        s.extend(v.to_le_bytes());
    }
    s.extend(2u16.to_le_bytes()); // line offset (relative: +2 bytes for remaining offsets)
    s.extend(rle);
    let f = &parse_shape(&s).unwrap()[0];
    assert_eq!((f.width, f.height), (3, 1));
    assert_eq!(f.pixels, vec![7, 7, 9]);
    assert!(f.mask.iter().all(|&m| m));
}

#[test]
#[ignore] // needs U8 data: U8_DATA=/path/to/U8 cargo test -- --ignored
fn real_music() {
    let Ok(dir) = std::env::var("U8_DATA") else { return };
    let path = u8data::find_file(std::path::Path::new(&dir), "SOUND/MUSIC.FLX").unwrap();
    let f = u8data::flex::Flex::parse(std::fs::read(path).unwrap()).unwrap();
    let songs = u8data::music::songs(&f);
    assert!(songs.len() > 20);
    for s in &songs {
        let ev = u8data::music::parse_xmidi(f.get(s.index).unwrap(), 0).unwrap_or_default();
        let secs = ev.last().map_or(0, |e| e.tick) / u8data::music::TICKS_PER_SEC;
        println!("{:10} #{:3}: {:5} events, {:3}s", s.name, s.index, ev.len(), secs);
        assert!(!ev.is_empty(), "{} parsed empty", s.name);
    }
}

#[test]
#[ignore] // needs U8 data: U8_DATA=/path/to/U8 cargo test -- --ignored
fn usecode_decodes_end_to_end() {
    let Ok(dir) = std::env::var("U8_DATA") else { return };
    let (uc, _) = u8data::load_usecode(std::path::Path::new(&dir)).unwrap();
    let mut classes = 0;
    for c in 0..uc.class_count() as u32 {
        let Some(code) = uc.class(c) else { continue };
        let mut p = u8data::usecode::BASE + u8data::usecode::EVENT_COUNT * 4;
        while p < code.len() {
            p += 1 + u8data::usecode::operand_len(code, p).unwrap_or_else(|| panic!("class {c}: bad opcode at {p:#x}"));
        }
        assert_eq!(p, code.len(), "class {c} overran");
        classes += 1;
    }
    assert!(classes > 500);
}

/// Parsers must reject or tolerate garbage, never panic: random bytes, and real-looking headers
/// with truncated bodies.
#[test]
fn parsers_survive_garbage() {
    use u8data::{anim::AnimDat, map, music, palette::Palette, typeflag::TypeFlags, usecode};
    let mut seed = 0x2545_F491_u32;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    let tf = TypeFlags::parse(&[0u8; 8 * 2048]);
    let rounds = std::env::var("FUZZ_ROUNDS").ok().and_then(|r| r.parse().ok()).unwrap_or(400);
    for round in 0..rounds {
        let len = (rnd() % if round % 4 == 0 { 4096 } else { 200 }) as usize;
        let mut d: Vec<u8> = (0..len).map(|_| rnd() as u8).collect();
        if round % 3 == 0 && d.len() > 0x60 {
            // Plausible Flex header: small entry count.
            d[0x54..0x58].copy_from_slice(&(rnd() % 8).to_le_bytes());
        }
        let _ = parse_shape(&d);
        let _ = Palette::parse(&d);
        let _ = TypeFlags::parse(&d);
        let _ = AnimDat::parse(&d);
        let _ = music::parse_xmidi(&d, 0);
        let _ = u8data::sound::decode(&d);
        let _ = u8data::save::Save::parse(&[&b"Ultima 8 SaveGame File.\0"[..], &d].concat());
        let _ = u8data::save::avatar_name(&d);
        if d.len() > 0x30 {
            // A plausible Sonarc header with a garbage frame (checksum fixed up to pass).
            let mut e = d.clone();
            let size = ((e.len() - 0x20) as u16 & !1).to_le_bytes();
            e[0x20..0x22].copy_from_slice(&size);
            let _ = u8data::sound::decode(&e);
        }
        let globs = Flex::parse(d.clone()).map(|f| map::Globs::parse(&f));
        if let Ok(g) = &globs {
            let _ = map::load_fixed(&d, g, &tf);
        }
        let _ = map::load_nonfixed(&d, &tf);
        let _ = map::load_actors(&d, (rnd() % 64) as usize);
        let _ = map::load_actor(&d, (rnd() % 256) as usize);
        if let Ok(f) = Flex::parse(d.clone()) {
            let _ = music::songs(&f);
            let _ = u8data::font::Fonts::parse(&f);
            let uc = usecode::Usecode::new(f);
            for c in 0..uc.class_count().min(4) as u32 {
                let _ = uc.class_name(c);
                if let Some(code) = uc.class(c) {
                    let _ = usecode::disasm(code, usecode::BASE, |_| None);
                }
            }
        }
        for p in 0..d.len().min(64) {
            let _ = usecode::operand_len(&d, p);
        }
        let _ = usecode::disasm(&d, 0, |_| None);
        let _ = usecode::disasm(&[0x0D, 0x05, 0x00], 0, |_| None); // string runs off the end
    }
}

#[test]
#[ignore] // needs U8 data: U8_DATA=/path/to/U8 cargo test -- --ignored
fn real_data_mutations() {
    let Ok(dir) = std::env::var("U8_DATA") else { return };
    let rd = |p: &str| std::fs::read(u8data::find_file(std::path::Path::new(&dir), p).unwrap()).unwrap();
    let shapes = Flex::parse(rd("STATIC/U8SHAPES.FLX")).unwrap();
    let fixed = Flex::parse(rd("STATIC/FIXED.DAT")).unwrap();
    let tf = u8data::typeflag::TypeFlags::parse(&rd("STATIC/TYPEFLAG.DAT"));
    let globs = u8data::map::Globs::parse(&Flex::parse(rd("STATIC/GLOB.FLX")).unwrap());
    let mut seed = 0x9E37_79B9_u32;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as usize
    };
    for _ in 0..3000 {
        for src in [&shapes, &fixed] {
            let Some(e) = src.get(rnd() % src.len()) else { continue };
            let mut d = e.to_vec();
            for _ in 0..1 + rnd() % 8 {
                if !d.is_empty() {
                    let i = rnd() % d.len();
                    d[i] = rnd() as u8;
                }
            }
            d.truncate(rnd() % (d.len() + 1));
            let _ = parse_shape(&d);
            let _ = u8data::map::load_fixed(&d, &globs, &tf);
        }
    }
}

#[test]
#[ignore] // needs U8 data: U8_DATA=/path/to/U8 cargo test -- --ignored
fn real_sound_effects_decode() {
    let Ok(dir) = std::env::var("U8_DATA") else { return };
    let f =
        Flex::parse(std::fs::read(u8data::find_file(std::path::Path::new(&dir), "SOUND/SOUND.FLX").unwrap()).unwrap())
            .unwrap();
    let names = u8data::sound::names(&f);
    let (mut n, mut smooth) = (0, 0);
    for i in 1..f.len() {
        let Some(e) = f.get(i) else { continue };
        let s = u8data::sound::decode(e)
            .unwrap_or_else(|| panic!("effect {i} {} doesn't decode", names.get(i).map_or("", |n| n.as_str())));
        assert_eq!(s.pcm.len() as u32, u8data::sound::length(e).unwrap().0);
        // Real audio is mostly smooth: sample-to-sample change below its loudness (noise is ~2×).
        // A few effects (crickets, fizzing) are legitimately hissy, so judge them together.
        let x: Vec<f32> = s.to_f32();
        let (e2, d2) = x.windows(2).fold((0.0, 0.0), |(e, d), w| (e + w[1] * w[1], d + (w[1] - w[0]).powi(2)));
        smooth += (d2 < e2) as usize;
        n += 1;
    }
    assert!(n > 120);
    assert!(smooth * 10 > n * 8, "only {smooth} of {n} effects decode to smooth audio");
}

#[test]
fn save_files_unpack() {
    let mut d = b"Ultima 8 SaveGame File.\0".to_vec();
    d.extend(2u16.to_le_bytes());
    for (name, body) in [("AVATAR.DAT", &b"\0\0\0\0\0\0Ryan\0"[..]), ("flag.dat", &[1, 2, 3][..])] {
        d.extend((name.len() as u32 + 1).to_le_bytes());
        d.extend(name.as_bytes());
        d.push(0);
        d.extend((body.len() as u32).to_le_bytes());
        d.extend(body);
    }
    let s = u8data::save::Save::parse(&d).unwrap();
    assert_eq!(s.get("FLAG.DAT"), Some(&[1u8, 2, 3][..]));
    assert_eq!(u8data::save::avatar_name(s.get("avatar.dat").unwrap()).as_deref(), Some("Ryan"));
    assert!(u8data::save::Save::parse(&d[..d.len() - 1]).is_err(), "truncated");
}

#[test]
fn flex_and_saves_write_what_they_read() {
    use u8data::{flex, save::Save};
    let a = flex::build(None, &[Some(b"one"), None, Some(b"three")]);
    let f = flex::Flex::parse(a.clone()).unwrap();
    assert_eq!((f.len(), f.get(0), f.get(1), f.get(2)), (3, Some(&b"one"[..]), None, Some(&b"three"[..])));
    assert_eq!(flex::build(Some(f.header()), &[f.get(0), f.get(1), f.get(2)]), a);
    let mut s = Save::default();
    s.set("flag.dat", vec![1, 2, 3]);
    s.set_extra(&[("ticks", "1234".into())]);
    let back = Save::parse(&s.to_bytes()).unwrap();
    assert_eq!(back.get("FLAG.DAT"), Some(&[1u8, 2, 3][..]));
    assert_eq!(back.ticks(), Some(1234));
    let it = u8data::map::Item {
        x: 1000,
        y: 2000,
        z: 48,
        shape: 68,
        frame: 7,
        flags: 0x20,
        quality: 9,
        npcnum: 1,
        mapnum: 2,
        objid: 777,
        ..Default::default()
    };
    let mut rec = it.record().to_vec();
    rec.extend_from_slice(&it.record());
    let tf = u8data::typeflag::TypeFlags::parse(&[]);
    assert_eq!(u8data::map::load_nonfixed(&rec, &tf), vec![it, it]);
}
