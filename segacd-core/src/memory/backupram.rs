use crate::memory::BACKUP_RAM_LEN;

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

pub fn new_formatted_backup_ram() -> Box<[u8; BACKUP_RAM_LEN]> {
    // If no backup RAM was provided during construction, initialize it pre-formatted.
    // Some games (Popful Mail) blow up horribly if backup RAM is not formatted, so it's more
    // convenient to initialize it already formatted.
    let mut backup_ram: Box<[u8; BACKUP_RAM_LEN]> =
        vec![0; BACKUP_RAM_LEN].into_boxed_slice().try_into().unwrap();

    // Most of a freshly-formatted backup RAM is filled with 0s, but the last 64 bytes need to be
    // filled in
    backup_ram[BACKUP_RAM_LEN - BACKUP_RAM_FOOTER_LEN..].copy_from_slice(&BACKUP_RAM_FOOTER);

    backup_ram
}
