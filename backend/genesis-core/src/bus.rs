use crate::api::debug::{DebugPendingWrite, GenesisDebuggerWithCpus, genesis_components};
use crate::input::InputState;
use crate::timing::CycleCounters;
use bincode::{Decode, Encode};
use genesis_components::cartridge::{Cartridge, GenesisRegionExt};
use genesis_components::memory::Memory;
use genesis_components::vdp::{Vdp, VdpBusView, VdpTickEffect};
use genesis_components::ym2612::Ym2612;
use genesis_components::{GenesisEmulatorConfigExt, vdp};
use genesis_config::{GenesisEmulatorConfig, GenesisRegion};
use jgenesis_common::frontend::{PartialClone, TimingMode};
use jgenesis_common::num::{GetBit, U16Ext};
use m68000_emu::debug::DummyM68000Debugger;
use s32x_core::api::debug::Dummy32XDebugger;
use s32x_core::api::{GenesisVdpInfo, Sega32X, Sega32XAudioOutput};
use segacd_core::api::debug::DummySegaCdDebugger;
use segacd_core::api::{SegaCd, SegaCdAudioOutput, SegaCdLoadResult};
use std::{cmp, mem};
use ti_sn76489::{Sn76489, Sn76489TickEffect, Sn76489Version};
use z80_emu::debug::DummyZ80Debugger;
use z80_emu::traits::InterruptLine;

pub mod debug;

const MARS: [u8; 4] = *b"MARS";

pub trait GenesisAudioOutput: SegaCdAudioOutput + Sega32XAudioOutput {
    fn collect_ym2612(&mut self, sample: (f64, f64));

    fn collect_psg(&mut self, sample: f64);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub struct Z80BankRegister {
    bank_number: u32,
    current_bit: u8,
}

impl Z80BankRegister {
    const BITS: u8 = 9;

    pub fn value(self) -> u32 {
        self.bank_number
    }

    pub fn map_to_68k_address(self, z80_address: u16) -> u32 {
        (self.bank_number << 15) | u32::from(z80_address & 0x7FFF)
    }

