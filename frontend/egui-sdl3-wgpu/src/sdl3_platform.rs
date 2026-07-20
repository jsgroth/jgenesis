//! Largely adapted (and simplified) from `egui_winit_platform`:
//! <https://github.com/hasenbanck/egui_winit_platform>

use crate::clipboard::Clipboard;
use egui::ahash::HashMapExt;
use egui::{
    Event, MouseWheelUnit, OutputCommand, PlatformOutput, TouchPhase, ViewportId, ViewportIdMap,
    ViewportInfo,
};
use sdl3::event::Event as SdlEvent;
use sdl3::event::WindowEvent as SdlWindowEvent;
use sdl3::mouse::MouseWheelDirection;

pub struct Platform {
    window_id: u32,
    window_display_scale: f32,
    window_pixel_density: f32,
    context: egui::Context,
    raw_input: egui::RawInput,
}

impl Platform {
    #[must_use]
    pub fn new(window: &sdl3::video::Window) -> Self {
        let context = egui::Context::default();

        let display_scale = window.display_scale();
        let pixel_density = window.pixel_density();

        let mut viewports = ViewportIdMap::new();
        viewports.insert(
            ViewportId::ROOT,
            ViewportInfo {
                native_pixels_per_point: Some(display_scale),
                ..ViewportInfo::default()
            },
        );

        let (width, height) = window.size_in_pixels();
        let raw_input = egui::RawInput {
            viewport_id: context.viewport_id(),
            viewports,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::default(),
                egui::Vec2::new(width as f32, height as f32) / display_scale,
            )),
            ..egui::RawInput::default()
        };

        Self {
            window_id: window.id(),
            window_display_scale: display_scale,
            window_pixel_density: pixel_density,
            context,
            raw_input,
        }
    }

    pub fn handle_event(&mut self, event: &SdlEvent, clipboard: &mut Clipboard) {
        match event {
            &SdlEvent::Window { window_id, win_event, .. } if window_id == self.window_id => {
                match win_event {
                    SdlWindowEvent::PixelSizeChanged(width, height) => {
                        self.raw_input.screen_rect = Some(egui::Rect::from_min_size(
                            egui::Pos2::default(),
                            egui::Vec2::new(width as f32, height as f32)
                                / self.window_display_scale,
                        ));
                        self.context.request_repaint();
                    }
                    SdlWindowEvent::FocusGained => {
                        self.raw_input.focused = true;
                        self.context.request_repaint();
                    }
                    SdlWindowEvent::FocusLost => {
                        self.raw_input.focused = false;
                        self.context.request_repaint();
                    }
                    _ => {}
                }
            }
            &SdlEvent::MouseMotion { window_id, x, y, .. } if window_id == self.window_id => {
                let pointer_pos = self.mouse_pos_to_egui_pos(x, y);
                self.raw_input.events.push(Event::PointerMoved(pointer_pos));
            }
            &SdlEvent::MouseButtonDown { window_id, mouse_btn, x, y, .. }
                if window_id == self.window_id =>
            {
                let Some(egui_button) = sdl_mouse_button_to_egui(mouse_btn) else { return };

                let pointer_pos = self.mouse_pos_to_egui_pos(x, y);
                self.raw_input.events.push(Event::PointerButton {
                    pos: pointer_pos,
                    button: egui_button,
                    pressed: true,
                    modifiers: self.raw_input.modifiers,
                });
            }
            &SdlEvent::MouseButtonUp { window_id, mouse_btn, x, y, .. }
                if window_id == self.window_id =>
            {
                let Some(egui_button) = sdl_mouse_button_to_egui(mouse_btn) else { return };

                let pointer_pos = self.mouse_pos_to_egui_pos(x, y);
                self.raw_input.events.push(Event::PointerButton {
                    pos: pointer_pos,
                    button: egui_button,
                    pressed: false,
                    modifiers: self.raw_input.modifiers,
                });
            }
            &SdlEvent::MouseWheel { window_id, direction, x, y, .. }
                if window_id == self.window_id =>
            {
                // Multiplier of 15 somewhat arbitrary - scrolling is way too slow without any multiplier
                let mut delta = 15.0 * egui::Vec2::new(x, y);
                if direction == MouseWheelDirection::Flipped {
                    delta *= -1.0;
                }
                self.raw_input.events.push(Event::MouseWheel {
                    unit: MouseWheelUnit::Point,
                    delta,
                    phase: TouchPhase::Move,
                    modifiers: self.raw_input.modifiers,
                });
            }
            &SdlEvent::KeyDown { window_id, keycode: Some(keycode), keymod, repeat, .. }
                if window_id == self.window_id =>
            {
                let modifiers = sdl_mod_to_egui(keymod);
                self.raw_input.modifiers = modifiers;

                let Some(egui_key) = sdl_keycode_to_egui(keycode) else { return };
                self.raw_input.events.push(Event::Key {
                    key: egui_key,
                    physical_key: None,
                    pressed: true,
                    repeat,
                    modifiers,
                });

                if modifiers.command {
                    match keycode {
                        sdl3::keyboard::Keycode::C => {
                            self.raw_input.events.push(Event::Copy);
                        }
                        sdl3::keyboard::Keycode::X => {
                            self.raw_input.events.push(Event::Cut);
                        }
                        sdl3::keyboard::Keycode::V => {
                            self.raw_input.events.push(Event::Paste(clipboard.load()));
                        }
                        _ => {}
                    }
                }
            }
            &SdlEvent::KeyUp { window_id, keycode: Some(keycode), keymod, repeat, .. }
                if window_id == self.window_id =>
            {
                let modifiers = sdl_mod_to_egui(keymod);
                self.raw_input.modifiers = modifiers;

                let Some(egui_key) = sdl_keycode_to_egui(keycode) else { return };
                self.raw_input.events.push(Event::Key {
                    key: egui_key,
                    physical_key: None,
                    pressed: false,
                    repeat,
                    modifiers,
                });
            }
            SdlEvent::TextInput { window_id, text, .. }
            | SdlEvent::TextEditing { window_id, text, .. }
                if *window_id == self.window_id =>
            {
                self.raw_input.events.push(Event::Text(text.clone()));
            }
            _ => {}
        }
    }

    fn mouse_pos_to_egui_pos(&self, x: f32, y: f32) -> egui::Pos2 {
        let multiplier = self.window_pixel_density / self.window_display_scale;
        [x * multiplier, y * multiplier].into()
    }

    pub(crate) fn has_pending_input_event(&self) -> bool {
        !self.raw_input.events.is_empty()
    }

    #[must_use]
    pub fn take_raw_input(&mut self, elapsed_secs: f64) -> egui::RawInput {
        self.raw_input.time = Some(elapsed_secs);
        self.raw_input.take()
    }

    #[must_use]
    pub fn context(&self) -> &egui::Context {
        &self.context
    }

    pub fn handle_egui_output(
        &mut self,
        platform_output: &PlatformOutput,
        clipboard: &mut Clipboard,
    ) {
        for command in &platform_output.commands {
            match command {
                OutputCommand::CopyText(text) => {
                    clipboard.store(text.clone());
                }
                OutputCommand::OpenUrl(url) => {
                    if let Err(err) = open::that(&url.url) {
                        log::error!("Error opening URL '{}': {err}", url.url);
                    }
                }
                OutputCommand::CopyImage(_) => {}
            }
        }
    }

    pub fn request_cut(&mut self) {
        self.raw_input.events.push(Event::Cut);
    }

    pub fn request_copy(&mut self) {
        self.raw_input.events.push(Event::Copy);
    }

    pub fn request_paste(&mut self, text: String) {
        self.raw_input.events.push(Event::Paste(text));
    }
}

