//! Drawing a frame: the world at its zoom, then over it (at the interface's size) the windows,
//! the carried thing, speech, pages, menus and the settings; and showing both on the GPU.
use super::*;

/// The paperdoll's stats panel as the original draws it: names right-aligned to the dotted line
/// (x 123, a colon every 9 rows from y 19), green numbers right-aligned after it, and the stats
/// button in the box under them.
fn paperdoll_stats(
    canvas: &mut Canvas,
    art: &mut art::GumpArt,
    ss: u32,
    ui: &ui::ScriptUi,
    gd: &u8data::GameData,
    (st, load, armour, pressed): (u8engine::stats::Npc, u32, i32, bool),
    at: (i32, i32),
) {
    let rows = [
        ("STR", st.str as u32),
        ("INT", st.int as u32),
        ("DEX", st.dex as u32),
        ("ARMR", armour.max(0) as u32),
        ("HITS", st.hp.max(0) as u32),
        ("MANA", st.mana.max(0) as u32),
        ("WGHT", load),
    ];
    for (k, (name, value)) in rows.iter().enumerate() {
        let mid = at.1 + 23 + k as i32 * 9;
        let (name, value) =
            (ui.text(STAT_NAMES, name, &gd.palette), ui.text(STAT_VALUES, &value.to_string(), &gd.palette));
        text::draw_plain(canvas, ss, &name, at.0 + 122 - name.w as i32, mid - name.h as i32 / 2);
        // Names end at the window's dotted colons (x 122); numbers centred between them and the
        // panel's right edge (124–148).
        text::draw_plain(canvas, ss, &value, at.0 + 136 - value.w as i32 / 2, mid - value.h as i32 / 2);
    }
    gumps::Windows::draw_stats_button(canvas, art, pressed, at);
}

/// Where speech goes over something on the map (screen pixels, unzoomed): centred on where it
/// stands, 16 above the top of its box — ScummVM's ItemRelativeGump, not the sprite, so a
/// halberd's swing (or a speaker hidden behind a wall) doesn't push it aside.
/// Also where its feet are (screen y), for what goes below it, and how wide its box shows.
fn speech_anchor(world: &u8engine::world::World, id: ObjId) -> Option<((i32, i32), i32, i32)> {
    let e = world.entry(world.index(world.root(id))?); // (something carried: over its carrier)
    let (sx, sy) = u8render::project(e.it.x, e.it.y, e.z1);
    let b = e.bounds();
    let wide = (b.x1 - b.x0 + b.y1 - b.y0) / 4;
    Some(((sx, sy - 16), u8render::project(e.it.x, e.it.y, e.it.z).1, wide))
}

/// How much bigger xBRZ makes things for a zoom (none below 1.5×; the GPU does the rest).
fn xbrz_factor(zoom: f32) -> u32 {
    (zoom.round() as u32).clamp(1, MAX_XBRZ)
}

impl App {
    /// The world frame's pixels per game pixel. The looks drawn from the sprites themselves
    /// (xBRZ'd, or plain pixels) are made at the zoom, rounded, so people move by less than a
    /// game pixel — smooth when zoomed in; the GPU's filters work on the original pixels (1).
    /// Kept while the zoom eases (the sprites are remade once it settles).
    pub(super) fn world_ss(&self) -> u32 {
        match self.view.mode {
            Mode::XbrzSprites | Mode::Nearest if self.zooming => self.cache.scale,
            Mode::XbrzSprites | Mode::Nearest => xbrz_factor(self.view.zoom_to),
            _ => 1,
        }
    }

