use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::EnumAll;

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumAll)]
pub enum InterruptType {
    VBlank = 0,
    HBlank = 1,
    VCounter = 2,
    Timer0 = 3,
    Timer1 = 4,
    Timer2 = 5,
    Timer3 = 6,
    Serial = 7,
    Dma0 = 8,
    Dma1 = 9,
    Dma2 = 10,
    Dma3 = 11,
    Keypad = 12,
    GamePak = 13,
}

impl InterruptType {
    fn name(self) -> &'static str {
        match self {
            Self::VBlank => "VBlank",
            Self::HBlank => "HBlank",
            Self::VCounter => "V counter match",
            Self::Timer0 => "Timer 0 overflow",
            Self::Timer1 => "Timer 1 overflow",
            Self::Timer2 => "Timer 2 overflow",
            Self::Timer3 => "Timer 3 overflow",
            Self::Serial => "Serial",
            Self::Dma0 => "DMA 0",
            Self::Dma1 => "DMA 1",
            Self::Dma2 => "DMA 2",
            Self::Dma3 => "DMA 3",
            Self::Keypad => "Keypad",
            Self::GamePak => "Game Pak",
        }
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct InterruptFlags(u16);

#[derive(Debug, Clone, Encode, Decode)]
pub struct InterruptRegisters {
    ime: bool,
    enabled: InterruptFlags,
    flags: InterruptFlags,
}

impl InterruptRegisters {
    pub fn new() -> Self {
        Self { ime: false, enabled: InterruptFlags(0), flags: InterruptFlags(0) }
    }

    // $4000200: IE (Interrupts enabled)
    pub fn read_ie(&self) -> u16 {
        self.enabled.0
    }

    // $4000200: IE (Interrupts enabled)
    pub fn write_ie(&mut self, value: u16) {
        self.enabled.0 = value;

        log::debug!("IE write: {value:04X}");
        for interrupt_type in InterruptType::ALL {
            log::debug!(
                "  {} IRQ enabled: {}",
                interrupt_type.name(),
                value.bit(interrupt_type as u8)
            );
        }
    }

    // $4000202: IF (Interrupt flags)
    pub fn read_if(&self) -> u16 {
        self.flags.0
    }

    // $4000202: IF (Interrupt flags)
    pub fn write_if(&mut self, value: u16) {
        // Writing 1 to a bit clears it
        self.flags.0 &= !value;

        log::debug!("IF write: {value:04X}");
    }

    // $4000208: IME (Interrupt master enable)
    pub fn read_ime(&self) -> u16 {
        self.ime.into()
    }

    // $4000208: IME (Interrupt master enable)
    pub fn write_ime(&mut self, value: u16) {
        self.ime = value.bit(0);

        log::debug!("IME write: {value:04X} (enabled = {})", self.ime);
    }

    pub fn set_flag(&mut self, interrupt_type: InterruptType) {
        self.flags.0 |= 1 << (interrupt_type as u8);

        log::debug!("{} interrupt flag set", interrupt_type.name());
    }

    pub fn pending(&self) -> bool {
        self.ime && self.enabled.0 & self.flags.0 != 0
    }
}
