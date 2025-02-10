use std::env;
use std::path::{Path, PathBuf};

#[inline]
#[must_use]
pub fn is_appimage_build() -> bool {
    option_env!("JGENESIS_APPIMAGE_BUILD").is_some_and(|var| !var.is_empty())
}

/// Fix relative input paths when running from inside an AppImage.
///
/// The AppImage runner changes the current working directory to a subdirectory inside the AppImage
/// package, which leads to very confusing behavior if a user passes a relative path as input. This
/// function prefixes relative paths with the working directory that the AppImage was launched from.
///
/// Returns the input path untouched if the input path is absolute or this is not an AppImage build.
#[inline]
#[must_use]
pub fn fix_appimage_relative_path(path: PathBuf) -> PathBuf {
    if !is_appimage_build() || path.is_absolute() {
        return path;
    }

    let Ok(owd) = env::var("OWD") else { return path };
    let converted = Path::new(&owd).join(&path);

    log::info!(
        "Converted relative path '{}' to absolute path '{}'",
        path.display(),
        converted.display()
    );

    converted
}
