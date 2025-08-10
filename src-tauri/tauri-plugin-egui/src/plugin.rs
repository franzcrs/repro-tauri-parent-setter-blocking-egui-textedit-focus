use anyhow::Error;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tauri::{AppHandle, Manager, PhysicalSize};
use tauri_runtime::window::CursorIcon;
use tauri_runtime::UserEvent;

use tauri_runtime_wry::{Context, PluginBuilder, WindowMessage};
use tauri_runtime_wry::{EventLoopIterationContext, Message, Plugin, WebContextStore};

use tauri_runtime_wry::tao::event::{
    ElementState, Event, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent as TaoWindowEvent,
};
use tauri_runtime_wry::tao::event_loop::{ControlFlow, EventLoopProxy, EventLoopWindowTarget};
use tauri_runtime_wry::tao::keyboard::{Key, KeyCode};

use crate::renderer::Renderer;
use crate::utils::{get_id_from_tao_id, get_label_from_tao_id};

/// A map of EguiWindow instances, keyed by their Tauri window label.
type EguiWindowMap = Arc<Mutex<HashMap<String, EguiWindow>>>;

// The builder pattern is mandatorily needed for a Tauri `.wry_plugin()`
// It sets up the tauri state + offers a hook into the event system
pub struct Builder {
    app: AppHandle,
}

impl Builder {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl<T: UserEvent> PluginBuilder<T> for Builder {
    type Plugin = EguiPlugin<T>;

