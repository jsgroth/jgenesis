//! GBA cartridge / game pak code

mod eeprom;
mod flashrom;
mod gpio;
pub mod rtc;
pub mod solar;

use crate::cartridge::eeprom::{Eeprom8K, Eeprom512};
use crate::cartridge::flashrom::{FlashRom64K, FlashRom128K};
use crate::cartridge::gpio::GpioPort;
use crate::cartridge::rtc::SeikoRealTimeClock;
use crate::cartridge::solar::SolarSensor;
use crate::dma::TransferUnit;
use crate::interrupts::InterruptRegisters;
use bincode::{Decode, Encode};
use crc::Crc;
use gba_config::GbaSaveMemory;
use jgenesis_common::boxedarray::BoxedByteArray;
use jgenesis_common::debug::{DebugBytesView, DebugMemoryView};
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::{FakeDecode, FakeEncode, PartialClone};
use std::mem;
use std::ops::Deref;

const MAX_ROM_LEN: usize = 32 * 1024 * 1024;
const SRAM_LEN: usize = 32 * 1024;

const GAME_CODE_ADDRESS: usize = 0x00000AC;

const CRC: Crc<u32> = Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);

#[derive(Debug, FakeEncode, FakeDecode)]
struct Rom(Box<[u8]>);

impl Default for Rom {
    fn default() -> Self {
        Self(vec![].into_boxed_slice())
    }
}

impl Deref for Rom {
    type Target = Box<[u8]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, Encode, Decode)]
enum RwMemory {
    Unknown,
    Sram(BoxedByteArray<SRAM_LEN>),
    EepromUnknownSize,
    Eeprom512(Eeprom512),
    Eeprom8K(Eeprom8K),
    FlashRom64K(FlashRom64K),
    FlashRom128K(FlashRom128K),
    None,
}

impl RwMemory {
    fn new_sram(initial_save: Option<&Vec<u8>>) -> Self {
        let mut sram = BoxedByteArray::<SRAM_LEN>::new();

        if let Some(initial_save) = initial_save
            && initial_save.len() >= SRAM_LEN
        {
            sram.copy_from_slice(&initial_save[..SRAM_LEN]);
        } else {
            sram.fill(0xFF);
        }

        Self::Sram(sram)
    }

    fn new_flash_rom_64k(initial_save: Option<&Vec<u8>>) -> Self {
        Self::FlashRom64K(FlashRom64K::new(initial_save))
    }

    fn new_flash_rom_128k(initial_save: Option<&Vec<u8>>) -> Self {
        Self::FlashRom128K(FlashRom128K::new(initial_save))
    }

    fn from_forced_type(initial_save: Option<&Vec<u8>>, forced_type: GbaSaveMemory) -> Self {
        match forced_type {
            GbaSaveMemory::Sram => Self::new_sram(initial_save),
            GbaSaveMemory::EepromUnknownSize => Self::EepromUnknownSize,
            GbaSaveMemory::Eeprom512 => Self::Eeprom512(Eeprom512::new(initial_save)),
            GbaSaveMemory::Eeprom8K => Self::Eeprom8K(Eeprom8K::new(initial_save)),
            GbaSaveMemory::FlashRom64K => Self::new_flash_rom_64k(initial_save),
            GbaSaveMemory::FlashRom128K => Self::new_flash_rom_128k(initial_save),
            GbaSaveMemory::None => Self::None,
        }
    }

    fn initial(
        rom: &[u8],
        initial_save: Option<&Vec<u8>>,
        forced_type: Option<GbaSaveMemory>,
    ) -> Self {
        type RwMemoryFn = fn(Option<&Vec<u8>>) -> RwMemory;

        // If the given ASCII string exists in ROM and followed by 3 digits, assume that type
        // of save memory (e.g. "SRAM_V123" indicates SRAM)
        const MEMORY_STRINGS: &[(&[u8], &str, RwMemoryFn)] = &[
            (b"SRAM_V", "SRAM", RwMemory::new_sram),
            (b"EEPROM_V", "EEPROM", |_| RwMemory::EepromUnknownSize),
            (b"FLASH_V", "Flash ROM 64 KB", RwMemory::new_flash_rom_64k),
            (b"FLASH512_V", "Flash ROM 64 KB", RwMemory::new_flash_rom_64k),
            (b"FLASH1M_V", "Flash ROM 128 KB", RwMemory::new_flash_rom_128k),
        ];

        if let Some(forced_type) = forced_type {
            log::info!("Forcing save memory type to {}", forced_type.name());
            return Self::from_forced_type(initial_save, forced_type);
        }

        let rom_checksum = CRC.checksum(rom);
        if rom_checksum == 0xDFC88D3E {
            // Top Gun - Combat Zones (USA)
            // Acts like it has save memory but hangs at the main menu if it can successfully write
            // to anything
            log::info!("Disabling save memory due to CRC32 match: {rom_checksum:08X}");
            return Self::None;
        }

        for i in 0..rom.len() {
            for &(string, name, init_fn) in MEMORY_STRINGS {
                if i + string.len() + 3 > rom.len() {
                    continue;
                }

                if &rom[i..i + string.len()] != string {
                    continue;
                }

                if !(i + string.len()..i + string.len() + 3).all(|j| rom[j].is_ascii_digit()) {
                    continue;
                }

                log::info!(
                    "Auto-detected save memory type {name} from string in ROM at ${i:07X}: {}",
                    str::from_utf8(&rom[i..i + string.len() + 3]).unwrap()
                );

                return init_fn(initial_save);
            }
        }

        log::info!("No matching save memory string found in ROM; will auto-detect based on usage");

        Self::Unknown
    }

