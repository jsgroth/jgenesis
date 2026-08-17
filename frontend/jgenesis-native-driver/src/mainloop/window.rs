use crate::NativeEmulatorResult;
use jgenesis_renderer::renderer;
use jgenesis_renderer::renderer::WgpuRenderer;
use sdl3::VideoSubsystem;
use sdl3::video::{FullscreenType, Window};
use std::ffi::NulError;

pub trait RendererExt {
    fn focus(&mut self);

    fn window_id(&self) -> u32;

    fn is_fullscreen(&self) -> bool;

    // Returns new fullscreen state
    fn toggle_fullscreen(&mut self) -> Result<bool, sdl3::Error>;

    /// Change the window title.
    ///
    /// # Errors
    ///
    /// Returns an error if the title contains any null characters.
    fn set_window_title(&mut self, title: &str) -> Result<(), NulError>;
}

impl RendererExt for WgpuRenderer<Window> {
    fn focus(&mut self) {
        // SAFETY: This is not reassigning the window
        unsafe {
            self.window_mut().raise();
        }
    }

    fn window_id(&self) -> u32 {
        self.window().id()
    }

    fn is_fullscreen(&self) -> bool {
        matches!(self.window().fullscreen_state(), FullscreenType::Desktop | FullscreenType::True)
    }

    fn toggle_fullscreen(&mut self) -> Result<bool, sdl3::Error> {
        // SAFETY: This is not reassigning the window
        unsafe {
            let window = self.window_mut();
            let currently_fullscreen = window.fullscreen_state() != FullscreenType::Off;
            let new_fullscreen = !currently_fullscreen;
            window.set_fullscreen(new_fullscreen)?;

            Ok(new_fullscreen)
        }
    }

    fn set_window_title(&mut self, title: &str) -> Result<(), NulError> {
        // SAFETY: This is not reassigning the window
        unsafe { self.window_mut().set_title(title) }
    }
}

pub fn create(
    video: &VideoSubsystem,
    title: &str,
    width: u32,
    height: u32,
    fullscreen: bool,
) -> NativeEmulatorResult<Window> {
    let display_scale = video
        .get_primary_display()
        .ok()
        .and_then(|display| display.get_content_scale().ok())
        .unwrap_or(1.0);

    let mut window_builder = video.window(
        title,
        (width as f32 * display_scale).round() as u32,
        (height as f32 * display_scale).round() as u32,
    );
    window_builder.metal_view();
    window_builder.high_pixel_density();
    window_builder.resizable();
    window_builder.position_centered();

    if fullscreen {
        window_builder.fullscreen();
    }

    let window = window_builder.build()?;
    Ok(window)
}

pub fn size(window: &Window) -> renderer::WindowSize {
    let (width, height) = window.size_in_pixels();
    let pixel_density = window.pixel_density();

    renderer::WindowSize { width, height, pixel_density }
}
