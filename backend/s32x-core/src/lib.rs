pub mod api;
mod audio;
mod bootrom;
mod bus;
mod core;
mod pwm;
mod registers;
mod vdp;

// The security program is located at $36C-$76B in the master SH-2 boot ROM. The 32X will refuse to
// boot any cartridge where $400-$7FF in cartridge ROM isn't an exact match for this security program.
//
// This can be used to auto-detect whether ROM files with generic extensions (e.g. .bin) are 32X ROMs
pub const SECURITY_PROGRAM_CARTRIDGE_ADDR: usize = 0x400;
pub const SECURITY_PROGRAM_LEN: usize = 0x400;

#[inline]
#[must_use]
pub fn security_program() -> &'static [u8] {
    &bootrom::SH2_MASTER[0x36C..0x36C + SECURITY_PROGRAM_LEN]
}
