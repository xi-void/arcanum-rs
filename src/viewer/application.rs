use super::*;

fn scale_step(scale: u16, step: i16) -> u16 {
    scale.saturating_add_signed(step).clamp(50, 300)
}

fn bound(key: &Key, binding: &str) -> bool {
    match key {
        Key::Named(NamedKey::Home) => binding.eq_ignore_ascii_case("Home"),
        Key::Named(key) => format!("{key:?}") == binding,
        _ => false,
    }
}

pub(super) fn outside_camera(center: (i32, i32), anchor: (i32, i32), viewport: (u32, u32)) -> bool {
    // CE's movement recenter limits are 360×180 at its reference viewport.
    let limit = |extent: u32, margin: u32| (extent / 2).saturating_sub(margin).max(16);
    center.0.abs_diff(anchor.0) > limit(viewport.0, 40)
        || center.1.abs_diff(anchor.1) > limit(viewport.1, 60)
}

fn edge_direction((x, y): (u32, u32), (width, height): (u32, u32), band: u32) -> (i32, i32) {
    if x >= width || y >= height {
        return (0, 0);
    }
    let edge = |pos, extent: u32| {
        if pos < band {
            -1
        } else if pos >= extent.saturating_sub(band) {
            1
        } else {
            0
        }
    };
    (edge(x, width), edge(y, height))
}

