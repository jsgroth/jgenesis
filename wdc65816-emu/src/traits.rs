pub trait BusInterface {
    const ADDRESS_MASK: u32 = 0xFFFFFF;

    fn read(&mut self, address: u32) -> u8;

    fn write(&mut self, address: u32, value: u8);

    fn idle(&mut self);

    fn nmi(&self) -> bool;

    fn acknowledge_nmi(&mut self);

    fn irq(&self) -> bool;

    fn halt(&self) -> bool;

    fn reset(&self) -> bool;
}
