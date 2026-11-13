use u8render::upscale::{nearest, xbrz, xbrz_par};

#[test]
fn upscalers_output_size() {
    let src = [255u8, 0, 0, 255].repeat(5 * 3);
    for f in 1..=8 {
        assert_eq!(xbrz(&src, 5, 3, f).len(), (5 * f * 3 * f * 4) as usize, "xbrz {f}");
        assert_eq!(nearest(&src, 5, 3, f).len(), (5 * f * 3 * f * 4) as usize, "nearest {f}");
    }
}

#[test]
fn xbrz_parallel_matches_serial() {
    let (w, h) = (97u32, 301u32);
    let src: Vec<u8> =
        (0..w * h).flat_map(|i| [(i * 37 % 251) as u8, (i / 7 % 256) as u8, (i % 13 * 19) as u8, 255]).collect();
    for f in [2, 3] {
        assert_eq!(xbrz(&src, w, h, f), xbrz_par(&src, w, h, f), "factor {f}");
    }
}

#[test]
#[ignore]
fn bench_xbrz_frame() {
    let (w, h) = (640u32, 400u32);
    let src: Vec<u8> = (0..w * h * 4).map(|i| (i.wrapping_mul(2654435761u32) >> 24) as u8 | 0x0F).collect();
    for f in [2, 3, 4] {
        let t = std::time::Instant::now();
        let out = xbrz_par(&src, w, h, f);
        println!("xbrz {f}x {w}x{h}: {:?} ({} bytes)", t.elapsed(), out.len());
    }
}

#[test]
fn sprite_cache_respects_budget() {
    // No game data needed: an empty cache over budget simply stays empty.
    let mut c = u8render::SpriteCache::unscaled().with_budget(0);
    c.begin_frame();
    assert_eq!(c.bytes(), 0);
}
