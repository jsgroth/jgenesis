use crate::memory;

const BACKUP_RAM_LEN: usize = memory::BACKUP_RAM_LEN;
const RAM_CARTRIDGE_LEN: usize = memory::RAM_CARTRIDGE_LEN;

const BACKUP_RAM_FOOTER_LEN: usize = 64;

#[rustfmt::skip]
const BACKUP_RAM_FOOTER: [u8; BACKUP_RAM_FOOTER_LEN] = [
    // $1FC0-$1FCF
    0x5F, 0x5F, 0x5F, 0x5F, 0x5F, 0x5F, 0x5F, 0x5F, 0x5F, 0x5F, 0x5F, 0x00, 0x00, 0x00, 0x00, 0x40,
    // $1FD0-$1FDF
    0x00, 0x7D, 0x00, 0x7D, 0x00, 0x7D, 0x00, 0x7D, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // $1FE0-$1FEF
    0x53, 0x45, 0x47, 0x41, 0x5F, 0x43, 0x44, 0x5F, 0x52, 0x4F, 0x4D, 0x00, 0x01, 0x00, 0x00, 0x00,
    // $1FF0-$1FFF
    0x52, 0x41, 0x4D, 0x5F, 0x43, 0x41, 0x52, 0x54, 0x52, 0x49, 0x44, 0x47, 0x45, 0x5F, 0x5F, 0x5F,
];

#[rustfmt::skip]
const RAM_CARTRIDGE_FOOTER: [u8; BACKUP_RAM_FOOTER_LEN] = [
    // $1FFC0-$1FFCF
    0x5F, 0x5F, 0x5F, 0x5F, 0x5F, 0x5F, 0x5F, 0x5F, 0x5F, 0x5F, 0x5F, 0x00, 0x00, 0x00, 0x00, 0x40,
    // $1FFD0-$1FFDF
    0x07, 0xFD, 0x07, 0xFD, 0x07, 0xFD, 0x07, 0xFD, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // $1FFE0-$1FFEF
    0x53, 0x45, 0x47, 0x41, 0x5F, 0x43, 0x44, 0x5F, 0x52, 0x4F, 0x4D, 0x00, 0x01, 0x00, 0x00, 0x00,
    // $1FFF0-$1FFFF
    0x52, 0x41, 0x4D, 0x5F, 0x43, 0x41, 0x52, 0x54, 0x52, 0x49, 0x44, 0x47, 0x45, 0x5F, 0x5F, 0x5F,
];

// Boxing is desired because these boxed arrays will be large (8KB / 128KB)
#[allow(clippy::unnecessary_box_returns)]
fn new_formatted_backup_ram<const LEN: usize>(
    footer: &[u8; BACKUP_RAM_FOOTER_LEN],
) -> Box<[u8; LEN]> {
    // If no backup RAM was provided during construction, initialize it pre-formatted.
    // Some games (Popful Mail) blow up horribly if backup RAM is not formatted, so it's more
    // convenient to initialize it already formatted.
    let mut backup_ram: Box<[u8; LEN]> = vec![0; LEN].into_boxed_slice().try_into().unwrap();

    // Most of a freshly-formatted backup RAM is filled with 0s, but the last 64 bytes need to be
    // filled in
    backup_ram[LEN - BACKUP_RAM_FOOTER_LEN..].copy_from_slice(footer);

    backup_ram
}

pub fn load_initial_backup_ram(
    initial_backup_ram: Option<&Vec<u8>>,
    initial_ram_cartridge: Option<&Vec<u8>>,
) -> (Box<[u8; BACKUP_RAM_LEN]>, Box<[u8; RAM_CARTRIDGE_LEN]>) {
    let backup_ram: Box<[u8; BACKUP_RAM_LEN]> = match initial_backup_ram {
        Some(backup_ram) if backup_ram.len() == BACKUP_RAM_LEN => {
            backup_ram.clone().into_boxed_slice().try_into().unwrap()
        }
        Some(combined_ram) if combined_ram.len() == BACKUP_RAM_LEN + RAM_CARTRIDGE_LEN => {
            Vec::from(&combined_ram[..BACKUP_RAM_LEN]).into_boxed_slice().try_into().unwrap()
        }
        _ => new_formatted_backup_ram(&BACKUP_RAM_FOOTER),
    };

    // Prefer to load RAM cartridge from the initial RAM cartridge data, and fall back to looking for
    // it at the end of the initial backup RAM data
    let ram_cartridge: Box<[u8; RAM_CARTRIDGE_LEN]> =
        match (initial_backup_ram, initial_ram_cartridge) {
            (_, Some(ram_cartridge)) if ram_cartridge.len() == RAM_CARTRIDGE_LEN => {
                ram_cartridge.clone().into_boxed_slice().try_into().unwrap()
            }
            (Some(combined_ram), _) if combined_ram.len() == BACKUP_RAM_LEN + RAM_CARTRIDGE_LEN => {
                Vec::from(&combined_ram[BACKUP_RAM_LEN..]).into_boxed_slice().try_into().unwrap()
            }
            _ => new_formatted_backup_ram(&RAM_CARTRIDGE_FOOTER),
        };

    (backup_ram, ram_cartridge)
}
