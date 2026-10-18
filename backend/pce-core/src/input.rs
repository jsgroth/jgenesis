use crate::api::PceEmulatorConfig;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use pce_config::{PceGamepadType, PceInputDevice, PceInputs, PceJoypadState, PceRegion};
use std::iter;

const TURBO_TAP_GAMEPADS: u8 = pce_config::TURBO_TAP_GAMEPADS;

trait PceJoypadStateExt {
    fn with_simultaneous_run_select(self, allow_simultaneous_run_select: bool) -> Self;
}

impl PceJoypadStateExt for PceJoypadState {
    fn with_simultaneous_run_select(mut self, allow_simultaneous_run_select: bool) -> Self {
        if allow_simultaneous_run_select {
            return self;
        }

        if self.run && self.select {
            self.run = false;
            self.select = false;
        }

        self
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct InputState {
    region: PceRegion,
    cd_hardware_present: bool,
    input_device: PceInputDevice,
    turbo_tap_connected: [bool; TURBO_TAP_GAMEPADS as usize],
    turbo_tap_types: [PceGamepadType; TURBO_TAP_GAMEPADS as usize],
    inputs: PceInputs,
    latched_inputs: PceInputs,
    select_pin: bool,
    clear_pin: bool,
    six_button_state: [bool; TURBO_TAP_GAMEPADS as usize],
    turbo_tap_gamepad_idx: u8,
    allow_opposing_directions: bool,
    allow_simultaneous_run_select: bool,
}

impl InputState {
    pub fn new(config: PceEmulatorConfig, cd_hardware_present: bool) -> Self {
        Self {
            region: config.region,
            cd_hardware_present,
            input_device: config.input_device,
            turbo_tap_connected: config.turbo_tap_connected,
            turbo_tap_types: config.turbo_tap_gamepad_types,
            inputs: PceInputs::default(),
            latched_inputs: PceInputs::default(),
            select_pin: false,
            clear_pin: false,
            six_button_state: [false; TURBO_TAP_GAMEPADS as usize],
            turbo_tap_gamepad_idx: TURBO_TAP_GAMEPADS,
            allow_opposing_directions: config.allow_opposing_joypad_directions,
            allow_simultaneous_run_select: config.allow_simultaneous_run_select,
        }
    }

    pub fn reload_config(&mut self, config: PceEmulatorConfig) {
        if self.input_device != config.input_device {
            self.six_button_state.fill(false);
            self.turbo_tap_gamepad_idx = TURBO_TAP_GAMEPADS;
        }

        if self.input_device == PceInputDevice::TurboTap {
            for (gamepad_type, six_button_extra_buttons) in
                iter::zip(config.turbo_tap_gamepad_types, &mut self.six_button_state)
            {
                if gamepad_type == PceGamepadType::TwoButton {
                    *six_button_extra_buttons = false;
                }
            }
        }

        self.region = config.region;
        self.input_device = config.input_device;
        self.turbo_tap_connected = config.turbo_tap_connected;
        self.turbo_tap_types = config.turbo_tap_gamepad_types;
        self.allow_opposing_directions = config.allow_opposing_joypad_directions;
        self.allow_simultaneous_run_select = config.allow_simultaneous_run_select;
    }

    pub fn update_inputs(&mut self, inputs: PceInputs) {
        self.inputs = inputs;
    }

    pub fn read_port(&mut self) -> u8 {
        let controller_data = match self.input_device {
            PceInputDevice::TwoButtonGamepad | PceInputDevice::SixButtonGamepad => {
                self.read_gamepad()
            }
            PceInputDevice::TurboTap => self.read_turbo_tap(),
        };

        // Bits 5 and 4 always read 1
        let cd_bit = u8::from(!self.cd_hardware_present);
        let region_bit = u8::from(self.region == PceRegion::PcEngine);
        let value = (cd_bit << 7) | (region_bit << 6) | (1 << 5) | (1 << 4) | controller_data;

        log::trace!(
            "I/O port read; SEL={} CLR={}, value={value:02X}",
            u8::from(self.select_pin),
            u8::from(self.clear_pin)
        );

        value
    }

    fn read_gamepad(&self) -> u8 {
        if self.clear_pin {
            return 0x00;
        }

        self.read_joypad(self.latched_inputs.p1, self.six_button_state[0])
    }

    fn read_turbo_tap(&self) -> u8 {
        if self.turbo_tap_gamepad_idx >= TURBO_TAP_GAMEPADS {
            return 0x00;
        }

        let idx = self.turbo_tap_gamepad_idx as usize;
        if !self.turbo_tap_connected[idx] {
            return 0x0F;
        }

        let joypad = [
            self.latched_inputs.p1,
            self.latched_inputs.p2,
            self.latched_inputs.p3,
            self.latched_inputs.p4,
            self.latched_inputs.p5,
        ][idx];
        self.read_joypad(joypad, self.six_button_state[idx])
    }

    fn read_joypad(&self, mut joypad: PceJoypadState, six_button_state: bool) -> u8 {
        joypad = joypad
            .with_allow_opposing_directions(self.allow_opposing_directions)
            .with_simultaneous_run_select(self.allow_simultaneous_run_select);

        let data = if !six_button_state {
            if self.select_pin {
                [joypad.up, joypad.right, joypad.down, joypad.left]
            } else {
                [joypad.button1, joypad.button2, joypad.select, joypad.run]
            }
        } else {
            if self.select_pin {
                [true; 4]
            } else {
                [joypad.button3, joypad.button4, joypad.button5, joypad.button6]
            }
        };

        (u8::from(!data[3]) << 3)
            | (u8::from(!data[2]) << 2)
            | (u8::from(!data[1]) << 1)
            | u8::from(!data[0])
    }

    pub fn write_port(&mut self, value: u8) {
        let prev_clear = self.clear_pin;
        let prev_select = self.select_pin;

        self.clear_pin = value.bit(1);
        self.select_pin = value.bit(0);

        match self.input_device {
            PceInputDevice::TwoButtonGamepad | PceInputDevice::SixButtonGamepad => {
                if prev_clear && !self.clear_pin {
                    // Latching inputs on CLR 1->0 transitions fixes Order of the Griffon sometimes
                    // double-reading button presses
                    self.latched_inputs = self.inputs;

                    // 6-button gamepads flip between normal buttons and extra buttons on CLR 1->0 transitions
                    if self.input_device == PceInputDevice::SixButtonGamepad {
                        self.six_button_state[0] = !self.six_button_state[0];
                    }
                }
            }
            PceInputDevice::TurboTap => {
                // Turbo Tap resets on CLR 0->1 transition while SEL=1
                if prev_select && self.select_pin && !prev_clear && self.clear_pin {
                    self.turbo_tap_gamepad_idx = 0;
                    self.latched_inputs = self.inputs;
                }

                // Turbo Tap counter increments on SEL 0->1 transitions while CLR=0
                if !prev_clear && !self.clear_pin && !prev_select && self.select_pin {
                    self.turbo_tap_gamepad_idx =
                        (self.turbo_tap_gamepad_idx + 1).min(TURBO_TAP_GAMEPADS);

                    // Counter increment to a 6-button gamepad flips extra button state
                    let tap_idx = self.turbo_tap_gamepad_idx as usize;
                    if self.turbo_tap_gamepad_idx < TURBO_TAP_GAMEPADS
                        && self.turbo_tap_types[tap_idx] == PceGamepadType::SixButton
                    {
                        self.six_button_state[tap_idx] = !self.six_button_state[tap_idx];
                    }
                }

                // If selected gamepad is 6-button, flips extra button state on CLR 1->0 transitions
                if prev_clear
                    && !self.clear_pin
                    && self.turbo_tap_gamepad_idx < TURBO_TAP_GAMEPADS
                    && self.turbo_tap_types[self.turbo_tap_gamepad_idx as usize]
                        == PceGamepadType::SixButton
                {
                    let tap_idx = self.turbo_tap_gamepad_idx as usize;
                    self.six_button_state[tap_idx] = !self.six_button_state[tap_idx];
                }
            }
        }

        log::trace!(
            "I/O port write: {value:02X} (SEL={} CLR={})",
            u8::from(self.select_pin),
            u8::from(self.clear_pin)
        );
    }
}
