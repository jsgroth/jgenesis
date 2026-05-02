use bincode::{Decode, Encode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub struct InterruptLines {
    pub irq1: bool,
    pub irq2: bool,
    pub tiq: bool,
}

impl InterruptLines {
    #[must_use]
    pub fn any(self) -> bool {
        self.irq1 || self.irq2 || self.tiq
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum ClockSpeed {
    #[default]
    Low,
    High,
}

pub trait BusInterface {
    fn read(&mut self, address: u32) -> u8;

    fn write(&mut self, address: u32, value: u8);

    fn idle(&mut self);

    fn interrupt_lines(&self) -> InterruptLines;

    fn set_clock_speed(&mut self, speed: ClockSpeed);
}