    pub fn write_bit(&mut self, bit: bool) {
        self.bank_number = (self.bank_number >> 1) | (u32::from(bit) << (Self::BITS - 1));
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct Z80Signals {
    busreq: bool,
    reset: bool,
}

impl Default for Z80Signals {
    fn default() -> Self {
        Self { busreq: false, reset: true }
    }
}

impl Z80Signals {
    fn busack(self) -> bool {
        self.busreq && !self.reset
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct PendingWrites {
    byte: Vec<(u32, u8)>,
    word: Vec<(u32, u16)>,
}

impl PendingWrites {
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self { byte: Vec::with_capacity(20), word: Vec::with_capacity(20) }
    }

    fn clear(&mut self) {
        self.byte.clear();
        self.word.clear();
    }

    pub fn to_debug_vec(&self) -> Vec<DebugPendingWrite> {
        self.byte
            .iter()
            .map(|&(address, value)| DebugPendingWrite::Byte { address, value })
            .chain(
                self.word
                    .iter()
                    .map(|&(address, value)| DebugPendingWrite::Word { address, value }),
            )
            .collect()
    }
}

struct VdpView<'bus> {
    sega_cd: Option<&'bus mut SegaCd>,
    sega_32x: Option<&'bus mut Sega32X>,
    cartridge: Option<&'bus mut Cartridge>,
    memory: &'bus mut Memory,
    open_bus: &'bus mut u16,
}

impl VdpBusView for VdpView<'_> {
    fn read_word_for_dma(&mut self, address: u32) -> u16 {
        match address {
            0x000000..=0x3FFFFF => {
                if let Some(cartridge) = &mut self.cartridge
                    && self.sega_32x.as_mut().is_none_or(|sega_32x| {
                        !sega_32x.adapter_enabled() || sega_32x.rom_to_vram_dma()
                    })
                {
                    cartridge.read_word_for_dma(address, self.open_bus)
                } else if let Some(sega_cd) = &mut self.sega_cd {
                    sega_cd.read_word_for_dma(address, self.open_bus)
                } else {
                    *self.open_bus
                }
            }
            0xE00000..=0xFFFFFF => self.memory.read_main_ram_word(address),
            _ => *self.open_bus,
        }
    }
}

#[derive(Debug, Encode, Decode, PartialClone)]
pub struct GenesisBus {
    #[partial_clone(partial)]
    pub sega_cd: Option<SegaCd>,
    pub sega_32x: Option<Sega32X>,
    pub memory: Memory,
    #[partial_clone(partial)]
    pub cartridge: Option<Cartridge>,
    pub vdp: Vdp,
    pub psg: Sn76489,
    pub ym2612: Ym2612,
    pub input: InputState,
    pub z80_bank: Z80BankRegister,
    pub z80_signals: Z80Signals,
    pub pending_writes: PendingWrites,
    pub cycles: CycleCounters,
    pub timing_mode: TimingMode,
    pub open_bus: u16,
    pub m68k_opcode: u16,
    pub m68k_reset: bool,
}

impl GenesisBus {
    pub fn new(
        timing_mode: TimingMode,
        cartridge: Option<Cartridge>,
        sega_cd: Option<SegaCd>,
        sega_32x: Option<Sega32X>,
        config: &GenesisEmulatorConfig,
    ) -> Self {
        let memory = Memory::new(config);
        let vdp = Vdp::new(timing_mode, config.to_vdp_config(sega_32x.is_some()));
        let psg = Sn76489::new(Sn76489Version::Standard);
        let ym2612 = Ym2612::new(config);

        let six_button_incompatible = cartridge
            .as_ref()
            .map(|cartridge| cartridge.metadata().six_button_incompatible)
            .or_else(|| sega_cd.as_ref().map(SegaCd::has_six_button_incompatible_game))
            .unwrap_or(false);
        let input = InputState::new(config, six_button_incompatible);

        let cycles = CycleCounters::new(config.clamped_m68k_divider());

        Self {
            sega_cd,
            sega_32x,
            memory,
            cartridge,
            vdp,
            psg,
            ym2612,
            input,
            z80_bank: Z80BankRegister::default(),
            z80_signals: Z80Signals::default(),
            pending_writes: PendingWrites::new(),
            cycles,
            timing_mode,
            open_bus: 0,
            m68k_opcode: 0,
            m68k_reset: true,
        }
    }

    #[inline]
    pub fn tick_components<const DEBUG: bool>(
        &mut self,
        m68k_cycles: u32,
        mclk_cycles: u64,
        m68k_wait: bool,
        audio_output: &mut impl GenesisAudioOutput,
        mut debugger: Option<GenesisDebuggerWithCpus<'_, '_>>,
    ) -> SegaCdLoadResult<VdpTickEffect> {
        if let Some(cartridge) = &mut self.cartridge {
            cartridge.tick(m68k_cycles);
        }

        self.input.tick(m68k_cycles);

        while self.cycles.should_tick_psg() {
            if self.psg.tick() == Sn76489TickEffect::Clocked {
                // PSG only has mono output in the Genesis; stereo output is only for Game Gear
                let (psg_sample, _) = self.psg.sample();
                audio_output.collect_psg(psg_sample);
            }

            self.cycles.psg_cycle();
        }

        let vdp_tick_effect = self.vdp.tick(
            mclk_cycles,
            &mut VdpView {
                sega_cd: self.sega_cd.as_mut(),
                sega_32x: self.sega_32x.as_mut(),
                cartridge: self.cartridge.as_mut(),
                memory: &mut self.memory,
                open_bus: &mut self.open_bus,
            },
        );

        if self.cycles.ym2612_sync_needed() || vdp_tick_effect == VdpTickEffect::FrameComplete {
            self.sync_ym2612();

            for sample in self.ym2612.drain_output_samples() {
                audio_output.collect_ym2612(sample);
            }
        }

        if let Some(sega_cd) = &mut self.sega_cd {
            if DEBUG && let Some(debugger) = &mut debugger {
                let mut sega_cd_debugger = debugger.for_sega_cd(
                    self.sega_32x.as_mut(),
                    genesis_components!(self),
                    self.cartridge.as_mut(),
                );
                sega_cd.tick::<true>(mclk_cycles, audio_output, &mut sega_cd_debugger)?;
            } else {
                sega_cd.tick::<false>(mclk_cycles, audio_output, &mut DummySegaCdDebugger)?;
            }
        }

        if let Some(sega_32x) = &mut self.sega_32x {
            let genesis_vdp_info = GenesisVdpInfo {
                scanline: self.vdp.scanline(),
                scanline_mclk: self.vdp.scanline_mclk(),
                frame_size: self.vdp.frame_size(),
                border_size: self.vdp.border_size(),
                scanlines_in_current_frame: self.vdp.scanlines_in_current_frame(),
            };

            if DEBUG && let Some(debugger) = &mut debugger {
                unsafe {
                    let mut s32x_debugger =
                        debugger.for_32x(self.sega_cd.as_mut(), genesis_components!(self));
                    sega_32x.tick::<true>(
                        mclk_cycles,
                        &mut self.cartridge,
                        genesis_vdp_info,
                        audio_output,
                        &mut s32x_debugger,
                    );
                }
            } else {
                sega_32x.tick::<false>(
                    mclk_cycles,
                    &mut self.cartridge,
                    genesis_vdp_info,
                    audio_output,
                    &mut Dummy32XDebugger,
                );
            }
        }

        check_for_long_dma_skip(&self.vdp, &mut self.cycles);

        if !m68k_wait {
            self.vdp.update_interrupt_latches();
        }

        self.apply_pending_writes();

        Ok(vdp_tick_effect)
    }

