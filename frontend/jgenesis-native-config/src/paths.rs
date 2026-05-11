use crate::AppConfig;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::{env, fs, io};
use thiserror::Error;

pub const CONFIG_FILENAME: &str = "jgenesis-config.toml";
pub const JGENESIS_SUBDIR: &str = "jgenesis";
pub const PORTABLE_TXT_FILE_NAME: &str = "portable.txt";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigDirType {
    UserProfile,
    EmulatorDirectory,
    Override { file_path: PathBuf },
}

#[derive(Debug, Clone)]
pub struct ConfigDirs {
    pub user_profile_dir: Option<PathBuf>,
    pub emulator_dir: Option<PathBuf>,
}

impl ConfigDirs {
    #[must_use]
    pub fn new() -> Self {
        let user_profile_dir = determine_user_profile_dir();
        let emulator_dir = determine_emulator_dir();

        if user_profile_dir.is_none() {
            log::error!("Unable to determine user profile directory");
        }

        if emulator_dir.is_none() {
            log::error!("Unable to determine emulator directory");
        }

        Self { user_profile_dir, emulator_dir }
    }

    #[must_use]
    pub fn get(&self, path_type: &ConfigDirType) -> Option<PathBuf> {
        match path_type {
            ConfigDirType::UserProfile => self.user_profile_dir.clone(),
            ConfigDirType::EmulatorDirectory => self.emulator_dir.clone(),
            ConfigDirType::Override { .. } => None,
        }
    }

    #[must_use]
    pub fn default_dir_type(&self, override_file_path: Option<PathBuf>) -> ConfigDirType {
        if let Some(override_path) = override_file_path {
            // Override path passed in via command line; takes priority
            return ConfigDirType::Override { file_path: override_path };
        }

        if self.emulator_dir.as_ref().is_some_and(|emulator_dir| {
            let portable_txt_path = emulator_dir.join(PORTABLE_TXT_FILE_NAME);
            portable_txt_path.exists()
                && portable_txt_path.metadata().ok().is_some_and(|metadata| metadata.is_file())
        }) {
            // portable.txt exists; default to storing settings in emulator directory
            return ConfigDirType::EmulatorDirectory;
        }

        // Otherwise, default to storing settings in user profile directory
        ConfigDirType::UserProfile
    }

    /// Create an empty portable.txt file in the emulator directory.
    ///
    /// # Errors
    ///
    /// Propagates any I/O errors.
    pub fn create_portable_txt(&self) -> io::Result<()> {
        let Some(emulator_dir) = &self.emulator_dir else {
            return Err(emulator_dir_not_found_error());
        };

        let portable_txt_path = emulator_dir.join(PORTABLE_TXT_FILE_NAME);
        log::info!("Creating portable.txt file at '{}'", portable_txt_path.display());
        File::create(portable_txt_path)?;

        Ok(())
    }

    /// Delete the portable.txt file in the emulator directory.
    ///
    /// # Errors
    ///
    /// Propagates any I/O errors, including if the file does not exist.
    pub fn delete_portable_txt(&self) -> io::Result<()> {
        let Some(emulator_dir) = &self.emulator_dir else {
            return Err(emulator_dir_not_found_error());
        };

        let portable_txt_path = emulator_dir.join(PORTABLE_TXT_FILE_NAME);
        log::info!("Deleting portable.txt file at '{}'", portable_txt_path.display());
        fs::remove_file(portable_txt_path)
    }
}

fn emulator_dir_not_found_error() -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, "unable to determine emulator directory".to_string())
}

impl Default for ConfigDirs {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Error)]
pub enum SaveConfigError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("TOML serialization error: {0}")]
    Toml(#[from] toml::ser::Error),
}

pub struct ConfigWithPath {
    pub config: AppConfig,
    pub path: PathBuf,
}

impl ConfigWithPath {
    /// Save the current config.
    ///
    /// # Errors
    ///
    /// Propagates any errors encountered while serializing to TOML or writing to disk.
    pub fn save_config(&self) -> Result<(), SaveConfigError> {
        let config_str = toml::to_string_pretty(&self.config)?;
        fs::write(&self.path, &config_str)?;

        Ok(())
    }
}

impl ConfigWithPath {
    #[must_use]
    pub fn new(config: AppConfig, path: PathBuf) -> Self {
        Self { config, path }
    }

    #[must_use]
    pub fn load_from_dir_or_default(
        config_dirs: &ConfigDirs,
        dir_type: &ConfigDirType,
        default: impl FnOnce() -> AppConfig,
    ) -> Self {
        let config_path = match dir_type {
            ConfigDirType::Override { file_path } => file_path.clone(),
            _ => {
                let Some(directory) = config_dirs.get(dir_type) else {
                    log::error!(
                        "Unable to determine filesystem path for directory type {dir_type:?}; config will probably not save"
                    );
                    return Self::new(default(), CONFIG_FILENAME.into());
                };

                if !directory.exists()
                    && let Err(err) = fs::create_dir_all(&directory)
                {
                    log::error!(
                        "Unable to create config directory '{}', app config will probably not save: {err}",
                        directory.display()
                    );
                    return Self::new(default(), CONFIG_FILENAME.into());
                }

                directory.join(CONFIG_FILENAME)
            }
        };

        if !config_path.exists() {
            // Config file used to always be at $CWD/jgenesis-config.toml
            // If this file exists but there's no file at the config path, bootstrap config path
            // by copying from that location
            if matches!(dir_type, ConfigDirType::UserProfile) && Path::new(CONFIG_FILENAME).exists()
            {
                log::info!(
                    "Bootstrapping config at '{}' from config in current directory",
                    config_path.display()
                );

                if let Err(err) = fs::copy(Path::new(CONFIG_FILENAME), &config_path) {
                    log::error!("Error copying config to '{}': {err}", config_path.display());
                }
            } else {
                // File does not exist and we're not going to bootstrap; use default/current config
                return Self::new(default(), config_path);
            }
        }

        log::info!("Loading initial config from '{}'", config_path.display());

        let mut config_str = fs::read_to_string(&config_path).unwrap_or_default();

        crate::migrate_config_str(&mut config_str);

        let config = toml::from_str(&config_str).unwrap_or_else(|err| {
            log::error!("Error deserializing app config at '{}': {err}", config_path.display());
            default()
        });

        let migrated_config = crate::migrate_config(&config, &config_str).unwrap_or(config);

        Self::new(migrated_config, config_path)
    }
}

// Typically:
//   `$HOME/.config/jgenesis/` on Linux
//   `%userprofile\AppData\Local\jgenesis\` on Windows
fn determine_user_profile_dir() -> Option<PathBuf> {
    let base_dirs = directories::BaseDirs::new()?;
    Some(base_dirs.config_local_dir().join(JGENESIS_SUBDIR))
}

// Directory containing the emulator executable
fn determine_emulator_dir() -> Option<PathBuf> {
    let exe_path: PathBuf = if jgenesis_common::is_appimage_build() {
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
