use crate::api::PceEmulatorConfig;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use pce_config::{PceInputs, PceRegion};

#[derive(Debug, Clone, Encode, Decode)]
pub struct InputState {
    region: PceRegion,
    inputs: PceInputs,
    select_pin: bool,
    clear_pin: bool,
    allow_simultaneous_run_select: bool,
}

impl InputState {
    pub fn new(config: PceEmulatorConfig) -> Self {
        Self {
            region: config.region,
            inputs: PceInputs::default(),
            select_pin: false,
            clear_pin: false,
            allow_simultaneous_run_select: config.allow_simultaneous_run_select,
        }
    }

    pub fn reload_config(&mut self, config: PceEmulatorConfig) {
        self.region = config.region;
        self.allow_simultaneous_run_select = config.allow_simultaneous_run_select;
    }

    pub fn update_inputs(&mut self, inputs: PceInputs) {
        self.inputs = inputs;
    }

    pub fn read_port(&mut self) -> u8 {
        let mut inputs = self.inputs.p1;

        if !self.allow_simultaneous_run_select && inputs.run && inputs.select {
            inputs.run = false;
            inputs.select = false;
        }

        let data = match (self.select_pin, self.clear_pin) {
            (false, false) => [inputs.button1, inputs.button2, inputs.select, inputs.run],
            (true, false) => [inputs.up, inputs.right, inputs.down, inputs.left],
            (_, true) => [true; 4],
        };

        // Bit 7: CD-ROM present (1 = not attached)
        // Bits 5 and 4 always read 1
        let region_bit = u8::from(self.region == PceRegion::PcEngine);
        let value = (1 << 7)
            | (region_bit << 6)
            | (1 << 5)
            | (1 << 4)
            | (u8::from(!data[3]) << 3)
            | (u8::from(!data[2]) << 2)
            | (u8::from(!data[1]) << 1)
            | u8::from(!data[0]);

        log::trace!(
            "I/O port read; SEL={} CLR={}, value={value:02X}",
            u8::from(self.select_pin),
            u8::from(self.clear_pin)
        );

        value
    }

    pub fn write_port(&mut self, value: u8) {
        self.clear_pin = value.bit(1);
        self.select_pin = value.bit(0);

        log::trace!(
            "I/O port write: {value:02X} (SEL={} CLR={})",
            u8::from(self.select_pin),
            u8::from(self.clear_pin)
        );
    }
}