    pub fn game_title(&mut self) -> Option<String> {
        self.cartridge
            .as_ref()
            .map(|cartridge| cartridge.program_title().to_owned())
            .or_else(|| self.sega_cd.as_ref().and_then(SegaCd::disc_title))
    }

    pub fn has_persistent_ram(&self) -> bool {
        self.sega_cd.is_some() || self.cartridge.as_ref().is_some_and(Cartridge::is_ram_persistent)
    }

    pub fn get_and_clear_persistent_ram_dirty(&mut self) -> bool {
        let mut dirty = self.sega_cd.as_mut().is_some_and(SegaCd::take_backup_ram_dirty);

        dirty |= self.cartridge.as_mut().is_some_and(Cartridge::get_and_clear_ram_dirty);

        dirty
    }

    pub fn reload_config(&mut self, config: &GenesisEmulatorConfig) {
        if let Some(sega_cd) = &mut self.sega_cd {
            sega_cd.reload_config(config);
        }

        if let Some(sega_32x) = &mut self.sega_32x {
            sega_32x.reload_config(&config.sega_32x);
        }

        if let Some(cartridge) = &mut self.cartridge {
            cartridge.reload_config(config);
        }

        self.memory.reload_config(config);
        self.vdp.reload_config(config.to_vdp_config(self.sega_32x.is_some()));
        self.ym2612.reload_config(config);
        self.input.reload_config(config);
        self.cycles.update_m68k_divider(config.clamped_m68k_divider());
    }

    pub fn reset(&mut self) {
        if let Some(sega_cd) = &mut self.sega_cd {
            sega_cd.reset();
        }

        if let Some(sega_32x) = &mut self.sega_32x {
            sega_32x.reset();
        }

        self.z80_signals = Z80Signals::default();
        self.ym2612.reset();
    }

    fn apply_pending_writes(&mut self) {
        let mut pending_writes = mem::take(&mut self.pending_writes);

        for &(address, value) in &pending_writes.byte {
            self.apply_write::<false>(address, value.into());
        }

        for &(address, value) in &pending_writes.word {
            self.apply_write::<true>(address, value);
        }

        pending_writes.clear();
        self.pending_writes = pending_writes;
    }

