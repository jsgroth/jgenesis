//! SH-2 bus interface
//!
//! Implementations can assume that all addresses are masked to the lowest 29 bits (`address & 0x1FFFFFFF`)
//! because the highest 3 bits are only used internally

pub trait BusInterface {
    fn read_byte(&mut self, address: u32) -> u8;

    fn read_word(&mut self, address: u32) -> u16;

    fn read_longword(&mut self, address: u32) -> u32;

    fn read_cache_line(&mut self, address: u32) -> [u32; 4];

    fn write_byte(&mut self, address: u32, value: u8);

    fn write_word(&mut self, address: u32, value: u16);

    fn write_longword(&mut self, address: u32, value: u32);

    /// The CPU will halt while this is `true` and then reset when it changes from `true` to `false`
    fn reset(&self) -> bool;

    /// Current external interrupt level from 0 to 15; 0 indicates no interrupt
    fn interrupt_level(&self) -> u8;

    /// DREQ line for DMA channel 0
    fn dma_request_0(&self) -> bool;

    /// DREQ line for DMA channel 1
    fn dma_request_1(&self) -> bool;

    /// Receive a byte from the serial interface, if any
    fn serial_rx(&mut self) -> Option<u8>;

    /// Transmit a byte to the serial interface
    fn serial_tx(&mut self, value: u8);
}
