use sdl3::video::Window;

pub struct Clipboard {
    #[cfg(all(unix, not(target_os = "macos")))]
    smithay: Option<smithay_clipboard::Clipboard>,
    arboard: Option<arboard::Clipboard>,
    fallback: String,
}

impl Clipboard {
    /// Create a new clipboard.
    ///
    /// # Safety
    ///
    /// The returned clipboard must not outlive the referenced window.
    #[allow(unused_variables)] // window param is not used on Windows/MacOS
    pub unsafe fn new(window: &Window) -> Self {
        Self {
            #[cfg(all(unix, not(target_os = "macos")))]
            smithay: unsafe { try_create_smithay_clipboard(window) },
            arboard: arboard::Clipboard::new().ok(),
            fallback: String::new(),
        }
    }

    pub fn load(&mut self) -> String {
        // Prefer smithay-clipboard on Wayland; arboard's Wayland support has some caveats
        #[cfg(all(unix, not(target_os = "macos")))]
        if let Some(clipboard) = &self.smithay {
            match clipboard.load() {
                Ok(text) => return text,
                Err(_) => {
                    log::error!(
                        "smithay-clipboard thread died; falling back to other clipboard implementations"
                    );
                    self.smithay = None;
                }
            }
        }

        if let Some(clipboard) = &mut self.arboard
            && let Ok(text) = clipboard.get_text()
        {
            return text;
        }

        self.fallback.clone()
    }

    pub fn store(&mut self, text: String) {
        self.fallback.clone_from(&text);

        #[cfg(all(unix, not(target_os = "macos")))]
        if let Some(clipboard) = &self.smithay {
            clipboard.store(text);
            return;
        }

        if let Some(clipboard) = &mut self.arboard {
            let _ = clipboard.set_text(text);
        }
    }
}

// SAFETY: The returned clipboard must not outlive the referenced window
#[cfg(all(unix, not(target_os = "macos")))]
unsafe fn try_create_smithay_clipboard(window: &Window) -> Option<smithay_clipboard::Clipboard> {
    use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};

    match window.display_handle().ok()?.as_raw() {
        RawDisplayHandle::Wayland(handle) => {
            // SAFETY: This is guaranteed to be a valid wl_display pointer
            unsafe { Some(smithay_clipboard::Clipboard::new(handle.display.as_ptr())) }
        }
        _ => None,
    }
}