    fn m68k_read<const WORD: bool>(&mut self, address: u32) -> u16 {
        let address = address & 0xFFFFFF;
        log::trace!("Main bus {} read, address={address:06X}", if WORD { "word" } else { "byte" });

        let value = match address {
            0x000000..=0x3FFFFF => {
                // If cartridge is present, maps to cartridge
                // Otherwise maps to Sega CD BIOS ROM / PRG RAM / Word RAM
                if let Some(sega_32x) = &mut self.sega_32x
                    && sega_32x.adapter_enabled()
                    && self.cartridge.is_some()
                {
                    sega_32x.m68k_read_cartridge::<WORD>(
                        address,
                        self.open_bus,
                        self.cartridge.as_mut(),
                    )
                } else if let Some(cartridge) = &mut self.cartridge {
                    cartridge.read::<WORD>(address, self.open_bus)
                } else if let Some(sega_cd) = &mut self.sega_cd {
                    sega_cd.main_read_memory::<WORD>(address)
                } else {
                    self.read_open_bus::<WORD>(address)
                }
            }
            0x400000..=0x7FFFFF => {
                // If cartridge is present, maps to Sega CD if present, otherwise let it go through to the cartridge
                // Otherwise, assuming Sega CD is present, maps to Sega CD RAM cartridge
                // TODO can 68000 access the cartridge here when 32X adapter is enabled?
                match (&mut self.cartridge, &mut self.sega_cd) {
                    (Some(_cartridge), Some(sega_cd)) => sega_cd.main_read_memory::<WORD>(address),
                    (Some(cartridge), None) => cartridge.read::<WORD>(address, self.open_bus),
                    (None, Some(sega_cd)) => sega_cd.read_ram_cartridge::<WORD>(address),
                    (None, None) => self.read_open_bus::<WORD>(address),
                }
            }
            0x800000..=0x9FFFFF
                if let Some(sega_32x) = &mut self.sega_32x
                    && sega_32x.adapter_enabled() =>
            {
                sega_32x.m68k_read_memory::<WORD>(address, self.open_bus, self.cartridge.as_mut())
            }
            0xA00000..=0xA0FFFF => {
                // Z80 memory map; 68k can only access when the Z80 is running and removed from the bus
                if self.z80_signals.busack() {
                    self.cycles.record_68k_z80_bus_access();

                    // For 68k access, $8000-$FFFF mirrors $0000-$7FFF
                    let byte = <Self as z80_emu::BusInterface>::read_memory(
                        self,
                        (address & 0x7FFF) as u16,
                    );
                    if WORD {
                        // All Z80 access is byte-size; word reads mirror the byte in both MSB and LSB
                        u16::from_ne_bytes([byte; 2])
                    } else {
                        byte.into()
                    }
                } else {
                    self.read_open_bus::<WORD>(address)
                }
            }
            0xA10000..=0xA1001F => {
                if WORD {
                    let byte = self.read_io_register(address | 1);
                    u16::from_ne_bytes([byte; 2])
                } else {
                    self.read_io_register(address).into()
                }
            }
            0xA11100 => {
                // Bit 8 is Z80 BUSACK (active low), other bits are unused
                let busack_bit = u16::from(!self.z80_signals.busack()) << 8;

                // Unused bits should read open bus; Danny Sullivan's Indy Heat (Proto) and Time Killers
                // depend on this or they will fail to boot
                let busack_word = busack_bit | (self.open_bus & !(1 << 8));
                if WORD { busack_word } else { busack_word.msb().into() }
            }
            0xA12000..=0xA12FFF if let Some(sega_cd) = &mut self.sega_cd => {
                sega_cd.main_read_register::<WORD>(address)
            }
            0xA130EC..=0xA130EF if self.sega_32x.is_some() => {
                // Always reads the ASCII string "MARS"; used by 32X games to detect 32X hardware
                let idx = (address & 3) as usize;
                if WORD {
                    u16::from_be_bytes(MARS[idx..idx + 2].try_into().unwrap())
                } else {
                    MARS[idx].into()
                }
            }
            0xA15000..=0xA15FFF => {
                if let Some(sega_32x) = &mut self.sega_32x {
                    sega_32x.m68k_read_register::<WORD>(address, self.open_bus)
                } else if let Some(cartridge) = &mut self.cartridge {
                    cartridge.read::<WORD>(address, self.open_bus)
                } else {
                    self.read_open_bus::<WORD>(address)
                }
            }
            0xC00000..=0xC0001F => self.read_vdp::<WORD>(address),
            0xE00000..=0xFFFFFF => self.memory.read_main_ram::<WORD>(address),
            _ => self.read_open_bus::<WORD>(address),
        };

        if WORD {
            self.open_bus = value;
        } else {
            // TODO this is probably not right, probably depends on memory region
            self.open_bus.set_msb(value as u8);
        }

        value
    }

