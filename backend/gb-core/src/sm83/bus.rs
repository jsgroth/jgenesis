use crate::sm83::InterruptType;

pub trait BusInterface {
    fn read(&mut self, address: u16) -> u8;

    fn write(&mut self, address: u16, value: u8);

    // Called to tick time for cycles where the CPU does not access the bus
    fn idle(&mut self);

    fn highest_priority_interrupt(&self) -> Option<InterruptType>;

    fn acknowledge_interrupt(&mut self, interrupt_type: InterruptType);
}
