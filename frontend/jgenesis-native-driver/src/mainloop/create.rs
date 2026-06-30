use crate::archive::{ArchiveEntry, ArchiveError};
use crate::config::CommonConfig;
use crate::mainloop::runner::{ChangeDiscFn, RemoveDiscFn};
use crate::mainloop::save::FsSaveWriter;
use crate::mainloop::{CreatedEmulator, NativeDebugFn, NativeEmulatorArgs, save};
use crate::{NativeEmulator, NativeEmulatorError, NativeEmulatorResult, archive, extensions};
use jgenesis_common::frontend::{EmulatorTrait, SaveWriter};
use jgenesis_native_config::input::mappings::ButtonMappingVec;
use std::fmt::Display;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct ReadInputResult<T> {
    pub input: T,
    pub rom_path: PathBuf,
    pub save_extension: String,
}

pub trait CreatableEmulator: EmulatorTrait + Sized {
    type NativeConfig: Display + Clone + Send + Sync + 'static;

    /// Anything the implementation wishes to pass from [`Self::read_create_input`] to [`Self::create`].
    ///
    /// Commonly [`Vec<u8>`] if the input is always a single file loaded entirely into RAM.
    type CreateInput: Clone + Send + Sync + 'static;

    /// Read inputs and determine the ROM path and extension to use for save files + save states.
    ///
    /// Most implementations can simply call [`read_rom_file`].
    fn read_create_input(
        config: &Self::NativeConfig,
    ) -> NativeEmulatorResult<ReadInputResult<Self::CreateInput>>;

    /// Create the emulator.
    fn create(
        input: ReadInputResult<Self::CreateInput>,
        config: &Self::NativeConfig,
        save_writer: &mut impl SaveWriter,
    ) -> NativeEmulatorResult<CreatedEmulator<Self>>;

    fn common_config(config: &Self::NativeConfig) -> &CommonConfig;

    fn emulator_config(config: &Self::NativeConfig) -> &Self::Config;

    /// Reload configuration, if necessary (e.g. to change controller types).
    ///
    /// Implementations do not need to explicitly reload common/emulator/input configuration as
    /// those are always reloaded before this function gets called.
    #[allow(unused_variables)]
    fn reload_native_config(
        emulator: &mut NativeEmulator<Self>,
        config: &Self::NativeConfig,
    ) -> NativeEmulatorResult<()> {
        Ok(())
    }

    #[allow(unused_variables)]
    fn initial_inputs(config: &Self::NativeConfig) -> Self::Inputs {
        Self::Inputs::default()
    }

    fn input_mappings(config: &Self::NativeConfig) -> ButtonMappingVec<'_, Self::Button>;

    #[allow(unused_variables)]
    fn turbo_input_mappings(config: &Self::NativeConfig) -> ButtonMappingVec<'_, Self::Button> {
        vec![]
    }

    fn disc_change_fns() -> Option<(ChangeDiscFn<Self>, RemoveDiscFn<Self>)> {
        None
    }

    fn debug_fn() -> Option<NativeDebugFn<Self>> {
        None
    }
}

impl<Emulator: CreatableEmulator> NativeEmulator<Emulator> {
    /// Create a new emulator instance.
    ///
    /// # Errors
    ///
    /// Propagates any errors encountered creating or initializing the emulator.
    pub fn create(config: Box<Emulator::NativeConfig>) -> NativeEmulatorResult<Self> {
        log::info!("Running with config: {config}");

        let input = Emulator::read_create_input(&config)?;

        let common_config = Emulator::common_config(&config);
        let determined_paths = save::determine_save_paths(
            &common_config.save_path,
            &common_config.state_path,
            &input.rom_path,
            &input.save_extension,
        )?;

        let save_extension = input.save_extension.clone();

        let create_emulator_fn = {
            let config = config.clone();
            move |save_writer: &mut FsSaveWriter| Emulator::create(input, &config, save_writer)
        };

        let mut args = NativeEmulatorArgs::new(
            Box::new(create_emulator_fn),
            Emulator::emulator_config(&config).clone(),
            common_config.clone(),
            save_extension,
            determined_paths.save_path,
            determined_paths.save_state_path,
            Emulator::input_mappings(&config),
        )
        .with_initial_inputs(Emulator::initial_inputs(&config))
        .with_turbo_mappings(Emulator::turbo_input_mappings(&config));

        if let Some((change_disc_fn, remove_disc_fn)) = Emulator::disc_change_fns() {
            args = args.with_disc_change_fns(change_disc_fn, remove_disc_fn);
        }

        if let Some(debug_fn) = Emulator::debug_fn() {
            args = args.with_debug_fn(debug_fn);
        }

        Self::new(args)
    }

