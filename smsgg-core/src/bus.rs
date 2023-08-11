use crate::input::InputState;
use crate::memory::Memory;
use crate::num::GetBit;
use crate::psg::Psg;
use crate::vdp::Vdp;
use z80_emu::traits::{BusInterface, InterruptLine};

pub struct Bus<'a> {
    memory: &'a mut Memory,
    vdp: &'a mut Vdp,
    psg: &'a mut Psg,
    input: &'a mut InputState,
}

impl<'a> Bus<'a> {
    pub fn new(
        memory: &'a mut Memory,
        vdp: &'a mut Vdp,
        psg: &'a mut Psg,
        input: &'a mut InputState,
    ) -> Self {
        Self {
            memory,
            vdp,
            psg,
            input,
        }
    }
}

impl<'a> BusInterface for Bus<'a> {
    fn read_memory(&mut self, address: u16) -> u8 {
        self.memory.read(address)
    }

    fn write_memory(&mut self, address: u16, value: u8) {
        self.memory.write(address, value);
    }

    fn read_io(&mut self, address: u16) -> u8 {
        match (address.bit(7), address.bit(6), address.bit(0)) {
            (false, false, _) => {
                // Invalid read addresses
                0xFF
            }
            (false, true, false) => {
                log::trace!("V counter read");
                self.vdp.v_counter()
            }
            (false, true, true) => {
                // TODO H counter
                log::trace!("H counter read");
                0x00
            }
            (true, false, false) => {
                log::trace!("VDP data read");
                self.vdp.read_data()
            }
            (true, false, true) => {
                log::trace!("VDP control read");
                self.vdp.read_control()
            }
            (true, true, false) => {
                log::trace!("I/O A/B read");
                self.input.port_dc()
            }
            (true, true, true) => {
                log::trace!("I/O B/misc. read");
                self.input.port_dd()
            }
        }
    }

    fn write_io(&mut self, address: u16, value: u8) {
        match (address.bit(7), address.bit(6), address.bit(0)) {
            (false, false, false) => {
                // TODO memory control
                log::trace!("Memory control write: {value:02X}");
            }
            (false, false, true) => {
                log::trace!("I/O control write: {value:02X}");
                self.input.write_control(value);
            }
            (false, true, _) => {
                log::trace!("PSG write: {value:02X}");
                self.psg.write(value);
            }
            (true, false, false) => {
                log::trace!("VDP data write: {value:02X}");
                self.vdp.write_data(value);
            }
            (true, false, true) => {
                log::trace!("VDP control write: {value:02X}");
                self.vdp.write_control(value);
            }
            (true, true, _) => {}
        }
    }

    fn nmi(&self) -> InterruptLine {
        // TODO PAUSE button
        InterruptLine::High
    }

    fn int(&self) -> InterruptLine {
        self.vdp.interrupt_line()
    }
}
