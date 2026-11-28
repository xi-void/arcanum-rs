use super::*;
use classic_menu::Screen;

impl Viewer<'_> {
    // Temporarily mount the shared creation panels when the gameplay package omits them.
    // Only the mounted HUD participates in drawing, hit testing or updates.
    pub(super) fn prepare_creation_hud(&mut self) -> Result<(), String> {
        let scale = self.ui_scale();
        if let ViewContent::Stream {
            hud,
            inventory_reader: Some(reader),
            ..
        } = &mut self.content
            && self.gameplay_hud.is_none()
            && hud
                .as_ref()
                .is_none_or(|h| !h.has_sheet() || !h.has_inventory())
        {
            let panels = Hud::load(&ThemeSource::hud(), reader.resources(), Some(scale))
                .map_err(|e| e.to_string())?;
            self.gameplay_hud = Some(hud.replace(Box::new(panels)));
        }
        Ok(())
    }

    pub(super) fn restore_gameplay_hud(&mut self) {
        let scale = self.ui_scale();
        if let Some(mut previous) = self.gameplay_hud.take()
            && let ViewContent::Stream { hud, .. } = &mut self.content
        {
            if let Some(original) = &mut previous {
                original.set_scale(scale);
            }
            self.pause_scale = scale;
            *hud = previous;
        }
    }

    fn creation_panel(&self) -> Option<Screen> {
        self.pause.as_ref()?.0.front.filter(|s| s.panel())
    }

    pub(super) fn creation_action(
        &mut self,
        command: MenuCommand,
        events: &ActiveEventLoop,
    ) -> Result<(), String> {
        let target = Destination {
            map: None,
            tile: (0, 0),
        };
        match command {
            MenuCommand::BackToIdentity => {
                let ViewContent::Stream { world, hud, .. } = &mut self.content else {
                    unreachable!()
                };
                let menu = &mut self.pause.as_mut().unwrap().0;
                menu.identity.allocated = Some(world.creation_build());
                if let Some(hud) = hud {
                    hud.reset_character();
                }
                menu.goto(if menu.identity.pregen.is_some() {
                    Screen::Pregen
                } else {
                    Screen::Identity
                });
            }
            MenuCommand::ReturnMain => {
                self.prepare_creation_hud()?;
                self.pause = Some((Menu::main(), Instant::now()));
                if let Some(view) = &mut self.view {
                    view.save_status = None;
                    view.developer_status = None;
                }
                if let Some(audio) = &mut self.audio {
                    audio.main_menu();
                }
            }
            MenuCommand::LastSave => {
                let id = self.saves.as_ref().map_err(Clone::clone)?.recent()?;
                self.restore_game(&id, events)?;
                self.menu_action(MenuCommand::Continue, events);
            }
            MenuCommand::Preview => {
                let (menu, _) = self.pause.as_mut().ok_or("Creation menu unavailable")?;
                let ViewContent::Stream {
                    inventory_reader: Some(reader),
                    ..
                } = &mut self.content
                else {
                    return Err("Character resources unavailable".into());
                };
                menu.prepare_preview(self.pause_theme.as_ref().unwrap(), reader)?;
            }
            MenuCommand::Allocate | MenuCommand::BackToAllocation | MenuCommand::Shop => {
                let shop = matches!(command, MenuCommand::Shop);
                let back = matches!(command, MenuCommand::BackToAllocation);
                let (menu, _) = self.pause.as_mut().unwrap();

                let build = if shop {
                    let ViewContent::Stream { world, .. } = &self.content else {
                        unreachable!()
                    };
                    let points = world.character_statistics().base[21];
                    if points != 0 {
                        return Err(format!(
                            "Spend all character points before continuing ({points} remaining)."
                        ));
                    }
                    let allocated = world.creation_build();
                    menu.identity.allocated = Some(allocated.clone());
                    allocated
                } else if back {
                    menu.identity
                        .allocated
                        .clone()
                        .ok_or("Missing allocation state")?
                } else {
                    let mut build = menu
                        .identity
                        .allocated
                        .clone()
                        .or_else(|| menu.identity.build.clone())
                        .ok_or("Character preview unavailable")?;
                    build.set_name(menu.identity.name.trim().into());
                    build
                };
                self.enter_location(target, 0, LocationMode::Creation(&build))?;
                let ViewContent::Stream {
                    hud: Some(hud),
                    inventory_reader: Some(reader),
                    world,
                    ..
                } = &mut self.content
                else {
                    return Err("Creation needs the character and inventory panels".into());
                };

                if shop {
                    if arcanum_rs::content::maps::Catalog::load(reader.resources())
                        .map_err(|e| e.to_string())?
                        .shopping()?
                        .is_none()
                    {
                        hud.sheet_action(hud::Action::ToggleCharacter, world, reader);
                        world.resume_creation_allocations(&build);
                        self.pause.as_mut().unwrap().0.confirm_start();
                        return Ok(());
                    }
                    let merchant = reader.open_starting_shop(world)?;
                    if merchant.is_none() {
                        return Err("Missing starting merchant on the shopping map".into());
                    }
                    hud.open_inventory(reader, world, merchant)?;
                } else {
                    if !hud.has_sheet() {
                        return Err("The active HUD has no character sheet".into());
                    }
                    hud.sheet_action(hud::Action::ToggleCharacter, world, reader);
                    world.resume_creation_allocations(&build);
                }
                self.pause.as_mut().unwrap().0.goto(if shop {
                    Screen::Shop
                } else {
                    Screen::Allocate
                });
            }
            MenuCommand::Credits => {
                let ViewContent::Stream {
                    inventory_reader: Some(reader),
                    ..
                } = &mut self.content
                else {
                    return Err("Slide resources unavailable".into());
                };
                let ids = arcanum_rs::content::script::slide_sequence(reader.resources(), 999)?;
                self.play_sequence(
                    ids.into_iter().map(cinematic::Clip::Number).collect(),
                    cinematic::Destination::Credits,
                    events,
                );
            }
            MenuCommand::Intro => self.play_movies([1, 7].into(), false, events),
            MenuCommand::Start => {
                let ViewContent::Stream { world, .. } = &self.content else {
                    unreachable!()
                };
                if world.character_statistics().base[21] != 0 {
                    return Err("Character points remain unspent".into());
                }
                self.play_movies([1, 7].into(), true, events);
            }
            MenuCommand::EnterGame => {
                let ViewContent::Stream {
                    world,
                    inventory_reader: Some(reader),
                    ..
                } = &mut self.content
                else {
                    return Err("Character resources unavailable".into());
                };
                if world.character_statistics().base[21] != 0 {
                    return Err("Character points remain unspent".into());
                }
                reader.equip_starting_possessions(world)?;
                let possessions = world.starting_possessions();
                let flag = self
                    .pause
                    .as_ref()
                    .unwrap()
                    .0
                    .identity
                    .pregen
                    .and_then(|n| self.pause_theme.as_ref()?.start_flag(n));
                self.enter_location(target, 0, LocationMode::Start(&possessions, flag))?;
                if let ViewContent::Stream {
                    map_ui: Some(map), ..
                } = &mut self.content
                {
                    map.reset_discovery();
                }
                self.restore_gameplay_hud();
                self.pause = None;
                self.epoch = Instant::now();
                self.paused_for = Duration::ZERO;
                self.clock_paused_for = Duration::ZERO;
                self.turn_clock = None;
                self.map_since = None;
                if let Some(audio) = &self.audio {
                    audio.pause(false);
                }
            }
            _ => unreachable!(),
        }
        Ok(())
    }

    pub(super) fn front_event(
        &mut self,
        events: &ActiveEventLoop,
        id: WindowId,
        event: WindowEvent,
    ) {
        let size = self.view.as_ref().unwrap().window.inner_size();
        let physical = [size.width, size.height];
        match &event {
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x >= 0. && position.y >= 0.)
                    .then_some((position.x as u32, position.y as u32))
            }
            WindowEvent::CursorLeft { .. } => self.cursor = None,
            WindowEvent::RedrawRequested => {
                if size.width > 0
                    && size.height > 0
                    && let Err(e) = self.draw_front(physical)
                {
                    self.fail(events, e);
                }
                return;
            }
            _ => {}
        }
        if let Some(screen) = self.creation_panel() {
            let scale = self.ui_scale();
            let theme = self.pause_theme.as_ref().unwrap();
            let menu = &mut self.pause.as_mut().unwrap().0;
            menu.hover(self.cursor, physical, scale, theme, self.audio.as_ref());
            let nav = menu.over_navigation();
            let ViewContent::Stream {
                world,
                hud: Some(hud),
                inventory_reader: Some(reader),
                ..
            } = &mut self.content
            else {
                return;
            };
            hud.layout_group(
                (size.width, size.height),
                None,
                Some(theme.panel_canvas(physical, scale)),
            );
            hud.hover(self.cursor);
            let drag = hud.dragging_inventory();
            let handled = match &event {
                WindowEvent::KeyboardInput { event: key, .. }
                    if key.state.is_pressed()
                        && adjust_hud_scale(hud, &mut self.preferences, &key.logical_key) =>
                {
                    true
                }
                WindowEvent::Focused(false) | WindowEvent::CursorLeft { .. } => {
                    hud.cancel_press();
                    hud.cancel_inventory_drag();
                    false
                }
                WindowEvent::KeyboardInput { event: key, .. } if hud.choosing_quantity() => {
                    if key.state.is_pressed() {
                        hud.inventory_quantity_key(reader, world, &key.logical_key);
                    }
                    true
                }
                WindowEvent::KeyboardInput { event: key, .. }
                    if drag && key.logical_key == Key::Named(NamedKey::Escape) =>
                {
                    hud.cancel_inventory_drag();
                    true
                }
                WindowEvent::MouseInput { .. } if hud.choosing_quantity() => true,
                WindowEvent::MouseInput { state, button, .. } if !nav || drag => {
                    let point = self.cursor;
                    if drag {
                        if !state.is_pressed() {
                            if *button == MouseButton::Left {
                                hud.finish_inventory_drag(reader, world, point, false);
                            } else {
                                hud.cancel_inventory_drag();
                            }
                        }
                    } else if let Some(p) = point {
                        match (button, state) {
                            (MouseButton::Left, ElementState::Pressed) => {
                                if screen == Screen::Shop
                                    && let Some(id) = hud.begin_inventory_drag(p)
                                {
                                    if let Some(audio) = &self.audio {
                                        audio.effect(id, false);
                                    }
                                } else if hud.press(p) {
                                    audio::ButtonSound::Artwork
                                        .play(audio::ButtonEvent::Down, self.audio.as_ref());
                                }
                            }
                            (MouseButton::Left, ElementState::Released) => {
                                if let Some(action) = hud.release(p) {
                                    audio::ButtonSound::Artwork
                                        .play(audio::ButtonEvent::Up, self.audio.as_ref());
                                    match action {
                                        hud::Action::Sheet(_) => {
                                            hud.sheet_action(action, world, reader)
                                        }
                                        hud::Action::InventoryArrange => {
                                            hud.inventory_action(reader, world, action)
                                        }
                                        hud::Action::InventoryScrollDown
                                        | hud::Action::InventoryScrollUp => {
                                            hud.scroll_inventory(
                                                None,
                                                if action == hud::Action::InventoryScrollDown {
                                                    1
                                                } else {
                                                    -1
                                                },
                                            );
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            (MouseButton::Right, ElementState::Released)
                                if screen == Screen::Shop =>
                            {
                                hud.quick_inventory_transfer(reader, world, p)
                            }
                            _ => {}
                        }
                    }
                    true
                }
                WindowEvent::MouseWheel { delta, .. } => {
                    let y = match delta {
                        MouseScrollDelta::LineDelta(_, y) => *y,
                        MouseScrollDelta::PixelDelta(p) => p.y as f32,
                    };
                    hud.scroll_inventory(self.cursor, -(y.signum() as i32));
                    true
                }
                _ => false,
            };
            if handled || matches!(event, WindowEvent::CursorMoved { .. }) {
                self.view.as_ref().unwrap().window.request_redraw();
            }
            if handled {
                return;
            }
        }
        if self.pause_event(events, &event) {
            return;
        }
        if matches!(
            event,
            WindowEvent::CloseRequested
                | WindowEvent::Resized(_)
                | WindowEvent::ScaleFactorChanged { .. }
                | WindowEvent::Occluded(_)
                | WindowEvent::Focused(_)
        ) {
            self.render_event(events, id, event, Duration::ZERO);
        }
    }

    fn draw_front(&mut self, size: [u32; 2]) -> Result<(), String> {
        let panel = self.creation_panel();
        let scale = self.ui_scale();
        let menu = &self.pause.as_ref().unwrap().0;
        let theme = self.pause_theme.as_mut().unwrap();
        let mut frame = theme.render(menu, size, scale, self.settings);
        // The gameplay lens in Options has no live camera before a game starts.
        frame.front_lens();
        let view = self.view.as_mut().unwrap();
        let ViewContent::Stream {
            world,
            hud,
            inventory_reader,
            camera,
            ..
        } = &mut self.content
        else {
            unreachable!()
        };
        let mut panel_hud = hud.as_deref_mut().filter(|_| panel.is_some());
        if let (Some(hud), Some(reader)) =
            (panel_hud.as_deref_mut(), inventory_reader.as_deref_mut())
        {
            hud.refresh_sheet(reader, world);
            hud.refresh_inventory(reader, world);
            hud.update_fatigue(world);
            if let Some(p) = world.merchant_portrait() {
                hud.set_lens_asset(hud::Binding::MerchantPortrait, p.clone());
            }
            hud.layout_group(
                (size[0], size[1]),
                None,
                Some(theme.panel_canvas(size, scale)),
            );
            hud.hover(self.cursor);
            let binding = if panel == Some(Screen::Shop) {
                hud::Binding::InventoryLens
            } else {
                hud::Binding::SheetLens
            };
            if let Some([w, h]) = hud.native_size(binding) {
                let inset = view
                    .inventory_inset
                    .get_or_insert_with(|| inventory_inset(w, h));
                if (inset.decoded.frame.width, inset.decoded.frame.height) != (w, h) {
                    *inset = inventory_inset(w, h);
                }
                let mut focused = camera.clone();
                world.follow_player(Duration::ZERO, &mut focused);
                view.renderer.render_inset(
                    inset,
                    0.5,
                    [0; 3],
                    scene_draws(
                        world,
                        &focused,
                        (2 * w, 2 * h),
                        Duration::ZERO,
                        self.settings,
                    ),
                )?;
                hud.set_lens_asset(binding, Arc::clone(inset));
            }
            let message = if panel == Some(Screen::Allocate) {
                hud.sheet_message()
            } else {
                hud.inventory_message()
            };
            if !message.is_empty() {
                theme.creation_message(&mut frame, message);
            }
        }
        view.renderer.redraw(
            ((size[0], size[1]), 1),
            [0; 3],
            std::iter::empty(),
            panel_hud
                .into_iter()
                .flat_map(|h| h.draws(false))
                .chain(frame.draws()),
            &view.window,
            false,
        )?;
        view.submitted_frames += 1;
        Ok(())
    }
}
