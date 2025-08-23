//! GBA input state and registers

use crate::interrupts::{InterruptRegisters, InterruptType};
use bincode::{Decode, Encode};
use gba_config::GbaInputs;
use jgenesis_common::define_bit_enum;
use jgenesis_common::num::GetBit;

define_bit_enum!(IrqLogic, [Or, And]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct InputState {
    inputs: GbaInputs,
    irq_enabled: bool,
    irq_logic: IrqLogic,
    irq_mask: u16,
    prev_irq_line: bool,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            inputs: GbaInputs::default(),
            irq_enabled: false,
            irq_logic: IrqLogic::default(),
            irq_mask: 0,
            prev_irq_line: false,
        }
    }

    pub fn update_inputs(
        &mut self,
        inputs: GbaInputs,
        cycles: u64,
        interrupts: &mut InterruptRegisters,
    ) {
        self.inputs = inputs;
        self.check_for_interrupt(cycles, interrupts);
    }

    // $4000130: KEYINPUT (Keypad input)
    pub fn read_keyinput(&self) -> u16 {
        [
            (self.inputs.joypad.a, 0),
            (self.inputs.joypad.b, 1),
            (self.inputs.joypad.select, 2),
            (self.inputs.joypad.start, 3),
            (self.inputs.joypad.right, 4),
            (self.inputs.joypad.left, 5),
            (self.inputs.joypad.up, 6),
            (self.inputs.joypad.down, 7),
            (self.inputs.joypad.r, 8),
            (self.inputs.joypad.l, 9),
        ]
        .into_iter()
        .map(|(pressed, bit)| u16::from(!pressed) << bit)
        .reduce(|a, b| a | b)
        .unwrap()
    }

    // $4000132: KEYCNT (Keypad interrupt control)
    pub fn write_keycnt(&mut self, value: u16, cycles: u64, interrupts: &mut InterruptRegisters) {
        self.irq_enabled = value.bit(14);
        self.irq_logic = IrqLogic::from_bit(value.bit(15));
        self.irq_mask = value & 0x3FF;

        self.check_for_interrupt(cycles, interrupts);

        log::trace!("KEYCNT write: {value:04X}");
        log::trace!("  IRQ enabled: {}", self.irq_enabled);
        log::trace!("  IRQ logic: {:?}", self.irq_logic);
        log::trace!("  IRQ mask: {:04X}", self.irq_mask);
    }

    // $4000132: KEYCNT (Keypad interrupt control)
    pub fn read_keycnt(&self) -> u16 {
        self.irq_mask | (u16::from(self.irq_enabled) << 14) | ((self.irq_logic as u16) << 15)
    }

    fn check_for_interrupt(&mut self, cycles: u64, interrupts: &mut InterruptRegisters) {
        if !self.irq_enabled {
            self.prev_irq_line = false;
            return;
        }

        let pressed = !self.read_keyinput();

        let irq_line = match self.irq_logic {
            IrqLogic::Or => pressed & self.irq_mask != 0,
            IrqLogic::And => pressed & self.irq_mask == self.irq_mask,
        };

        if !self.prev_irq_line && irq_line {
            interrupts.set_flag(InterruptType::Keypad, cycles);
        }
        self.prev_irq_line = irq_line;
    }
}