    #[inline(always)]
    fn read_open_bus<const WORD: bool>(&self, address: u32) -> u16 {
        if WORD { self.open_bus } else { self.open_bus.be_byte(address & 1).into() }
    }

    fn apply_write<const WORD: bool>(&mut self, address: u32, value: u16) {
        let address = address & 0xFFFFFF;

        if WORD {
            log::trace!("Main bus word write: address={address:06X}, value={value:04X}");
        } else {
            log::trace!("Main bus byte write: address={address:06X}, value={:02X}", value & 0xFF);
        }

        match address {
            0x000000..=0x3FFFFF => {
                // If cartridge is present, maps to cartridge
                // Otherwise maps to Sega CD BIOS ROM / PRG RAM / Word RAM
                if let Some(sega_32x) = &mut self.sega_32x
                    && sega_32x.adapter_enabled()
                    && self.cartridge.is_some()
                {
                    sega_32x.m68k_write_cartridge::<WORD>(address, value, self.cartridge.as_mut());
                } else if let Some(cartridge) = &mut self.cartridge {
                    cartridge.write::<WORD>(address, value);
                } else if let Some(sega_cd) = &mut self.sega_cd {
                    sega_cd.main_write_memory::<WORD>(address, value);
                }
            }
            0x400000..=0x7FFFFF => {
                // If cartridge is present, maps to Sega CD if present, otherwise let it go through to the cartridge
                // Otherwise, assuming Sega CD is present, maps to Sega CD RAM cartridge
                // TODO can 68000 access the cartridge here when 32X adapter is enabled?
                match (&mut self.cartridge, &mut self.sega_cd) {
                    (Some(_cartridge), Some(sega_cd)) => {
                        sega_cd.main_write_memory::<WORD>(address, value);
                    }
                    (Some(cartridge), None) => {
                        cartridge.write::<WORD>(address, value);
                    }
                    (None, Some(sega_cd)) => {
                        sega_cd.write_ram_cartridge::<WORD>(address, value);
                    }
                    (None, None) => {}
                }
            }
            0x800000..=0x9FFFFF
                if let Some(sega_32x) = &mut self.sega_32x
                    && sega_32x.adapter_enabled() =>
            {
                sega_32x.m68k_write_memory::<WORD>(address, value, self.cartridge.as_mut());
            }
            0xA00000..=0xA0FFFF if self.z80_signals.busack() => {
                // Z80 memory map; writable by the 68k only when the Z80 is removed from the bus
                // and not reset
                self.cycles.record_68k_z80_bus_access();

                // Word-size writes write the MSB as a byte-size write
                let byte = if WORD { value.msb() } else { value as u8 };

                // For 68k access, $8000-$FFFF mirrors $0000-$7FFF
                <Self as z80_emu::BusInterface>::write_memory(
                    self,
                    (address & 0x7FFF) as u16,
                    byte,
                );
            }
            0xA10000..=0xA1000F => {
                if WORD {
                    self.write_io_register(address | 1, value.lsb());
                } else {
                    self.write_io_register(address, value as u8);
                }
            }
            0xA11100 => {
                self.z80_signals.busreq = if WORD { value.bit(8) } else { value.bit(0) };
                log::trace!("Set Z80 BUSREQ to {}", self.z80_signals.busreq);
            }
            0xA11200 => {
                let z80_reset = if WORD { !value.bit(8) } else { !value.bit(0) };
                self.set_z80_reset(z80_reset);
            }
            0xA12000..=0xA12FFF if let Some(sega_cd) = &mut self.sega_cd => {
                sega_cd.main_write_register::<WORD>(address, value);
            }
            0xA13000..=0xA13FFF if let Some(cartridge) = &mut self.cartridge => {
                // TIME signal; used by various cartridge mapper registers
                cartridge.write::<WORD>(address, value);
            }
            0xA15000..=0xA15FFF => {
                if let Some(sega_32x) = &mut self.sega_32x {
                    sega_32x.m68k_write_register::<WORD>(address, value);
                } else if let Some(cartridge) = &mut self.cartridge {
                    // SVP maps registers to $A15xxx
                    cartridge.write::<WORD>(address, value);
                }
            }
            0xC00000..=0xC0001F => {
                self.write_vdp_psg::<WORD>(address, value);
            }
            0xE00000..=0xFFFFFF => {
                self.memory.write_main_ram::<WORD>(address, value);
            }
            _ => {}
        }
    }

