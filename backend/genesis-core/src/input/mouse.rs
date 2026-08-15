use crate::input::Pins;
use bincode::{Decode, Encode};
use genesis_config::MegaMouseJoypadState;
use std::cmp;

const MAX_COUNTER: u8 = 9;

const COUNTER_INCREMENT_WAIT_CYCLES: u32 = 23; // Roughly 3 μs, some games depend on counter changing well before TL changes
const TL_CHANGE_WAIT_CYCLES: u32 = 76; // Roughly 10 μs

#[derive(Debug, Clone, Encode, Decode)]
pub struct MegaMouseState {
    pub joypad: MegaMouseJoypadState,
    latched: MegaMouseJoypadState,
    latched_x: i16,
    latched_y: i16,
    sensitivity: f64,
    counter: u8,
    tl: bool,
    clock_frozen: bool,
    counter_increment_wait_cycles: u32,
    tl_change_wait_cycles: u32,
    prev_th: bool,
    prev_tr: bool,
}

impl MegaMouseState {
    pub fn new(joypad: MegaMouseJoypadState, sensitivity: f64) -> Self {
        Self {
            joypad,
            latched: joypad,
            latched_x: 0,
            latched_y: 0,
            sensitivity,
            counter: 0,
            tl: true,
            clock_frozen: false,
            counter_increment_wait_cycles: 0,
            tl_change_wait_cycles: 0,
            prev_th: true,
            prev_tr: true,
        }
    }

    pub fn update_pins(&mut self, pins: &mut Pins) {
        let th = pins.th();
        let tr = pins.tr();

        let th_changed = th != self.prev_th;
        let tr_changed = tr != self.prev_tr;

        if tr_changed {
            if tr != self.tl && !self.clock_frozen {
                // TL changes to reflect TR, but only after a delay (which several games depend on)
                self.tl_change_wait_cycles = TL_CHANGE_WAIT_CYCLES;

                if !th {
                    // TR changes while TH=0 clock the internal counter
                    self.counter_increment_wait_cycles = COUNTER_INCREMENT_WAIT_CYCLES;
                }
            } else if tr == self.tl {
                self.counter_increment_wait_cycles = 0;
                self.tl_change_wait_cycles = 0;
            }
        }

        if th {
            // While TH=1, mouse is deselected and outputs constants 0s
            self.counter = 0;
            self.counter_increment_wait_cycles = 0;

            // TH=1 + TR=1 resets the mouse
            self.clock_frozen &= !tr;

            // There appears to be additional delay after a reset, but not always, and it's not
            // clear to me why that delay happens so I'm not emulating it
        } else if th_changed {
            // TH 1->0 transition begins a transfer
            self.latch_inputs();
            self.counter = 1;

            // If previous state was TH=1 TR=0, the clock freezes until the mouse is reset (based on test ROM)
            // TODO does this only happen if there was also a TR 0->1 transition?
            self.clock_frozen = !self.prev_tr;
        }

        self.prev_th = th;
        self.prev_tr = tr;

        log::debug!(
            "Mouse update, TH={} TR={}; TL={} counter={}",
            u8::from(th),
            u8::from(tr),
            u8::from(self.tl),
            self.counter
        );

        pins.input_tl(self.tl);

        match self.counter {
            0 => {
                // No transfer in progress, output constant 0s
                pins.input_data_nibble(0b0000);
            }
            1..=3 => {
                // Mouse ID (3 nibble sequence)
                pins.input_data_nibble([0xB, 0xF, 0xF][(self.counter - 1) as usize]);
            }
            4 => {
                // Axis sign and overflow bits (overflow not emulated)
                pins.input_d3(false); // Y overflow
                pins.input_d2(false); // X overflow
                pins.input_d1(self.latched_y < 0);
                pins.input_d0(self.latched_x < 0);
            }
            5 => {
                // Buttons
                pins.input_d3(self.latched.start);
                pins.input_d2(self.latched.middle);
                pins.input_d1(self.latched.right);
                pins.input_d0(self.latched.left);
            }
            6 => {
                // X axis, high nibble
                pins.input_data_nibble((self.latched_x as u8) >> 4);
            }
            7 => {
                // X axis, low nibble
                pins.input_data_nibble((self.latched_x as u8) & 0xF);
            }
            8 => {
                // Y axis, high nibble
                pins.input_data_nibble((self.latched_y as u8) >> 4);
            }
            9 => {
                // Y axis, low nibble
                pins.input_data_nibble((self.latched_y as u8) & 0xF);
            }
            _ => panic!(
                "invalid Mega Mouse counter value {}, should be <= {MAX_COUNTER}",
                self.counter
            ),
        }
    }

    fn latch_inputs(&mut self) {
        fn f64_to_i9(value: f64) -> i16 {
            // Mega Mouse axis values are signed 9-bit
            // Arbitrarily multiply by 0.3 because using full pixel values is way too sensitive
            (0.3 * value).round().clamp(-256.0, 255.0) as i16
        }

        let dx = self.sensitivity * (self.joypad.x_position.get() - self.latched.x_position.get());
        let dy = self.sensitivity * (self.joypad.y_position.get() - self.latched.y_position.get());
        self.latched = self.joypad;

        self.latched_x = f64_to_i9(dx);
        self.latched_y = f64_to_i9(-dy); // Mega Mouse Y axis is inverted (+ is up, - is down)
    }

    pub fn tick(&mut self, m68k_cycles: u32, pins: &mut Pins) {
        if self.counter_increment_wait_cycles == 0 && self.tl_change_wait_cycles == 0 {
            return;
        }

        if self.counter_increment_wait_cycles != 0 {
            self.counter_increment_wait_cycles =
                self.counter_increment_wait_cycles.saturating_sub(m68k_cycles);
            if self.counter_increment_wait_cycles == 0 {
                self.counter = cmp::min(MAX_COUNTER, self.counter + 1);
            }
        }

        if self.tl_change_wait_cycles != 0 {
            self.tl_change_wait_cycles = self.tl_change_wait_cycles.saturating_sub(m68k_cycles);
            if self.tl_change_wait_cycles == 0 {
                self.tl = self.prev_tr;
            }
        }

        self.update_pins(pins);
    }

    pub fn set_sensitivity(&mut self, sensitivity: f64) {
        self.sensitivity = sensitivity;
    }
}
