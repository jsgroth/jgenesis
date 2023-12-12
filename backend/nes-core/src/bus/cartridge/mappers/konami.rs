mod irq;
mod vrc4;
mod vrc6;
mod vrc7;

use crate::bus::cartridge::mappers::{BankSizeKb, ChrType, NametableMirroring, PpuMapResult};
pub(crate) use vrc4::Vrc4;
pub(crate) use vrc6::Vrc6;
pub(crate) use vrc7::Vrc7;

fn map_ppu_address<N: Into<u32> + Copy>(
    address: u16,
    chr_banks: &[N; 8],
    chr_type: ChrType,
    nametable_mirroring: NametableMirroring,
) -> PpuMapResult {
    match address {
        0x0000..=0x1FFF => {
            let chr_bank_index = address / 0x0400;
            let chr_bank_number = chr_banks[chr_bank_index as usize];
            let chr_addr = BankSizeKb::One.to_absolute_address(chr_bank_number, address);
            chr_type.to_map_result(chr_addr)
        }
        0x2000..=0x3EFF => PpuMapResult::Vram(nametable_mirroring.map_to_vram(address)),
        0x3F00..=0xFFFF => panic!("invalid PPU map address: {address:04X}"),
    }
}
