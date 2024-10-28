pub mod audio;
pub mod boxedarray;
pub mod frontend;
pub mod input;
pub mod num;
pub mod timeutils;

use cfg_if::cfg_if;
use std::thread;
use std::time::Duration;

#[inline]
pub fn sleep(duration: Duration) {
    cfg_if! {
        if #[cfg(target_os = "windows")] {
            // SAFETY: thread::sleep cannot panic, so timeEndPeriod will always be called after timeBeginPeriod.
            unsafe {
                windows::Win32::Media::timeBeginPeriod(1);
                thread::sleep(duration);
                windows::Win32::Media::timeEndPeriod(1);
            }
        } else {
            thread::sleep(duration);
        }
    }
}

#[inline]
#[must_use]
pub fn is_appimage_build() -> bool {
    option_env!("JGENESIS_APPIMAGE_BUILD").is_some_and(|var| !var.is_empty())
}
