pub mod audio;
pub mod boxedarray;
pub mod frontend;
pub mod input;
pub mod num;
pub mod timeutils;

#[inline]
#[must_use]
pub fn is_appimage_build() -> bool {
    option_env!("JGENESIS_APPIMAGE_BUILD").is_some_and(|var| !var.is_empty())
}
