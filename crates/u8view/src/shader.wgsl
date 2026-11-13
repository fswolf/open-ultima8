// The presenter's shader (see gpu.rs): one fullscreen triangle per layer; each screen pixel finds
// its texel and filters it (plain, sharp bilinear, xBR or CRT), then the game's fade and gamma.
struct U {
    // texels per screen pixel; the filter (0 plain, 1 sharp, 2 xBR, 3 CRT)
    k: f32, look: f32, gamma: f32, linearize: f32,
    off: vec2<f32>, size: vec2<f32>,
    m: array<vec4<f32>, 3>,
    // how far the screen has turned upside down (0–65535, 32768 over), and its height in pixels
    inv: f32, h: f32, pad: vec2<f32>,
};
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
@group(0) @binding(2) var<uniform> u: U;
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

fn texel(c: vec2<i32>) -> vec3<f32> {
    let d = vec2<i32>(textureDimensions(tex)) - 1;
    return textureLoad(tex, clamp(c, vec2<i32>(0), d), 0).rgb;
}
// How different two colours look (brightness-weighted).
fn df(a: vec3<f32>, b: vec3<f32>) -> f32 { return dot(abs(a - b), vec3<f32>(0.299, 0.587, 0.114)); }
fn eq(a: vec3<f32>, b: vec3<f32>) -> bool { return df(a, b) < 0.06; }

// The 5×5 texels around the one being drawn (each read once for all four corners).
alias Around = array<vec3<f32>, 25>;
fn at(n: ptr<function, Around>, s: vec2<i32>, x: i32, y: i32) -> vec3<f32> {
    let o = vec2<i32>(x, y) * s + 2;
    return (*n)[o.y * 5 + o.x];
}

// xBR (after Hyllian's level 2): does an edge cut the middle texel's corner towards `s` (±1, ±1),
// and how much of it at `f` (the point in the texel, mirrored so that corner is at 1, 1)? The
// colour across the edge, and how much of it shows.
fn corner(n: ptr<function, Around>, s: vec2<i32>, f: vec2<f32>, aa: f32) -> vec4<f32> {
    let e = at(n, s, 0, 0);
    let ff = at(n, s, 1, 0);
    let h = at(n, s, 0, 1);
    let i = at(n, s, 1, 1);
    let cc = at(n, s, 1, -1);
    let g = at(n, s, -1, 1);
    let b = at(n, s, 0, -1);
    let d = at(n, s, -1, 0);
    let f4 = at(n, s, 2, 0);
    let i4 = at(n, s, 2, 1);
    let h5 = at(n, s, 0, 2);
    let i5 = at(n, s, 1, 2);
    // Along the edge (F–H) against across it (E–I), with the neighbours that continue each.
    let along = df(e, cc) + df(e, g) + df(i, f4) + df(i, h5) + 4.0 * df(h, ff);
    let across = df(h, d) + df(h, i5) + df(ff, i4) + df(ff, b) + 4.0 * df(e, i);
    let allowed = !eq(e, ff) && !eq(e, h)
        && ((!eq(ff, b) && !eq(h, d)) || (eq(e, i) && !eq(ff, i4) && !eq(h, i5)) || eq(e, g) || eq(e, cc));
    if (!(along < across && allowed)) {
        return vec4<f32>(e, 0.0);
    }
    // 45°, or shallow (F–G) / steep (H–C) at 2:1.
    var cover = smoothstep(1.5 - aa, 1.5 + aa, f.x + f.y);
    if (2.0 * df(ff, g) <= df(h, cc) && !eq(e, g) && !eq(d, g)) {
        cover = max(cover, smoothstep(1.0 - aa, 1.0 + aa, f.y + 0.5 * f.x));
    }
    if (df(ff, g) >= 2.0 * df(h, cc) && !eq(e, cc) && !eq(b, cc)) {
        cover = max(cover, smoothstep(2.0 - aa, 2.0 + aa, f.y + 2.0 * f.x));
    }
    return vec4<f32>(select(h, ff, df(e, ff) <= df(e, h)), cover);
}

