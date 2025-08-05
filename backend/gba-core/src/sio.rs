//! SIO / the serial port is largely not emulated, only stubbed out enough for games that access
//! it to kind of work

use bincode::{Decode, Encode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum Mode {
    #[default]
    Normal,
    MultiPlayer,
    Uart,
    JoyBus,
    GeneralPurpose,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct SerialPort {
    mode: Mode,
    rcnt: u16,
    siocnt: u16,
}

impl SerialPort {
    pub fn new() -> Self {
        Self { mode: Mode::default(), rcnt: 0, siocnt: 0 }
    }

    pub fn read_register(&mut self, address: u32) -> u16 {
        match address {
            0x4000128 => self.read_rcnt(),
            0x4000134 => self.read_siocnt(),
            0x4000136 | 0x4000142 | 0x400015A => 0, // Invalid addresses that return 0
            _ => {
                log::warn!("Unimplemented SIO read {address:08X}");
                !0
            }
        }
    }

    pub fn write_register(&mut self, address: u32, value: u16) {
        match address {
            0x4000128 => self.write_rcnt(value),
            0x4000134 => self.write_siocnt(value),
            _ => {
                log::warn!("Unimplemented SIO write {address:08X} {value:04X}");
            }
        }
    }

    fn read_rcnt(&self) -> u16 {
        self.rcnt
    }

    fn write_rcnt(&mut self, value: u16) {
        self.rcnt = value;
        self.update_mode();

        log::trace!("RCNT write: {value:04X}");
        log::trace!("  SIO mode: {:?}", self.mode);
    }

    fn read_siocnt(&self) -> u16 {
        self.siocnt
    }

    fn write_siocnt(&mut self, value: u16) {
        self.siocnt = value;
        self.update_mode();

        log::trace!("SIOCNT write: {value:04X}");
        log::trace!("  SIO mode: {:?}", self.mode);
    }

    fn update_mode(&mut self) {
        let bits = ((self.rcnt >> 14) << 2) | ((self.siocnt >> 12) & 3);

        self.mode = if bits & 0b1010 == 0b0000 {
            Mode::Normal
        } else if bits & 0b1011 == 0b0010 {
            Mode::MultiPlayer
        } else if bits & 0b1011 == 0b0011 {
            Mode::Uart
        } else if bits & 0b1100 == 0b1000 {
            Mode::GeneralPurpose
        } else {
            // bits & 0b1100 == 0b1100
            Mode::JoyBus
        };
    }
}
