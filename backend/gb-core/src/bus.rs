//! Game Boy bus / address mapping

use crate::HardwareMode;
use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::cgb::{CgbRegisters, CpuSpeed};
use crate::dma::DmaUnit;
use crate::inputs::InputState;
use crate::interrupts::InterruptRegisters;
use crate::memory::Memory;
use crate::ppu::Ppu;
use crate::serial::SerialPort;
use crate::sm83::InterruptType;
use crate::sm83::bus::BusInterface;
use crate::timer::GbTimer;

pub struct Bus<'a> {
    pub hardware_mode: HardwareMode,
    pub ppu: &'a mut Ppu,
    pub apu: &'a mut Apu,
    pub memory: &'a mut Memory,
    pub serial_port: &'a mut SerialPort,
    pub cartridge: &'a mut Cartridge,
    pub interrupt_registers: &'a mut InterruptRegisters,
    pub cgb_registers: &'a mut CgbRegisters,
    pub timer: &'a mut GbTimer,
    pub dma_unit: &'a mut DmaUnit,
    pub input_state: &'a mut InputState,
}

fn cgb_only_read(bus: &Bus<'_>, read_fn: impl FnOnce(&Bus<'_>) -> u8) -> u8 {
    match (bus.hardware_mode, bus.cgb_registers.dmg_compatibility) {
        (HardwareMode::Dmg, _) | (HardwareMode::Cgb, true) => 0xFF,
        (HardwareMode::Cgb, false) => read_fn(bus),
    }
}

fn cgb_only_write(bus: &mut Bus<'_>, write_fn: impl FnOnce(&mut Bus<'_>)) {
    // The CGB boot ROM depends on still being able to write to some CGB registers between writing
    // to KEY0 (enable DMG compatibility mode) and writing to BANK (unmap boot ROM)
    if bus.hardware_mode == HardwareMode::Cgb
        && (!bus.cgb_registers.dmg_compatibility || bus.memory.boot_rom_mapped())
    {
        write_fn(bus);
    }
}

fn cgb_boot_rom_only_write(bus: &mut Bus<'_>, write_fn: impl FnOnce(&mut Bus<'_>)) {
    if bus.hardware_mode == HardwareMode::Cgb && bus.memory.boot_rom_mapped() {
        write_fn(bus);
    }
}

impl Bus<'_> {
    fn read_io_register(&self, address: u16) -> u8 {
        log::trace!("I/O register read: {address:04X}");

        match address & 0x7F {
            0x00 => self.input_state.read_joyp(),
            0x01 => self.serial_port.read_data(),
            0x02 => self.serial_port.read_control(),
            0x04 => self.timer.read_div(),
            0x05 => self.timer.read_tima(),
            0x06 => self.timer.read_tma(),
            0x07 => self.timer.read_tac(),
            0x0F => self.interrupt_registers.read_if(),
            0x10..=0x3F => self.apu.read_register(address),
            0x40..=0x45 | 0x47..=0x4B => self.ppu.read_register(address),
            0x4F | 0x68..=0x6B => cgb_only_read(self, |bus| bus.ppu.read_register(address)),
            0x46 => self.dma_unit.read_dma_register(),
            0x4D => cgb_only_read(self, |bus| bus.cgb_registers.read_key1()),
            0x55 => cgb_only_read(self, |bus| bus.dma_unit.read_hdma5()),
            0x6C => cgb_only_read(self, |bus| bus.cgb_registers.read_opri()),
            0x70 => cgb_only_read(self, |bus| bus.memory.read_svbk()),
            0x76 => cgb_only_read(self, |bus| bus.apu.read_pcm12()),
            0x77 => cgb_only_read(self, |bus| bus.apu.read_pcm34()),
            _ => {
                log::debug!("Unexpected I/O register read: {address:04X}");
                0xFF
            }
        }
    }

    fn write_io_register(&mut self, address: u16, value: u8) {
        log::trace!("I/O register write: {address:04X} {value:02X}");

        match address & 0x7F {
            0x00 => self.input_state.write_joyp(value),
            0x01 => self.serial_port.write_data(value),
            0x02 => self.serial_port.write_control(value),
            0x04 => self.timer.write_div(),
            0x05 => self.timer.write_tima(value),
            0x06 => self.timer.write_tma(value),
            0x07 => self.timer.write_tac(value),
            0x0F => self.interrupt_registers.write_if(value),
            0x10..=0x3F => self.apu.write_register(address, value),
            0x40..=0x45 | 0x47..=0x4B => self.write_ppu_register(address, value),
            0x4F | 0x68..=0x6B => {
                cgb_only_write(self, |bus| bus.write_ppu_register(address, value));
            }
            0x46 => self.dma_unit.write_dma_register(value),
            0x4C => cgb_boot_rom_only_write(self, |bus| bus.cgb_registers.write_key0(value)),
            0x4D => cgb_only_write(self, |bus| bus.cgb_registers.write_key1(value)),
            0x50 => self.memory.write_bank(value),
            0x51 => cgb_only_write(self, |bus| bus.dma_unit.write_hdma1(value)),
            0x52 => cgb_only_write(self, |bus| bus.dma_unit.write_hdma2(value)),
            0x53 => cgb_only_write(self, |bus| bus.dma_unit.write_hdma3(value)),
            0x54 => cgb_only_write(self, |bus| bus.dma_unit.write_hdma4(value)),
            0x55 => cgb_only_write(self, |bus| bus.dma_unit.write_hdma5(value, bus.ppu.mode())),
            0x6C => cgb_boot_rom_only_write(self, |bus| bus.cgb_registers.write_opri(value)),
            0x70 => cgb_only_write(self, |bus| bus.memory.write_svbk(value)),
            _ => {
                log::debug!("Unexpected I/O register write: {address:04X} {value:02X}");
            }
        }
    }

    fn write_ppu_register(&mut self, address: u16, value: u8) {
        self.ppu.write_register(address, value, self.cgb_registers.speed, self.interrupt_registers);
    }

    fn tick_components(&mut self) {
        self.timer.tick_m_cycle(self.interrupt_registers);
        self.dma_unit.oam_dma_tick_m_cycle(self.cartridge, self.memory, self.ppu);
        self.serial_port.tick(self.interrupt_registers);

        if self.cgb_registers.speed == CpuSpeed::Double {
            self.cgb_registers.double_speed_odd_cycle = !self.cgb_registers.double_speed_odd_cycle;
            if self.cgb_registers.double_speed_odd_cycle {
                return;
            }
        }

        for _ in 0..2 {
            self.dma_unit.vram_dma_copy_byte(self.cartridge, self.memory, self.ppu);
        }

        for _ in 0..4 {
            self.ppu.tick_dot(*self.cgb_registers, self.dma_unit, self.interrupt_registers);
        }

        self.apu.tick_m_cycle(self.timer, self.cgb_registers.speed);

        self.cartridge.tick_cpu();
    }
}

impl BusInterface for Bus<'_> {
    fn read(&mut self, address: u16) -> u8 {
        self.tick_components();

        match address {
            0x0000..=0x7FFF => self
                .memory
                .try_read_boot_rom(address)
                .unwrap_or_else(|| self.cartridge.read_rom(address)),
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
        self.cgb_registers.speed_switch_armed
    }

    fn perform_speed_switch(&mut self) {
        self.cgb_registers.perform_speed_switch();
    }
}
