//! Detection for games that are known to use EEPROM chips, which require knowing how the EEPROM
//! chip is mapped into the cartridge's address space
//!
//! List of games and metadata from this thread:
//! <https://gendev.spritesmind.net/forum/viewtopic.php?f=25&t=206>

use crc::Crc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EepromType {
    X24C01,
    X24C02,
    X24C08,
    X24C16,
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

const CRC: Crc<u32> = Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);

pub fn eeprom(rom: &[u8]) -> Option<EepromMetadata> {
    let serial_number: String = rom[0x183..0x18B].iter().map(|&b| b as char).collect();
    match serial_number.as_str() {
        // NBA Jam (UE)
        // NBA Jam (J)
        "T-081326" | "T-81033 " => Some(NBA_JAM_METADATA),
        // NBA Jam Tournament Edition (JUE)
        // NFL Quarterback Club (JUE)
        "T-81406 " | "T-081276" => Some(ACCLAIM_24C02_METADATA),
        // NFL Quarterback Club '96 (UE)
        "T-081586" => Some(ACCLAIM_24C16_METADATA),
        // Mega Man: The Wily Wars (E)
        // Rockman: Mega World (J)
        // Evander "Real Deal" Holyfield's Boxing (JUE)
        // Greatest Heavyweights of the Ring (U)
        // Greatest Heavyweights of the Ring (J)
        // Greatest Heavyweights of the Ring (E)
        // Wonder Boy in Monster World (UE)
        // Wonder Boy V: Monster World III (J)
        "T-12046 " | "T-12053 " | "MK-1215 " | "MK-1228 " | "G-5538  " | "PR-1993 "
        | "G-4060  " => Some(SEGA_CAPCOM_METADATA),
        // NHLPA Hockey '93 (UE)
        // Rings of Power (UE)
        "T-50396 " | "T-50176 " => Some(EA_METADATA),
        _ => {
            // Micro Machines 2: Turbo Tournament (E)
            if is_micro_machines_2(rom) {
                return Some(CODEMASTERS_24C08_METADATA);
            }

            let mut crc_digest = CRC.digest();
            crc_digest.update(rom);
            let checksum = crc_digest.finalize();
            log::info!("ROM CRC32: {checksum:08X}");

            match checksum {
                // Micro Machines: Military (E)
                0xB3ABB15E => Some(CODEMASTERS_24C08_METADATA),
                // Micro Machines: Turbo Tournament 96 (E)
                0x23319D0D => Some(CODEMASTERS_24C16_METADATA),
                _ => None,
            }
        }
    }
}

pub fn is_micro_machines_2(rom: &[u8]) -> bool {
    let extended_serial: String = rom[0x183..0x18E].iter().map(|&b| b as char).collect();
    extended_serial.as_str() == "T-120096-50"
}
