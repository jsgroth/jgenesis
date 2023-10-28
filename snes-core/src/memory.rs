use bincode::{Decode, Encode};
use jgenesis_traits::num::GetBit;

const MAIN_RAM_LEN: usize = 128 * 1024;

type MainRam = [u8; MAIN_RAM_LEN];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CartridgeLocation {
    Rom(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum Mapper {
    LoRom,
}

impl Mapper {
    fn guess_from_rom(_rom: &[u8]) -> Option<Self> {
        // TODO actually try to guess the mapper
        Some(Mapper::LoRom)
    }

    fn map_address(self, address: u32) -> CartridgeLocation {
        match self {
            Self::LoRom => {
                // TODO handle SRAM
                let rom_addr = ((address & 0xFF0000) >> 1) | (address & 0x007FFF);
                CartridgeLocation::Rom(rom_addr)
            }
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct Cartridge {
    rom: Box<[u8]>,
    mapper: Mapper,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum Memory2Speed {
    Fast,
    #[default]
    Slow,
}

impl Memory2Speed {
    fn from_byte(byte: u8) -> Self {
        if byte.bit(0) { Self::Fast } else { Self::Slow }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Memory {
    cartridge: Cartridge,
    main_ram: Box<MainRam>,
    wram_port_address: u32,
}

impl Memory {
    pub fn from_rom(rom: Vec<u8>) -> Self {
        let mapper = Mapper::guess_from_rom(&rom).expect("unable to determine mapper");
        let cartridge = Cartridge { rom: rom.into_boxed_slice(), mapper };

        Self {
            cartridge,
            main_ram: vec![0; MAIN_RAM_LEN].into_boxed_slice().try_into().unwrap(),
            wram_port_address: 0,
        }
    }

    pub fn read_cartridge(&mut self, address: u32) -> u8 {
        match self.cartridge.mapper.map_address(address) {
            CartridgeLocation::Rom(rom_addr) => {
                // TODO figure out mirroring for unusual ROM sizes
                self.cartridge.rom[(rom_addr as usize) % self.cartridge.rom.len()]
            }
        }
    }

    pub fn write_cartridge(&mut self, address: u32, value: u8) {
        todo!("write cartridge {address:06X} {value:02X}")
    }

    pub fn read_wram(&self, address: u32) -> u8 {
        self.main_ram[(address as usize) & (MAIN_RAM_LEN - 1)]
    }

    pub fn write_wram(&mut self, address: u32, value: u8) {
        self.main_ram[(address as usize) & (MAIN_RAM_LEN - 1)] = value;
    }

    pub fn read_wram_port(&mut self) -> u8 {
        let value = self.main_ram[self.wram_port_address as usize];
        self.increment_wram_port_address();
        value
    }

    pub fn write_wram_port(&mut self, value: u8) {
        self.main_ram[self.wram_port_address as usize] = value;
        self.increment_wram_port_address();
    }

    fn increment_wram_port_address(&mut self) {
        self.wram_port_address = (self.wram_port_address + 1) & ((MAIN_RAM_LEN - 1) as u32);
    }

    pub fn write_wram_port_address_low(&mut self, value: u8) {
        self.wram_port_address = (self.wram_port_address & 0xFFFF00) | u32::from(value);
    }

    pub fn write_wram_port_address_mid(&mut self, value: u8) {
        self.wram_port_address = (self.wram_port_address & 0xFF00FF) | (u32::from(value) << 8);
    }

    pub fn write_wram_port_address_high(&mut self, value: u8) {
        // Only 1 bit used from high byte
        self.wram_port_address =
            (self.wram_port_address & 0x00FFFF) | (u32::from(value & 0x01) << 16);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum IrqMode {
    // No IRQs
    #[default]
    Off,
    // IRQ at H=HTIME, every line
    H,
    // IRQ at V=VTIME + H=0
    V,
    // IRQ at V=VTIME + H=HTIME
    HV,
}

impl IrqMode {
    fn from_byte(byte: u8) -> Self {
        match byte & 0x30 {
            0x00 => Self::Off,
            0x10 => Self::H,
            0x20 => Self::V,
            0x30 => Self::HV,
            _ => unreachable!("value & 0x30 will always be one of the above values"),
        }
    }
}

// Registers/ports that are on the 5A22 chip but are not part of the 65816
#[derive(Debug, Clone, Encode, Decode)]
pub struct CpuInternalRegisters {
    nmi_enabled: bool,
    irq_mode: IrqMode,
    auto_joypad_read_enabled: bool,
    irq_htime: u16,
    irq_vtime: u16,
    multiply_operand_l: u8,
    multiply_operand_r: u8,
    multiply_product: u16,
    division_dividend: u16,
    division_divisor: u8,
    division_quotient: u16,
    memory_2_speed: Memory2Speed,
    vblank_flag: bool,
    hblank_flag: bool,
    last_h: u16,
}

impl CpuInternalRegisters {
    pub fn new() -> Self {
        Self {
            nmi_enabled: false,
            irq_mode: IrqMode::default(),
            auto_joypad_read_enabled: false,
            irq_htime: 0,
            irq_vtime: 0,
            multiply_operand_l: 0xFF,
            multiply_operand_r: 0xFF,
            multiply_product: 0,
            division_dividend: 0xFFFF,
            division_divisor: 0xFF,
            division_quotient: 0,
            memory_2_speed: Memory2Speed::default(),
            vblank_flag: false,
            hblank_flag: false,
            last_h: 0,
        }
    }

    pub fn read_register(&mut self, address: u32) -> u8 {
        match address {
            0x4214 => {
                // RDDIVL: Division quotient, low byte
                self.division_quotient as u8
            }
            0x4215 => {
                // RDDIVH: Division quotient, high byte
                (self.division_quotient >> 8) as u8
            }
            0x4216 => {
                // RDMPYL: Multiply product / division remainder, low byte
                self.multiply_product as u8
            }
            0x4217 => {
                // RDMPYH: Multiply product / division remainder, high byte
                (self.multiply_product >> 8) as u8
            }
            _ => todo!("read register {address:06X}"),
        }
    }

    pub fn write_register(&mut self, address: u32, value: u8) {
        match address & 0xFFFF {
            0x4016 => {
                // JOYWR: Joypad output
                // TODO handle strobe in bit 0
            }
            0x4200 => {
                // NMITIMEN: Interrupt enable and joypad request
                self.auto_joypad_read_enabled = value.bit(0);
                self.irq_mode = IrqMode::from_byte(value);
                self.nmi_enabled = value.bit(7);
            }
            0x4201 => {
                // WRIO: Joypad programmable I/O port (write)
                // TODO implement this?
            }
            0x4202 => {
                // WRMPYA: Multiplication 8-bit operand A
                self.multiply_operand_l = value;
            }
            0x4203 => {
                // WRMPYB: Multiplication 8-bit operand B + start multiplication
                self.multiply_operand_r = value;

                // TODO delay setting the result? takes 8 CPU cycles on real hardware
                self.multiply_product = u16::from(self.multiply_operand_l) * u16::from(value);

                // Multiplication always writes operand B to the division quotient register
                self.division_quotient = value.into();
            }
            0x4204 => {
                // WRDIVL: Division 16-bit dividend, low byte
                self.division_dividend = (self.division_dividend & 0xFF00) | u16::from(value);
            }
            0x4205 => {
                // WRDIVH: Division 16-bit dividend, high byte
                self.division_dividend =
                    (self.division_dividend & 0x00FF) | (u16::from(value) << 8);
            }
            0x4206 => {
                // WRDIVB: Division 8-bit divisor + start division
                self.division_divisor = value;

                // TODO delay setting the result? takes 16 CPU cycles on real hardware
                if value != 0 {
                    self.division_quotient = self.division_dividend / u16::from(value);

                    // Division writes remainder to the multiply product register
                    self.multiply_product = self.division_dividend % u16::from(value);
                } else {
                    // Divide by 0 always sets quotient to $FFFF and remainder to dividend
                    self.division_quotient = 0xFFFF;
                    self.multiply_product = self.division_dividend;
                }
            }
            0x4207 => {
                // HTIMEL: H-count timer setting, low byte
                self.irq_htime = (self.irq_htime & 0xFF00) | u16::from(value);
            }
            0x4208 => {
                // HTIMEH: H-count timer setting, high byte (really just highest bit)
                self.irq_htime = (self.irq_htime & 0x00FF) | (u16::from(value & 0x01) << 8);
            }
            0x4209 => {
                // VTIMEL: V-count timer setting, low byte
                self.irq_vtime = (self.irq_vtime & 0xFF00) | u16::from(value);
            }
            0x420A => {
                // VTIMEH: V-count timer setting, high byte (really just highest bit)
                self.irq_vtime = (self.irq_vtime & 0x00FF) | (u16::from(value & 0x01) << 8);
            }
            0x420B => {
                // MDMAEN: Select general purpose DMA channels + start transfer
                if value != 0 {
                    todo!("GPDMA: {value:02X}")
                }
            }
            0x420C => {
                // HDMAEN: Select HBlank DMA channels + start transfer
                if value != 0 {
                    todo!("HDMA: {value:02X}")
                }
            }
            0x420D => {
                // MEMSEL: Memory-2 waitstate control
                self.memory_2_speed = Memory2Speed::from_byte(value);
            }
            _ => todo!("write register {address:06X} {value:02X}"),
        }
    }
}
