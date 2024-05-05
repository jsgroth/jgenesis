//! Game Boy bus / address mapping

use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::dma::DmaUnit;
use crate::inputs::InputState;
use crate::interrupts::InterruptRegisters;
use crate::memory::Memory;
use crate::ppu::Ppu;
use crate::serial::SerialPort;
use crate::sm83::bus::BusInterface;
use crate::sm83::InterruptType;
use crate::speed::{CpuSpeed, SpeedRegister};
use crate::timer::GbTimer;
use crate::HardwareMode;

trait HardwareModeExt {
    fn read_opri(self) -> u8;
}

impl HardwareModeExt for HardwareMode {
    fn read_opri(self) -> u8 {
        // OPRI: Object priority
        // This CGB-only register is not writable by software; it should read $FE (?) on CGB and $FF on DMG
        match self {
            Self::Dmg => 0xFF,
            Self::Cgb => 0xFE,
        }
    }
}

pub struct Bus<'a> {
    pub hardware_mode: HardwareMode,
    pub ppu: &'a mut Ppu,
    pub apu: &'a mut Apu,
    pub memory: &'a mut Memory,
    pub serial_port: &'a mut SerialPort,
    pub cartridge: &'a mut Cartridge,
    pub interrupt_registers: &'a mut InterruptRegisters,
    pub speed_register: &'a mut SpeedRegister,
    pub timer: &'a mut GbTimer,
    pub dma_unit: &'a mut DmaUnit,
    pub input_state: &'a mut InputState,
}

macro_rules! cgb_only_read {
    ($bus:ident.$($op:tt)*) => {
        match $bus.hardware_mode {
            HardwareMode::Dmg => 0xFF,
            HardwareMode::Cgb => $bus.$($op)*,
        }
    }
}

macro_rules! cgb_only_write {
    ($bus:ident.$($op:tt)*) => {
        if $bus.hardware_mode == HardwareMode::Cgb {
            $bus.$($op)*;
        }
    }
}

impl<'a> Bus<'a> {
    fn read_io_register(&self, address: u16) -> u8 {
        log::trace!("I/O register read: {address:04X}");

        match address & 0x7F {
            0x00 => self.input_state.read_joyp(),
            0x02 => self.serial_port.read_control(),
            0x04 => self.timer.read_div(),
            0x05 => self.timer.read_tima(),
            0x06 => self.timer.read_tma(),
            0x07 => self.timer.read_tac(),
            0x0F => self.interrupt_registers.read_if(),
            0x10..=0x3F => self.apu.read_register(address),
            0x40..=0x45 | 0x47..=0x4B | 0x4F | 0x68..=0x6B => self.ppu.read_register(address),
            0x46 => self.dma_unit.read_dma_register(),
            0x4D => cgb_only_read!(self.speed_register.read_key1()),
            0x55 => cgb_only_read!(self.dma_unit.read_hdma5()),
            0x6C => self.hardware_mode.read_opri(),
            0x70 => cgb_only_read!(self.memory.read_svbk()),
            _ => 0xFF,
        }
    }

    fn write_io_register(&mut self, address: u16, value: u8) {
        log::trace!("I/O register write: {address:04X} {value:02X}");

        match address & 0x7F {
            0x00 => self.input_state.write_joyp(value),
            0x02 => self.serial_port.write_control(value),
            0x04 => self.timer.write_div(),
            0x05 => self.timer.write_tima(value),
            0x06 => self.timer.write_tma(value),
            0x07 => self.timer.write_tac(value),
            0x0F => self.interrupt_registers.write_if(value),
            0x10..=0x3F => self.apu.write_register(address, value),
            0x40..=0x45 | 0x47..=0x4B | 0x4F | 0x68..=0x6B => {
                self.ppu.write_register(address, value, self.interrupt_registers);
            }
            0x46 => self.dma_unit.write_dma_register(value),
            0x4D => cgb_only_write!(self.speed_register.write_key1(value)),
            0x51 => cgb_only_write!(self.dma_unit.write_hdma1(value)),
            0x52 => cgb_only_write!(self.dma_unit.write_hdma2(value)),
            0x53 => cgb_only_write!(self.dma_unit.write_hdma3(value)),
            0x54 => cgb_only_write!(self.dma_unit.write_hdma4(value)),
            0x55 => cgb_only_write!(self.dma_unit.write_hdma5(value, self.ppu.mode())),
            0x70 => cgb_only_write!(self.memory.write_svbk(value)),
            _ => {}
        }
    }