    fn read_io_register(&mut self, address: u32) -> u8 {
        match address {
            // Version register
            0xA10001 => {
                let region = self
                    .cartridge
                    .as_ref()
                    .map(Cartridge::region)
                    .or_else(|| self.sega_cd.as_ref().map(SegaCd::region))
                    .unwrap_or(GenesisRegion::Americas);

                // TODO version (lowest 4 bits) hardcoded to 0
                (u8::from(region.version_bit()) << 7)
                    | (u8::from(self.timing_mode == TimingMode::Pal) << 6)
                    | (u8::from(self.sega_cd.is_none()))
            }
            0xA10003 => self.input.read_p1_data(),
            0xA10005 => self.input.read_p2_data(),
            0xA10007 => self.input.read_ext_data(),
            0xA10009 => self.input.read_p1_ctrl(),
            0xA1000B => self.input.read_p2_ctrl(),
            0xA1000D => self.input.read_ext_ctrl(),
            0xA1000F => self.input.read_p1_tx_data(),
            0xA10015 => self.input.read_p2_tx_data(),
            0xA1001B => self.input.read_ext_tx_data(),
            // Other I/O registers return 0x00 by default
            _ => 0x00,
        }
    }

    fn write_io_register(&mut self, address: u32, value: u8) {
        match address {
            0xA10003 => self.input.write_p1_data(value),
            0xA10005 => self.input.write_p2_data(value),
            0xA10007 => self.input.write_ext_data(value),
            0xA10009 => self.input.write_p1_ctrl(value),
            0xA1000B => self.input.write_p2_ctrl(value),
            0xA1000D => self.input.write_ext_ctrl(value),
            0xA1000F => self.input.write_p1_tx_data(value),
            0xA10015 => self.input.write_p2_tx_data(value),
            0xA1001B => self.input.write_ext_tx_data(value),
            _ => {}
        }
    }

    fn read_vdp<const WORD: bool>(&mut self, address: u32) -> u16 {
        let word = match address & 0x1F {
            0x00..=0x03 => self.vdp.read_data(),
            0x04..=0x07 => {
                // Highest 6 bits of VDP status register are open bus; VDPFIFOTesting DMA busy flag tests
                // depend on this
                self.vdp.read_status(self.m68k_opcode, self.cycles.m68k_divider.get())
                    | (self.open_bus & 0xFC00)
            }
            0x08..=0x0B => self.vdp.hv_counter(self.m68k_opcode, self.cycles.m68k_divider.get()),
            0x0C..=0x1F => {
                // PSG / unused space; PSG is not readable
                self.open_bus
            }
            _ => unreachable!("address & 0x1F is always <= 0x1F"),
        };

        if WORD { word } else { word.to_be_bytes()[(address & 1) as usize].into() }
    }

    fn write_vdp_psg<const WORD: bool>(&mut self, address: u32, value: u16) {
        // Byte-size VDP writes duplicate the byte into a word
        let vdp_word = if WORD { value } else { (value & 0xFF) | (value << 8) };

        match address & 0x1F {
            0x00..=0x03 => self.vdp.write_data(vdp_word),
            0x04..=0x07 => self.vdp.write_control(vdp_word),
            0x10..=0x17 if WORD || address.bit(0) => self.psg.write(value as u8),
            0x1C..=0x1D => self.vdp.write_debug_register(vdp_word),
            _ => {}
        }
    }

    fn set_z80_reset(&mut self, z80_reset: bool) {
        if !self.z80_signals.reset && z80_reset {
            // Z80 RESET also resets the YM2612
            // Fantastic Dizzy depends on this or music will not mute correctly when you pause the game
            self.ym2612.reset();
        }

        self.z80_signals.reset = z80_reset;
        log::trace!("Set Z80 RESET to {}", self.z80_signals.reset);
    }

