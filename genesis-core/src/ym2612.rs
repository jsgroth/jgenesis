#[derive(Debug, Clone)]
pub struct Ym2612 {
    r1_address: u8,
    r2_address: u8,
}

impl Ym2612 {
    pub fn new() -> Self {
        Self {
            r1_address: 0,
            r2_address: 0,
        }
    }

    // Set the address register for group 1 (system registers + channels 1-3)
    pub fn write_address_1(&mut self, value: u8) {
        self.r1_address = value;
    }

    // Write to the data port for group 1 (system registers + channels 1-3)
    #[allow(clippy::unused_self)]
    pub fn write_data_1(&mut self, _value: u8) {
        // TODO
    }

    // Set the address register for group 2 (channels 4-6)
    pub fn write_address_2(&mut self, value: u8) {
        self.r2_address = value;
    }

    // Write to the data port for group 2 (channels 4-6)
    #[allow(clippy::unused_self)]
    pub fn write_data_2(&mut self, _value: u8) {
        // TODO
    }

    #[allow(clippy::unused_self)]
    pub fn read_register(&self) -> u8 {
        // TODO busy bit, maybe timer overflow bits
        0x00
    }
}
