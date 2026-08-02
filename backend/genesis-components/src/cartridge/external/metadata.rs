//! Detection for games that are known to use EEPROM chips, which require knowing how the EEPROM
//! chip is mapped into the cartridge's address space
//!
//! List of games and metadata from this thread:
//! <https://gendev.spritesmind.net/forum/viewtopic.php?f=25&t=206>

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EepromType {
    X24C01,
    X24C02,
    X24C08,
    X24C16,
    X24C64,
}

#[derive(Debug, Clone)]
pub struct EepromMetadata {
    pub eeprom_type: EepromType,
    pub sda_in_addr: u32,
    pub sda_in_bit: u8,
    pub sda_out_addr: u32,
    pub sda_out_bit: u8,
    pub scl_addr: u32,
    pub scl_bit: u8,
}

// Values from https://gendev.spritesmind.net/forum/viewtopic.php?f=25&t=206

const NBA_JAM_METADATA: EepromMetadata = EepromMetadata {
    eeprom_type: EepromType::X24C02,
    sda_in_addr: 0x200000,
    sda_in_bit: 0,
    sda_out_addr: 0x200000,
    sda_out_bit: 1,
    scl_addr: 0x200000,
    scl_bit: 1,
};

const ACCLAIM_24C02_METADATA: EepromMetadata = EepromMetadata {
    eeprom_type: EepromType::X24C02,
    sda_in_addr: 0x200001,
    sda_in_bit: 0,
    sda_out_addr: 0x200001,
    sda_out_bit: 0,
    scl_addr: 0x200000,
    scl_bit: 0,
};

const ACCLAIM_24C16_METADATA: EepromMetadata = EepromMetadata {
    eeprom_type: EepromType::X24C16,
    sda_in_addr: 0x200001,
    sda_in_bit: 0,
    sda_out_addr: 0x200001,
    sda_out_bit: 0,
    scl_addr: 0x200000,
    scl_bit: 0,
};

const ACCLAIM_24C64_METADATA: EepromMetadata = EepromMetadata {
    eeprom_type: EepromType::X24C64,
    sda_in_addr: 0x200001,
    sda_in_bit: 0,
    sda_out_addr: 0x200001,
    sda_out_bit: 0,
    scl_addr: 0x200000,
    scl_bit: 0,
};

const SEGA_CAPCOM_METADATA: EepromMetadata = EepromMetadata {
    eeprom_type: EepromType::X24C01,
    sda_in_addr: 0x200001,
    sda_in_bit: 0,
    sda_out_addr: 0x200001,
    sda_out_bit: 0,
    scl_addr: 0x200001,
    scl_bit: 1,
};

const EA_METADATA: EepromMetadata = EepromMetadata {
    eeprom_type: EepromType::X24C01,
    sda_in_addr: 0x200000,
    sda_in_bit: 7,
    sda_out_addr: 0x200000,
    sda_out_bit: 7,
    scl_addr: 0x200000,
    scl_bit: 6,
};

const CODEMASTERS_24C08_METADATA: EepromMetadata = EepromMetadata {
    eeprom_type: EepromType::X24C08,
    sda_in_addr: 0x300000,
    sda_in_bit: 0,
    sda_out_addr: 0x380001,
    sda_out_bit: 7,
    scl_addr: 0x300000,
    scl_bit: 1,
};

const CODEMASTERS_24C16_METADATA: EepromMetadata = EepromMetadata {
    eeprom_type: EepromType::X24C16,
    sda_in_addr: 0x300000,
    sda_in_bit: 0,
    sda_out_addr: 0x380001,
    sda_out_bit: 7,
    scl_addr: 0x300000,
    scl_bit: 1,
};

// Mostly from https://gendev.spritesmind.net/forum/viewtopic.php?p=2485#p2485
#[rustfmt::skip]
const SERIAL_NUMBER_TO_METADATA: &[(&[u8], EepromMetadata)] = &[
    (b"G-4060  ", SEGA_CAPCOM_METADATA),   // Wonder Boy in Monster World (U/E)
    (b"PR-1993 ", SEGA_CAPCOM_METADATA),   // Wonder Boy V: Monster World III (J)
    (b"T-12046 ", SEGA_CAPCOM_METADATA),   // Mega Man: The Wily Wars (E)
    (b"T-12053 ", SEGA_CAPCOM_METADATA),   // Rockman Mega World (J)
    (b"MK-1215 ", SEGA_CAPCOM_METADATA),   // Evander Holyfield's "Real Deal" Boxing (World)
    (b"MK-1228 ", SEGA_CAPCOM_METADATA),   // Greatest Heavyweights (U/E)
    (b"G-5538  ", SEGA_CAPCOM_METADATA),   // Greatest Heavyweights (J)
    (b"T-081326", NBA_JAM_METADATA),       // NBA Jam (U/E)
    (b"T-81033 ", NBA_JAM_METADATA),       // NBA Jam (J)
    (b"T-81406 ", ACCLAIM_24C02_METADATA), // NBA Jam Tournament Edition (World)
    (b"T-8104B ", ACCLAIM_24C02_METADATA), // NBA Jam Tournament Edition (32X) (World)
    (b"T-081276", ACCLAIM_24C02_METADATA), // NFL Quarterback Club (World)
    (b"T-8102B ", ACCLAIM_24C02_METADATA), // NFL Quarterback Club (32X) (World)
    (b"T-081586", ACCLAIM_24C16_METADATA), // NFL Quarterback Club 96 (U/E)
    (b"T-81576 ", ACCLAIM_24C64_METADATA), // College Slam (U)
    (b"T-81476 ", ACCLAIM_24C64_METADATA), // Frank Thomas Big Hurt Baseball (U/E)
    (b"T-50176 ", EA_METADATA),            // Rings of Power (U/E)
    (b"T-50396 ", EA_METADATA),            // NHLPA Hockey 93 (U/E)
];

pub fn eeprom(rom: &[u8], checksum: u32) -> Option<EepromMetadata> {
    let rom_serial_number = &rom[0x183..0x18B];
    for (serial_number, metadata) in SERIAL_NUMBER_TO_METADATA {
        if rom_serial_number == *serial_number {
            return Some(metadata.clone());
        }
    }

    // Micro Machines 2: Turbo Tournament (E)
    if is_micro_machines_2(rom) {
        return Some(CODEMASTERS_24C08_METADATA);
    }

    match checksum {
        // Micro Machines: Military (E)
        0xB3ABB15E => Some(CODEMASTERS_24C08_METADATA),
        // Micro Machines: Turbo Tournament 96 (E)
        0x23319D0D => Some(CODEMASTERS_24C16_METADATA),
        // Honoo no Toukyuuji - Dodge Danpei (J)
        0x630F07C6 => Some(SEGA_CAPCOM_METADATA),
        _ => None,
    }
}

pub fn is_micro_machines_2(rom: &[u8]) -> bool {
    let extended_serial = &rom[0x183..0x18E];
    extended_serial == b"T-120096-50"
}