    /// Reload configuration.
    ///
    /// # Errors
    ///
    /// Propagates any errors encountered while reloading.
    pub fn reload_config(
        &mut self,
        config: Box<Emulator::NativeConfig>,
    ) -> NativeEmulatorResult<()> {
        log::info!("Reloading config: {config}");

        let common_config = Emulator::common_config(&config);
        self.update_and_reload_config(common_config, Emulator::emulator_config(&config))?;

        self.input_mapper.update_mappings(
            common_config.axis_deadzone,
            &Emulator::input_mappings(&config),
            &Emulator::turbo_input_mappings(&config),
            &common_config.hotkey_config.to_mapping_vec(),
        );

        Emulator::reload_native_config(self, &config)
    }
}

pub type ReadRomResult = ReadInputResult<Vec<u8>>;

pub(crate) fn read_rom_file(
    path: &Path,
    supported_extensions: &[&str],
) -> NativeEmulatorResult<ReadRomResult> {
    struct NameWithExtension {
        file_name: String,
        extension: String,
    }

    #[derive(Default)]
    struct ArchiveListCallback {
        first_supported_file: Option<NameWithExtension>,
    }

    impl ArchiveListCallback {
        fn as_fn_mut<'ext>(
            &mut self,
            supported_extensions: &'ext [&str],
        ) -> impl FnMut(ArchiveEntry<'_>) + use<'_, 'ext> {
            |entry| {
                if self.first_supported_file.is_some() {
                    return;
                }

                let Some(extension) = extensions::from_path(entry.file_name) else { return };
                if supported_extensions.contains(&extension.as_str()) {
                    self.first_supported_file =
                        Some(NameWithExtension { file_name: entry.file_name.into(), extension });
                }
            }
        }

        fn open_file(
            self,
            archive_path: &Path,
            read_fn: fn(&Path, &str) -> Result<Vec<u8>, ArchiveError>,
        ) -> NativeEmulatorResult<(Vec<u8>, String)> {
            let first_supported_file = self.first_supported_file.ok_or_else(|| {
                NativeEmulatorError::Archive(ArchiveError::NoSupportedFiles {
                    path: archive_path.display().to_string(),
                })
            })?;

            let contents = read_fn(archive_path, &first_supported_file.file_name)
                .map_err(NativeEmulatorError::Archive)?;
            Ok((contents, first_supported_file.extension))
        }
    }

    let extension = extensions::from_path(path).unwrap_or_default();
    let (contents, extension) = match extension.as_str() {
        "zip" => {
            let mut callback = ArchiveListCallback::default();
            archive::list_files_zip(path, callback.as_fn_mut(supported_extensions))
                .map_err(NativeEmulatorError::Archive)?;
            callback.open_file(path, archive::read_file_zip)
        }
        "7z" => {
            let mut callback = ArchiveListCallback::default();
            archive::list_files_7z(path, callback.as_fn_mut(supported_extensions))
                .map_err(NativeEmulatorError::Archive)?;
            callback.open_file(path, archive::read_file_7z)
        }
        _ => {
            let contents = fs::read(path).map_err(|source| NativeEmulatorError::RomRead {
                path: path.display().to_string(),
                source,
            })?;

            Ok((contents, extension))
        }
    }?;

    Ok(ReadRomResult { input: contents, rom_path: path.into(), save_extension: extension })
}
