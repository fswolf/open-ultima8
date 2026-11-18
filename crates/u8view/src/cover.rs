//! Ours on top of the original's book (U8GUMPS 6, which is only the open pages and the edge of
//! their boards): a leather cover round it with its shadow, and the bottom corner of a page turned
//! up where there's a page to turn to (the left one back, the right one on). Drawn at the
//! screen's own resolution like the page text; sizes are in game pixels.
use u8render::Canvas;

/// The cover's reach past the art (left and right, top, bottom).
const RIM: (f32, f32, f32) = (5.0, 3.0, 4.0);
/// Its corners' radius.
const ROUND: f32 = 4.0;
/// Leather, top to bottom, and the gilt line tooled round it.
const LEATHER: ([f32; 3], [f32; 3]) = ([104.0, 58.0, 26.0], [62.0, 32.0, 10.0]);
const GILT: [f32; 3] = [200.0, 156.0, 76.0];
/// A turned-up corner's size.
pub const CURL: f32 = 10.0;
/// The rounding of its tip.
const TIP: f32 = 2.5;
/// The back of a page: by the fold, and at the tip.
const BACK: ([f32; 3], [f32; 3]) = ([246.0, 224.0, 184.0], [212.0, 180.0, 134.0]);
const EDGE: [f32; 3] = [126.0, 92.0, 52.0];

/// How much of a screen pixel a shape covers, from its signed distance (game pixels, < 0 inside).
fn cover_of(d: f32, ss: f32) -> f32 {
    (0.5 - d * ss).clamp(0.0, 1.0)
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t)
}

/// Lay `rgb` over a pixel, `a` of the way (the canvas is premultiplied).
fn blend(c: &mut Canvas, x: i32, y: i32, rgb: [f32; 3], a: f32) {
    if a <= 0.0 || x < 0 || y < 0 || x >= c.w as i32 || y >= c.h as i32 {
        return;
    }
    let d = ((y as u32 * c.w + x as u32) * 4) as usize;
    for (k, v) in rgb.into_iter().enumerate() {
        c.px[d + k] = (v * a + c.px[d + k] as f32 * (1.0 - a)).round() as u8;
    }
    c.px[d + 3] = (255.0 * a + c.px[d + 3] as f32 * (1.0 - a)).round() as u8;
}

/// Darken a pixel by `f` (1 = as it is).
fn darken(c: &mut Canvas, x: i32, y: i32, f: f32) {
    if x >= 0 && y >= 0 && x < c.w as i32 && y < c.h as i32 {
        let d = ((y as u32 * c.w + x as u32) * 4) as usize;
        (0..3).for_each(|k| c.px[d + k] = (c.px[d + k] as f32 * f) as u8);
    }
}

/// Visit the screen pixels over game rectangle [x0, x1] × [y0, y1] with their centres in game
/// pixels.
fn each(ss: u32, (x0, y0, x1, y1): (f32, f32, f32, f32), mut f: impl FnMut(i32, i32, f32, f32)) {
    let s = ss as f32;
    for sy in (y0 * s).floor() as i32..(y1 * s).ceil() as i32 {
        for sx in (x0 * s).floor() as i32..(x1 * s).ceil() as i32 {
            f(sx, sy, (sx as f32 + 0.5) / s, (sy as f32 + 0.5) / s);
        }
    }
}

/// Distance from (px, py) to a box with rounded corners (negative inside).
fn rounded(px: f32, py: f32, (x0, y0, x1, y1): (f32, f32, f32, f32), r: f32) -> f32 {
    let (cx, cy, hw, hh) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0, (x1 - x0) / 2.0 - r, (y1 - y0) / 2.0 - r);
    let (qx, qy) = ((px - cx).abs() - hw, (py - cy).abs() - hh);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r
}

