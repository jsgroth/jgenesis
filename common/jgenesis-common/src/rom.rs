pub fn mirror_to_next_power_of_two(rom: &mut Vec<u8>) {
    if rom.is_empty() {
        log::error!("Cannot mirror empty ROM");
        return;
    }

    let ones_count = rom.len().count_ones();
    if ones_count == 1 {
        // ROM size is already a power of two
        return;
    }

    let trailing_zeroes = rom.len().trailing_zeros();
    let source_len = 1 << trailing_zeroes;
    let source_mask = source_len - 1;

    let remaining_rom_len = rom.len() & !source_len;
    let copy_len = (1 << (remaining_rom_len.trailing_zeros())) - source_len;

    log::debug!(
        "ROM len is {}; duplicating last {source_len} bytes of ROM to last {copy_len} bytes",
        rom.len()
    );

    let base_addr = rom.len() & !source_len;
    for i in 0..copy_len {
        rom.push(rom[base_addr + (i & source_mask)]);
    }

    // Recurse in case there are more than 2 ROM chips (e.g. fan translated version of Daikaijuu
    // Monogatari II which is 5.5MB)
    mirror_to_next_power_of_two(rom);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::array;

    fn new_vec<const LEN: usize>() -> Vec<u8> {
        Vec::from(array::from_fn::<u8, LEN, _>(|i| i as u8))
    }

    #[test]
    fn mirror_empty_rom() {
        let mut rom = vec![];
        mirror_to_next_power_of_two(&mut rom);
        assert_eq!(rom, vec![]);
    }

    #[test]
    fn mirror_power_of_two() {
        let mut rom = new_vec::<8>();
        mirror_to_next_power_of_two(&mut rom);
        assert_eq!(rom, vec![0, 1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn mirror_6_to_8() {
        let mut rom = new_vec::<6>();
        mirror_to_next_power_of_two(&mut rom);
        assert_eq!(rom, vec![0, 1, 2, 3, 4, 5, 4, 5]);
    }

    #[test]
    fn mirror_5_to_8() {
        let mut rom = new_vec::<5>();
        mirror_to_next_power_of_two(&mut rom);
        assert_eq!(rom, vec![0, 1, 2, 3, 4, 4, 4, 4]);
    }

    #[test]
    fn mirror_11_to_16() {
        let mut rom = new_vec::<11>();
        mirror_to_next_power_of_two(&mut rom);
        assert_eq!(rom, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 10, 8, 9, 10, 10]);
    }
}
