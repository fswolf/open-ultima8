//! Text and panels over a frame. Positions are in game pixels; `scale` is canvas pixels per game pixel.
use crate::Canvas;
use u8data::font::TextImage;

/// Draw rendered text with a 1-pixel dark outline so it reads over any background.
pub fn draw(canvas: &mut Canvas, scale: u32, img: &TextImage, x: i32, y: i32) {
    let s = scale as i32;
    let mut put = |px: i32, py: i32, c: [u8; 4]| {
        for dy in 0..s {
            for dx in 0..s {
                canvas.put(px * s + dx, py * s + dy, c);
            }
        }
    };
    let on = |cx: i32, cy: i32| {
        cx >= 0
            && cy >= 0
            && (cx as u32) < img.w
            && (cy as u32) < img.h
            && img.rgba[((cy as u32 * img.w + cx as u32) * 4 + 3) as usize] > 0
    };
    for cy in -1..=img.h as i32 {
        for cx in -1..=img.w as i32 {
            if !on(cx, cy) && [(-1, 0), (1, 0), (0, -1), (0, 1)].iter().any(|(dx, dy)| on(cx + dx, cy + dy)) {
                put(x + cx, y + cy, [0, 0, 0, 255]);
            }
        }
    }
    for cy in 0..img.h as i32 {
        for cx in 0..img.w as i32 {
            if on(cx, cy) {
                let k = ((cy as u32 * img.w + cx as u32) * 4) as usize;
                put(x + cx, y + cy, [img.rgba[k], img.rgba[k + 1], img.rgba[k + 2], 255]);
            }
        }
    }
}

/// Draw rendered text as it is, without an outline (dark text on a page).
pub fn draw_plain(canvas: &mut Canvas, scale: u32, img: &TextImage, x: i32, y: i32) {
    let s = scale as i32;
    for cy in 0..img.h as i32 {
        for cx in 0..img.w as i32 {
            let k = ((cy as u32 * img.w + cx as u32) * 4) as usize;
            if img.rgba[k + 3] > 0 {
                let c = [img.rgba[k], img.rgba[k + 1], img.rgba[k + 2], 255];
                for dy in 0..s {
                    for dx in 0..s {
                        canvas.put((x + cx) * s + dx, (y + cy) * s + dy, c);
                    }
                }
            }
        }
    }
}

/// Draw an image already at the canvas's resolution, blended by its alpha (smooth type), its
/// top-left at canvas pixel (x, y).
pub fn draw_blended(canvas: &mut Canvas, img: &TextImage, x: i32, y: i32) {
    for cy in 0..img.h as i32 {
        for cx in 0..img.w as i32 {
            let k = ((cy as u32 * img.w + cx as u32) * 4) as usize;
            let a = img.rgba[k + 3] as u32;
            let (px, py) = (x + cx, y + cy);
            if a == 0 || px < 0 || py < 0 || px >= canvas.w as i32 || py >= canvas.h as i32 {
                continue;
            }
            let d = ((py as u32 * canvas.w + px as u32) * 4) as usize;
            for c in 0..4 {
                let src = if c == 3 { 255 } else { img.rgba[k + c] as u32 };
                canvas.px[d + c] = ((src * a + canvas.px[d + c] as u32 * (255 - a)) / 255) as u8;
            }
        }
    }
}

/// Darken a rectangle [x, y, w, h] to sit text on.
pub fn shade(canvas: &mut Canvas, scale: u32, r: [i32; 4]) {
    let s = scale as i32;
    let (w, h) = (canvas.w as i32, canvas.h as i32);
    for y in (r[1] * s).max(0)..((r[1] + r[3]) * s).min(h) {
        for x in (r[0] * s).max(0)..((r[0] + r[2]) * s).min(w) {
            let d = ((y * w + x) * 4) as usize;
            for k in 0..3 {
                canvas.px[d + k] /= 3;
            }
            // (over an overlay's clear pixels: two-thirds black, so what's under darkens too)
            canvas.px[d + 3] = 255 - (255 - canvas.px[d + 3]) / 3;
        }
    }
}

/// Fill a rectangle [x, y, w, h] with one colour.
pub fn block(canvas: &mut Canvas, scale: u32, r: [i32; 4], c: [u8; 4]) {
    let s = scale as i32;
    for y in r[1] * s..(r[1] + r[3]) * s {
        for x in r[0] * s..(r[0] + r[2]) * s {
            canvas.put(x, y, c);
        }
    }
}
