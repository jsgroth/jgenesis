use crate::{BusInterface, Z80};

pub trait Z80Debugger {
    fn check_read_memory(&mut self, address: u16, cpu: &mut Z80);

    fn check_read_io(&mut self, address: u16, cpu: &mut Z80);

    fn check_write_memory(&mut self, address: u16, value: u8, cpu: &mut Z80);

    fn check_write_io(&mut self, address: u16, value: u8, cpu: &mut Z80);

    fn check_execute(&mut self, pc: u16, cpu: &mut Z80);
}

pub struct DummyZ80Debugger;

impl Z80Debugger for DummyZ80Debugger {
    fn check_read_memory(&mut self, _address: u16, _cpu: &mut Z80) {}

    fn check_read_io(&mut self, _address: u16, _cpu: &mut Z80) {}

    fn check_write_memory(&mut self, _address: u16, _value: u8, _cpu: &mut Z80) {}

    fn check_write_io(&mut self, _address: u16, _value: u8, _cpu: &mut Z80) {}

    fn check_execute(&mut self, _pc: u16, _cpu: &mut Z80) {}
}

pub(crate) trait BusDebugExt {
    fn read_memory_debug(&mut self, address: u16, cpu: &mut Z80) -> u8;

    fn read_io_debug(&mut self, address: u16, cpu: &mut Z80) -> u8;

    fn write_memory_debug(&mut self, address: u16, value: u8, cpu: &mut Z80);

    fn write_io_debug(&mut self, address: u16, value: u8, cpu: &mut Z80);

    fn check_execute(&mut self, pc: u16, cpu: &mut Z80);
}

impl<B: BusInterface> BusDebugExt for B {
    fn read_memory_debug(&mut self, address: u16, cpu: &mut Z80) -> u8 {
        if let Some(mut debugger) = self.debug_view() {
            debugger.check_read_memory(address, cpu);
        }
        self.read_memory(address)
    }

    fn read_io_debug(&mut self, address: u16, cpu: &mut Z80) -> u8 {
        if let Some(mut debugger) = self.debug_view() {
            debugger.check_read_io(address, cpu);
        }
        self.read_io(address)
    }

    fn write_memory_debug(&mut self, address: u16, value: u8, cpu: &mut Z80) {
        if let Some(mut debugger) = self.debug_view() {
            debugger.check_write_memory(address, value, cpu);
        }
        self.write_memory(address, value);
    }

    fn write_io_debug(&mut self, address: u16, value: u8, cpu: &mut Z80) {
        if let Some(mut debugger) = self.debug_view() {
            debugger.check_write_io(address, value, cpu);
        }
        self.write_io(address, value);
    }

    fn check_execute(&mut self, pc: u16, cpu: &mut Z80) {
        if let Some(mut debugger) = self.debug_view() {
            debugger.check_execute(pc, cpu);
        }
    }
}
