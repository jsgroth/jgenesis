use crate::input::InputState;
use crate::memory::Memory;
use crate::psg::Psg;
use crate::vdp::Vdp;
use crate::ym2413::Ym2413;
use crate::VdpVersion;
use jgenesis_traits::num::GetBit;
use z80_emu::traits::{BusInterface, InterruptLine};

pub struct Bus<'a> {
    version: VdpVersion,
    memory: &'a mut Memory,
    vdp: &'a mut Vdp,
    psg: &'a mut Psg,
    ym2413: Option<&'a mut Ym2413>,
    input: &'a mut InputState,
}

impl<'a> Bus<'a> {
    pub fn new(
        version: VdpVersion,
        memory: &'a mut Memory,
        vdp: &'a mut Vdp,
        psg: &'a mut Psg,
        ym2413: Option<&'a mut Ym2413>,
        input: &'a mut InputState,
    ) -> Self {
        Self { version, memory, vdp, psg, ym2413, input }
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
        let address = address & 0xFF;
        if self.version == VdpVersion::GameGear && address <= 0x06 {
            // TODO Game Gear registers
            return match address {
                0x00 => (u8::from(!self.input.pause_pressed()) << 7) | 0x40,
                0x01 => 0x7F,
                0x02 | 0x04 | 0x06 => 0xFF,
                0x03 | 0x05 => 0x00,
                _ => unreachable!("value is <= 0x06"),
            };
        }

        if self.ym2413.is_some() && address == 0xF2 {
            return self.memory.read_audio_control();
        }

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
        let address = address & 0xFF;
        if self.version == VdpVersion::GameGear && address <= 0x06 {
            if address == 0x06 {
                self.psg.write_stereo_control(value);
            }
            return;
        }

        if let Some(ym2413) = &mut self.ym2413 {
            match address {
                0xF0 => {
                    ym2413.select_register(value);
                    return;
                }
                0xF1 => {
                    ym2413.write_data(value);
                    return;
                }
                0xF2 => {
                    self.memory.write_audio_control(value);
                    return;
                }
                _ => {}
            }
        }

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
        if self.version.is_master_system() && self.input.pause_pressed() {
            InterruptLine::Low
        } else {
            InterruptLine::High
        }
    }

    fn int(&self) -> InterruptLine {
        self.vdp.interrupt_line()
    }

    fn busreq(&self) -> bool {
        false
    }

    fn reset(&self) -> bool {
        false
    }
}
