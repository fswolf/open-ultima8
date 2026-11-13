//! The window's events (winit): creating it, input, resizing, and pacing redraws.
use super::*;

/// What desktops know the game by (its .desktop file and icon in the Linux package).
const APP_NAME: &str = "open-ultima8";

/// The window's icon (icon/open-u8-128.png, built in).
fn icon() -> Option<winit::window::Icon> {
    let (rgba, w, h) = decode_png(include_bytes!("../../../icon/open-u8-128.png"))?;
    winit::window::Icon::from_rgba(rgba, w, h).ok()
}

/// An RGBA PNG's pixels and size.
fn decode_png(data: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let mut r = png::Decoder::new(std::io::Cursor::new(data)).read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()?];
    let info = r.next_frame(&mut buf).ok()?;
    (info.color_type == png::ColorType::Rgba && info.bit_depth == png::BitDepth::Eight)
        .then(|| (buf[..info.buffer_size()].to_vec(), info.width, info.height))
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        #[allow(unused_mut)]
        let mut attrs = Window::default_attributes()
            .with_title("Open Ultima8")
            .with_window_icon(icon())
            .with_inner_size(winit::dpi::LogicalSize::new(1280, 800));
        // (Linux desktops match the window to open-ultima8.desktop and its icon by this name.)
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            use winit::platform::{wayland::WindowAttributesExtWayland, x11::WindowAttributesExtX11};
            attrs = WindowAttributesExtWayland::with_name(attrs, APP_NAME, APP_NAME);
            attrs = WindowAttributesExtX11::with_name(attrs, APP_NAME, APP_NAME);
        }
        let init = || -> Result<(Arc<Window>, gpu::Gpu)> {
            let win = Arc::new(el.create_window(attrs)?);
            let gpu = pollster::block_on(gpu::Gpu::new(el, win.clone()))?;
            Ok((win, gpu))
        };
        match init() {
            Ok((win, gpu)) => {
                win.request_redraw();
                self.fullscreen = self.settings.fullscreen;
                if self.fullscreen {
                    win.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
                }
                self.window = Some(win);
                self.gpu = Some(gpu);
            }
            Err(e) => {
                self.error = Some(e.context("window / GPU init failed"));
                el.exit();
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, ev: WindowEvent) {
        match ev {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(sz) => {
                if let Some(g) = &mut self.gpu {
                    g.resize(sz.width, sz.height);
                }
                self.dirty = true;
            }
            WindowEvent::Focused(false) => {
                self.held.clear(); // don't keep walking after alt-tab
                (self.press, self.pan, self.left, self.right, self.dragging) = (None, None, false, false, false);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(k) = event.physical_key else { return };
                if event.state == ElementState::Released {
                    self.held.release(k);
                    return;
                }
                if let Some(s) = &mut self.slider {
                    if let Some(a) = s.key(k) {
                        self.answer_slider(a);
                    }
                    self.dirty = true;
                    return;
                }
                if self.film.is_some() {
                    if !event.repeat {
                        self.end_film(); // skipped
                    }
                    return;
                }
                if self.roll.is_some() {
                    if k == winit::keyboard::KeyCode::Escape {
                        self.end_roll(); // enough
                    }
                    return;
                }
                if let Some(m) = self.menu.as_mut().filter(|m| m.is_open()) {
                    // The menu has the keyboard (typing a diary entry; Esc goes back; 1–8 pick).
                    let typing = m.typing();
                    let action = m.key(k, event.text.as_deref());
                    self.dirty = true;
                    match action {
                        Some(a) => self.menu_action(a, el),
                        None if !typing && !event.repeat => {
                            if let Some(c @ (Command::Quit | Command::Fullscreen)) = input::bind(k, self.mods) {
                                self.command(c);
                            }
                        }
                        None => {}
                    }
                    return;
                }
                if !event.repeat && self.read_key(k) {
                    return;
                }
                self.held.press(k);
                if !event.repeat {
                    if let Some(c) = input::bind(k, self.mods) {
                        self.command(c);
                        self.keep_settings();
                    }
                }
            }
            WindowEvent::ModifiersChanged(m) => self.mods = m.state().into(),
            WindowEvent::MouseInput { button, state, .. } => {
                let down = state == ElementState::Pressed;
                if self.slider.is_some() {
                    let left =
                        matches!((button, self.swapped), (MouseButton::Left, false) | (MouseButton::Right, true));
                    let at = self.mouse_game();
                    let (Some(s), Some(art)) = (&mut self.slider, &self.art) else { return };
                    let answer = match (left, down) {
                        (true, true) => s.press(art, at),
                        (false, true) => Some(crate::slider::Answer::Cancel),
                        _ => {
                            s.release();
                            None
                        }
                    };
                    if let Some(a) = answer {
                        self.answer_slider(a);
                    }
                    if !down {
                        (self.left, self.right) = (false, false);
                    }
                    self.dirty = true;
                    return;
                }
                if self.film.is_some() {
                    if down {
                        self.end_film(); // skipped
                    }
                    return;
                }
                if self.menu_open() {
                    if !down {
                        (self.left, self.right) = (false, false); // pressed before the menu opened
                    }
                    let (gx, gy) = self.mouse_game();
                    let Some(m) = self.menu.as_mut() else { return };
                    let action = match (button, self.swapped, down) {
                        (MouseButton::Left, false, true) | (MouseButton::Right, true, true) => m.click(gx, gy),
                        (MouseButton::Right, false, true) | (MouseButton::Left, true, true) => {
                            m.back();
                            None
                        }
                        _ => None,
                    };
                    if let Some(a) = action {
                        self.menu_action(a, el);
                    }
                    self.dirty = true;
                    return;
                }
                match (button, self.swapped) {
                    (MouseButton::Left, false) | (MouseButton::Right, true) => self.left_button(down),
                    (MouseButton::Right, false) | (MouseButton::Left, true) => self.right_button(down),
                    (MouseButton::Middle, _) => self.pan = down.then_some(self.mouse),
                    _ => {}
                }
            }
            WindowEvent::CursorMoved { position: p, .. } if self.win_drag.is_some() => {
                let (item, from) = self.win_drag.unwrap();
                let s = self.view.scale as f64;
                let (dx, dy) = (((p.x - from.0) / s) as i32, ((p.y - from.1) / s) as i32);
                if (dx, dy) != (0, 0) {
                    self.windows.move_by(item, dx, dy);
                    self.win_drag = Some((item, (from.0 + dx as f64 * s, from.1 + dy as f64 * s)));
                    self.dirty = true;
                }
                self.mouse = (p.x, p.y);
            }
            WindowEvent::CursorMoved { position: p, .. } => {
                if let Some(d) = self.press {
                    // A drag is never a click; dragging an item picks it up.
                    self.dragging |= (p.x - d.0).abs() + (p.y - d.1).abs() > 6.0;
                    if self.dragging && self.item_drag.is_none() {
                        // Pick it up only if the Avatar can (else say why, once).
                        if let Some(id) = self.press_item.take() {
                            match self.game.can_pick_up(id) {
                                Ok(()) => self.item_drag = Some(id),
                                Err(why) => self.notice(why),
                            }
                        }
                    }
                }
                if self.item_drag.is_some() {
                    self.dirty = true; // the carried item follows the pointer
                }
                // Middle drag pans; with a free camera, so does left drag.
                let left_pan = self.press.filter(|_| self.dragging && !self.controlling() && self.item_drag.is_none());
                if let Some(d) = self.pan.or(left_pan) {
                    self.view.follow = false; // panning detaches the camera
                    self.view.cam.0 -= ((p.x - d.0) / self.view.zoom as f64) as f32;
                    self.view.cam.1 -= ((p.y - d.1) / self.view.zoom as f64) as f32;
                    if self.pan.is_some() {
                        self.pan = Some((p.x, p.y));
                    } else {
                        self.press = Some((p.x, p.y));
                    }
                    self.dirty = true;
                }
                self.mouse = (p.x, p.y);
                let (gx, gy) = self.mouse_game();
                if let Some(s) = &mut self.slider {
                    self.dirty |= s.drag(gx);
                }
                if let Some(ui) = &mut self.ui {
                    self.dirty |= ui.hover(gx, gy);
                }
                if let Some(m) = self.menu.as_mut().filter(|m| m.is_open()) {
                    self.dirty |= m.hover(gx, gy);
                }
            }
            WindowEvent::MouseWheel { delta, .. } if !self.menu_open() => {
                // Zoom the world: a notch a step (touchpads: by how far they scrolled).
                let steps = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y,
                    winit::event::MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                self.zoom_by(steps);
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        // The game's arrow cursor while the player controls the Avatar, the system one otherwise.
        let combat = self.game.in_combat();
        let frame = self.pointer().map(|(sector, reach)| match combat {
            true => mouse::combat_arrow(sector),
            false => mouse::arrow(sector, reach),
        });
        let frame = if self.targeting.is_some() { Some(mouse::TARGET) } else { frame };
        if let (Some(c), Some(w)) = (&mut self.cursors, &self.window) {
            c.show(el, w, frame, self.view.scale, self.view.mode == Mode::XbrzSprites);
        }
        // While anything moves, redraw every frame (vsync paces us); otherwise wake now and then
        // for scenery animation. All time-based updates happen in `redraw`.
        let active = self.moving
            || self.held.axes() != (0, 0)
            || self.jump
            || self.right
            || self.zooming
            || self.view.zoom != self.view.zoom_to;
        let now = Instant::now();
        let wake = now >= self.next_wake;
        if wake {
            // Running scripts tick at the game's 30 Hz; otherwise just keep scenery animating.
            self.next_wake = now + if self.scripting { SCRIPT_WAKE } else { IDLE_WAKE };
        }
        if active || self.dirty || wake {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
        el.set_control_flow(if active { ControlFlow::Wait } else { ControlFlow::WaitUntil(self.next_wake) });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_icon_is_built_in() {
        let (rgba, w, h) = super::decode_png(include_bytes!("../../../icon/open-u8-128.png")).expect("an RGBA PNG");
        assert_eq!((w, h, rgba.len()), (128, 128, 128 * 128 * 4));
        assert!(super::icon().is_some());
    }
}