    fn build(self, _: Context<T>) -> Self::Plugin {
        let egui_window_map: EguiWindowMap = Arc::new(Mutex::new(HashMap::new()));
        self.app.manage(egui_window_map.clone());
        EguiPlugin::new(egui_window_map)
    }
}

pub struct EguiPlugin<T: UserEvent> {
    windows: EguiWindowMap,
    _phantom: std::marker::PhantomData<T>, // this does nothing, just keeps compiler happy
}

impl<T: UserEvent> EguiPlugin<T> {
    fn new(windows: EguiWindowMap) -> Self {
        Self {
            windows,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T: UserEvent> Plugin<T> for EguiPlugin<T> {
    fn on_event(
        &mut self,
        event: &Event<Message<T>>,
        _event_loop: &EventLoopWindowTarget<Message<T>>,
        proxy: &EventLoopProxy<Message<T>>,
        _control_flow: &mut ControlFlow,
        context: EventLoopIterationContext<'_, T>,
        _: &WebContextStore,
    ) -> bool {
        match event {
            Event::WindowEvent {
                event, window_id, ..
            } => {
                if let Some(label) = get_label_from_tao_id(window_id, &context) {
                    let mut windows = self.windows.lock().unwrap();
                    if let Some(egui_win) = windows.get_mut(&label) {
                        match event {
                            TaoWindowEvent::Resized(size) => {
                                egui_win.size = PhysicalSize::new(size.width, size.height);
                                egui_win.renderer.resize(size.width, size.height);
                                return true;
                            }
                            _ => {
                                let consumed = egui_win.handle_event(event);

                                let win_id = get_id_from_tao_id(window_id, &context);

                                // Request redraw after input events to process accumulated events
                                if let Some(id) = win_id {
                                    proxy
                                        .send_event(Message::Window(
                                            id,
                                            WindowMessage::RequestRedraw,
                                        ))
                                        .ok();
                                }

                                // Request a redraw after any input event
                                return consumed;
                            }
                        }
                    }
                }
            }
            Event::RedrawRequested(window_id) => {
                if let Some(label) = get_label_from_tao_id(window_id, &context) {
                    let mut windows = self.windows.lock().unwrap();
                    if let Some(egui_win) = windows.get_mut(&label) {
                        // Get the egui context from the EguiWindow
                        let raw_input = egui_win.take_egui_input();

                        // Run `ui_fn` (which describes the UI)
                        // This function comes from the tauri app itself and runs every frame.
                        // The `ctx.run()` method processes the inputs and drawings and returns output:
                        // 1. texture info to give to GPU
                        // 2. platform_output to handl events like cursor, copy-paste etc.
                        // 3. pixels_per_point which is the scale factor for rendering
                        let egui::FullOutput {
                            textures_delta,
                            shapes,
                            pixels_per_point,
                            platform_output,
                            ..
                        } = egui_win.context.run(raw_input, |ctx| {
                            (egui_win.ui_fn)(ctx);
                        });

                        // Handle platform output (clipboard, cursor, links)
                        if let Some(win_id) = get_id_from_tao_id(window_id, &context) {
                            if let Err(e) =
                                egui_win.handle_platform_output(&platform_output, win_id, &proxy)
                            {
                                eprintln!("Error handling platform output: {}", e);
                            }
                        }

                        // Converts all the shapes into triangles meshes
                        let paint_jobs = egui_win.context.tessellate(shapes, pixels_per_point);

                        let width = egui_win.size.width;
                        let height = egui_win.size.height;

                        let screen_descriptor = egui_wgpu::ScreenDescriptor {
                            size_in_pixels: [width, height],
                            pixels_per_point: pixels_per_point,
                        };

                        // Finally we render textures, paint jobs, etc. using the GPU
                        egui_win.renderer.render_frame(
                            screen_descriptor,
                            paint_jobs,
                            textures_delta,
                        );

                        // Check if egui wants us to repaint and request another redraw
                        if egui_win.context.has_requested_repaint() {
                            let win_id = get_id_from_tao_id(window_id, &context);
                            if let Some(id) = win_id {
                                proxy
                                    .send_event(Message::Window(id, WindowMessage::RequestRedraw))
                                    .ok();
                            }
                        }
                    }
                }
            }
            &_ => {}
        }

        // Return false to let other plugins/handlers process the event
        false
    }
}

/// A collection egui context, renderer and a UI function
struct EguiWindow {
    context: egui::Context,
    renderer: Renderer,
    size: PhysicalSize<u32>,
    ui_fn: Box<dyn FnMut(&egui::Context)>,
    start_time: Instant,
    egui_input: egui::RawInput,
    pointer_pos: Option<egui::Pos2>,
    scale_factor: f32,
    modifiers: egui::Modifiers,
}

unsafe impl Send for EguiWindow {}
unsafe impl Sync for EguiWindow {}

impl EguiWindow {
    fn handle_event(&mut self, event: &TaoWindowEvent) -> bool {
        match event {
            TaoWindowEvent::CursorMoved { position, .. } => {
                let pos = egui::Pos2::new(
                    position.x as f32 / self.scale_factor,
                    position.y as f32 / self.scale_factor,
                );
                self.pointer_pos = Some(pos);
                self.egui_input.events.push(egui::Event::PointerMoved(pos));
                true
            }
            TaoWindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = egui::Modifiers {
                    alt: modifiers.alt_key(),
                    ctrl: modifiers.control_key(),
                    shift: modifiers.shift_key(),
                    #[cfg(target_os = "macos")]
                    mac_cmd: modifiers.super_key(),
                    #[cfg(target_os = "macos")]
                    command: modifiers.super_key(),
                    #[cfg(not(target_os = "macos"))]
                    mac_cmd: false,
                    #[cfg(not(target_os = "macos"))]
                    command: modifiers.control_key(),
                };
                self.egui_input.modifiers = self.modifiers;
                true
            }
            TaoWindowEvent::MouseInput { state, button, .. } => {
                let pressed = *state == ElementState::Pressed;
                let button = match button {
                    MouseButton::Left => egui::PointerButton::Primary,
                    MouseButton::Right => egui::PointerButton::Secondary,
                    MouseButton::Middle => egui::PointerButton::Middle,
                    _ => return false,
                };

                // Use current pointer position, or default to (0,0) if not set
                let pos = self.pointer_pos.unwrap_or(egui::Pos2::ZERO);

                self.egui_input.events.push(egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers: self.modifiers,
                });
                true
            }
            TaoWindowEvent::MouseWheel { delta, .. } => {
                let (x, y) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (*x * 60.0, *y * 60.0),
                    MouseScrollDelta::PixelDelta(pos) => (
                        pos.x as f32 / self.scale_factor,
                        pos.y as f32 / self.scale_factor,
                    ),
                    _ => (0.0, 0.0),
                };
                self.egui_input.events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::Vec2::new(x, y),
                    modifiers: self.modifiers,
                });
                true
            }
            TaoWindowEvent::KeyboardInput { event, .. } => self.handle_keyboard_event(event),
            _ => false,
        }
    }

    fn handle_keyboard_event(&mut self, event: &KeyEvent) -> bool {
        let pressed = event.state == ElementState::Pressed;
        let mut handled = false;

        // Handle text input from the text field
        if pressed {
            if let Some(text) = &event.text {
                if !text.is_empty() {
                    // Filter out control characters
                    let filtered: String = text
                        .chars()
                        .filter(|c| !c.is_control() || *c == '\t' || *c == '\n' || *c == '\r')
                        .collect();

                    if !filtered.is_empty() {
                        self.egui_input.events.push(egui::Event::Text(filtered));
                        handled = true;
                    }
                }
            }
        }

        // Handle key events (logical key first, then physical key fallback)
        if let Some(key) = translate_logical_key(&event.logical_key) {
            self.egui_input.events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: event.repeat,
                modifiers: self.modifiers,
            });
            handled = true;
        } else if let Some(key) = translate_physical_key(&event.physical_key) {
            self.egui_input.events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: event.repeat,
                modifiers: self.modifiers,
            });
            handled = true;
        }

        handled
    }

    fn handle_platform_output(
        &mut self,
        platform_output: &egui::PlatformOutput,
        window_id: tauri_runtime::window::WindowId,
        proxy: &EventLoopProxy<Message<impl UserEvent>>,
    ) -> Result<(), Error> {
        // Handle cursor changes
        let cursor_icon = platform_output.cursor_icon;
        let tauri_cursor = egui_cursor_to_tauri_cursor(cursor_icon);

        if let Err(e) = proxy.send_event(Message::Window(
            window_id,
            WindowMessage::SetCursorIcon(tauri_cursor),
        )) {
            eprintln!("Failed to send cursor message: {}", e);
        }

        // Handle commands (clipboard, URL opening, etc.)
        for command in &platform_output.commands {
            match command {
                egui::output::OutputCommand::CopyText(text) => {
                    // TODO: Set clipboard content
                    // For now, just print to console as a placeholder
                    println!("Clipboard copy text requested: {}", text);
                }
                egui::output::OutputCommand::CopyImage(image) => {
                    // TODO: Set clipboard image content
                    // For now, just print to console as a placeholder
                    println!(
                        "Clipboard copy image requested: {}x{}",
                        image.width(),
                        image.height()
                    );
                }
                egui::output::OutputCommand::OpenUrl(url) => {
                    // TODO: Open URL in default browser
                    // For now, just print to console as a placeholder
                    println!(
                        "URL open requested: {} (target: {:?})",
                        url.url, url.new_tab
                    );
                }
            }
        }

        // Handle IME (Input Method Editor) positioning
        if let Some(ime_pos) = platform_output.ime {
            // TODO: Set IME position
            // For now, just print to console as a placeholder
            println!("IME position requested: {:?}", ime_pos);
        }

        Ok(())
    }

    fn take_egui_input(&mut self) -> egui::RawInput {
        let mut input = std::mem::take(&mut self.egui_input);
        input.time = Some(self.start_time.elapsed().as_secs_f64());
        input.screen_rect = Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::Vec2::new(
                self.size.width as f32 / self.scale_factor,
                self.size.height as f32 / self.scale_factor,
            ),
        ));
        input
    }
}

