#![allow(clippy::needless_range_loop)]

use std::sync::OnceLock;

const PI: f64 = std::f64::consts::PI;

type Cx4Rom = [u32; 1024];

pub(super) fn read(address: u32) -> u32 {
    static ROM: OnceLock<Box<Cx4Rom>> = OnceLock::new();

    let rom = ROM.get_or_init(|| {
        let mut rom: Box<Cx4Rom> = vec![0; 1024].into_boxed_slice().try_into().unwrap();

        populate_div(&mut rom);
        populate_sqrt(&mut rom);
        populate_sin(&mut rom);
        populate_asin(&mut rom);
        populate_tan(&mut rom);
        populate_cos(&mut rom);

        rom
    });
    rom[(address & 0x3FF) as usize]
}

fn populate_div(rom: &mut Cx4Rom) {
    // Division by 0 produces $FFFFFF (overflow)
    rom[0] = 0xFFFFFF;

    for i in 0x001..0x100 {
        rom[i as usize] = 0x800000 / i;
    }
}

fn populate_sqrt(rom: &mut Cx4Rom) {
    for i in 0x100..0x200 {
        let input = i - 0x100;
        let scaled_sqrt = f64::from(0x100000) * (input as f64).sqrt();
        rom[i] = (scaled_sqrt as u32) & 0xFFFFFF;
    }
}

fn populate_sin(rom: &mut Cx4Rom) {
    for i in 0x200..0x280 {
        let input = i - 0x200;
        let scaled_sin = f64::from(0x1000000) * (input as f64 / 256.0 * PI).sin();
        rom[i] = (scaled_sin as u32) & 0xFFFFFF;
    }
}

fn populate_asin(rom: &mut Cx4Rom) {
    for i in 0x280..0x300 {
        let input = i - 0x280;
        let scaled_asin = f64::from(0x800000) / (PI / 2.0) * (input as f64 / 128.0).asin();
        rom[i] = (scaled_asin as u32) & 0xFFFFFF;
    }
}

fn populate_tan(rom: &mut Cx4Rom) {
    for i in 0x300..0x380 {
        let input = i - 0x300;
        let scaled_tan = f64::from(0x10000) * (input as f64 / 256.0 * PI).tan();
        rom[i] = (scaled_tan as u32) & 0xFFFFFF;
    }

    // tan(pi / 4) is defined as 1; without this the value will be ~0.9999999
    rom[0x340] = 0x010000;
}

fn populate_cos(rom: &mut Cx4Rom) {
    // cos(0) produces $FFFFFF (overflow)
    rom[0x380] = 0xFFFFFF;

    for i in 0x381..0x400 {
        let input = i - 0x380;
        let scaled_cos = f64::from(0x1000000) * (input as f64 / 256.0 * PI).cos();
        rom[i] = (scaled_cos as u32) & 0xFFFFFF;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn div() {
        assert_eq!(read(0x000), 0xFFFFFF);
        assert_eq!(read(0x0FF), 0x008080);
    }

    #[test]
    fn sqrt() {
        assert_eq!(read(0x100), 0x000000);
        assert_eq!(read(0x1FF), 0xFF7FDF);
    }

    #[test]
    fn sin() {
        assert_eq!(read(0x200), 0x000000);
        assert_eq!(read(0x27F), 0xFFFB10);
    }

    #[test]
    fn asin() {
        assert_eq!(read(0x280), 0x000000);
        assert_eq!(read(0x2FF), 0x75CEB4);
    }

    #[test]
    fn tan() {
        assert_eq!(read(0x300), 0x000000);
        assert_eq!(read(0x37F), 0x517BB5);
    }

    #[test]
    fn cos() {
        assert_eq!(read(0x380), 0xFFFFFF);
        assert_eq!(read(0x3FF), 0x03243A);
    }

    #[test]
    fn sum() {
        let mut sum = 0;
        for i in 0..0x400 {
            let value = read(i);
            sum += value & 0xFF;
            sum += (value >> 8) & 0xFF;
            sum += (value >> 16) & 0xFF;
        }

        assert_eq!(sum, 0x054336);
    }
}
