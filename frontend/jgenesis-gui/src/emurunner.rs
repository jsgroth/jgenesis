use crate::app::ActiveCheats;
use genesis_config::cheats::GenesisCheats;
use jgenesis_native_config::AppConfig;
use jgenesis_native_driver::config::AppConfigExt;
use jgenesis_native_driver::extensions::Console;
use jgenesis_native_driver::{
    Native32XEmulator, NativeEmulatorError, NativeEmulatorResult, NativeGameBoyEmulator,
    NativeGbaEmulator, NativeGenesisEmulator, NativeNesEmulator, NativeSegaCdEmulator,
    NativeSmsGgEmulator, NativeSnesEmulator, NativeTickEffect, SaveStateMetadata,
};
use jgenesis_native_driver::{NativePcEngineEmulator, SdlSubsystems};
use jgenesis_proc_macros::MatchEachVariantMacro;
use smsgg_config::cheats::SmsGgCheats;
use smsgg_core::SmsGgHardware;
use std::cell::{Cell, Ref, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(jgenesis_proc_macros::EnumAll))]
pub enum EmuRunnerStatus {
    Idle,
    WaitingForFirstCommand,
    RunningSms,
    RunningGameGear,
    RunningGenesis,
    RunningSegaCd,
    Running32X,
    RunningNes,
    RunningSnes,
    RunningGameBoy,
    RunningGba,
    RunningPcEngine,
}

impl EmuRunnerStatus {
    pub fn is_running(self) -> bool {
        matches!(
            self,
            Self::RunningSms
                | Self::RunningGenesis
                | Self::RunningSegaCd
                | Self::Running32X
                | Self::RunningNes
                | Self::RunningSnes
                | Self::RunningGameBoy
                | Self::RunningGba
                | Self::RunningGameGear
                | Self::RunningPcEngine
        )
    }

    pub fn is_running_smsgg(self) -> bool {
        matches!(self, Self::RunningSms | Self::RunningGameGear)
    }

    pub fn is_running_handheld(self) -> bool {
        matches!(self, Self::RunningGameGear | Self::RunningGameBoy | Self::RunningGba)
    }

    pub fn running_console(self) -> Option<Console> {
        match self {
            Self::RunningSms => Some(Console::MasterSystem),
            Self::RunningGameGear => Some(Console::GameGear),
            Self::RunningGenesis => Some(Console::Genesis),
            Self::RunningSegaCd => Some(Console::SegaCd),
            Self::Running32X => Some(Console::Sega32X),
            Self::RunningNes => Some(Console::Nes),
            Self::RunningSnes => Some(Console::Snes),
            Self::RunningGameBoy => Some(Console::GameBoy),
            Self::RunningGba => Some(Console::GameBoyAdvance),
            Self::RunningPcEngine => Some(Console::PcEngine),
            EmuRunnerStatus::Idle | EmuRunnerStatus::WaitingForFirstCommand => None,
        }
    }
}

trait ConsoleExt {
    fn running_status(self) -> EmuRunnerStatus;
}

impl ConsoleExt for Console {
    fn running_status(self) -> EmuRunnerStatus {
        match self {
            Self::MasterSystem | Self::Sg1000 => EmuRunnerStatus::RunningSms,
            Self::GameGear => EmuRunnerStatus::RunningGameGear,
            Self::Genesis => EmuRunnerStatus::RunningGenesis,
            Self::SegaCd => EmuRunnerStatus::RunningSegaCd,
            Self::Sega32X => EmuRunnerStatus::Running32X,
            Self::Nes => EmuRunnerStatus::RunningNes,
            Self::Snes => EmuRunnerStatus::RunningSnes,
            Self::GameBoy | Self::GameBoyColor => EmuRunnerStatus::RunningGameBoy,
            Self::GameBoyAdvance => EmuRunnerStatus::RunningGba,
            Self::PcEngine => EmuRunnerStatus::RunningPcEngine,
        }
    }
}

#[derive(Debug, Clone)]
pub enum EmulatorRunInput {
    OpenFile(PathBuf),
    RunBios,
}

