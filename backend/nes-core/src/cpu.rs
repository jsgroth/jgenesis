//! CPU emulation code.

use crate::bus::{CpuBus, PpuRegister};
use bincode::{Decode, Encode};
use mos6502_emu::Mos6502;
use mos6502_emu::bus::BusInterface;

#[derive(Debug, Clone, Encode, Decode)]
struct OamDmaState {
    cycles_remaining: u16,
    source_high_byte: u8,
    last_read_value: u8,
}

#[derive(Debug, Clone, Encode, Decode)]
enum State {
    CpuExecuting,
    OamDmaDelay(OamDmaState),
    OamDma(OamDmaState),
}

impl Default for State {
    fn default() -> Self {
        Self::CpuExecuting
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct CpuState {
    mos6502: Mos6502,
    state: State,
}

impl CpuState {
    pub fn new(bus: &mut CpuBus<'_>) -> Self {
        let mos6502 = Mos6502::new_nes(bus);

        Self { mos6502, state: State::default() }
    }
}

/// Run the CPU for 1 CPU cycle.
pub fn tick(state: &mut CpuState, bus: &mut CpuBus<'_>, is_apu_active_cycle: bool) {
    if state.mos6502.frozen() {
        return;
    }

    state.state = match std::mem::take(&mut state.state) {
        State::CpuExecuting => {
            if bus.is_oamdma_dirty() {
                // Dummy opcode read
                bus.read(state.mos6502.pc());

                bus.clear_oamdma_dirty();

                let source_high_byte = bus.read_oamdma_for_transfer();
                log::trace!("OAM: Initiating OAM DMA transfer from {source_high_byte:02X}");

                let oam_dma_state =
                    OamDmaState { cycles_remaining: 512, source_high_byte, last_read_value: 0 };
                if is_apu_active_cycle {
                    State::OamDmaDelay(oam_dma_state)
                } else {
                    State::OamDma(oam_dma_state)
                }
            } else {
                state.mos6502.tick(bus);
                State::CpuExecuting
            }
        }
        State::OamDmaDelay(state) => State::OamDma(state),
        State::OamDma(OamDmaState {
            mut cycles_remaining,
            source_high_byte,
            mut last_read_value,
        }) => {
            cycles_remaining -= 1;

            if cycles_remaining % 2 == 1 {
                let source_low_byte = (0xFF - cycles_remaining / 2) as u8;
                last_read_value = bus.read(u16::from_le_bytes([source_low_byte, source_high_byte]));
            } else {
                bus.write(PpuRegister::OAMDATA.to_address(), last_read_value);
            }

            if cycles_remaining > 0 {
                State::OamDma(OamDmaState { cycles_remaining, source_high_byte, last_read_value })
            } else {
                State::CpuExecuting
            }
        }
    };
}

/// Reset the CPU, as if the console's reset button was pressed.
///
/// Reset does the following:
/// * Immediately update the PC to point to the RESET vector, and abandon the currently-in-progress instruction (if any)
/// * Subtract 3 from the stack pointer
/// * Disable IRQs
pub fn reset<B: BusInterface>(cpu_state: &mut CpuState, bus: &mut B) {
    cpu_state.mos6502.reset(bus);
}
