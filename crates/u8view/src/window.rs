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
                // (Mid-window until the mouse says otherwise: a pad starts from there.)
                let sz = win.inner_size();
                self.mouse = (sz.width as f64 / 2.0, sz.height as f64 / 2.0);
                self.window = Some(win);
                self.gpu = Some(gpu);
                self.fit_interface();
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
                self.fit_interface();
                self.dirty = true;
            }
            WindowEvent::Focused(true) => self.focused = true,
            WindowEvent::Focused(false) => {
                self.focused = false; // (the pad lets go of everything at its next look)
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
                if let Some(p) = &mut self.panel {
                    // The settings have the keyboard (and full screen and quit still work).
                    // (Held keys repeat only to move about: a held F1 or Esc doesn't close it.)
                    use winit::keyboard::KeyCode as K;
                    let moves = matches!(k, K::ArrowUp | K::ArrowDown | K::ArrowLeft | K::ArrowRight);
                    let out = if event.repeat && !moves { None } else { p.key(k, self.mods.shift) };
                    if out.is_none() && !event.repeat {
                        if let Some(c @ (Command::Quit | Command::Fullscreen)) = input::bind(k, self.mods) {
                            self.command(c);
                        }
                    }
                    self.panel_out(out);
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
                // (The system's copy of a touch is the touch's own business.)
                if self.touch.echo(Instant::now(), self.os_cursor) {
                    return;
                }
                if state == ElementState::Pressed {
                    self.pad_aim = None; // (the mouse aims by the pointer)
                }
                self.mouse_button(button, state == ElementState::Pressed, el);
            }
            WindowEvent::Touch(t) => self.touched(t, el),
            WindowEvent::CursorMoved { position: p, .. } => {
                self.os_cursor = (p.x, p.y);
                if self.touch.echo(Instant::now(), self.os_cursor) {
                    return;
                }
                self.owner = crate::Owner::Mouse; // (the mouse has it back)
                if self.touch.buttons_shown() {
                    self.touch.hide_buttons();
                    self.dirty = true;
                }
                self.moved_to((p.x, p.y));
            }
            WindowEvent::MouseWheel { delta, .. } if self.panel.is_some() => {
                let y = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y,
                    winit::event::MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                if let (Some(p), true) = (&mut self.panel, y != 0.0) {
                    p.wheel(if y > 0.0 { -1 } else { 1 }); // (up the list as the wheel goes up)
                    self.dirty = true;
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
        self.poll_pad(el);
        let out = self.touch.tick(Instant::now());
        self.apply_touch(out, el);
        // The game's arrow cursor while the player controls the Avatar, the system one otherwise.
        let combat = self.game.in_combat();
        let frame = self.pointer().map(|(sector, reach)| match combat {
            true => mouse::combat_arrow(sector),
            false => mouse::arrow(sector, reach),
        });
        let frame = if self.targeting.is_some() { Some(mouse::TARGET) } else { frame };
        let crossed = self.cross_until.is_some_and(|t| Instant::now() < t);
        let frame = if crossed { Some(mouse::CROSS) } else { frame };
        if let (Some(c), Some(w)) = (&mut self.cursors, &self.window) {
            c.show(el, w, frame, self.view.scale, self.view.mode == Mode::XbrzSprites);
            // The pad's pointer: drawn by us where the system's can't be put (and hidden).
            // (Under a finger, no cursor at all.)
            let drawn = (self.owner == crate::Owner::Pad).then(|| frame.unwrap_or(mouse::DOT));
            self.dirty |= std::mem::replace(&mut self.pad_cursor, drawn) != drawn;
            let hide = self.owner != crate::Owner::Mouse;
            if self.cursor_hidden != hide {
                self.cursor_hidden = hide;
                w.set_cursor_visible(!hide);
            }
        }
        // While anything moves, redraw every frame (vsync paces us); otherwise wake now and then
        // for scenery animation. All time-based updates happen in `redraw`.
        let active = self.moving
            || self.held.axes() != (0, 0)
            || self.jump
            || self.right
            || self.zooming
            || self.pad.active()
            || self.touch.busy()
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
        // (A controller isn't heard from by the window: with one plugged in, look often.)
        let until = if self.pad.connected() { self.next_wake.min(now + PAD_POLL) } else { self.next_wake };
        el.set_control_flow(if active { ControlFlow::Wait } else { ControlFlow::WaitUntil(until) });
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