#[derive(Debug, Clone)]
pub enum EmuRunnerCommand {
    Run {
        console: Console,
        config: Box<AppConfig>,
        cheats: Arc<ActiveCheats>,
        input: EmulatorRunInput,
    },
    ReloadConfig(Box<AppConfig>, Arc<ActiveCheats>, PathBuf),
    StopEmulator,
    SoftReset,
    HardReset,
    OpenMemoryViewer,
    SaveState {
        slot: usize,
    },
    LoadState {
        slot: usize,
    },
    SegaCdRemoveDisc,
    SegaCdChangeDisc(PathBuf),
}

pub struct GuiEmulatorRunner {
    emulator: Option<GenericEmulator>,
    emulator_error: Rc<RefCell<Option<NativeEmulatorError>>>,
    sdl: SdlSubsystems,
    egui_ctx: egui::Context,
    status: Rc<Cell<EmuRunnerStatus>>,
    exit_signal: Rc<Cell<bool>>,
    gui_focused: Rc<Cell<bool>>,
    save_state_metadata: Rc<RefCell<SaveStateMetadata>>,
    queued_commands: Rc<RefCell<Vec<EmuRunnerCommand>>>,
}

pub struct GuiEmulatorRunnerHandle {
    emulator_error: Rc<RefCell<Option<NativeEmulatorError>>>,
    status: Rc<Cell<EmuRunnerStatus>>,
    exit_signal: Rc<Cell<bool>>,
    gui_focused: Rc<Cell<bool>>,
    save_state_metadata: Rc<RefCell<SaveStateMetadata>>,
    queued_commands: Rc<RefCell<Vec<EmuRunnerCommand>>>,
}

impl GuiEmulatorRunner {
    #[must_use]
    pub fn new(sdl: SdlSubsystems, egui_ctx: egui::Context) -> (Self, GuiEmulatorRunnerHandle) {
        let runner = Self {
            emulator: None,
            emulator_error: Rc::new(RefCell::new(None)),
            sdl,
            egui_ctx,
            status: Rc::new(Cell::new(EmuRunnerStatus::WaitingForFirstCommand)),
            exit_signal: Rc::new(Cell::new(false)),
            gui_focused: Rc::new(Cell::new(true)),
            save_state_metadata: Rc::new(RefCell::new(SaveStateMetadata::default())),
            queued_commands: Rc::new(RefCell::new(Vec::new())),
        };

        let handle = GuiEmulatorRunnerHandle {
            emulator_error: Rc::clone(&runner.emulator_error),
            status: Rc::clone(&runner.status),
            exit_signal: Rc::clone(&runner.exit_signal),
            gui_focused: Rc::clone(&runner.gui_focused),
            save_state_metadata: Rc::clone(&runner.save_state_metadata),
            queued_commands: Rc::clone(&runner.queued_commands),
        };

        (runner, handle)
    }

    pub fn run(&mut self, sdl_event_handler: impl FnMut(&sdl3::event::Event)) {
        self.process_commands();

        let Some(emulator) = &mut self.emulator else {
            self.drain_sdl_events(sdl_event_handler);
            return;
        };

        emulator.update_gui_focused(self.gui_focused.get());
        *self.save_state_metadata.borrow_mut() = emulator.save_state_metadata();

        match emulator.run(sdl_event_handler) {
            Ok(None) => {}
            Ok(Some(NativeTickEffect::PowerOff)) => {
                self.stop_emulator();
            }
            Ok(Some(NativeTickEffect::Exit)) => {
                self.stop_emulator();
                self.exit_signal.set(true);
            }
            Err(err) => {
                log::error!("Emulator terminated with an error: {err}");
                self.stop_emulator();
                *self.emulator_error.borrow_mut() = Some(err);
            }
        }
    }