fn translate_logical_key(key: &Key) -> Option<egui::Key> {
    match key {
        Key::Character(ch) => {
            let ch = ch.chars().next()?;
            match ch {
                'a'..='z' | 'A'..='Z' => {
                    let key_char = ch.to_ascii_uppercase();
                    match key_char {
                        'A' => Some(egui::Key::A),
                        'B' => Some(egui::Key::B),
                        'C' => Some(egui::Key::C),
                        'D' => Some(egui::Key::D),
                        'E' => Some(egui::Key::E),
                        'F' => Some(egui::Key::F),
                        'G' => Some(egui::Key::G),
                        'H' => Some(egui::Key::H),
                        'I' => Some(egui::Key::I),
                        'J' => Some(egui::Key::J),
                        'K' => Some(egui::Key::K),
                        'L' => Some(egui::Key::L),
                        'M' => Some(egui::Key::M),
                        'N' => Some(egui::Key::N),
                        'O' => Some(egui::Key::O),
                        'P' => Some(egui::Key::P),
                        'Q' => Some(egui::Key::Q),
                        'R' => Some(egui::Key::R),
                        'S' => Some(egui::Key::S),
                        'T' => Some(egui::Key::T),
                        'U' => Some(egui::Key::U),
                        'V' => Some(egui::Key::V),
                        'W' => Some(egui::Key::W),
                        'X' => Some(egui::Key::X),
                        'Y' => Some(egui::Key::Y),
                        'Z' => Some(egui::Key::Z),
                        _ => None,
                    }
                }
                '0'..='9' => match ch {
                    '0' => Some(egui::Key::Num0),
                    '1' => Some(egui::Key::Num1),
                    '2' => Some(egui::Key::Num2),
                    '3' => Some(egui::Key::Num3),
                    '4' => Some(egui::Key::Num4),
                    '5' => Some(egui::Key::Num5),
                    '6' => Some(egui::Key::Num6),
                    '7' => Some(egui::Key::Num7),
                    '8' => Some(egui::Key::Num8),
                    '9' => Some(egui::Key::Num9),
                    _ => None,
                },
                ' ' => Some(egui::Key::Space),
                '\t' => Some(egui::Key::Tab),
                '\n' | '\r' => Some(egui::Key::Enter),
                '\x08' => Some(egui::Key::Backspace),
                '\x7f' => Some(egui::Key::Delete),
                '\x1b' => Some(egui::Key::Escape),
                _ => None,
            }
        }
        _ => None,
    }
}

