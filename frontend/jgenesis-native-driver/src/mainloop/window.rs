use crate::NativeEmulatorResult;
use jgenesis_renderer::renderer;
use jgenesis_renderer::renderer::WgpuRenderer;
use sdl3::VideoSubsystem;
use sdl3::video::{FullscreenType, HitTestResult, Window};
use std::ffi::NulError;
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

pub struct AtomicWindowSize {
    width: AtomicU32,
    height: AtomicU32,
    pixel_density: AtomicU32, // f32 bits as u32
}

impl AtomicWindowSize {
    fn new(window: &Window) -> Self {
        let window_size = size(window);

        Self {
            width: AtomicU32::new(window_size.width),
            height: AtomicU32::new(window_size.height),
            pixel_density: AtomicU32::new(window_size.pixel_density.to_bits()),
        }
    }

    pub fn update(&self, window_size: renderer::WindowSize) {
        self.width.store(window_size.width, Ordering::Relaxed);
        self.height.store(window_size.height, Ordering::Relaxed);
        self.pixel_density.store(window_size.pixel_density.to_bits(), Ordering::Relaxed);
    }

    fn width(&self) -> u32 {
        self.width.load(Ordering::Relaxed)
    }

    fn height(&self) -> u32 {
        self.height.load(Ordering::Relaxed)
    }

    fn pixel_density(&self) -> f32 {
        f32::from_bits(self.pixel_density.load(Ordering::Relaxed))
    }
}

pub trait RendererExt {
    fn focus(&mut self);

    fn window_id(&self) -> u32;

    fn is_fullscreen(&self) -> bool;

    /// Toggle fullscreen on/off. Returns the new fullscreen state.
    ///
    /// # Errors
    ///
    /// Propagates any SDL3 video errors.
    fn toggle_fullscreen(
        &mut self,
        borderless_window: bool,
        window_size: &Arc<AtomicWindowSize>,
    ) -> Result<bool, sdl3::Error>;

    fn update_borderless(&mut self, borderless: bool, window_size: &Arc<AtomicWindowSize>);

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

    fn toggle_fullscreen(
        &mut self,
        borderless_window: bool,
        window_size: &Arc<AtomicWindowSize>,
    ) -> Result<bool, sdl3::Error> {
        // SAFETY: This is not reassigning the window
        unsafe {
            let window = self.window_mut();
            let currently_fullscreen = window.fullscreen_state() != FullscreenType::Off;
            let new_fullscreen = !currently_fullscreen;
            window.set_fullscreen(new_fullscreen)?;

            if borderless_window {
                update_window_hit_test(
                    self.window_mut(),
                    new_fullscreen,
                    borderless_window,
                    window_size,
                );
            }

            Ok(new_fullscreen)
        }
    }

    fn update_borderless(&mut self, borderless: bool, window_size: &Arc<AtomicWindowSize>) {
        // SAFETY: This is not reassigning the window
        unsafe {
            self.window_mut().set_bordered(!borderless);

            let fullscreen = self.is_fullscreen();
            update_window_hit_test(self.window_mut(), fullscreen, borderless, window_size);
        }
    }

    fn set_window_title(&mut self, title: &str) -> Result<(), NulError> {
        // SAFETY: This is not reassigning the window
        unsafe { self.window_mut().set_title(title) }
    }
}

pub struct CreateWindowArgs<'a> {
    pub title: &'a str,
    pub width: u32,
    pub height: u32,
    pub fullscreen: bool,
    pub borderless: bool,
}

pub fn create(
    video: &VideoSubsystem,
    CreateWindowArgs { title, width, height, fullscreen, borderless }: CreateWindowArgs<'_>,
) -> NativeEmulatorResult<(Window, Arc<AtomicWindowSize>)> {
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

    if borderless {
        window_builder.borderless();
    }

    let mut window = window_builder.build()?;
    let window_size = Arc::new(AtomicWindowSize::new(&window));

    if borderless && !fullscreen {
        set_window_hit_test(&mut window, borderless_hit_test(Arc::clone(&window_size)));
    }

    Ok((window, window_size))
}

pub fn size(window: &Window) -> renderer::WindowSize {
    let (width, height) = window.size_in_pixels();
    let pixel_density = window.pixel_density();

    renderer::WindowSize { width, height, pixel_density }
}

fn update_window_hit_test(
    window: &mut Window,
    fullscreen: bool,
    borderless: bool,
    window_size: &Arc<AtomicWindowSize>,
) {
    if borderless && !fullscreen {
        set_window_hit_test(window, borderless_hit_test(Arc::clone(window_size)));
    } else {
        disable_window_hit_test(window);
    }
}

fn set_window_hit_test(
    window: &mut Window,
    hit_test: impl Fn(sdl3::rect::Point) -> HitTestResult + 'static,
) {
    if let Err(err) = window.set_hit_test(hit_test) {
        log::error!("Error setting window hit test: {err}");
    }
}

fn disable_window_hit_test(window: &mut Window) {
    // Setting a hit test that always returns Normal works on Linux but not Windows; on Windows it
    // makes the window non-resizable. Disabling hit testing works on every platform (that I've tested)
    //
    // SAFETY: Window pointer is guaranteed to be valid, and SDL3 documentation states that passing
    // None/NULL for the callback disables hit testing. The third parameter (callback data) is not
    // used when hit testing is disabled
    unsafe {
        if !sdl3_sys::video::SDL_SetWindowHitTest(window.raw(), None, ptr::null_mut()) {
            log::error!("Error disabling window hit testing: {}", sdl3::get_error());
        }
    }
}

fn borderless_hit_test(
    window_size: Arc<AtomicWindowSize>,
) -> impl Fn(sdl3::rect::Point) -> HitTestResult {
    const RESIZE_BORDER_PIXELS: f64 = 20.0;

    move |point| {
        let width: f64 = window_size.width().into();
        let height: f64 = window_size.height().into();
        let pixel_density: f64 = window_size.pixel_density().into();

        let x = f64::from(point.x) * pixel_density;
        let y = f64::from(point.y) * pixel_density;

        let mut resize_left = x < RESIZE_BORDER_PIXELS;
        let mut resize_right = x >= width - RESIZE_BORDER_PIXELS;
        let mut resize_top = y < RESIZE_BORDER_PIXELS;
        let mut resize_bottom = y >= height - RESIZE_BORDER_PIXELS;

        if resize_left && resize_right {
            resize_left = false;
            resize_right = false;
        }

        if resize_top && resize_bottom {
            resize_top = false;
            resize_bottom = false;
        }

        if resize_top {
            if resize_left {
                HitTestResult::ResizeTopLeft
            } else if resize_right {
                HitTestResult::ResizeTopRight
            } else {
                HitTestResult::ResizeTop
            }
        } else if resize_bottom {
            if resize_left {
                HitTestResult::ResizeBottomLeft
            } else if resize_right {
                HitTestResult::ResizeBottomRight
            } else {
                HitTestResult::ResizeBottom
            }
        } else if resize_left {
            HitTestResult::ResizeLeft
        } else if resize_right {
            HitTestResult::ResizeRight
        } else {
            HitTestResult::Draggable
        }
    }
}
