use bincode::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct Timer<const INTERVAL: u32, const MAX: u32> {
    enabled: bool,
    overflow_flag_enabled: bool,
    overflow_flag: bool,
    // Interval and counter are in mclk cycles
    interval: u32,
    counter: u32,
}

// Timer A logically ticks every 72 mclk cycles and timer B logically ticks every 1152 mclk cycles
pub type TimerA = Timer<72, 1024>;
pub type TimerB = Timer<1152, 256>;

impl<const INTERVAL: u32, const MAX: u32> Timer<INTERVAL, MAX> {
    pub fn new() -> Self {
        Self {
            enabled: false,
            overflow_flag_enabled: false,
            overflow_flag: false,
            interval: 0,
            counter: Self::counter_reload_value(0),
        }
    }

    #[inline]
    fn counter_reload_value(interval: u32) -> u32 {
        2 * INTERVAL * (MAX - interval)
    }

    #[inline]
    pub fn tick(&mut self) {
        if !self.enabled {
            return;
        }

        self.counter -= 1;
        if self.counter == 0 {
            self.counter = Self::counter_reload_value(self.interval);

            if self.overflow_flag_enabled {
                self.overflow_flag = true;
            }
        }
    }

    pub fn overflow_flag(&self) -> bool {
        self.overflow_flag
    }

    pub fn clear_overflow_flag(&mut self) {
        self.overflow_flag = false;
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if !self.enabled && enabled {
            self.counter = Self::counter_reload_value(self.interval);
        }
        self.enabled = enabled;
    }

    pub fn set_overflow_flag_enabled(&mut self, value: bool) {
        self.overflow_flag_enabled = value;
    }

    pub fn interval(&self) -> u32 {
        self.interval
    }

    pub fn set_interval(&mut self, interval: u32) {
        self.interval = interval;
    }
}