impl Viewer<'_> {
    pub(super) fn persist(&self, section: &str, key: &str, value: impl Into<toml::Value>) {
        self.persist_values(section, [(key, value.into())]);
    }

    fn persist_values<'a>(
        &self,
        section: &str,
        values: impl IntoIterator<Item = (&'a str, toml::Value)>,
    ) {
        if let Some(app) = &self.application
            && let Err(e) = app.persist_values(section, values)
        {
            let message = format!("Could not save {section} settings: {e}");
            eprintln!("{message}");
            if let Some(view) = &self.view {
                view.window.set_title(&message);
            }
        }
    }
    pub(super) fn persist_menu_changes(&self, prior: Settings) {
        for (key, before, after) in [
            ("run", prior.run, self.settings.run),
            ("lighting", prior.lighting, self.settings.lighting),
            ("shadows", prior.shadows, self.settings.shadows),
        ] {
            if before != after {
                self.persist("display", key, after);
            }
        }
        if prior.zoom != self.settings.zoom {
            self.persist("camera", "zoom", i64::from(self.settings.zoom));
        }
    }

    pub(super) fn camera_available(&self) -> bool {
        self.focused
            && self.pause.is_none()
            && self.map_since.is_none()
            && self.developer.as_ref().is_none_or(|d| !d.console_open())
            && self.view.as_ref().is_some_and(|v| !v.occluded)
            && matches!(&self.content,ViewContent::Stream{world,..} if world.playing() && !world.inventory_is_open())
    }

    pub(super) fn camera_deadline(&self) -> Option<Instant> {
        self.camera_available()
            .then_some(self.camera_navigation.deadline())
            .flatten()
    }

    pub(super) fn advance_play_camera(&mut self, now: Instant) -> bool {
        if self.camera_deadline().is_none_or(|at| now < at) {
            return false;
        }
        let nav = &mut self.camera_navigation;
        let Some(last) = nav.sampled_at.replace(now) else {
            return false;
        };
        let ViewContent::Stream {
            camera,
            waiting,
            direction,
            ..
        } = &mut self.content
        else {
            return false;
        };
        if *waiting {
            return false;
        }
        let motion = nav.direction();
        let motion = if motion == (0, 0) {
            self.edge_direction
        } else {
            motion
        };
        let speed = f64::from(self.camera_preferences.speed);
        // CE scroll.c uses 20 × 10 projected pixels per tick at Normal,
        // plus 4 × 2 on diagonals. The reference installation runs at 60 Hz.
        let diagonal = if motion.0 != 0 && motion.1 != 0 {
            1.2
        } else {
            1.
        };
        let delta = nav.advance_delta(now, motion, (speed * diagonal, speed * diagonal / 2.), last);
        if delta == (0, 0) {
            return false;
        }
        let size = logical_size(
            self.view.as_ref().unwrap().window.inner_size(),
            self.settings.zoom,
        );
        if camera.pan(delta, size) {
            *direction = delta;
            true
        } else {
            nav.sampled_at = None;
            false
        }
    }

    pub(super) fn recenter_camera(&mut self) {
        let elapsed = self.play_elapsed(Instant::now());
        if let ViewContent::Stream {
            world,
            camera,
            direction,
            ..
        } = &mut self.content
        {
            world.follow_player(elapsed, camera);
            *direction = (0, 0);
        }
    }

    pub(super) fn active_interface(&self) -> &'static str {
        if self.developer.as_ref().is_some_and(Developer::console_open) {
            "console"
        } else if self.pause.is_some() {
            "menu"
        } else if self.map_since.is_some() {
            "map"
        } else if matches!(&self.content,ViewContent::Stream{world,..} if world.inventory_is_open())
        {
            "inventory"
        } else {
            "hud"
        }
    }

    fn scale_active(&mut self, step: i16) -> bool {
        let group = self.active_interface();
        let ViewContent::Stream {
            world,
            inventory,
            map_ui,
            hud,
            ..
        } = &mut self.content
        else {
            return false;
        };
        if !world.playing() {
            return false;
        }
        let scale = match group {
            "console" => self
                .developer
                .as_mut()
                .map(|d| d.adjust_console_scale(step)),
            "menu" => {
                self.menu_scale = scale_step(self.menu_scale, step);
                Some(self.menu_scale)
            }
            "map" => map_ui.as_mut().map(|m| m.adjust_scale(step)),
            _ => {
                if let Some(current) = hud
                    .as_ref()
                    .map(|hud| hud.scale())
                    .or_else(|| inventory.as_ref().map(|ui| ui.scale))
                {
                    let viewport = self.view.as_ref().unwrap().window.inner_size();
                    let (_, fit) = hud::gameplay_layout(
                        (viewport.width, viewport.height),
                        hud.as_deref(),
                        inventory.as_ref().map(|ui| ui.extent),
                        300,
                    );
                    let maximum = (fit as u16).max(50);
                    let scale = scale_step(current.min(maximum), step).min(maximum);
                    if scale == current {
                        return true;
                    }
                    if let Some(hud) = hud {
                        hud.set_scale(scale);
                    }
                    if let Some(ui) = inventory {
                        ui.scale = scale;
                    }
                    self.persist_values(
                        "ui",
                        ["hud_scale", "inventory_scale"].map(|key| (key, i64::from(scale).into())),
                    );
                    self.view.as_ref().unwrap().window.request_redraw();
                }
                return true;
            }
        };
        if let Some(scale) = scale {
            self.persist("ui", &format!("{group}_scale"), i64::from(scale));
            self.view.as_ref().unwrap().window.request_redraw();
        }
        true
    }

    pub(super) fn application_input(
        &mut self,
        loop_: &ActiveEventLoop,
        event: &WindowEvent,
    ) -> bool {
        if !self.camera_available() {
            self.camera_navigation = Navigation::default();
            self.edge_direction = (0, 0);
        }
        match event {
            WindowEvent::Focused(focused) => {
                self.focused = *focused;
                if !focused {
                    self.edge_direction = (0, 0);
                    self.camera_navigation = Navigation::default();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let mut next = (0, 0);
                if self.camera_available()
                    && self.camera_preferences.edge_scroll
                    && position.x >= 0.
                    && position.y >= 0.
                {
                    let view = self.view.as_ref().unwrap();
                    let size = view.window.inner_size();
                    let point = (position.x as u32, position.y as u32);
                    next = edge_direction(
                        point,
                        (size.width, size.height),
                        (8. * view.window.scale_factor()).round() as u32,
                    );
                }
                if next != self.edge_direction {
                    self.edge_direction = next;
                    if self.camera_navigation.held == 0 {
                        self.camera_navigation.sampled_at = (next != (0, 0)).then(Instant::now);
                    }
                    self.update_control_flow(loop_);
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.edge_direction = (0, 0);
                if self.camera_navigation.held == 0 {
                    self.camera_navigation.sampled_at = None;
                }
                self.update_control_flow(loop_);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state.is_pressed();
                let key = &event.logical_key;
                if pressed
                    && !event.repeat
                    && let Key::Character(text) = key
                    && matches!(text.as_str(), "[" | "]")
                {
                    return self.scale_active(if text == "[" { -25 } else { 25 });
                }
                if !self.camera_available() {
                    return false;
                }
                if pressed && !event.repeat {
                    if bound(key, &self.camera_preferences.recenter) {
                        self.recenter_camera();
                        self.view.as_ref().unwrap().window.request_redraw();
                        return true;
                    }
                    if bound(key, &self.camera_preferences.follow_toggle) {
                        self.camera_preferences.follow = !self.camera_preferences.follow;
                        if self.camera_preferences.follow {
                            self.recenter_camera();
                        }
                        self.persist("camera", "follow", self.camera_preferences.follow);
                        self.view.as_ref().unwrap().window.request_redraw();
                        return true;
                    }
                }
                if let Some(bit) = Navigation::key(key).filter(|b| *b < 16)
                    && ((pressed && self.modifiers.alt_key())
                        || (!pressed && self.camera_navigation.held & bit != 0))
                {
                    self.camera_navigation.set_key(bit, pressed, Instant::now());
                    if self.camera_navigation.held == 0 && self.edge_direction != (0, 0) {
                        self.camera_navigation.sampled_at = Some(Instant::now());
                    }
                    self.update_control_flow(loop_);
                    return true;
                }
            }
            _ => {}
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edge_band_and_movement_recentering_follow_viewport() {
        assert_eq!(edge_direction((0, 499), (800, 500), 8), (-1, 1));
        assert_eq!(edge_direction((400, 250), (800, 500), 8), (0, 0));
        assert_eq!(edge_direction((800, 250), (800, 500), 8), (0, 0));
        assert!(!outside_camera((0, 0), (360, 180), (800, 480)));
        assert!(outside_camera((0, 0), (361, 180), (800, 480)));
        assert!(!outside_camera((0, 0), (361, 180), (1920, 1080)));
    }
}
