pub mod audio;
pub mod boxedarray;
pub mod frontend;
pub mod input;
pub mod num;
pub mod timeutils;

use cfg_if::cfg_if;
use std::thread;
use std::time::Duration;

#[inline]
pub fn sleep(duration: Duration) {
    cfg_if! {
        if #[cfg(target_os = "windows")] {
            // SAFETY: thread::sleep cannot panic, so timeEndPeriod will always be called after timeBeginPeriod.
            unsafe {
                windows::Win32::Media::timeBeginPeriod(1);
                thread::sleep(duration);
                windows::Win32::Media::timeEndPeriod(1);
            }
        } else {
            thread::sleep(duration);
        }
    }
}

#[inline]
#[must_use]
pub fn is_appimage_build() -> bool {
    option_env!("JGENESIS_APPIMAGE_BUILD").is_some_and(|var| !var.is_empty())
}

/// Mirror a ROM up to the next highest power of two.
///
/// For example, with a 5.5MB ROM, this would do the following to extend the ROM to 8MB:
///   1. Repeat the last 512KB, extending the ROM to 6MB
///   2. Repeat the last 2MB, extending the ROM to 8MB
pub fn mirror_to_power_of_two(rom: &mut Vec<u8>) {
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

    // Call recursively until the ROM size is a power of two
    mirror_to_power_of_two(rom);
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
        mirror_to_power_of_two(&mut rom);
        assert_eq!(rom, vec![]);
    }

    #[test]
    fn mirror_power_of_two() {
        let mut rom = new_vec::<8>();
        mirror_to_power_of_two(&mut rom);
        assert_eq!(rom, vec![0, 1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn mirror_6_to_8() {
        let mut rom = new_vec::<6>();
        mirror_to_power_of_two(&mut rom);
        assert_eq!(rom, vec![0, 1, 2, 3, 4, 5, 4, 5]);
    }

    #[test]
    fn mirror_5_to_8() {
        let mut rom = new_vec::<5>();
        mirror_to_power_of_two(&mut rom);
        assert_eq!(rom, vec![0, 1, 2, 3, 4, 4, 4, 4]);
    }

    #[test]
    fn mirror_11_to_16() {
        let mut rom = new_vec::<11>();
        mirror_to_power_of_two(&mut rom);
        assert_eq!(rom, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 10, 8, 9, 10, 10]);
    }
}