fn sdl_mouse_button_to_egui(button: sdl3::mouse::MouseButton) -> Option<egui::PointerButton> {
    use sdl3::mouse::MouseButton::*;

    match button {
        Left => Some(egui::PointerButton::Primary),
        Right => Some(egui::PointerButton::Secondary),
        Middle => Some(egui::PointerButton::Middle),
        X1 => Some(egui::PointerButton::Extra1),
        X2 => Some(egui::PointerButton::Extra2),
        Unknown => None,
    }
}

fn sdl_keycode_to_egui(key: sdl3::keyboard::Keycode) -> Option<egui::Key> {
    use egui::Key;
    use sdl3::keyboard::Keycode;

    match key {
        Keycode::A => Some(Key::A),
        Keycode::B => Some(Key::B),
        Keycode::C => Some(Key::C),
        Keycode::D => Some(Key::D),
        Keycode::E => Some(Key::E),
        Keycode::F => Some(Key::F),
        Keycode::G => Some(Key::G),
        Keycode::H => Some(Key::H),
        Keycode::I => Some(Key::I),
        Keycode::J => Some(Key::J),
        Keycode::K => Some(Key::K),
        Keycode::L => Some(Key::L),
        Keycode::M => Some(Key::M),
        Keycode::N => Some(Key::N),
        Keycode::O => Some(Key::O),
        Keycode::P => Some(Key::P),
        Keycode::Q => Some(Key::Q),
        Keycode::R => Some(Key::R),
        Keycode::S => Some(Key::S),
        Keycode::T => Some(Key::T),
        Keycode::U => Some(Key::U),
        Keycode::V => Some(Key::V),
        Keycode::W => Some(Key::W),
        Keycode::X => Some(Key::X),
        Keycode::Y => Some(Key::Y),
        Keycode::Z => Some(Key::Z),
        Keycode::Kp0 | Keycode::_0 => Some(Key::Num0),
        Keycode::Kp1 | Keycode::_1 => Some(Key::Num1),
        Keycode::Kp2 | Keycode::_2 => Some(Key::Num2),
        Keycode::Kp3 | Keycode::_3 => Some(Key::Num3),
        Keycode::Kp4 | Keycode::_4 => Some(Key::Num4),
        Keycode::Kp5 | Keycode::_5 => Some(Key::Num5),
        Keycode::Kp6 | Keycode::_6 => Some(Key::Num6),
        Keycode::Kp7 | Keycode::_7 => Some(Key::Num7),
        Keycode::Kp8 | Keycode::_8 => Some(Key::Num8),
        Keycode::Kp9 | Keycode::_9 => Some(Key::Num9),
        Keycode::Left => Some(Key::ArrowLeft),
        Keycode::Right => Some(Key::ArrowRight),
        Keycode::Up => Some(Key::ArrowUp),
        Keycode::Down => Some(Key::ArrowDown),
        Keycode::Escape => Some(Key::Escape),
        Keycode::Tab => Some(Key::Tab),
        Keycode::Backspace | Keycode::KpBackspace => Some(Key::Backspace),
        Keycode::Return => Some(Key::Enter),
        Keycode::Space | Keycode::KpSpace => Some(Key::Space),
        Keycode::Insert => Some(Key::Insert),
        Keycode::Delete => Some(Key::Delete),
        Keycode::Home => Some(Key::Home),
        Keycode::End => Some(Key::End),
        Keycode::PageUp => Some(Key::PageUp),
        Keycode::PageDown => Some(Key::PageDown),
        Keycode::Cut => Some(Key::Cut),
        Keycode::Copy => Some(Key::Copy),
        Keycode::Paste => Some(Key::Paste),
        Keycode::Colon | Keycode::KpColon => Some(Key::Colon),
        Keycode::Comma | Keycode::KpComma => Some(Key::Comma),
        Keycode::Backslash => Some(Key::Backslash),
        Keycode::Slash => Some(Key::Slash),
        Keycode::Pipe => Some(Key::Pipe),
        Keycode::Question => Some(Key::Questionmark),
        Keycode::LeftBracket => Some(Key::OpenBracket),
        Keycode::RightBracket => Some(Key::CloseBracket),
        Keycode::LeftBrace | Keycode::KpLeftBrace => Some(Key::OpenCurlyBracket),
        Keycode::RightBrace | Keycode::KpRightBrace => Some(Key::CloseCurlyBracket),
        Keycode::Grave => Some(Key::Backtick),
        Keycode::Minus | Keycode::KpMinus => Some(Key::Minus),
        Keycode::Period | Keycode::KpPeriod => Some(Key::Period),
        Keycode::Plus | Keycode::KpPlus => Some(Key::Plus),
        Keycode::Equals | Keycode::KpEquals => Some(Key::Equals),
        Keycode::Semicolon => Some(Key::Semicolon),
        Keycode::Apostrophe => Some(Key::Quote),
        Keycode::F1 => Some(Key::F1),
        Keycode::F2 => Some(Key::F2),
        Keycode::F3 => Some(Key::F3),
        Keycode::F4 => Some(Key::F4),
        Keycode::F5 => Some(Key::F5),
        Keycode::F6 => Some(Key::F6),
        Keycode::F7 => Some(Key::F7),
        Keycode::F8 => Some(Key::F8),
        Keycode::F9 => Some(Key::F9),
        Keycode::F10 => Some(Key::F10),
        Keycode::F11 => Some(Key::F11),
        Keycode::F12 => Some(Key::F12),
        Keycode::F13 => Some(Key::F13),
        Keycode::F14 => Some(Key::F14),
        Keycode::F15 => Some(Key::F15),
        Keycode::F16 => Some(Key::F16),
        Keycode::F17 => Some(Key::F17),
        Keycode::F18 => Some(Key::F18),
        Keycode::F19 => Some(Key::F19),
        Keycode::F20 => Some(Key::F20),
        Keycode::F21 => Some(Key::F21),
        Keycode::F22 => Some(Key::F22),
        Keycode::F23 => Some(Key::F23),
        Keycode::F24 => Some(Key::F24),
        _ => None,
    }
}

fn sdl_mod_to_egui(sdl_mod: sdl3::keyboard::Mod) -> egui::Modifiers {
    use sdl3::keyboard::Mod;

    let ctrl = sdl_mod & (Mod::LCTRLMOD | Mod::RCTRLMOD) != Mod::NOMOD;
    let gui = sdl_mod & (Mod::LGUIMOD | Mod::RGUIMOD) != Mod::NOMOD;

    egui::Modifiers {
        alt: sdl_mod & (Mod::LALTMOD | Mod::RALTMOD) != Mod::NOMOD,
        ctrl,
        shift: sdl_mod & (Mod::LSHIFTMOD | Mod::RSHIFTMOD) != Mod::NOMOD,
        mac_cmd: cfg!(target_os = "macos") && gui,
        command: cfg_select! {
            target_os = "macos" => gui,
            _ => ctrl,
        },
    }
}