    fn sync_ym2612(&mut self) {
        if self.cycles.has_ym2612_ticks() {
            let ticks = self.cycles.take_ym2612_ticks();
            self.ym2612.tick(ticks);
        }
    }
}

// If a long DMA is in progress (i.e. the DMA will not finish on this line), preemptively skip the
// 68000 forward by a large number of mclk cycles (up to 1250).
//
// This function is public so that it can be used by the Sega CD core
#[inline]
fn check_for_long_dma_skip(vdp: &Vdp, cycles: &mut CycleCounters) {
    if !vdp.long_halting_dma_in_progress() {
        return;
    }

    if !cycles.z80_halt {
        // Don't advance for very long time slices if the Z80 is still active; doing so causes
        // video/audio desync in Overdrive 2.
        // 8 68K cycles is slightly less than 4 Z80 cycles
        cycles.m68k_wait_cpu_cycles = 8;
        return;
    }

    // Skip as close as possible to the end of the current scanline
    let wait_cycles = cmp::max(
        cycles.m68k_wait_cpu_cycles,
        cmp::min(
            cycles.max_wait_cpu_cycles,
            (vdp::MCLK_CYCLES_PER_SCANLINE - vdp.scanline_mclk()) as u32
                / cycles.m68k_divider_u32.get(),
        ),
    );
    cycles.m68k_wait_cpu_cycles = wait_cycles;

    log::trace!(
        "Skipping {wait_cycles} 68000 CPU cycles in long DMA optimization, scanline mclk is {}",
        vdp.scanline_mclk()
    );
}

impl m68000_emu::BusInterface for GenesisBus {
    type DebugView<'a>
        = DummyM68000Debugger
    where
        Self: 'a;

    #[inline]
    fn read_byte(&mut self, address: u32) -> u8 {
        self.m68k_read::<false>(address) as u8
    }

    #[inline]
    fn read_word(&mut self, address: u32) -> u16 {
        self.m68k_read::<true>(address)
    }

    #[inline]
    fn write_byte(&mut self, address: u32, value: u8) {
        self.pending_writes.byte.push((address, value));
    }

    #[inline]
    fn write_word(&mut self, address: u32, value: u16) {
        self.pending_writes.word.push((address, value));
    }

    #[inline]
    fn interrupt_level(&self) -> u8 {
        self.vdp.m68k_interrupt_level()
    }

    #[inline]
    fn acknowledge_interrupt(&mut self, _interrupt_level: u8) {
        // When the 68000 acknowledges a VDP interrupt, the VDP acknowledges whatever level it is
        // currently raising rather than paying attention to the 68000's IACK lines. This is noted
        // in official documentation which describes this hardware bug: If both HINT are VINT are
        // enabled and the 68000 executes a long instruction right before HINT would trigger on line
        // 224, it's possible for the following sequence of events to happen:
        //   1. The VDP sets IPL2 to indicate a level 4 interrupt (HINT)
        //   2. After the 68000 finishes its long instruction, it begins to handle the level 4 interrupt
        //   3. Before the acknowledge, VBlank begins and the VDP sets IPL2+IPL1 for level 6 interrupt (VINT)
        //   4. The 68000 sets its VPA signal to acknowledge the interrupt, and the VDP acknowledges VINT instead of HINT
        //   5. After the 68000 returns from its HINT handler, it immediately handles the HINT a second time (having missed VINT)
        self.vdp.acknowledge_m68k_interrupt();
    }

    #[inline]
    fn halt(&self) -> bool {
        self.vdp.should_halt_cpu()
    }

    #[inline]
    fn reset(&self) -> bool {
        self.m68k_reset
    }
}

impl z80_emu::BusInterface for GenesisBus {
    type DebugView<'a>
        = DummyZ80Debugger
    where
        Self: 'a;