    /// Draw what's changed, and show the world and what's over it.
    pub(super) fn redraw(&mut self) {
        let (Some(win), Some(_)) = (self.window.clone(), &self.gpu) else { return };
        let sz = win.inner_size();
        if sz.width == 0 || sz.height == 0 {
            return; // minimised
        }
        self.update();
        if self.item_drag.is_some() {
            self.update_drag(); // (the Avatar moving brings things into reach or out of it)
        }
        if self.dirty {
            let t0 = Instant::now();
            self.draw_frame();
            self.dirty = false;
            let ms = t0.elapsed().as_secs_f32() * 1000.0;
            self.frame_ms = if self.frame_ms == 0.0 { ms } else { self.frame_ms * 0.9 + ms * 0.1 };
            self.update_title(&win);
        }
        self.present();
    }

    /// Is anything over the world? (Nothing, the usual while walking about: no overlay to draw
    /// or send.)
    fn overlay_used(&self) -> bool {
        !self.windows.items().is_empty()
            || self.windows.gauge.is_some()
            || self.item_drag.is_some()
            || self.ui.as_ref().is_some_and(|u| !u.is_empty())
            || self.roll.is_some()
            || self.pad_cursor.is_some()
            || self.touch_buttons_up()
            || self.menu_open()
    }

    /// The world frame's and the overlay's pixels per game pixel, the sprite caches made to
    /// match. (Pages to read and the settings are drawn at the window's resolution, so their
    /// type is crisp.)
    fn set_scales(&mut self) -> (u32, u32) {
        let xbrz = self.view.mode == Mode::XbrzSprites;
        let world_ss = self.world_ss();
        let world_xbrz = xbrz && world_ss > 1;
        let reading = self.ui.as_ref().is_some_and(|u| u.reading()) || self.panel.is_some();
        let ui_ss = if xbrz || reading { self.view.scale } else { 1 };
        let up = |smooth: bool| -> u8render::Upscaler {
            if smooth {
                Box::new(upscale::xbrz)
            } else {
                Box::new(upscale::nearest)
            }
        };
        if (self.cache.scale, self.world_xbrz) != (world_ss, world_xbrz) {
            self.cache.set_scale(world_ss, up(world_xbrz));
            self.world_xbrz = world_xbrz;
        }
        if self.ui_cache.scale != ui_ss {
            self.ui_cache.set_scale(ui_ss, up(ui_ss > 1));
        }
        (world_ss, ui_ss)
    }

    /// Draw the world and the overlay and hand them to the GPU.
    fn draw_frame(&mut self) {
        let (world_ss, ui_ss) = self.set_scales();
        let (ww, wh) = self.world_size();
        let (bw, bh) = (ww * world_ss, wh * world_ss);
        self.buf.resize((bw * bh * 4) as usize, 0);
        let cam = self.view_origin();
        let mut canvas = Canvas::new(&mut self.buf, bw, bh);
        self.painter.draw(&self.game.data, &mut self.game.world, &mut self.cache, cam, &mut canvas, self.view.opts);
        // Where the carried thing would go: its outline (drawn see-through, as the original
        // draws its copy flagged unseen: DLITEM.C).
        if let (Some(controls::Ghost::Map((x, y, z))), Some(it)) =
            (self.drag_ghost, self.item_drag.and_then(|id| self.game.world.get(id)))
        {
            let (gd, s) = (&self.game.data, world_ss as i32);
            if let Some(sp) =
                self.cache.get(gd, it.shape, it.frame, it.flags & u8engine::world::item_flag::FLIPPED != 0)
            {
                let (px, py) = u8render::project(x, y, z);
                let (px, py) = ((px - cam.0) * s - sp.xoff, (py - cam.1) * s - sp.yoff);
                canvas.recolour(&sp, px, py, |c| gd.xform.unseen(&gd.palette, c));
            }
        }
        // Over it, at the interface's size (in a buffer of its own while the rest of `self` draws).
        let used = self.overlay_used();
        let (vw, vh) = self.view_size();
        let (ow, oh) = if used { (vw * ui_ss, vh * ui_ss) } else { (1, 1) };
        let mut px = std::mem::take(&mut self.overlay);
        px.resize((ow * oh * 4) as usize, 0);
        let mut canvas = Canvas::new(&mut px, ow, oh);
        canvas.clear();
        if used {
            self.draw_overlay(&mut canvas, ui_ss, (vw, vh));
        }
        self.overlay = px;
        let gpu = self.gpu.as_mut().unwrap();
        gpu.upload(gpu::Layer::World, &self.buf, bw, bh);
        if used {
            gpu.upload(gpu::Layer::Overlay, &self.overlay, ow, oh);
        }
        (self.frame_ss, self.overlay_ss, self.overlay_shown) = (world_ss, ui_ss, used);
    }

