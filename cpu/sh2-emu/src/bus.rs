pub trait BusInterface {
    fn read_byte(&mut self, address: u32) -> u8;

    fn read_word(&mut self, address: u32) -> u16;

    fn read_longword(&mut self, address: u32) -> u32;

    fn read_cache_line(&mut self, address: u32) -> [u32; 4];

    fn write_byte(&mut self, address: u32, value: u8);

    fn write_word(&mut self, address: u32, value: u16);

    fn write_longword(&mut self, address: u32, value: u32);

    fn reset(&self) -> bool;

    fn interrupt_level(&self) -> u8;

    fn dma_request_0(&self) -> bool;

    fn dma_request_1(&self) -> bool;

    fn serial_rx(&mut self) -> Option<u8>;

    fn serial_tx(&mut self, value: u8);
}