    fn process_commands(&mut self) {
        for command in Rc::clone(&self.queued_commands).borrow_mut().drain(..) {
            match command {
                EmuRunnerCommand::Run { console, config, cheats, input } => {
                    self.launch_emulator(console, config, cheats, input);
                }
                EmuRunnerCommand::ReloadConfig(config, cheats, path) => {
                    self.do_with_emulator(|emulator| emulator.reload_config(config, cheats, path));
                }
                EmuRunnerCommand::StopEmulator => {
                    self.stop_emulator();
                }
                EmuRunnerCommand::SoftReset => {
                    self.do_with_emulator(GenericEmulator::soft_reset);
                }
                EmuRunnerCommand::HardReset => {
                    self.do_with_emulator(GenericEmulator::hard_reset);
                }
                EmuRunnerCommand::OpenMemoryViewer => {
                    self.do_with_emulator(GenericEmulator::open_memory_viewer);
                }
                EmuRunnerCommand::SaveState { slot } => {
                    self.do_with_emulator(|emulator| emulator.save_state(slot));
                }
                EmuRunnerCommand::LoadState { slot } => {
                    self.do_with_emulator(|emulator| emulator.load_state(slot));
                }
                EmuRunnerCommand::SegaCdRemoveDisc => {
                    self.do_with_emulator(GenericEmulator::remove_disc);
                }
                EmuRunnerCommand::SegaCdChangeDisc(path) => {
                    self.do_with_emulator(|emulator| emulator.change_disc(path));
                }
            }
        }
    }

    fn drain_sdl_events(&mut self, mut sdl_event_handler: impl FnMut(&sdl3::event::Event)) {
        assert!(
            self.emulator.is_none(),
            "drain_sdl_events() should never be called while an emulator is running"
        );

        let mut event_pump = self.sdl.event_pump.borrow_mut();

        for event in event_pump.poll_iter() {
            sdl_event_handler(&event);

            if let Err(err) = self.sdl.joysticks.borrow_mut().handle_sdl_event(&event) {
                log::error!("Error handling joystick event: {err}");
            }
        }
    }

    fn launch_emulator(
        &mut self,
        console: Console,
        config: Box<AppConfig>,
        cheats: Arc<ActiveCheats>,
        input: EmulatorRunInput,
    ) {
        if self.emulator.is_some() {
            self.stop_emulator();
        }

        let emulator = match input {
            EmulatorRunInput::OpenFile(file_path) => {
                GenericEmulator::create(self.sdl.clone(), console, config, cheats, file_path)
                    .map(Some)
            }
            EmulatorRunInput::RunBios => {
                GenericEmulator::create_run_bios(self.sdl.clone(), console, config)
            }
        };

        let emulator = match emulator {
            Ok(Some(emulator)) => emulator,
            Ok(None) => return,
            Err(err) => {
                log::error!("Error initializing emulator: {err}");
                *self.emulator_error.borrow_mut() = Some(err);
                self.egui_ctx.request_repaint();
                return;
            }
        };

        self.emulator = Some(emulator);
        self.status.set(console.running_status());
        self.egui_ctx.request_repaint();
    }

    fn stop_emulator(&mut self) {
        self.emulator = None;
        self.status.set(EmuRunnerStatus::Idle);

        // Force a repaint after an emulator exits. This will immediately display the error window
        // if there was an error, and it will also force quit immediately if auto-close is enabled
        self.egui_ctx.request_repaint();
    }

    fn do_with_emulator(
        &mut self,
        action: impl FnOnce(&mut GenericEmulator) -> NativeEmulatorResult<()>,
    ) {
        let Some(emulator) = &mut self.emulator else { return };

        if let Err(err) = action(emulator) {
            self.stop_emulator();
            *self.emulator_error.borrow_mut() = Some(err);
        }
    }
}

impl GuiEmulatorRunnerHandle {
    #[must_use]
    pub fn status(&self) -> EmuRunnerStatus {
        self.status.get()
    }

    #[must_use]
    pub fn emulator_error(&self) -> Rc<RefCell<Option<NativeEmulatorError>>> {
        Rc::clone(&self.emulator_error)
    }

    pub fn update_gui_focused(&self, gui_focused: bool) {
        self.gui_focused.set(gui_focused);
    }

