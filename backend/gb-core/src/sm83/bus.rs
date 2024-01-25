use crate::sm83::InterruptType;

pub trait BusInterface {
    /// Read a memory address and advance all components by one M-cycle
    fn read(&mut self, address: u16) -> u8;

    /// Write a memory address and advance all components by one M-cycle
    fn write(&mut self, address: u16, value: u8);

    /// Called to tick time for cycles where the CPU does not access the bus; advances all components by one M-cycle
    fn idle(&mut self);

    /// Read the IE (interrupts enabled) register. The upper 3 bits should always be clear
    fn read_ie_register(&self) -> u8;

    /// Read the IF (interrupt flags) register. The upper 3 bits should always be clear
    fn read_if_register(&self) -> u8;

    /// Return whether an interrupt is pending (IE & IF != 0)
    fn interrupt_pending(&self) -> bool {
        self.read_ie_register() & self.read_if_register() != 0
    }

    /// Acknowledge an interrupt, which should clear the corresponding flag in the IF (interrupt flags) register
    fn acknowledge_interrupt(&mut self, interrupt_type: InterruptType);

    /// Whether the CPU should be halted due to an in-progress VRAM DMA
    fn halt(&self) -> bool;

    /// Whether a CGB speed switch is currently armed
    fn speed_switch_armed(&self) -> bool;

    /// Perform a CGB speed switch
    fn perform_speed_switch(&mut self);
}