    fn min_eeprom_address(&self, rom_len: u32) -> u32 {
        // EEPROM is at $D000000-$DFFFFFF for ROMs <=16MB, and $DFFFF00-$DFFFFFF for 32MB
        match self {
            Self::EepromUnknownSize | Self::Eeprom8K(_) | Self::Eeprom512(_) => {
                if rom_len <= 16 * 1024 * 1024 { 0xD000000 } else { 0xDFFFF00 }
            }
            _ => u32::MAX,
        }
    }
}

fn pad_if_classic_nes_rom(rom: &mut Vec<u8>) {
    if rom.len() <= GAME_CODE_ADDRESS {
        return;
    }

    // Classic NES series games uniquely have a game code of 'F' (uppercase ASCII character)
    // Most other games have 'A' or 'B', some cartridges with special hardware use other characters
    if rom[GAME_CODE_ADDRESS] == b'F' {
        log::info!(
            "Detected Classic NES Series cartridge; padding to 32 MB to emulate ROM mirroring"
        );

        // Manually mirror ROM up to 32 MB
        while rom.len() < MAX_ROM_LEN {
            for i in 0..rom.len() {
                rom.push(rom[i]);
            }
        }
    }
}

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct Cartridge {
    #[partial_clone(default)]
    rom: Rom,
    rom_len: u32,
    burst_active: bool,
    burst_address: u32,
    rw_memory: RwMemory,
    rw_memory_dirty: bool,
    min_eeprom_address: u32,
    gpio: GpioPort,
    rtc: Option<SeikoRealTimeClock>,
    solar: Option<SolarSensor>,
    // Kept here so that they can be copied into R/W memory after auto-detection
    initial_save: Option<Vec<u8>>,
    initial_rtc: Option<SeikoRealTimeClock>,
}

impl Cartridge {
    pub fn new(
        mut rom: Vec<u8>,
        initial_save: Option<Vec<u8>>,
        initial_rtc: Option<SeikoRealTimeClock>,
        forced_save_memory_type: Option<GbaSaveMemory>,
    ) -> Self {
        jgenesis_common::rom::mirror_to_next_power_of_two(&mut rom);

        // Record ROM length before possibly mirroring; Classic NES Series games depend on this to
        // map EEPROM correctly
        let rom_len = rom.len() as u32;
        pad_if_classic_nes_rom(&mut rom);

        let rw_memory = RwMemory::initial(&rom, initial_save.as_ref(), forced_save_memory_type);
        let min_eeprom_address = rw_memory.min_eeprom_address(rom_len);

        let has_solar_sensor = rom.get(0xAC).copied() == Some(b'U');
        if has_solar_sensor {
            log::info!("Detected solar sensor peripheral; assuming RTC is also present");
        }

        // TODO allow forcing RTC?

        Self {
            rom: Rom(rom.into_boxed_slice()),
            rom_len,
            burst_active: false,
            burst_address: 0,
            rw_memory,
            rw_memory_dirty: false,
            min_eeprom_address,
            gpio: GpioPort::new(),
            rtc: has_solar_sensor
                .then(|| initial_rtc.clone().unwrap_or_else(SeikoRealTimeClock::new)),
            solar: has_solar_sensor.then(SolarSensor::new),
            initial_save,
            initial_rtc,
        }
    }

    pub fn rom_burst_active(&self) -> bool {
        self.burst_active
    }

    pub fn end_rom_burst(&mut self) {
        self.burst_active = false;
    }

