use crate::archive::{ArchiveEntry, ArchiveError};
use crate::config::CommonConfig;
use crate::input::Joysticks;
use crate::mainloop::runner::RunnerCommand;
use crate::mainloop::save::FsSaveWriter;
use crate::mainloop::{CreatedEmulator, MODAL_DURATION, NativeDebugFn, NativeEmulatorArgs, save};
use crate::{NativeEmulator, NativeEmulatorError, NativeEmulatorResult, archive, extensions};
use cdrom::reader::{CdRom, CdRomFileFormat};
use jgenesis_common::frontend::{EmulatorTrait, SaveWriter};
use jgenesis_common::patch::{BpsError, IpsError};
use jgenesis_native_config::common::ApplyPatches;
use jgenesis_native_config::input::mappings::ButtonMappingVec;
use sdl3::mouse::MouseUtil;
use sdl3::{AudioSubsystem, EventPump, VideoSubsystem};
use std::cell::RefCell;
use std::fmt::Display;
use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[derive(Clone)]
pub struct SdlSubsystems {
    pub video: Rc<RefCell<VideoSubsystem>>,
    pub audio: Rc<RefCell<AudioSubsystem>>,
    pub joysticks: Rc<RefCell<Joysticks>>,
    pub event_pump: Rc<RefCell<EventPump>>,
    pub mouse_util: Rc<RefCell<MouseUtil>>,
}

