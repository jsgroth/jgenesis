use std::env;
use std::path::{Path, PathBuf};

#[inline]
#[must_use]
pub const fn is_appimage_build() -> bool {
    match option_env!("JGENESIS_APPIMAGE_BUILD") {
        Some(var) => !var.is_empty(),
        None => false,
    }
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

/// Directory containing the emulator executable
///
/// Returns `None` if unable to determine
#[inline]
#[must_use]
pub fn determine_emulator_dir() -> Option<PathBuf> {
    let exe_path: PathBuf = if is_appimage_build() {
        // When running from inside an AppImage, env::current_exe() returns a path inside the mount's
        // temp dir.
        //
        // APPIMAGE env var should contain the path to the AppImage file:
        //    https://docs.appimage.org/packaging-guide/environment-variables.html
        env::var("APPIMAGE").ok()?.into()
    } else {
        env::current_exe().ok()?
    };

    exe_path.parent().map(ToOwned::to_owned)
}
