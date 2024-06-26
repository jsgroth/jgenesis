pub type M68kVectors = [u8; 256];

pub const M68K_VECTORS: &M68kVectors = include_bytes!("m68k_vectors.bin");
pub const SH2_MASTER: &[u8; 2048] = include_bytes!("sh2_master_boot_rom.bin");
pub const SH2_SLAVE: &[u8; 1024] = include_bytes!("sh2_slave_boot_rom.bin");