fn translate_physical_key(key: &KeyCode) -> Option<egui::Key> {
    match key {
        KeyCode::ArrowDown => Some(egui::Key::ArrowDown),
        KeyCode::ArrowLeft => Some(egui::Key::ArrowLeft),
        KeyCode::ArrowRight => Some(egui::Key::ArrowRight),
        KeyCode::ArrowUp => Some(egui::Key::ArrowUp),
        KeyCode::Escape => Some(egui::Key::Escape),
        KeyCode::Tab => Some(egui::Key::Tab),
        KeyCode::Backspace => Some(egui::Key::Backspace),
        KeyCode::Delete => Some(egui::Key::Delete),
        KeyCode::Enter => Some(egui::Key::Enter),
        KeyCode::Space => Some(egui::Key::Space),
        KeyCode::Insert => Some(egui::Key::Insert),
        KeyCode::Home => Some(egui::Key::Home),
        KeyCode::End => Some(egui::Key::End),
        KeyCode::PageUp => Some(egui::Key::PageUp),
        KeyCode::PageDown => Some(egui::Key::PageDown),
        KeyCode::F1 => Some(egui::Key::F1),
        KeyCode::F2 => Some(egui::Key::F2),
        KeyCode::F3 => Some(egui::Key::F3),
        KeyCode::F4 => Some(egui::Key::F4),
        KeyCode::F5 => Some(egui::Key::F5),
        KeyCode::F6 => Some(egui::Key::F6),
        KeyCode::F7 => Some(egui::Key::F7),
        KeyCode::F8 => Some(egui::Key::F8),
        KeyCode::F9 => Some(egui::Key::F9),
        KeyCode::F10 => Some(egui::Key::F10),
        KeyCode::F11 => Some(egui::Key::F11),
        KeyCode::F12 => Some(egui::Key::F12),
        _ => None,
    }
}