    #[inline]
    #[allow(clippy::match_same_arms)]
    fn read_memory(&mut self, address: u16) -> u8 {
        log::trace!("Z80 bus read from {address:04X}");

        match address {
            0x0000..=0x3FFF => {
                // Z80 RAM (mirrored at $2000-$3FFF)
                self.memory.read_audio_ram(address)
            }
            0x4000..=0x5FFF => {
                // YM2612 registers/ports (mirrored every 4 addresses)
                self.sync_ym2612();
                self.ym2612.read_register(address)
            }
            0x6000..=0x60FF => {
                // Bank number register
                // TODO what should this do on reads?
                0xFF
            }
            0x6100..=0x7EFF => {
                // Unused address space
                0xFF
            }
            0x7F00..=0x7F1F => {
                // VDP ports
                self.cycles.record_z80_68k_bus_access();
                self.read_vdp::<false>(address.into()) as u8
            }
            0x7F20..=0x7FFF => {
                // Invalid addresses
                0xFF
            }
            0x8000..=0xFFFF => {
                self.cycles.record_z80_68k_bus_access();

                let m68k_addr = self.z80_bank.map_to_68k_address(address);
                match m68k_addr {
                    0xA00000..=0xA0FFFF => {
                        // TODO this should lock up the system
                        log::error!(
                            "Z80 attempted to read its own memory from the 68k bus; z80_addr={address:04X}, m68k_addr={m68k_addr:08X}"
                        );
                        0xFF
                    }
                    0xE00000..=0xFFFFFF => {
                        // Z80 cannot read from 68000 working RAM
                        // TODO should probably return Z80 open bus instead?
                        0xFF
                    }
                    _ => <Self as m68000_emu::BusInterface>::read_byte(self, m68k_addr),
                }
            }
        }
    }

    #[inline]
    fn write_memory(&mut self, address: u16, value: u8) {
        log::trace!("Z80 bus write at {address:04X}");

        match address {
            0x0000..=0x3FFF => {
                // Z80 RAM (mirrored at $2000-$3FFF)
                self.memory.write_audio_ram(address, value);
            }
            0x4000..=0x5FFF => {
                // YM2612 registers/ports (mirrored every 4 addresses)
                self.sync_ym2612();
                match address & 0x03 {
                    0x00 => self.ym2612.write_address_1(value),
                    0x02 => self.ym2612.write_address_2(value),
                    0x01 | 0x03 => self.ym2612.write_data(value),
                    _ => unreachable!("value & 0x03 is always <= 0x03"),
                }
            }
            0x6000..=0x60FF => {
                self.z80_bank.write_bit(value.bit(0));
            }
            0x6100..=0x7EFF | 0x7F20..=0x7FFF => {
                // Unused / invalid addresses
                // TODO writes to $7F20-$7FFF should halt the system
            }
            0x7F00..=0x7F1F => {
                // VDP addresses
                self.cycles.record_z80_68k_bus_access();
                self.write_vdp_psg::<false>(address.into(), value.into());
            }
            0x8000..=0xFFFF => {
                self.cycles.record_z80_68k_bus_access();

                let m68k_addr = self.z80_bank.map_to_68k_address(address);
                if !(0xA00000..=0xA0FFFF).contains(&m68k_addr) {
                    self.apply_write::<false>(m68k_addr, value.into());
                } else {
                    // TODO this should lock up the system
                    log::error!(
                        "Z80 attempted to write to its own memory from the 68k bus; z80_addr={address:04X}, m68k_addr={m68k_addr:08X}, value={value:02X}"
                    );
                }
            }
        }
    }

    #[inline]
    fn read_io(&mut self, _address: u16) -> u8 {
        // I/O ports are not wired up to the Z80
        0xFF
    }

    #[inline]
    fn write_io(&mut self, _address: u16, _value: u8) {
        // I/O ports are not wired up to the Z80
    }

    #[inline]
    fn nmi(&self) -> InterruptLine {
        // The NMI line is not connected to anything
        InterruptLine::High
    }

    #[inline]
    fn int(&self) -> InterruptLine {
        self.vdp.z80_interrupt_line()
    }

    #[inline]
    fn busreq(&self) -> bool {
        self.z80_signals.busreq
    }

    #[inline]
    fn reset(&self) -> bool {
        self.z80_signals.reset
    }
}
