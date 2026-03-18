use crate::{BusInterface, M68000};

pub trait M68000Debugger {
    fn check_read<const WORD: bool>(&mut self, address: u32, cpu: &mut M68000);

    fn check_write<const WORD: bool>(&mut self, address: u32, value: u16, cpu: &mut M68000);

    fn check_execute(&mut self, pc: u32, cpu: &mut M68000);
}

pub struct DummyM68000Debugger;

impl M68000Debugger for DummyM68000Debugger {
    fn check_read<const WORD: bool>(&mut self, _address: u32, _cpu: &mut M68000) {}

    fn check_write<const WORD: bool>(&mut self, _address: u32, _value: u16, _cpu: &mut M68000) {}

    fn check_execute(&mut self, _pc: u32, _cpu: &mut M68000) {}
}

pub(crate) trait BusDebugExt {
    fn read_byte_debug(&mut self, address: u32, cpu: &mut M68000) -> u8;

    fn read_word_debug(&mut self, address: u32, cpu: &mut M68000) -> u16;

    fn read_longword_debug(&mut self, address: u32, cpu: &mut M68000) -> u32 {
        let high: u32 = self.read_word_debug(address, cpu).into();
        let low: u32 = self.read_word_debug(address + 2, cpu).into();
        low | (high << 16)
    }

    fn write_byte_debug(&mut self, address: u32, value: u8, cpu: &mut M68000);

    fn write_word_debug(&mut self, address: u32, value: u16, cpu: &mut M68000);

    fn write_longword_debug(&mut self, address: u32, value: u32, cpu: &mut M68000) {
        self.write_word_debug(address, (value >> 16) as u16, cpu);
        self.write_word_debug(address + 2, value as u16, cpu);
    }

    fn check_execute(&mut self, pc: u32, cpu: &mut M68000);
}

impl<B: BusInterface> BusDebugExt for B {
    fn read_byte_debug(&mut self, address: u32, cpu: &mut M68000) -> u8 {
        if let Some(mut debugger) = self.debug_view() {
            debugger.check_read::<false>(address, cpu);
        }
        self.read_byte(address)
    }

    fn read_word_debug(&mut self, address: u32, cpu: &mut M68000) -> u16 {
        if let Some(mut debugger) = self.debug_view() {
            debugger.check_read::<true>(address, cpu);
        }
        self.read_word(address)
    }

    fn write_byte_debug(&mut self, address: u32, value: u8, cpu: &mut M68000) {
        if let Some(mut debugger) = self.debug_view() {
            debugger.check_write::<false>(address, value.into(), cpu);
        }
        self.write_byte(address, value);
    }

    fn write_word_debug(&mut self, address: u32, value: u16, cpu: &mut M68000) {
        if let Some(mut debugger) = self.debug_view() {
            debugger.check_write::<true>(address, value, cpu);
        }
        self.write_word(address, value);
    }

    fn check_execute(&mut self, pc: u32, cpu: &mut M68000) {
        if let Some(mut debugger) = self.debug_view() {
            debugger.check_execute(pc, cpu);
        }
    }
}