    // Returns address that should be used for the ROM access
    #[must_use]
    fn update_burst_state(&mut self, address: u32) -> u32 {
        if !self.burst_active {
            // Non-sequential ROM accesses begin a burst and latch A1-16
            self.burst_active = true;
            self.burst_address = address & 0x1FFFE;
        } else if address & 0x1FFFE != self.burst_address {
            log::debug!(
                "Cartridge read address does not match burst address! {address:08X} {:05X}",
                self.burst_address
            );
        }

        let rom_addr = (address & !0x1FFFF) | self.burst_address;
        self.burst_address = (self.burst_address + 2) & 0x1FFFF;

        if address & 0x1FFFE == 0x1FFFE {
            // Bursts always end when the final halfword of a 128KB page is requested
            self.burst_active = false;
        }

        rom_addr
    }

    pub fn read_rom(&mut self, address: u32) -> u16 {
        debug_assert!((0x08000000..0x0E000000).contains(&address));

        let address = self.update_burst_state(address);

        if address >= self.min_eeprom_address
            && let Some(bit) = self.try_eeprom_read()
        {
            return bit.into();
        }

        if (0x080000C4..0x080000CA).contains(&address)
            && let Some(gpio_value) = self.try_gpio_read(address)
        {
            return gpio_value;
        }

        let rom_addr = (address as usize) & 0x1FFFFFF & !1;
        if rom_addr >= self.rom.len() {
            log::debug!(
                "Out of bounds cartridge ROM read {address:07X}, len {:07X}",
                self.rom.len()
            );
            let open_bus = rom_addr >> 1;
            return open_bus as u16;
        }

        u16::from_le_bytes(self.rom[rom_addr..rom_addr + 2].try_into().unwrap())
    }

    fn try_eeprom_read(&mut self) -> Option<bool> {
        match &mut self.rw_memory {
            RwMemory::Eeprom512(eeprom) => Some(eeprom.read()),
            RwMemory::Eeprom8K(eeprom) => Some(eeprom.read()),
            _ => None,
        }
    }

    fn try_gpio_read(&mut self, address: u32) -> Option<u16> {
        match address {
            0x080000C4 => self.gpio.read_data(self.rtc.as_ref(), self.solar.as_ref()),
            0x080000C6 => self.gpio.read_pin_directions(),
            0x080000C8 => self.gpio.read_mode(),
            _ => None,
        }
    }

    pub fn write_rom(&mut self, address: u32, value: u16) {
        debug_assert!((0x08000000..0x0E000000).contains(&address));

        let address = self.update_burst_state(address);

        if (0x080000C4..0x080000CA).contains(&address) {
            self.gpio_write(address, value);
            return;
        }

        if address < self.min_eeprom_address {
            log::debug!("Ignoring write to ROM address: {address:08X} {value:04X}");
            return;
        }

        self.rw_memory_dirty = true;

        match &mut self.rw_memory {
            RwMemory::Eeprom512(eeprom) => eeprom.write(value.bit(0)),
            RwMemory::Eeprom8K(eeprom) => eeprom.write(value.bit(0)),
            _ => {
                log::debug!("Ignoring write to ROM address: {address:08X} {value:04X}");
            }
        }
    }

    fn gpio_write(&mut self, address: u32, value: u16) {
        self.rtc.get_or_insert_with(|| {
            log::info!("Auto-detected RTC based on write to ${address:07X}");
            self.initial_rtc.clone().unwrap_or_else(SeikoRealTimeClock::new)
        });

        match address {
            0x080000C4 => self.gpio.write_data(value, self.rtc.as_mut(), self.solar.as_mut()),
            0x080000C6 => self.gpio.write_pin_directions(value),
            0x080000C8 => self.gpio.write_mode(value),
            _ => {}
        }
    }

    pub fn notify_dma_to_rom(&mut self, address: u32, length: u16, unit: TransferUnit) {
        if unit != TransferUnit::Halfword {
            // Word transfers will never be the correct length (and don't make sense for EEPROM anyway)
            return;
        }

        if !matches!(self.rw_memory, RwMemory::Unknown | RwMemory::EepromUnknownSize) {
            return;
        }

        // Check against original ROM length instead of possibly-padded length (Classic NES Series)
        let rom_len = self.rom_len;
        if address < RwMemory::EepromUnknownSize.min_eeprom_address(rom_len) {
            return;
        }

        match length {
            9 => {
                // 6-bit address; 512 B EEPROM
                self.rw_memory = RwMemory::Eeprom512(Eeprom512::new(self.initial_save.as_ref()));
                self.min_eeprom_address = self.rw_memory.min_eeprom_address(rom_len);

                log::info!(
                    "Auto-detected EEPROM size of 512 bytes from DMA of length {length} to ${address:07X}"
                );
            }
            17 => {
                // 14-bit address; 8 KB EEPROM
                self.rw_memory = RwMemory::Eeprom8K(Eeprom8K::new(self.initial_save.as_ref()));
                self.min_eeprom_address = self.rw_memory.min_eeprom_address(rom_len);

                log::info!(
                    "Auto-detected EEPROM size of 8 KB from DMA of length {length} to ${address:07X}"
                );
            }
            _ => {
                log::warn!("Unexpected initial EEPROM DMA length: {length}");
            }
        }
    }