impl SdlSubsystems {
    /// Initialize SDL3 and all required subsystems.
    ///
    /// # Errors
    ///
    /// Propagates any SDL initialization errors, e.g. if SDL3 is already initialized.
    pub fn init() -> NativeEmulatorResult<Self> {
        let sdl_ctx = sdl3::init().map_err(NativeEmulatorError::SdlInit)?;
        let video = sdl_ctx.video().map_err(NativeEmulatorError::SdlVideoInit)?;
        let audio = sdl_ctx.audio().map_err(NativeEmulatorError::SdlAudioInit)?;
        let joystick = sdl_ctx.joystick().map_err(NativeEmulatorError::SdlJoystickInit)?;
        let event_pump = sdl_ctx.event_pump().map_err(NativeEmulatorError::SdlEventPumpInit)?;
        let mouse_util = sdl_ctx.mouse();

        // Allow gamepad inputs while window does not have focus
        // https://wiki.libsdl.org/SDL3/SDL_HINT_JOYSTICK_ALLOW_BACKGROUND_EVENTS
        sdl3::hint::set("SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS", "1");

        // Register mouse down events for mouse clicks that change window focus
        // https://wiki.libsdl.org/SDL3/SDL_HINT_MOUSE_FOCUS_CLICKTHROUGH
        sdl3::hint::set("SDL_MOUSE_FOCUS_CLICKTHROUGH", "1");

        let joysticks = Joysticks::new(joystick);

        Ok(Self {
            video: Rc::new(RefCell::new(video)),
            audio: Rc::new(RefCell::new(audio)),
            joysticks: Rc::new(RefCell::new(joysticks)),
            event_pump: Rc::new(RefCell::new(event_pump)),
            mouse_util: Rc::new(RefCell::new(mouse_util)),
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub enum RomPatchResult {
    None,
    IpsSuccess,
    IpsError(IpsError),
    BpsSuccess,
    BpsError(BpsError),
}

#[derive(Clone)]
pub struct ReadInputResult<T> {
    pub input: T,
    pub rom_path: PathBuf,
    pub save_extension: String,
    pub patch_result: RomPatchResult,
}

#[derive(Debug, Clone)]
pub struct WindowTitle(pub String);

impl Deref for WindowTitle {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
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

    #[allow(unused_variables)]
    fn change_disc(
        &mut self,
        disc_path: &Path,
        config: &<Self as EmulatorTrait>::Config,
    ) -> NativeEmulatorResult<Option<WindowTitle>> {
        log::warn!("This backend does not support disc change");
        Ok(None)
    }

    fn remove_disc(&mut self) -> Option<WindowTitle> {
        log::warn!("This backend does not support disc change");
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
    #[inline(never)] // Bloats compile times and binary size
    pub fn create(
        sdl: SdlSubsystems,
        config: Box<Emulator::NativeConfig>,
    ) -> NativeEmulatorResult<Self> {
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
        let patch_result = input.patch_result;

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
        .with_turbo_mappings(Emulator::turbo_input_mappings(&config))
        .with_disc_change_fns(Emulator::change_disc, Emulator::remove_disc);

        if let Some(debug_fn) = Emulator::debug_fn() {
            args = args.with_debug_fn(debug_fn);
        }

        let mut emulator = Self::new(sdl, args)?;

        match patch_result {
            RomPatchResult::None => {}
            RomPatchResult::IpsSuccess => {
                log::info!("Successfully applied IPS patch");
                emulator.renderer.add_modal("Applied IPS patch".into(), MODAL_DURATION);
            }
            RomPatchResult::IpsError(err) => {
                log::error!("Failed to apply IPS patch: {err}");
                emulator.renderer.add_modal("Failed to apply IPS patch".into(), MODAL_DURATION);
            }
            RomPatchResult::BpsSuccess => {
                log::info!("Successfully applied BPS patch");
                emulator.renderer.add_modal("Applied BPS patch".into(), MODAL_DURATION);
            }
            RomPatchResult::BpsError(err) => {
                log::error!("Failed to apply BPS patch: {err}");
                emulator.renderer.add_modal("Failed to apply BPS patch".into(), MODAL_DURATION);
            }
        }

        Ok(emulator)
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

    /// Remove a disc from the emulated system's drive.
    ///
    /// # Errors
    ///
    /// Returns an error if the runner thread has disconnected.
    pub fn remove_disc(&mut self) -> NativeEmulatorResult<()> {
        self.runner.send_command(RunnerCommand::RemoveDisc)
    }

    /// Change the disc in the emulated system's drive.
    ///
    /// # Errors
    ///
    /// Returns an error if the runner thread has disconnected.
    pub fn change_disc(&mut self, disc_path: PathBuf) -> NativeEmulatorResult<()> {
        self.runner.send_command(RunnerCommand::ChangeDisc(disc_path))
    }
}

pub type ReadRomResult = ReadInputResult<Vec<u8>>;

type ArchiveReadFn = fn(&Path, &str, usize) -> Result<Vec<u8>, ArchiveError>;

pub(crate) fn read_rom_file(
    path: &Path,
    supported_extensions: &[&str],
    apply_patches: ApplyPatches,
) -> NativeEmulatorResult<ReadRomResult> {
    struct NameWithExtension {
        file_name: String,
        extension: String,
    }

    #[derive(Default)]
    struct ArchiveListCallback {
        first_supported_file: Option<NameWithExtension>,
        potential_patch_files: Vec<String>,
    }

    impl ArchiveListCallback {
        fn as_fn_mut<'ext>(
            &mut self,
            supported_extensions: &'ext [&str],
        ) -> impl FnMut(ArchiveEntry<'_>) + use<'_, 'ext> {
            |entry| {
                let Some(extension) = extensions::from_path(entry.file_name) else { return };

                if matches!(extension.as_str(), "ips" | "bps") {
                    self.potential_patch_files.push(entry.file_name.into());
                    return;
                }

                if self.first_supported_file.is_some() {
                    return;
                }

                if supported_extensions.contains(&extension.as_str()) {
                    self.first_supported_file =
                        Some(NameWithExtension { file_name: entry.file_name.into(), extension });
                }
            }
        }

        fn open_file(
            self,
            archive_path: &Path,
            apply_patches: ApplyPatches,
            read_fn: ArchiveReadFn,
        ) -> NativeEmulatorResult<(Vec<u8>, String, RomPatchResult)> {
            let first_supported_file = self.first_supported_file.ok_or_else(|| {
                NativeEmulatorError::Archive(ArchiveError::NoSupportedFiles {
                    path: archive_path.display().to_string(),
                })
            })?;

            let mut contents = read_fn(archive_path, &first_supported_file.file_name, usize::MAX)
                .map_err(NativeEmulatorError::Archive)?;

            let rom_patch_result = match apply_patches {
                ApplyPatches::Yes => apply_patch_from_archive(
                    &mut contents,
                    archive_path,
                    &first_supported_file.file_name,
                    &self.potential_patch_files,
                    read_fn,
                ),
                ApplyPatches::No => RomPatchResult::None,
            };

            Ok((contents, first_supported_file.extension, rom_patch_result))
        }
    }

    let extension = extensions::from_path(path).unwrap_or_default();
    let (contents, extension, patch_result) = match extension.as_str() {
        "zip" => {
            let mut callback = ArchiveListCallback::default();
            archive::list_files_zip(path, callback.as_fn_mut(supported_extensions))
                .map_err(NativeEmulatorError::Archive)?;
            callback.open_file(path, apply_patches, archive::read_file_zip)
        }
        "7z" => {
            let mut callback = ArchiveListCallback::default();
            archive::list_files_7z(path, callback.as_fn_mut(supported_extensions))
                .map_err(NativeEmulatorError::Archive)?;
            callback.open_file(path, apply_patches, archive::read_file_7z)
        }
        _ => {
            let mut contents = fs::read(path).map_err(|source| NativeEmulatorError::RomRead {
                path: path.display().to_string(),
                source,
            })?;

            let patch_result = match apply_patches {
                ApplyPatches::Yes => apply_patch_from_fs(&mut contents, path),
                ApplyPatches::No => RomPatchResult::None,
            };

            Ok((contents, extension, patch_result))
        }
    }?;

    Ok(ReadRomResult {
        input: contents,
        rom_path: path.into(),
        save_extension: extension,
        patch_result,
    })
}

pub(crate) fn read_cdrom_image(path: &Path, open_in_memory: bool) -> NativeEmulatorResult<CdRom> {
    let disc_format = CdRomFileFormat::from_file_path(path).unwrap_or_else(|| {
        log::warn!("Unable to determine CD-ROM image format; assuming CUE/BIN");
        CdRomFileFormat::CueBin
    });

    let disc = if open_in_memory {
        CdRom::open_in_memory(path, disc_format)
    } else {
        CdRom::open(path, disc_format)
    };

    disc.map_err(NativeEmulatorError::CdRomRead)
}

fn apply_patch_from_fs(rom: &mut Vec<u8>, rom_path: &Path) -> RomPatchResult {
    let bps_path = rom_path.with_extension("bps");
    if let Ok(bps) = fs::read(bps_path) {
        return apply_bps_patch(rom, &bps);
    }

    let ips_path = rom_path.with_extension("ips");
    if let Ok(ips) = fs::read(ips_path) {
        return apply_ips_patch(rom, &ips);
    }

    RomPatchResult::None
}

fn apply_bps_patch(rom: &mut Vec<u8>, bps: &[u8]) -> RomPatchResult {
    match jgenesis_common::patch::apply_bps(rom, bps) {
        Ok(patched) => {
            *rom = patched;
            RomPatchResult::BpsSuccess
        }
        Err(err) => RomPatchResult::BpsError(err),
    }
}

fn apply_ips_patch(rom: &mut Vec<u8>, ips: &[u8]) -> RomPatchResult {
    match jgenesis_common::patch::apply_ips(rom, ips) {
        Ok(patched) => {
            *rom = patched;
            RomPatchResult::IpsSuccess
        }
        Err(err) => RomPatchResult::IpsError(err),
    }
}

fn apply_patch_from_archive(
    rom: &mut Vec<u8>,
    archive_path: &Path,
    file_name: &str,
    potential_patch_files: &[String],
    read_fn: ArchiveReadFn,
) -> RomPatchResult {
    let bps_name = Path::new(file_name).with_extension("bps");
    let bps_name = bps_name.to_string_lossy();
    if potential_patch_files.iter().any(|patch_file| patch_file.as_str() == bps_name.as_ref())
        && let Ok(bps) = read_fn(archive_path, &bps_name, usize::MAX)
    {
        return apply_bps_patch(rom, &bps);
    }

    let ips_name = Path::new(file_name).with_extension("ips");
    let ips_name = ips_name.to_string_lossy();
    if potential_patch_files.iter().any(|patch_file| patch_file.as_str() == ips_name.as_ref())
        && let Ok(ips) = read_fn(archive_path, &ips_name, usize::MAX)
    {
        return apply_ips_patch(rom, &ips);
    }

    // Fall back to looking for patches outside of the archive with the same name as the archive
    apply_patch_from_fs(rom, archive_path)
}