    fn tick_components(&mut self) {
        self.timer.tick_m_cycle(self.interrupt_registers);
        self.dma_unit.oam_dma_tick_m_cycle(self.cartridge, self.memory, self.ppu);
        self.serial_port.tick(self.interrupt_registers);

        if self.speed_register.speed == CpuSpeed::Double {
            self.speed_register.double_speed_odd_cycle =
                !self.speed_register.double_speed_odd_cycle;
            if self.speed_register.double_speed_odd_cycle {
                return;
            }
        }

        for _ in 0..2 {
            self.dma_unit.vram_dma_copy_byte(self.cartridge, self.memory, self.ppu);
        }

        for _ in 0..4 {
            self.ppu.tick_dot(self.dma_unit, self.interrupt_registers);
        }

        self.apu.tick_m_cycle(self.timer, self.speed_register.speed);
    }
}

impl<'a> BusInterface for Bus<'a> {
    fn read(&mut self, address: u16) -> u8 {
        self.tick_components();

        match address {
            0x0000..=0x7FFF => self.cartridge.read_rom(address),
            0x8000..=0x9FFF => self.ppu.read_vram(address),
            0xA000..=0xBFFF => self.cartridge.read_ram(address),
            0xC000..=0xFDFF => self.memory.read_main_ram(address),
            0xFE00..=0xFE9F => {
                if !self.dma_unit.oam_dma_in_progress() {
                    self.ppu.read_oam(address)
                } else {
                    0xFF
                }
            }
            // Unusable memory
            0xFEA0..=0xFEFF => 0xFF,
            0xFF00..=0xFF7F => self.read_io_register(address),
            0xFF80..=0xFFFE => self.memory.read_hram(address),
            0xFFFF => self.interrupt_registers.read_ie(),
        }
    }

    fn write(&mut self, address: u16, value: u8) {
        self.tick_components();

        match address {
            0x0000..=0x7FFF => self.cartridge.write_rom(address, value),
            0x8000..=0x9FFF => self.ppu.write_vram(address, value),
            0xA000..=0xBFFF => self.cartridge.write_ram(address, value),
            0xC000..=0xFDFF => self.memory.write_main_ram(address, value),
            0xFE00..=0xFE9F => {
                if !self.dma_unit.oam_dma_in_progress() {
                    self.ppu.write_oam(address, value);
                }
            }
            // Unusable memory
            0xFEA0..=0xFEFF => {}
            0xFF00..=0xFF7F => self.write_io_register(address, value),
            0xFF80..=0xFFFE => self.memory.write_hram(address, value),
            0xFFFF => self.interrupt_registers.write_ie(value),
        }
    }

    fn idle(&mut self) {
        self.tick_components();
    }

    fn read_ie_register(&self) -> u8 {
        self.interrupt_registers.read_ie() & 0x1F
    }

    fn read_if_register(&self) -> u8 {
        self.interrupt_registers.read_if() & 0x1F
    }

    fn acknowledge_interrupt(&mut self, interrupt_type: InterruptType) {
        self.interrupt_registers.clear_flag(interrupt_type);
    }

    fn halt(&self) -> bool {
        self.dma_unit.vram_dma_active()
    }

    fn speed_switch_armed(&self) -> bool {
        self.speed_register.switch_armed
    }

    fn perform_speed_switch(&mut self) {
        self.speed_register.perform_speed_switch();
    }
}