    /// Everything over the world, bottom to top: windows, the carried thing, speech and pages,
    /// the credits, a film, the slider, the book, the touch buttons, the settings, the pad's
    /// pointer.
    fn draw_overlay(&mut self, canvas: &mut Canvas, ui_ss: u32, (vw, vh): (u32, u32)) {
        let (mouse, buttons_up) = (self.mouse_game(), self.touch_buttons_up());
        self.draw_windows(canvas, ui_ss);
        if let (Some(controls::Ghost::Window), Some(it)) =
            (self.drag_ghost, self.item_drag.and_then(|id| self.game.world.get(id)))
        {
            // Where the carried thing would go in a window: its outline there, under the pointer.
            let (gd, flip) = (&self.game.data, it.flags & u8engine::world::item_flag::FLIPPED != 0);
            if let Some(sp) = self.ui_cache.get(gd, it.shape, it.frame, flip) {
                let (x, y) = (mouse.0 * ui_ss as i32 - sp.xoff, mouse.1 * ui_ss as i32 - sp.yoff);
                canvas.recolour(&sp, x, y, |c| gd.xform.unseen(&gd.palette, c));
            }
        }
        self.draw_speech(canvas, ui_ss, (vw, vh));
        if let Some((r, _)) = &self.roll {
            r.draw(canvas, ui_ss, (vw, vh));
        }
        if let Some(f) = &self.film {
            f.draw(canvas, ui_ss, (vw, vh));
        }
        if let (Some(sl), Some(art)) = (self.slider.as_mut(), self.art.as_mut()) {
            sl.draw(canvas, art, self.menu.as_ref().map(|m| m.fonts()), ui_ss, (vw, vh));
        }
        if let (Some(m), Some(art)) = (self.menu.as_mut().filter(|m| m.is_open()), self.art.as_mut()) {
            let name = self.game.data.avatar_name.as_deref().unwrap_or("");
            if m.avatar != name {
                m.avatar = name.to_string();
            }
            m.draw(canvas, art, (vw, vh));
            if let Some(ui) = &mut self.ui {
                ui.draw_notices(canvas, ui_ss, vw); // "Not now." etc. stay readable over the book
            }
        }
        // The touch screen's buttons, down the right edge.
        if let (true, Some(ui)) = (buttons_up, &self.ui) {
            for (r, (_, name)) in touch::button_rects((vw, vh)).iter().zip(touch::BUTTONS) {
                text::shade(canvas, ui_ss, *r);
                let img = ui.text(0, name, &self.game.data.palette);
                let (x, y) = (r[0] + (r[2] - img.w as i32) / 2, r[1] + (r[3] - img.h as i32) / 2);
                text::draw_plain(canvas, ui_ss, &img, x, y);
            }
        }
        if let Some(p) = &mut self.panel {
            p.draw(canvas, ui_ss, (vw, vh));
            if let Some(ui) = &mut self.ui {
                ui.draw_notices(canvas, ui_ss, vw); // ("Cheats on" and the like)
            }
        }
        // The pad's pointer, over everything (the system's cursor is hidden meanwhile).
        if let Some(sp) = self.pad_cursor.and_then(|f| self.cursors.as_ref()?.sprite(f, ui_ss)) {
            canvas.blit(&sp, mouse.0 * ui_ss as i32 - sp.xoff, mouse.1 * ui_ss as i32 - sp.yoff);
        }
    }

