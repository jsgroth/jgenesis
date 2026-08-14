use crate::input::Pins;
use bincode::{Decode, Encode};
use genesis_config::Xe1apJoypadState;
use jgenesis_common::num::GetBit;
use std::cmp;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum Xe1apTransferState {
    Idle,
    Active,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Xe1apState {
    pub joypad: Xe1apJoypadState,
    latched: Xe1apJoypadState,
    transfer_state: Xe1apTransferState,
    transfer_counter: u8,
    transfer_ack: bool,
    transfer_cycles_remaining: u32,
    last_th: bool,
}

impl Xe1apState {
    // Timings based on: https://archive.org/details/micomBASIC_1990-10/ (pages 79-80)
    // When connected to the Genesis, TR is ACK and TL is L/H
    // These timings are based on the fastest transfer speed
    //
    // Each pair of nibbles is transferred in a 4-step pattern:
    //   A: ACK=0, L/H=0 (game reads first nibble here)
    //   B: ACK=1, L/H=1
    //   C: ACK=0, L/H=1 (game reads second nibble here)
    //   D: ACK=1, L/H=0
    //
    // L/H appears to change shortly after ACK 0->1 transitions; this is not emulated
    //
    // Transfers seem to begin in step D after a TH 1->0 transition
    const TRANSFER_A_CYCLES: u32 = 92; // Roughly 12 μs
    const TRANSFER_B_CYCLES: u32 = 30; // Roughly 4 μs
    const TRANSFER_C_CYCLES: u32 = 92; // Roughly 12 μs
    const TRANSFER_D_CYCLES: u32 = 168; // Roughly 22 μs

    pub fn new(joypad: Xe1apJoypadState) -> Self {
        Self {
            joypad,
            latched: joypad,
            transfer_state: Xe1apTransferState::Idle,
            transfer_counter: 0,
            transfer_ack: true,
            transfer_cycles_remaining: 0,
            last_th: true,
        }
    }

    pub fn update_pins(&mut self, pins: &mut Pins) {
        let th = pins.th();
        if self.last_th && !th {
            // TH 1->0 transition begins a new transfer
            self.latched = self.joypad;
            self.transfer_state = Xe1apTransferState::Active;
            self.transfer_counter = 0;
            self.transfer_ack = true;
            self.transfer_cycles_remaining = Self::TRANSFER_D_CYCLES;
        }
        self.last_th = th;

        pins.input_tl(self.transfer_counter.bit(0));
        pins.input_tr(self.transfer_ack);

        match self.transfer_state {
            Xe1apTransferState::Idle => {
                pins.input_data_nibble(0b1111);
            }
            Xe1apTransferState::Active => {
                // Only update D3-D0 pins when TR=0
                if !self.transfer_ack {
                    self.update_data_pins(pins);
                }
            }
        }

        log::debug!(
            "XE-1AP pins update: data={:04b}, TL={}, TR={}, counter={}",
            pins.pins & 0x0F,
            u8::from(pins.pins.bit(Pins::TL)),
            u8::from(pins.pins.bit(Pins::TR)),
            self.transfer_counter
        );
    }

    #[allow(clippy::match_same_arms)]
    pub fn update_data_pins(&self, pins: &mut Pins) {
        match self.transfer_counter {
            0 => {
                // E1, E2, Start, Select
                pins.input_d3(!self.latched.e1);
                pins.input_d2(!self.latched.e2);
                pins.input_d1(!self.latched.start);
                pins.input_d0(!self.latched.select);
            }
            1 => {
                // A|A', B|B', C, D
                pins.input_d3(!(self.latched.a || self.latched.ap));
                pins.input_d2(!(self.latched.b || self.latched.bp));
                pins.input_d1(!self.latched.c);
                pins.input_d0(!self.latched.d);
            }
            2 => {
                // Analog stick X, high nibble
                pins.input_data_nibble(self.latched.analog_x >> 4);
            }
            3 => {
                // Analog stick Y, high nibble
                pins.input_data_nibble(self.latched.analog_y >> 4);
            }
            4 => {
                // Always 0s
                pins.input_data_nibble(0b0000);
            }
            5 => {
                // Analog slider, high nibble
                pins.input_data_nibble(self.latched.slider >> 4);
            }
            6 => {
                // Analog stick X, low nibble
                pins.input_data_nibble(self.latched.analog_x & 0x0F);
            }
            7 => {
                // Analog stick Y, low nibble
                pins.input_data_nibble(self.latched.analog_y & 0x0F);
            }
            8 => {
                // Always 0s
                pins.input_data_nibble(0b0000);
            }
            9 => {
                // Analog slider, low nibble
                pins.input_data_nibble(self.latched.slider & 0x0F);
            }
            10 => {
                // Always 1s
                pins.input_data_nibble(0b1111);
            }
            11 => {
                // A, B, A', B'
                pins.input_d3(!self.latched.a);
                pins.input_d2(!self.latched.b);
                pins.input_d1(!self.latched.ap);
                pins.input_d0(!self.latched.bp);
            }
            _ => panic!(
                "XE-1AP transfer counter should always be <= 11, was {}",
                self.transfer_counter
            ),
        }
    }

    pub fn tick(&mut self, mut m68k_cycles: u32, pins: &mut Pins) {
        if self.transfer_state == Xe1apTransferState::Idle {
            return;
        }

        while m68k_cycles != 0 {
            match self.transfer_state {
                Xe1apTransferState::Idle => return,
                Xe1apTransferState::Active => {
                    let elapsed = cmp::min(m68k_cycles, self.transfer_cycles_remaining);
                    m68k_cycles -= elapsed;
                    self.transfer_cycles_remaining -= elapsed;
                    if self.transfer_cycles_remaining != 0 {
                        return;
                    }

                    self.transfer_cycles_remaining =
                        match (self.transfer_ack, self.transfer_counter.bit(0)) {
                            (true, false) => Self::TRANSFER_A_CYCLES,
                            (false, false) => Self::TRANSFER_B_CYCLES,
                            (true, true) => Self::TRANSFER_C_CYCLES,
                            (false, true) => Self::TRANSFER_D_CYCLES,
                        };

                    self.transfer_ack = !self.transfer_ack;
                    if self.transfer_ack {
                        if self.transfer_counter < 11 {
                            self.transfer_counter += 1;
                        } else {
                            // Transfer has ended
                            self.transfer_state = Xe1apTransferState::Idle;
                            self.transfer_counter = 0;
                            self.transfer_ack = true;
                        }
                    }

                    self.update_pins(pins);
                }
            }
        }
    }
}
