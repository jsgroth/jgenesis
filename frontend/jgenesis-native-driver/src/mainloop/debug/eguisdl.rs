//! Largely adapted (and simplified) from `egui_winit_platform`:
//! <https://github.com/hasenbanck/egui_winit_platform>

use sdl2::event::Event as SdlEvent;
use sdl2::event::WindowEvent as SdlWindowEvent;
use sdl2::mouse::MouseWheelDirection;

pub struct Platform {
    window_id: u32,
    scale_factor: f32,
    context: egui::Context,
    raw_input: egui::RawInput,
}

impl Platform {
    pub fn new(window: &sdl2::video::Window, scale_factor: f32) -> Self {
        let context = egui::Context::default();

        let (width, height) = window.size();
        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::default(),
                egui::Vec2::new(width as f32, height as f32) / scale_factor,
            )),
            pixels_per_point: Some(scale_factor),
            ..egui::RawInput::default()
        };

        Self { window_id: window.id(), scale_factor, context, raw_input }
    }

    pub fn handle_event(&mut self, event: &SdlEvent) {
        match *event {
            SdlEvent::Window { window_id, win_event, .. } if window_id == self.window_id => {
                match win_event {
                    SdlWindowEvent::Resized(width, height)
                    | SdlWindowEvent::SizeChanged(width, height) => {
                        self.raw_input.screen_rect = Some(egui::Rect::from_min_size(
                            egui::Pos2::default(),
                            egui::Vec2::new(width as f32, height as f32) / self.scale_factor,
                        ));
                    }
                    _ => {}
                }
            }
            SdlEvent::MouseMotion { window_id, x, y, .. } if window_id == self.window_id => {
                let pointer_pos =
                    egui::Pos2::new(x as f32 / self.scale_factor, y as f32 / self.scale_factor);
                self.raw_input.events.push(egui::Event::PointerMoved(pointer_pos));
            }
            SdlEvent::MouseButtonDown { window_id, mouse_btn, x, y, .. }
                if window_id == self.window_id =>
            {
                let Some(egui_button) = sdl_mouse_button_to_egui(mouse_btn) else { return };

                let pointer_pos =
                    egui::Pos2::new(x as f32 / self.scale_factor, y as f32 / self.scale_factor);
                self.raw_input.events.push(egui::Event::PointerButton {
                    pos: pointer_pos,
                    button: egui_button,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                });
            }
            SdlEvent::MouseButtonUp { window_id, mouse_btn, x, y, .. }
                if window_id == self.window_id =>
            {
                let Some(egui_button) = sdl_mouse_button_to_egui(mouse_btn) else { return };

                let pointer_pos =
                    egui::Pos2::new(x as f32 / self.scale_factor, y as f32 / self.scale_factor);
                self.raw_input.events.push(egui::Event::PointerButton {
                    pos: pointer_pos,
                    button: egui_button,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                });
            }
            SdlEvent::MouseWheel { window_id, direction, precise_x, precise_y, .. }
                if window_id == self.window_id =>
            {
                // Multiplier of 15 somewhat arbitrary - scrolling is way too slow without any multiplier
                let mut delta = 15.0 * egui::Vec2::new(precise_x, precise_y);
                if direction == MouseWheelDirection::Flipped {
                    delta *= -1.0;
                }
                self.raw_input.events.push(egui::Event::Scroll(delta));
            }
            _ => {}
        }
    }

    pub fn update_time(&mut self, elapsed_secs: f64) {
        self.raw_input.time = Some(elapsed_secs);
    }

    pub fn begin_frame(&mut self) {
        self.context.begin_frame(self.raw_input.take());
    }

    pub fn context(&self) -> &egui::Context {
        &self.context
    }

    pub fn end_frame(&self) -> egui::FullOutput {
        self.context.end_frame()
    }
}

fn sdl_mouse_button_to_egui(button: sdl2::mouse::MouseButton) -> Option<egui::PointerButton> {
    use sdl2::mouse::MouseButton::*;

    match button {
        Left => Some(egui::PointerButton::Primary),
        Right => Some(egui::PointerButton::Secondary),
        Middle => Some(egui::PointerButton::Middle),
        X1 => Some(egui::PointerButton::Extra1),
        X2 => Some(egui::PointerButton::Extra2),
        Unknown => None,
    }
}