    pub fn take_rom(&mut self) -> Vec<u8> {
        mem::take(&mut self.rom.0).into_vec()
    }

    pub fn take_rom_from(&mut self, other: &mut Self) {
        self.rom = mem::take(&mut other.rom);
    }

    pub fn read_sram(&mut self, address: u32) -> u8 {
        debug_assert!((0x0E000000..0x10000000).contains(&address));

        if matches!(self.rw_memory, RwMemory::Unknown) {
            self.rw_memory = RwMemory::new_sram(self.initial_save.as_ref());
            log::info!("Auto-detected save memory type as SRAM due to read ${address:08X}");
        }

        match &self.rw_memory {
            RwMemory::Sram(sram) => {
                let sram_addr = (address as usize) & (SRAM_LEN - 1);
                sram[sram_addr]
            }
            RwMemory::FlashRom64K(flash_rom) => flash_rom.read(address),
            RwMemory::FlashRom128K(flash_rom) => flash_rom.read(address),
            _ => {
                // SRAM area reads always return 0xFF when no SRAM or flash ROM is present
                // (save/none in jsmolka gba-tests)
                0xFF
            }
        }
    }

    pub fn write_sram(&mut self, address: u32, value: u8) {
        debug_assert!((0x0E000000..0x10000000).contains(&address));

        if matches!(self.rw_memory, RwMemory::Unknown) {
            if address & 0xFFFF == 0x5555 && value == 0xAA {
                // Probably Flash ROM
                // TODO is it possible to auto-detect 64K vs. 128K?
                self.rw_memory = RwMemory::new_flash_rom_128k(self.initial_save.as_ref());
                log::info!(
                    "Auto-detected save memory type as Flash ROM 64 KB due to write ${address:08X} 0x{value:02X}"
                );
            } else {
                // Probably SRAM
                self.rw_memory = RwMemory::new_sram(self.initial_save.as_ref());
                log::info!(
                    "Auto-detected save memory type as SRAM due to write ${address:08X} 0x{value:02X}"
                );
            }
        }

        self.rw_memory_dirty = true;

        match &mut self.rw_memory {
            RwMemory::Sram(sram) => {
                let sram_addr = (address as usize) & (SRAM_LEN - 1);
                sram[sram_addr] = value;
            }
            RwMemory::FlashRom64K(flash_rom) => flash_rom.write(address, value),
            RwMemory::FlashRom128K(flash_rom) => flash_rom.write(address, value),
            _ => {
                log::debug!("Unexpected SRAM address write {address:08X} {value:02X}");
            }
        }
    }

    pub fn take_rw_memory_dirty(&mut self) -> bool {
        mem::take(&mut self.rw_memory_dirty)
    }

    pub fn rw_memory(&self) -> Option<&[u8]> {
        match &self.rw_memory {
            RwMemory::Sram(sram) => Some(sram.as_slice()),
            RwMemory::Eeprom8K(eeprom) => Some(eeprom.memory()),
            RwMemory::Eeprom512(eeprom) => Some(eeprom.memory()),
            RwMemory::FlashRom64K(flash_rom) => Some(flash_rom.memory()),
            RwMemory::FlashRom128K(flash_rom) => Some(flash_rom.memory()),
            RwMemory::Unknown | RwMemory::EepromUnknownSize | RwMemory::None => None,
        }
    }

    pub fn update_rtc_time(&mut self, cycles: u64, interrupts: &mut InterruptRegisters) {
        if let Some(rtc) = &mut self.rtc {
            rtc.update_time(cycles, interrupts);
        }
    }

    pub fn rtc(&self) -> Option<impl Encode> {
        self.rtc.as_ref()
    }

    pub fn set_solar_brightness(&mut self, brightness: u8) {
        if let Some(solar) = &mut self.solar {
            solar.set_brightness(brightness);
        }
    }

    pub fn debug_rom_view(&mut self) -> impl DebugMemoryView {
        DebugBytesView(self.rom.0.as_mut())
    }
}