fn egui_cursor_to_tauri_cursor(egui_cursor: egui::CursorIcon) -> CursorIcon {
    match egui_cursor {
        egui::CursorIcon::Default => CursorIcon::Default,
        egui::CursorIcon::None => CursorIcon::Default, // No equivalent, use default
        egui::CursorIcon::ContextMenu => CursorIcon::ContextMenu,
        egui::CursorIcon::Help => CursorIcon::Help,
        egui::CursorIcon::PointingHand => CursorIcon::Hand,
        egui::CursorIcon::Progress => CursorIcon::Progress,
        egui::CursorIcon::Wait => CursorIcon::Wait,
        egui::CursorIcon::Cell => CursorIcon::Cell,
        egui::CursorIcon::Crosshair => CursorIcon::Crosshair,
        egui::CursorIcon::Text => CursorIcon::Text,
        egui::CursorIcon::VerticalText => CursorIcon::VerticalText,
        egui::CursorIcon::Alias => CursorIcon::Alias,
        egui::CursorIcon::Copy => CursorIcon::Copy,
        egui::CursorIcon::Move => CursorIcon::Move,
        egui::CursorIcon::NoDrop => CursorIcon::NoDrop,
        egui::CursorIcon::NotAllowed => CursorIcon::NotAllowed,
        egui::CursorIcon::Grab => CursorIcon::Grab,
        egui::CursorIcon::Grabbing => CursorIcon::Grabbing,
        egui::CursorIcon::AllScroll => CursorIcon::AllScroll,
        egui::CursorIcon::ResizeHorizontal => CursorIcon::EwResize,
        egui::CursorIcon::ResizeNeSw => CursorIcon::NeswResize,
        egui::CursorIcon::ResizeNwSe => CursorIcon::NwseResize,
        egui::CursorIcon::ResizeVertical => CursorIcon::NsResize,
        egui::CursorIcon::ZoomIn => CursorIcon::ZoomIn,
        egui::CursorIcon::ZoomOut => CursorIcon::ZoomOut,
        // Fallback for any other variants
        _ => CursorIcon::Default,
    }
}

pub trait AppHandleExt {
    fn start_egui_for_window(
        &self,
        label: &str,
        ui_fn: Box<dyn FnMut(&egui::Context)>,
    ) -> Result<(), Error>;
}

impl AppHandleExt for AppHandle {
    fn start_egui_for_window(
        &self,
        label: &str,
        ui_fn: Box<dyn FnMut(&egui::Context)>,
    ) -> Result<(), Error> {
        // check if plugin is init'd
        let egui_windows = self
            .try_state::<EguiWindowMap>()
            .ok_or(Error::msg("TauriPluginEgui is not initialized"))?;

        // check if window exists
        let window = self
            .get_window(label)
            .ok_or(Error::msg("No Window found with the provided label."))?;

        // extract relevant window details
        let scale_factor = window.scale_factor().unwrap_or(1.0) as f32;
        let size = window.inner_size()?;
        let PhysicalSize { width, height } = size;

        // create egui context + renderer
        let context = egui::Context::default();
        context.set_zoom_factor(scale_factor);
        let renderer =
            tauri::async_runtime::block_on(
                async move { Renderer::new(window, width, height).await },
            )?;

        // track in the plugin state
        let mut managed_windows = egui_windows.lock().unwrap();
        managed_windows.insert(
            label.to_string(),
            EguiWindow {
                context,
                renderer,
                ui_fn,
                size,
                start_time: Instant::now(),
                egui_input: egui::RawInput::default(),
                pointer_pos: None,
                scale_factor,
                modifiers: egui::Modifiers::NONE,
            },
        );

        Ok(())
    }
}
