use crate::cdrom::cue::CueParser;
use crate::cdrom::reader::CdRom;
use crate::graphics::GraphicsCoprocessor;
use crate::memory::{SegaCd, SubBus};
use crate::rf5c164::Rf5c164;
use anyhow::anyhow;
use genesis_core::input::InputState;
use genesis_core::memory::{MainBus, MainBusSignals, Memory};
use genesis_core::vdp::{Vdp, VdpTickEffect};
use genesis_core::ym2612::{Ym2612, YmTickEffect};
use jgenesis_traits::frontend::TimingMode;
use m68000_emu::M68000;
use smsgg_core::psg::{Psg, PsgTickEffect, PsgVersion};
use std::fs;
use std::path::Path;
use z80_emu::Z80;

const MAIN_CPU_DIVIDER: u64 = 7;
const SUB_CPU_DIVIDER: u64 = 4;
const Z80_DIVIDER: u64 = 15;

const NTSC_GENESIS_MASTER_CLOCK_RATE: u64 = 53_693_175;
const PAL_GENESIS_MASTER_CLOCK_RATE: u64 = 53_203_424;
const SEGA_CD_MASTER_CLOCK_RATE: u64 = 50_000_000;

#[derive(Debug)]
pub struct SegaCdEmulator {
    memory: Memory<SegaCd>,
    main_cpu: M68000,
    sub_cpu: M68000,
    z80: Z80,
    vdp: Vdp,
    graphics_coprocessor: GraphicsCoprocessor,
    ym2612: Ym2612,
    psg: Psg,
    pcm: Rf5c164,
    input: InputState,
    timing_mode: TimingMode,
    genesis_mclk_cycles: u64,
    sega_cd_mclk_cycles: u64,
    sega_cd_mclk_cycles_float: f64,
    sub_cpu_wait_cycles: u64,
}

impl SegaCdEmulator {
    /// # Errors
    ///
    /// Returns an error in any of the following conditions:
    /// * Unable to read the given CUE file
    /// * Unable to read every BIN file that is referenced in the CUE
    /// * Unable to read boot information from the beginning of the data track
    pub fn create<P: AsRef<Path>>(bios: Vec<u8>, cue_path: P) -> anyhow::Result<Self> {
        let cue_path = cue_path.as_ref();
        let cue_parent_dir = cue_path.parent().ok_or_else(|| {
            anyhow!("Unable to determine parent directory of CUE file '{}'", cue_path.display())
        })?;

        let cue_contents = fs::read_to_string(cue_path)?;

        let track_list = CueParser::new().parse(&cue_contents)?;
        let disc = CdRom::open(track_list, cue_parent_dir)?;

        // TODO read header information from disc

        // TODO
        let timing_mode = TimingMode::Ntsc;

        let mut memory = Memory::new(SegaCd::new(bios, disc));
        let mut main_cpu = M68000::default();
        let sub_cpu = M68000::default();
        let z80 = Z80::new();
        let mut vdp = Vdp::new(timing_mode, true);
        let graphics_coprocessor = GraphicsCoprocessor;
        let mut ym2612 = Ym2612::new();
        let mut psg = Psg::new(PsgVersion::Standard);
        let pcm = Rf5c164;
        let mut input = InputState::new();

        // Reset main CPU
        main_cpu.execute_instruction(&mut MainBus::new(
            &mut memory,
            &mut vdp,
            &mut psg,
            &mut ym2612,
            &mut input,
            timing_mode,
            MainBusSignals { z80_busack: false, m68k_reset: true },
        ));

        Ok(Self {
            memory,
            main_cpu,
            sub_cpu,
            z80,
            vdp,
            graphics_coprocessor,
            ym2612,
            psg,
            pcm,
            input,
            timing_mode,
            genesis_mclk_cycles: 0,
            sega_cd_mclk_cycles: 0,
            sega_cd_mclk_cycles_float: 0.0,
            sub_cpu_wait_cycles: 0,
        })
    }

    pub fn tick(&mut self) {
        let mut main_bus = MainBus::new(
            &mut self.memory,
            &mut self.vdp,
            &mut self.psg,
            &mut self.ym2612,
            &mut self.input,
            self.timing_mode,
            MainBusSignals { z80_busack: self.z80.stalled(), m68k_reset: false },
        );
        let main_cpu_cycles = self.main_cpu.execute_instruction(&mut main_bus);

        let genesis_mclk_elapsed = u64::from(main_cpu_cycles) * MAIN_CPU_DIVIDER;
        let z80_cycles = (self.genesis_mclk_cycles + genesis_mclk_elapsed) / Z80_DIVIDER
            - self.genesis_mclk_cycles / Z80_DIVIDER;

        let genesis_master_clock_rate = match self.timing_mode {
            TimingMode::Ntsc => NTSC_GENESIS_MASTER_CLOCK_RATE,
            TimingMode::Pal => PAL_GENESIS_MASTER_CLOCK_RATE,
        };
        self.genesis_mclk_cycles += genesis_mclk_elapsed;

        for _ in 0..z80_cycles {
            self.z80.tick(&mut main_bus);
        }

        // TODO avoid floating point
        let sega_cd_mclk_elapsed_float = genesis_mclk_elapsed as f64
            * SEGA_CD_MASTER_CLOCK_RATE as f64
            / genesis_master_clock_rate as f64;
        self.sega_cd_mclk_cycles_float += sega_cd_mclk_elapsed_float;
        let prev_scd_mclk_cycles = self.sega_cd_mclk_cycles;
        self.sega_cd_mclk_cycles = self.sega_cd_mclk_cycles_float.round() as u64;

        let sub_cpu_cycles =
            self.sega_cd_mclk_cycles / SUB_CPU_DIVIDER - prev_scd_mclk_cycles / SUB_CPU_DIVIDER;
        self.tick_sub_cpu(sub_cpu_cycles);

        self.input.tick(main_cpu_cycles);

        for _ in 0..z80_cycles {
            if self.psg.tick() == PsgTickEffect::Clocked {
                // TODO
            }
        }

        for _ in 0..main_cpu_cycles {
            if self.ym2612.tick() == YmTickEffect::OutputSample {
                // TODO
            }
        }

        if self.vdp.tick(genesis_mclk_elapsed, &mut self.memory) == VdpTickEffect::FrameComplete {
            // TODO
        }
    }

    fn tick_sub_cpu(&mut self, sub_cpu_cycles: u64) {
        if sub_cpu_cycles >= self.sub_cpu_wait_cycles {
            let wait_cycles = self.sub_cpu_wait_cycles;
            let mut bus =
                SubBus::new(&mut self.memory, &mut self.graphics_coprocessor, &mut self.pcm);
            self.sub_cpu_wait_cycles = self.sub_cpu.execute_instruction(&mut bus).into();
            self.tick_sub_cpu(sub_cpu_cycles - wait_cycles);
        } else {
            self.sub_cpu_wait_cycles -= sub_cpu_cycles;
        }
    }
}
