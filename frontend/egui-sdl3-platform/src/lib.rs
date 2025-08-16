//! Largely adapted (and simplified) from `egui_winit_platform`:
//! <https://github.com/hasenbanck/egui_winit_platform>

use egui::ahash::HashMapExt;
use egui::{MouseWheelUnit, ViewportIdMap, ViewportInfo};
use sdl3::event::Event as SdlEvent;
use sdl3::event::WindowEvent as SdlWindowEvent;
use sdl3::mouse::MouseWheelDirection;

pub struct Platform {
    window_id: u32,
    scale_factor: f32,
    context: egui::Context,
    raw_input: egui::RawInput,
}

impl Platform {
    #[must_use]
    pub fn new(window: &sdl3::video::Window, scale_factor: f32) -> Self {
        let context = egui::Context::default();

        let mut viewports = ViewportIdMap::new();
        viewports.insert(
            context.viewport_id(),
            ViewportInfo { native_pixels_per_point: Some(scale_factor), ..ViewportInfo::default() },
        );

        let (width, height) = window.size();
        let raw_input = egui::RawInput {
            viewport_id: context.viewport_id(),
            viewports,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::default(),
                egui::Vec2::new(width as f32, height as f32) / scale_factor,
            )),
            ..egui::RawInput::default()
        };

        Self { window_id: window.id(), scale_factor, context, raw_input }
    }

    pub fn handle_event(&mut self, event: &SdlEvent) {
        match event {
            &SdlEvent::Window {
                window_id,
                win_event:
                    SdlWindowEvent::Resized(width, height)
                    | SdlWindowEvent::PixelSizeChanged(width, height),
                ..
            } if window_id == self.window_id => {
                self.raw_input.screen_rect = Some(egui::Rect::from_min_size(
                    egui::Pos2::default(),
                    egui::Vec2::new(width as f32, height as f32) / self.scale_factor,
                ));
            }
            &SdlEvent::MouseMotion { window_id, x, y, .. } if window_id == self.window_id => {
                let pointer_pos = egui::Pos2::new(x / self.scale_factor, y / self.scale_factor);
                self.raw_input.events.push(egui::Event::PointerMoved(pointer_pos));
            }
            &SdlEvent::MouseButtonDown { window_id, mouse_btn, x, y, .. }
                if window_id == self.window_id =>
            {
                let Some(egui_button) = sdl_mouse_button_to_egui(mouse_btn) else { return };

                let pointer_pos = egui::Pos2::new(x / self.scale_factor, y / self.scale_factor);
                self.raw_input.events.push(egui::Event::PointerButton {
                    pos: pointer_pos,
                    button: egui_button,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                });
            }
            &SdlEvent::MouseButtonUp { window_id, mouse_btn, x, y, .. }
                if window_id == self.window_id =>
            {
                let Some(egui_button) = sdl_mouse_button_to_egui(mouse_btn) else { return };

                let pointer_pos = egui::Pos2::new(x / self.scale_factor, y / self.scale_factor);
                self.raw_input.events.push(egui::Event::PointerButton {
                    pos: pointer_pos,
                    button: egui_button,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
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
                self.raw_input.events.push(egui::Event::MouseWheel {
                    unit: MouseWheelUnit::Point,
                    delta,
                    modifiers: self.raw_input.modifiers,
                });
            }
            &SdlEvent::KeyDown { window_id, keycode: Some(keycode), keymod, repeat, .. }
                if window_id == self.window_id =>
            {
                let Some(egui_key) = sdl_keycode_to_egui(keycode) else { return };

                let modifiers = sdl_mod_to_egui(keymod);

                self.raw_input.events.push(egui::Event::Key {
                    key: egui_key,
                    physical_key: None,
                    pressed: true,
                    repeat,
                    modifiers,
                });
            }
            &SdlEvent::KeyUp { window_id, keycode: Some(keycode), keymod, repeat, .. }
                if window_id == self.window_id =>
            {
                let Some(egui_key) = sdl_keycode_to_egui(keycode) else { return };

                let modifiers = sdl_mod_to_egui(keymod);

                self.raw_input.events.push(egui::Event::Key {
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
                self.raw_input.events.push(egui::Event::Text(text.clone()));
            }
            _ => {}
        }
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
        Keycode::Backspace => Some(Key::Backspace),
        Keycode::Delete => Some(Key::Delete),
        Keycode::Return => Some(Key::Enter),
        Keycode::Left => Some(Key::ArrowLeft),
        Keycode::Right => Some(Key::ArrowRight),
        Keycode::Up => Some(Key::ArrowUp),
        Keycode::Down => Some(Key::ArrowDown),
        Keycode::Tab => Some(Key::Tab),
        Keycode::Insert => Some(Key::Insert),
        Keycode::Space => Some(Key::Space),
        Keycode::Home => Some(Key::Home),
        Keycode::End => Some(Key::End),
        _ => None,
    }
}

fn sdl_mod_to_egui(sdl_mod: sdl3::keyboard::Mod) -> egui::Modifiers {
    use sdl3::keyboard::Mod;

    let ctrl = sdl_mod & Mod::LCTRLMOD != Mod::NOMOD || sdl_mod & Mod::RCTRLMOD != Mod::NOMOD;
    egui::Modifiers {
        alt: sdl_mod & Mod::LALTMOD != Mod::NOMOD || sdl_mod & Mod::RALTMOD != Mod::NOMOD,
        ctrl,
        shift: sdl_mod & Mod::LSHIFTMOD != Mod::NOMOD || sdl_mod & Mod::RSHIFTMOD != Mod::NOMOD,
        mac_cmd: false,
        // TODO this is not correct for MacOS
        command: ctrl,
    }
}