/// The cover behind book art of `w`×`h` at (x, y): a soft shadow under it, then the boards in
/// leather with a darker edge, a lit inner edge and a gilt line, all of it grained a game pixel
/// at a time like the art.
pub fn cover(c: &mut Canvas, ss: u32, (x, y, w, h): (i32, i32, i32, i32)) {
    let (s, (x, y, w, h)) = (ss as f32, (x as f32, y as f32, w as f32, h as f32));
    let board = (x - RIM.0, y - RIM.1, x + w + RIM.0, y + h + RIM.2);
    let blur = 4.0;
    let shadow = (board.0 + 1.0, board.1 + 3.0, board.2 + 2.0, board.3 + 3.0);
    each(ss, (shadow.0 - blur, shadow.1 - blur, shadow.2 + blur, shadow.3 + blur), |sx, sy, px, py| {
        let d = rounded(px, py, shadow, ROUND);
        blend(c, sx, sy, [0.0; 3], 0.45 * (1.0 - (d / blur).clamp(0.0, 1.0)).powi(2));
    });
    each(ss, board, |sx, sy, px, py| {
        let d = rounded(px, py, board, ROUND);
        let a = cover_of(d, s);
        if a <= 0.0 {
            return;
        }
        let t = ((py - board.1) / (board.3 - board.1)).clamp(0.0, 1.0);
        // (grain: one game pixel at a time)
        let g = ((px.floor() as i32).wrapping_mul(73_856_093) ^ (py.floor() as i32).wrapping_mul(19_349_663)) as u32;
        let grain = ((g >> 7) % 13) as f32 - 6.0;
        let mut rgb = mix(LEATHER.0, LEATHER.1, t).map(|v| v + grain);
        if d > -1.0 {
            rgb = rgb.map(|v| v * 0.55); // the edge
        } else if d > -1.8 {
            rgb = rgb.map(|v| v * 1.18); // lit just inside it
        } else if (d + 2.6).abs() < 0.5 / s + 0.15 {
            rgb = mix(rgb, GILT, 0.8);
        }
        blend(c, sx, sy, rgb, a);
    });
}

/// Turn up the bottom corner of a page at `corner` (game pixels): its outer bottom point, on the
/// `left` page or the right. The flap shows the page's back, shaded from the fold to its tip, and
/// casts a shadow on the page; under it, the next page in the fold's shade.
pub fn curl(c: &mut Canvas, ss: u32, corner: (i32, i32), left: bool) {
    let (s, k) = (ss as f32, CURL);
    let (cx, cy) = (corner.0 as f32, corner.1 as f32);
    // (inward and upward from the corner)
    let local = |px: f32, py: f32| (if left { px - cx } else { cx - px }, cy - py);
    let area = if left { (cx, cy - k - 3.0, cx + k + 3.0, cy) } else { (cx - k - 3.0, cy - k - 3.0, cx, cy) };
    let diag = std::f32::consts::SQRT_2;
    each(ss, area, |sx, sy, px, py| {
        let (lx, ly) = local(px, py);
        // Under the flap: the page below, darker by the fold.
        let under = (-lx).max(-ly).max((lx + ly - k) / diag);
        if cover_of(under, s) > 0.0 {
            let by_fold = ((k - lx - ly) / k).clamp(0.0, 1.0);
            darken(c, sx, sy, 1.0 - cover_of(under, s) * (0.32 - 0.14 * by_fold));
        }
        // The flap (its tip rounded: the page bends there), and its shadow on the page round it.
        let r = TIP;
        let tip_d = (lx - (k - r)).max(0.0).hypot((ly - (k - r)).max(0.0)) - r;
        let out = (lx - k).max(ly - k).max(tip_d);
        let flap = out.max((k - lx - ly) / diag);
        if flap > 0.0 && out < 2.5 && lx + ly > k {
            blend(c, sx, sy, [40.0, 24.0, 8.0], 0.34 * (1.0 - out.max(0.0) / 2.5));
        }
        // The flap: the page's back.
        let a = cover_of(flap, s);
        if a > 0.0 {
            let tip = ((lx + ly - k) / k).clamp(0.0, 1.0);
            let mut rgb = mix(BACK.0, BACK.1, tip);
            if out > -1.0 / s - 0.05 {
                rgb = EDGE; // its cut edges
            }
            blend(c, sx, sy, rgb, a);
        }
    });
}

/// Is (x, y) on the corner at `corner` (where a click turns that way)?
pub fn on_curl((x, y): (i32, i32), corner: (i32, i32), left: bool) -> bool {
    let lx = if left { x - corner.0 } else { corner.0 - x };
    let ly = corner.1 - y;
    let k = CURL as i32 + 3;
    (-1..=k).contains(&lx) && (-1..=k).contains(&ly)
}