    /// The item windows (the paperdoll's stats with it) and the health and mana gauge.
    fn draw_windows(&mut self, canvas: &mut Canvas, ui_ss: u32) {
        let xbrz = self.view.mode == Mode::XbrzSprites;
        if let Some(art) = &mut self.art {
            art.set_style(ui_ss, xbrz);
            let (game, gd, ui, stats_pressed) = (&self.game, &self.game.data, &self.ui, self.stats_pressed);
            // The paperdoll's stats panel goes with its window (under any window above it).
            // (A changeling as the Avatar opens its own paperdoll, as in the original: its numbers,
            // its kind's armour, what it carries.)
            let weighs = |id: ObjId| match id {
                AVATAR => game.load().0,
                _ => game
                    .world
                    .contents(id)
                    .iter()
                    .map(|&t| u8engine::equip::weight(gd, &game.world, t))
                    .sum::<u32>()
                    .div_ceil(10),
            };
            let armour = |id: ObjId| match id {
                AVATAR => game.armour_class(),
                _ => game.world.get(id).and_then(|it| u8engine::monsters::get(it.shape)).map_or(0, |m| m.armour as i32),
            };
            let stats = |canvas: &mut Canvas, art: &mut art::GumpArt, item: ObjId, gump: u32, at: (i32, i32)| {
                let st = game.stats.get(item);
                if let (Some(st), Some(ui), gumps::PAPERDOLL) = (st, ui, gump) {
                    // (Weighing walks everything carried: only when the paperdoll is open.)
                    paperdoll_stats(canvas, art, ui_ss, ui, gd, (st, weighs(item), armour(item), stats_pressed), at);
                }
            };
            self.windows.draw(canvas, art, gd, &self.game.world, &mut self.ui_cache, stats);
            if let Some(st) = self.game.stats.get(AVATAR) {
                let bars = [(st.hp, st.max_hp()), (st.mana, st.max_mana())];
                self.windows.draw_gauge(canvas, art, gd, ui_ss, bars);
            }
        }
    }

    /// Speech over whoever speaks, replies, and pages to read.
    fn draw_speech(&mut self, canvas: &mut Canvas, ui_ss: u32, (vw, vh): (u32, u32)) {
        // Where things on the map are on the overlay.
        let ((ox, oy), _) = self.world_view();
        let k = self.view.zoom / self.view.scale as f32;
        if let Some(ui) = &mut self.ui {
            let (world, windows, art, gd) = (&self.game.world, &self.windows, &self.art, &self.game.data);
            // Speech goes over the thing speaking: on the map, or in an open window.
            // (ScummVM's ItemRelativeGump.)
            let rect_of = |id| {
                // In an open window: over it there (on the overlay).
                if world.container(id).is_some() {
                    if let Some(r) = art.as_ref().and_then(|a| windows.item_rect(gd, a, world, id)) {
                        return Some([r[0], r[1] - 2, r[2], r[3] + 2]); // (2 clear of it)
                    }
                }
                // Else over what it's in, on the map (a bag on someone's back: over them).
                let id = world.root(id);
                if world.get(id).is_none() || world.is_void(id) {
                    // Nobody to stand over (the Guardian): low in the middle, its bottom 8 up.
                    return Some([vw as i32 / 2, vh as i32 - 8, 0, 0]);
                }
                if !world.same_floor(AVATAR, id) {
                    return None; // (nor seen from another storey)
                }
                // (Over the head, down to the feet.)
                let ((sx, sy), feet, wide) = speech_anchor(world, id)?;
                let (x, y) = ((sx as f32 - ox) * k, (sy as f32 - oy) * k);
                let (down, w) = (((feet - sy) as f32 * k).round() as i32, (wide as f32 * k).round().max(1.0) as i32);
                Some([x.round() as i32 - w / 2, y.round() as i32, w, down])
            };
            ui.draw(canvas, ui_ss, (vw, vh), rect_of);
            ui.draw_reading(canvas, self.art.as_mut(), ui_ss, (vw, vh));
        }
    }