fn xbr(t: vec2<f32>) -> vec3<f32> {
    let base = floor(t);
    let f = t - base;
    let c = vec2<i32>(base);
    var n: Around;
    for (var y = 0; y < 5; y++) {
        for (var x = 0; x < 5; x++) {
            n[y * 5 + x] = texel(c + vec2<i32>(x - 2, y - 2));
        }
    }
    // Smoothing about half a screen pixel wide.
    let aa = max(0.5 * u.k, 0.02);
    var best = vec4<f32>(n[12], 0.0);
    for (var q = 0; q < 4; q++) {
        let s = vec2<i32>(select(-1, 1, (q & 1) == 0), select(-1, 1, (q & 2) == 0));
        let fm = select(1.0 - f, f, s > vec2<i32>(0));
        let r = corner(&n, s, fm, aa);
        if (r.w > best.w) { best = r; }
    }
    return mix(n[12], best.rgb, best.w);
}

// A CRT's look: each row of texels a soft beam (brighter colours wider), dark between, and a faint
// shadow mask.
fn crt(t: vec2<f32>, pos: vec2<f32>) -> vec3<f32> {
    let row = floor(t.y);
    let x = floor(t.x - 0.5) + 0.5 + clamp((fract(t.x - 0.5) - 0.5) / u.k + 0.5, 0.0, 1.0);
    var c = vec3<f32>(0.0);
    for (var r = -1; r <= 1; r++) {
        let y = row + f32(r) + 0.5;
        let col = textureSampleLevel(tex, smp, vec2<f32>(x, y) / u.size, 0.0).rgb;
        let dist = t.y - y;
        let w = mix(0.24, 0.36, dot(col, vec3<f32>(0.3, 0.59, 0.11)));
        c += col * exp(-dist * dist / (2.0 * w * w));
    }
    let lane = u32(pos.x) % 3u;
    var mask = vec3<f32>(0.85);
    mask[lane] = 1.0;
    // (Too small for rows to show: just the picture.)
    let strength = clamp(1.0 / u.k - 1.5, 0.0, 1.0);
    let plain = textureSampleLevel(tex, smp, vec2<f32>(x, t.y) / u.size, 0.0).rgb;
    // (The beams fill about half of each row: brighter to make up.)
    return mix(plain, min(c * 1.6, vec3<f32>(1.0)) * mask, strength);
}

// invertScreen (ScummVM's InverterGump): which screen row shows on row `i`, the rows
// interleaved as the picture turns over.
fn inverted_row(i: i32) -> i32 {
    let h = i32(u.h);
    let state = i32(u.inv);
    if (state == 0) { return i; }
    if (state == 32768) { return h - 1 - i; }
    let n = max(h / 2, 1);
    let t = (state * h) / 65536;
    let index = select(2 * n - 1 - i / 2, i / 2, i % 2 == 0);
    let j = (index + t) % (2 * n);
    return select(2 * j, 2 * n - 1 - 2 * (j - n), j >= n);
}

@fragment fn fs(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let pos = vec2<f32>(frag.x, f32(inverted_row(i32(frag.y))) + 0.5);
    var t = pos.xy * u.k + u.off;
    var s: vec4<f32>;
    if (u.look > 2.5) {
        s = vec4<f32>(crt(t, pos.xy), 1.0);
    } else if (u.look > 1.5) {
        s = vec4<f32>(xbr(t), 1.0);
    } else {
        if (u.look > 0.5) {
            // Sharp bilinear: whole texels stay crisp at any zoom, only their edges blend.
            let c = floor(t - 0.5);
            let f = clamp((t - 0.5 - c - 0.5) / u.k + 0.5, vec2<f32>(0.0), vec2<f32>(1.0));
            t = c + 0.5 + f;
        }
        s = textureSampleLevel(tex, smp, t / u.size, 0.0);
    }
    // Premultiplied: back to straight colour for the fade and gamma.
    var c = s.rgb / max(s.a, 1e-4);
    // The game's palette fade (rows of r, g, b weights and a constant).
    c = clamp(vec3<f32>(dot(u.m[0].xyz, c) + u.m[0].w, dot(u.m[1].xyz, c) + u.m[1].w, dot(u.m[2].xyz, c) + u.m[2].w), vec3<f32>(0.0), vec3<f32>(1.0));
    // Colour fix for displays that sRGB-encode our already-sRGB pixels (washed-out look).
    if (u.linearize > 0.5) {
        c = select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
    }
    return vec4<f32>(pow(c, vec3<f32>(u.gamma)) * s.a, s.a);
}