    #[must_use]
    pub fn save_state_metadata(&self) -> Ref<'_, SaveStateMetadata> {
        self.save_state_metadata.borrow()
    }

    #[must_use]
    pub fn exit_signal(&self) -> bool {
        self.exit_signal.get()
    }

    pub fn push_command(&self, command: EmuRunnerCommand) {
        self.queued_commands.borrow_mut().push(command);
    }

    pub fn clear_waiting_for_first_command(&mut self) {
        if self.status.get() == EmuRunnerStatus::WaitingForFirstCommand {
            self.status.set(EmuRunnerStatus::Idle);
        }
    }
}

#[derive(MatchEachVariantMacro)]
enum GenericEmulator {
    SmsGg(Box<NativeSmsGgEmulator>),
    Genesis(Box<NativeGenesisEmulator>),
    SegaCd(Box<NativeSegaCdEmulator>),
    Sega32X(Box<Native32XEmulator>),
    Nes(Box<NativeNesEmulator>),
    Snes(Box<NativeSnesEmulator>),
    GameBoy(Box<NativeGameBoyEmulator>),
    GameBoyAdvance(Box<NativeGbaEmulator>),
    PcEngine(Box<NativePcEngineEmulator>),
}

impl GenericEmulator {
    fn create(
        sdl: SdlSubsystems,
        console: Console,
        config: Box<AppConfig>,
        cheats: Arc<ActiveCheats>,
        path: PathBuf,
    ) -> NativeEmulatorResult<Self> {
        let emulator = match console {
            Console::MasterSystem => Self::SmsGg(Box::new(NativeSmsGgEmulator::create(
                sdl,
                config.smsgg_config(
                    path,
                    Some(SmsGgHardware::MasterSystem),
                    cheats.smsgg_or_default(),
                ),
            )?)),
            Console::GameGear => Self::SmsGg(Box::new(NativeSmsGgEmulator::create(
                sdl,
                config.smsgg_config(path, Some(SmsGgHardware::GameGear), cheats.smsgg_or_default()),
            )?)),
            Console::Sg1000 => Self::SmsGg(Box::new(NativeSmsGgEmulator::create(
                sdl,
                config.smsgg_config(path, Some(SmsGgHardware::Sg1000), cheats.smsgg_or_default()),
            )?)),
            Console::Genesis => Self::Genesis(Box::new(NativeGenesisEmulator::create(
                sdl,
                config.genesis_config(path, cheats.genesis_or_default()),
            )?)),
            Console::SegaCd => Self::SegaCd(Box::new(NativeSegaCdEmulator::create(
                sdl,
                config.sega_cd_config(path, cheats.genesis_or_default()),
            )?)),
            Console::Sega32X => Self::Sega32X(Box::new(Native32XEmulator::create(
                sdl,
                config.sega_32x_config(path, cheats.genesis_or_default()),
            )?)),
            Console::Nes => {
                Self::Nes(Box::new(NativeNesEmulator::create(sdl, config.nes_config(path))?))
            }
            Console::Snes => {
                Self::Snes(Box::new(NativeSnesEmulator::create(sdl, config.snes_config(path))?))
            }
            Console::GameBoy | Console::GameBoyColor => {
                Self::GameBoy(Box::new(NativeGameBoyEmulator::create(sdl, config.gb_config(path))?))
            }
            Console::GameBoyAdvance => Self::GameBoyAdvance(Box::new(NativeGbaEmulator::create(
                sdl,
                config.gba_config(path),
            )?)),
            Console::PcEngine => Self::PcEngine(Box::new(NativePcEngineEmulator::create(
                sdl,
                config.pce_config(path),
            )?)),
        };

        Ok(emulator)
    }