    /// Show the world (placed to a fraction of a pixel, in its look) and the overlay.
    fn present(&mut self) {
        let (s, zoom) = (self.view.scale, self.view.zoom);
        // The world's frame starts at whole game pixels; the view, at a fraction of one.
        let ((x, y), _) = self.world_view();
        let (tx, ty) = self.view_origin();
        let fs = self.frame_ss as f32;
        let world = gpu::Place {
            k: fs / zoom,
            off: ((x - tx as f32) * fs, (y - ty as f32) * fs),
            filter: match self.view.mode {
                Mode::Smooth => gpu::Filter::Plain,
                Mode::Xbr => gpu::Filter::Xbr,
                Mode::Crt => gpu::Filter::Crt,
                Mode::Nearest | Mode::XbrzSprites => gpu::Filter::Sharp,
            },
        };
        // (Windows and text stay crisp whatever the world's look.)
        // (At the scale it was last drawn at: the window's while a book is open, so its type is crisp.)
        let overlay = gpu::Place { k: self.overlay_ss as f32 / s as f32, off: (0.0, 0.0), filter: gpu::Filter::Sharp };
        self.gpu.as_mut().unwrap().present(
            [Some(world), self.overlay_shown.then_some(overlay)],
            self.view.gamma,
            self.view.colour_fix,
            self.game.scripts.as_ref().map_or(u8engine::fade::Fade::default().rows(), |s| s.vm.fade.rows()),
            self.game.scripts.as_ref().map_or(0, |s| s.vm.inverter.state),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gauge comes up twice its old size in the bottom-left corner, answers clicks over all
    /// of it, and fills its bars from the bottom; the paperdoll's numbers sit centred between
    /// the dotted colons and the panel's edge. With `U8_SHOTS` set, written there as PPM.
    #[test]
    #[ignore]
    fn gauge_is_bigger_and_stats_are_centred() {
        let Ok(dir) = std::env::var("U8_DATA") else { return };
        let gd = u8data::GameData::load_fixed(std::path::Path::new(&dir)).unwrap();
        let root = std::path::PathBuf::from(&dir);
        let mut art = art::GumpArt::load(&root, &gd.palette).unwrap();
        let read = |p: &str| std::fs::read(u8data::find_file(&root, p).unwrap()).unwrap();
        let ui = ui::ScriptUi::new(u8data::font::Fonts::parse(
            &u8data::flex::Flex::parse(read("STATIC/U8FONTS.FLX")).unwrap(),
        ));
        let world = u8engine::world::World::build(&gd, 0);
        let (vw, vh) = (320u32, 200u32);
        let mut w = gumps::Windows::default();
        w.show_gauge(&art, (vw, vh));
        let (gx, gy) = w.gauge.unwrap();
        let (sw, sh) = art.size(gumps::GAUGE);
        assert_eq!((gx, gy + sh * gumps::GAUGE_SCALE), (5, vh as i32 - 5), "bottom-left, at its new size");
        let (cx, cy) = (gx + sw * gumps::GAUGE_SCALE - 4, gy + sh * gumps::GAUGE_SCALE - 4);
        assert!(matches!(w.hit(&gd, &art, &world, cx, cy), Some(gumps::Hit::Gauge)), "its far corner is part of it");
        let mut px = vec![0u8; (vw * vh * 4) as usize];
        let mut canvas = Canvas::new(&mut px, vw, vh);
        art.set_style(1, false);
        w.draw_gauge(&mut canvas, &mut art, &gd, 1, [(15, 30), (30, 30)]);
        // Ryan: without the art's black outline — its outermost pixels aren't drawn.
        let k = gumps::GAUGE_SCALE;
        let edge = [(gx, gy), (gx + sw * k - 1, gy), (gx + 4 * k, gy + sh * k - 1), (gx, gy + 6 * k)];
        assert!(edge.iter().all(|&(x, y)| canvas.px[((y as u32 * vw + x as u32) * 4 + 3) as usize] == 0));
        assert!(canvas.px[(((gy + k) as u32 * vw + (gx + 2 * k) as u32) * 4 + 3) as usize] == 255, "the rest is");
        let st = u8engine::stats::Npc { str: 15, int: 15, dex: 15, hp: 30, mana: 30, ..Default::default() };
        art.draw(&mut canvas, gumps::PAPERDOLL, 0, 100, 10);
        paperdoll_stats(&mut canvas, &mut art, 1, &ui, &gd, (st, 35, 7, false), (100, 10));
        // Values: centred about x 136 in the window.
        let ink = |x0: i32, x1: i32, y0: i32, y1: i32| {
            let cols: Vec<i32> = (x0..x1)
                .filter(|&x| (y0..y1).any(|y| px_at(&px, vw, x, y)[1] as i32 > px_at(&px, vw, x, y)[0] as i32 + 60))
                .collect();
            (cols.first().copied(), cols.last().copied())
        };
        let (l, r) = ink(100 + 124, 100 + 150, 10 + 18, 10 + 28); // STR's "15", in green
        let (l, r) = (l.unwrap() - 100, r.unwrap() - 100);
        assert!(((l + r) / 2 - 136).abs() <= 1, "15 spans {l}..{r}");
        if let Ok(out) = std::env::var("U8_SHOTS") {
            let mut ppm = format!("P6 {vw} {vh} 255\n").into_bytes();
            px.chunks(4).for_each(|p| ppm.extend(&p[..3]));
            std::fs::write(format!("{out}/gauge_stats.ppm"), ppm).unwrap();
        }
    }

    /// Speech sits over where someone stands, the same whichever frame they're on (a guard's
    /// halberd swinging wide doesn't push it aside): 16 above their head, centred on their feet.
    #[test]
    #[ignore]
    fn speech_sits_over_the_speaker_whatever_the_frame() {
        let Ok(dir) = std::env::var("U8_DATA") else { return };
        let gd = u8data::GameData::load_fixed(std::path::Path::new(&dir)).unwrap();
        let mut world = u8engine::world::World::build(&gd, 40);
        let guard = world.objects().find(|&id| world.get(id).is_some_and(|it| it.shape == 574)).expect("a guard");
        let it = world.get(guard).unwrap();
        let at = speech_anchor(&world, guard).unwrap();
        assert_eq!(at.0 .0, it.x / 4 - it.y / 4, "centred on where it stands");
        assert!(at.2 > 0, "as wide as it shows");
        let e = world.entry(world.index(guard).unwrap());
        assert_eq!(at.0 .1, it.x / 8 + it.y / 8 - e.z1 - 16, "16 above its head");
        assert_eq!(at.1, it.x / 8 + it.y / 8 - it.z, "and its feet");
        for f in 0..gd.shapes.get(574).len() as u32 {
            world.set(&gd, guard, u8data::map::Item { frame: f, ..it });
            assert_eq!(speech_anchor(&world, guard), Some(at), "frame {f}");
        }
    }

    /// Something in a closed container speaks from over the container on the map (in a bag on
    /// someone's back: over them).
    #[test]
    #[ignore]
    fn things_inside_speak_from_their_container() {
        let Ok(dir) = std::env::var("U8_DATA") else { return };
        let gd = u8data::GameData::load_fixed(std::path::Path::new(&dir)).unwrap();
        let g = u8engine::Game::new(gd, None, Some(40)).unwrap();
        let world = &g.world;
        let held = (1..u16::MAX).find(|&id| world.container(id).is_some()).expect("something in a chest");
        let carrier = world.root(held);
        assert_ne!(carrier, held);
        assert!(speech_anchor(world, held).is_some());
        assert_eq!(speech_anchor(world, held), speech_anchor(world, carrier));
    }

    fn px_at(px: &[u8], w: u32, x: i32, y: i32) -> [u8; 4] {
        let i = ((y as u32 * w + x as u32) * 4) as usize;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    }
}
