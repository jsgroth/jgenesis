pub trait BusInterface {
    fn read_byte(&mut self, address: u32) -> u8;

    fn read_halfword(&mut self, address: u32) -> u16;

    fn read_word(&mut self, address: u32) -> u32;

    fn write_byte(&mut self, address: u32, value: u8);

    fn write_halfword(&mut self, address: u32, value: u16);

    fn write_word(&mut self, address: u32, value: u32);
}
