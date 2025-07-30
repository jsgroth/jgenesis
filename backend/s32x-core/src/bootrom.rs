pub type M68kVectors = [u8; 256];

pub const SH2_MASTER: &[u8; 2048] = include_bytes!("sh2_master_boot_rom.bin");
pub const SH2_SLAVE: &[u8; 1024] = include_bytes!("sh2_slave_boot_rom.bin");

const M68K_VECTORS: &M68kVectors = include_bytes!("m68k_vectors.bin");

pub const fn new_m68k_vectors() -> M68kVectors {
    let mut vectors = *M68K_VECTORS;

    // 68K HINT vector is initialized to 0 - verified on hardware by testpico
    vectors[0x70] = 0;
    vectors[0x71] = 0;
    vectors[0x72] = 0;
    vectors[0x73] = 0;

    vectors
}
