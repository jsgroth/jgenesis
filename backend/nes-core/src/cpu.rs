//! CPU emulation code.

use crate::apu::ApuState;
use crate::bus::{CpuBus, PpuRegister};
use bincode::{Decode, Encode};
use mos6502_emu::Mos6502;
use mos6502_emu::bus::BusInterface;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum OamDmaState {
    Idle,
    Pending,
    ReadReady { address_low: u8 },
    WriteReady { address_low: u8, byte: u8 },
}

impl OamDmaState {
    #[must_use]
    fn progress_noop(self) -> Self {
        match self {
            Self::Pending => Self::ReadReady { address_low: 0 },
            _ => self,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum DmcDmaState {
    Idle,
    PendingLoad,
    PendingReload,
    Pending,
    DummyCycle,
    ReadReady,
}

impl DmcDmaState {
    #[must_use]
    fn progress_noop(self, cpu_halted: bool, dma_cycle: DmaCycle, still_needs_dma: bool) -> Self {
        if !still_needs_dma {
            return Self::Idle;
        }

        match (self, dma_cycle) {
            (Self::PendingLoad, DmaCycle::Get) | (Self::PendingReload, DmaCycle::Put) => {
                if cpu_halted {
                    Self::DummyCycle
                } else {
                    Self::Pending
                }
            }
            (Self::Pending, _) if cpu_halted => Self::DummyCycle,
            (Self::DummyCycle, _) => Self::ReadReady,
            _ => self,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DmaCycle {
    Get,
    Put,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct CpuState {
    cpu: Mos6502,
    halted_cpu_address: Option<u16>,
    oam_dma: OamDmaState,
    dmc_dma: DmcDmaState,
}

impl CpuState {
    pub fn new(bus: &mut CpuBus<'_>) -> Self {
        let cpu = Mos6502::new_nes(bus);

        Self {
            cpu,
            halted_cpu_address: None,
            oam_dma: OamDmaState::Idle,
            dmc_dma: DmcDmaState::Idle,
        }
    }
}

/// Run the CPU for 1 CPU cycle.
pub fn tick(state: &mut CpuState, bus: &mut CpuBus<'_>, apu: &mut ApuState) {
    if state.cpu.frozen() {
        return;
    }

    if bus.is_oamdma_dirty() {
        bus.clear_oamdma_dirty();
        state.oam_dma = OamDmaState::Pending;
    }

    let needs_dmc_dma = apu.needs_dmc_dma();
    if needs_dmc_dma && state.dmc_dma == DmcDmaState::Idle {
        state.dmc_dma = if apu.dmc_dma_initial_load() {
            DmcDmaState::PendingLoad
        } else {
            DmcDmaState::PendingReload
        };
    }

    if state.oam_dma == OamDmaState::Idle && state.dmc_dma == DmcDmaState::Idle {
        state.cpu.tick(bus);
        state.halted_cpu_address = None;
        return;
    }

    let dma_cycle = if apu.is_active_cycle() { DmaCycle::Put } else { DmaCycle::Get };
    let Some(halted_cpu_address) = state.halted_cpu_address else {
        try_halt_cpu(state, dma_cycle, needs_dmc_dma, bus);
        return;
    };

    progress_dma(state, apu, halted_cpu_address, dma_cycle, bus);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CpuCycle {
    Read { address: u16 },
    Write,
}

fn try_halt_cpu(
    state: &mut CpuState,
    dma_cycle: DmaCycle,
    still_needs_dmc_dma: bool,
    bus: &mut CpuBus<'_>,
) {
    struct CapturingBus<'a, 'b>(&'a mut CpuBus<'b>, CpuCycle);

    impl BusInterface for CapturingBus<'_, '_> {
        fn read(&mut self, address: u16) -> u8 {
            self.1 = CpuCycle::Read { address };
            self.0.read(address)
        }

        fn write(&mut self, address: u16, value: u8) {
            self.1 = CpuCycle::Write;
            self.0.write(address, value);
        }

        fn nmi(&self) -> bool {
            self.0.nmi()
        }

        fn acknowledge_nmi(&mut self) {
            // NMIs are only acknowledged during write cycles, so let it go through
            self.0.acknowledge_nmi();
        }

        fn irq(&self) -> bool {
            self.0.irq()
        }
    }

    let mut cpu_clone = state.cpu.clone();
    let mut bus = CapturingBus(bus, CpuCycle::Write);
    cpu_clone.tick(&mut bus);

    let cpu_halted;
    match bus.1 {
        CpuCycle::Read { address } => {
            // Halt succeeded
            cpu_halted = true;
            state.halted_cpu_address = Some(address);
            state.oam_dma = state.oam_dma.progress_noop();
        }
        CpuCycle::Write => {
            // Halt failed; try again next cycle
            cpu_halted = false;
            state.cpu = cpu_clone;
        }
    }

    state.dmc_dma = state.dmc_dma.progress_noop(cpu_halted, dma_cycle, still_needs_dmc_dma);

    log::trace!(
        "Attempted to halt CPU; CPU cycle was {:?}, OAM DMA state {:?}, DMC DMA state {:?}",
        bus.1,
        state.oam_dma,
        state.dmc_dma
    );
}

fn progress_dma(
    state: &mut CpuState,
    apu: &mut ApuState,
    halted_cpu_address: u16,
    dma_cycle: DmaCycle,
    bus: &mut CpuBus<'_>,
) {
    log::trace!(
        "Progressing DMA, current OAM DMA state {:?}, current DMC DMA state {:?}",
        state.oam_dma,
        state.dmc_dma
    );

    let still_needs_dmc_dma = apu.needs_dmc_dma();

    match dma_cycle {
        DmaCycle::Get => {
            if state.dmc_dma == DmcDmaState::ReadReady {
                // DMC DMA read cycle; takes priority over OAM DMA read if both are ready
                apu.dmc_dma_read(bus);
                state.dmc_dma = DmcDmaState::Idle;

                state.oam_dma = state.oam_dma.progress_noop();

                log::trace!("  DMC DMA read; OAM DMA state is now {:?}", state.oam_dma);

                return;
            }

            if let OamDmaState::ReadReady { address_low } = state.oam_dma {
                // OAM DMA read cycle
                let address = u16::from_le_bytes([address_low, bus.read_oamdma_for_transfer()]);
                let byte = bus.read(address);
                state.oam_dma = OamDmaState::WriteReady { address_low, byte };

                state.dmc_dma = state.dmc_dma.progress_noop(true, dma_cycle, still_needs_dmc_dma);

                log::trace!(
                    "  OAM DMA read; OAM DMA state is now {:?}, DMC DMA state is now {:?}",
                    state.oam_dma,
                    state.dmc_dma
                );

                return;
            }
        }
        DmaCycle::Put => {
            if let OamDmaState::WriteReady { mut address_low, byte } = state.oam_dma {
                // OAM DMA write cycle
                bus.write(PpuRegister::OAMDATA.to_address(), byte);

                let done;
                (address_low, done) = address_low.overflowing_add(1);
                state.oam_dma =
                    if done { OamDmaState::Idle } else { OamDmaState::ReadReady { address_low } };

                state.dmc_dma = state.dmc_dma.progress_noop(true, dma_cycle, still_needs_dmc_dma);

                log::trace!(
                    "  OAM DMA write; OAM DMA state is now {:?}, DMC DMA state is now {:?}",
                    state.oam_dma,
                    state.dmc_dma
                );

                return;
            }
        }
    }

    // Neither DMA used the bus this cycle; progress no-op cycles and perform a dummy read
    state.oam_dma = state.oam_dma.progress_noop();
    state.dmc_dma = state.dmc_dma.progress_noop(true, dma_cycle, still_needs_dmc_dma);

    // Joypad dummy read behavior varies by console and CPU revision - this roughly matches NES with 2A03
    if halted_cpu_address != 0x4016 && halted_cpu_address != 0x4017 {
        bus.read(halted_cpu_address);
    }

    log::trace!(
        "  DMA no-op cycle; OAM DMA state is now {:?}, DMC DMA state is now {:?}",
        state.oam_dma,
        state.dmc_dma
    );
}

/// Reset the CPU, as if the console's reset button was pressed.
///
/// Reset does the following:
/// * Immediately update the PC to point to the RESET vector, and abandon the currently-in-progress instruction (if any)
/// * Subtract 3 from the stack pointer
/// * Disable IRQs
pub fn reset<B: BusInterface>(state: &mut CpuState, bus: &mut B) {
    state.cpu.reset(bus);
}