    fn create_run_bios(
        sdl: SdlSubsystems,
        console: Console,
        config: Box<AppConfig>,
    ) -> NativeEmulatorResult<Option<Self>> {
        let emulator = match console {
            Console::MasterSystem => {
                let mut sms_config = config.smsgg_config(
                    PathBuf::new(),
                    Some(SmsGgHardware::MasterSystem),
                    &SmsGgCheats::default(),
                );
                sms_config.sms_boot_from_bios = true;
                sms_config.run_without_cartridge = true;

                Self::SmsGg(Box::new(NativeSmsGgEmulator::create(sdl, sms_config)?))
            }
            Console::GameGear => {
                let mut gg_config = config.smsgg_config(
                    PathBuf::new(),
                    Some(SmsGgHardware::GameGear),
                    &SmsGgCheats::default(),
                );
                gg_config.gg_boot_from_bios = true;
                gg_config.run_without_cartridge = true;

                Self::SmsGg(Box::new(NativeSmsGgEmulator::create(sdl, gg_config)?))
            }
            Console::SegaCd => {
                let mut scd_config =
                    config.sega_cd_config(PathBuf::new(), &GenesisCheats::default());
                scd_config.run_without_disc = true;

                Self::SegaCd(Box::new(NativeSegaCdEmulator::create(sdl, scd_config)?))
            }
            _ => return Ok(None),
        };

        Ok(Some(emulator))
    }

    fn reload_config(
        &mut self,
        config: Box<AppConfig>,
        cheats: Arc<ActiveCheats>,
        path: PathBuf,
    ) -> NativeEmulatorResult<()> {
        match self {
            Self::SmsGg(emulator) => {
                emulator.reload_config(config.smsgg_config(path, None, cheats.smsgg_or_default()))
            }
            Self::Genesis(emulator) => {
                emulator.reload_config(config.genesis_config(path, cheats.genesis_or_default()))
            }
            Self::SegaCd(emulator) => {
                emulator.reload_config(config.sega_cd_config(path, cheats.genesis_or_default()))
            }
            Self::Sega32X(emulator) => {
                emulator.reload_config(config.sega_32x_config(path, cheats.genesis_or_default()))
            }
            Self::Nes(emulator) => emulator.reload_config(config.nes_config(path)),
            Self::Snes(emulator) => emulator.reload_config(config.snes_config(path)),
            Self::GameBoy(emulator) => emulator.reload_config(config.gb_config(path)),
            Self::GameBoyAdvance(emulator) => emulator.reload_config(config.gba_config(path)),

            Self::PcEngine(emulator) => emulator.reload_config(config.pce_config(path)),
        }
    }

    fn remove_disc(&mut self) -> NativeEmulatorResult<()> {
        if let Self::SegaCd(emulator) = self {
            emulator.remove_disc()?;
        }

        Ok(())
    }

    fn change_disc(&mut self, path: PathBuf) -> NativeEmulatorResult<()> {
        if let Self::SegaCd(emulator) = self {
            emulator.change_disc(path)?;
        }

        Ok(())
    }

    fn run(
        &mut self,
        sdl_event_handler: impl FnMut(&sdl3::event::Event),
    ) -> NativeEmulatorResult<Option<NativeTickEffect>> {
        match_each_variant!(self, emulator => emulator.run(sdl_event_handler))
    }

    fn soft_reset(&mut self) -> NativeEmulatorResult<()> {
        match_each_variant!(self, emulator => emulator.soft_reset())
    }

    fn hard_reset(&mut self) -> NativeEmulatorResult<()> {
        match_each_variant!(self, emulator => emulator.hard_reset())
    }

    fn open_memory_viewer(&mut self) -> NativeEmulatorResult<()> {
        match_each_variant!(self, emulator => emulator.open_memory_viewer())
    }

    fn save_state(&mut self, slot: usize) -> NativeEmulatorResult<()> {
        match_each_variant!(self, emulator => emulator.save_state(slot))
    }

    fn load_state(&mut self, slot: usize) -> NativeEmulatorResult<()> {
        match_each_variant!(self, emulator => emulator.load_state(slot))
    }

    fn save_state_metadata(&self) -> SaveStateMetadata {
        match_each_variant!(self, emulator => emulator.save_state_metadata())
    }

    fn update_gui_focused(&mut self, gui_focused: bool) {
        match_each_variant!(self, emulator => emulator.update_gui_focused(gui_focused));
    }
}
