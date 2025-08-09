//! Seiko S-3511A real-time clock chip
//!
//! Used by Pokemon, Boktai, Rockman EXE 4.5, and maybe others

use crate::interrupts::{InterruptRegisters, InterruptType};
use bincode::{Decode, Encode};
use jgenesis_common::num::{GetBit, U16Ext};
use jgenesis_common::{define_bit_enum, timeutils};

define_bit_enum!(Hours, [Twelve, TwentyFour]);
define_bit_enum!(BeforeNoon, [Am, Pm]);

impl BeforeNoon {
    #[must_use]
    fn toggle(self) -> Self {
        match self {
            Self::Am => Self::Pm,
            Self::Pm => Self::Am,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct Control {
    power_cycled: bool,         // POWER
    hours: Hours,               // 12/24
    alarm_interrupt: bool,      // INTAE
    per_minute_interrupt: bool, // INTME
    frequency_interrupt: bool,  // INTFE
}

impl Default for Control {
    fn default() -> Self {
        // Initialized to 0x82 at power-on (POWER and INTFE set)
        Self {
            power_cycled: true,
            hours: Hours::default(),
            alarm_interrupt: false,
            per_minute_interrupt: false,
            frequency_interrupt: false,
        }
    }
}

impl Control {
    fn read(&self) -> u8 {
        (u8::from(self.power_cycled) << 7)
            | ((self.hours as u8) << 6)
            | (u8::from(self.alarm_interrupt) << 5)
            | (u8::from(self.per_minute_interrupt) << 3)
            | (u8::from(self.frequency_interrupt) << 1)
    }

    fn write(&mut self, value: u8) {
        self.hours = Hours::from_bit(value.bit(6));
        self.alarm_interrupt = value.bit(5);
        self.per_minute_interrupt = value.bit(3);
        self.frequency_interrupt = value.bit(1);

        log::trace!("Control write: {value:02X}");
        log::trace!("  12/24: {:?}", self.hours);
        log::trace!("  INTAE: {}", self.alarm_interrupt);
        log::trace!("  INTME: {}", self.per_minute_interrupt);
        log::trace!("  INTFE: {}", self.frequency_interrupt);

        if self.frequency_interrupt && !self.per_minute_interrupt {
            log::error!("Frequency interrupts are not implemented");
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
struct DateTime {
    year: u8,
    month: u8,
    day: u8,
    day_of_week: u8,
    hour: u8,
    before_noon: BeforeNoon,
    minute: u8,
    second: u8,
    nanos: u128,
}

impl Default for DateTime {
    fn default() -> Self {
        Self {
            year: 0,
            month: 1,
            day: 1,
            day_of_week: 0,
            hour: 0,
            before_noon: BeforeNoon::Am,
            minute: 0,
            second: 0,
            nanos: 0,
        }
    }
}

define_bit_enum!(CommandDirection, [Write, Read]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum Command {
    Reset,
    Status,
    DataFromYear,
    DataFromHour,
    InterruptRegisterLow,
    InterruptRegisterHigh,
}

impl Command {
    fn parse(command_byte: u8) -> Option<Self> {
        // All command bytes must start with 0110
        if command_byte >> 4 != 0b0110 {
            return None;
        }

        match (command_byte >> 1) & 7 {
            0b000 => Some(Self::Reset),
            0b001 => Some(Self::Status),
            0b010 => Some(Self::DataFromYear),
            0b011 => Some(Self::DataFromHour),
            0b100 => Some(Self::InterruptRegisterLow),
            0b101 => Some(Self::InterruptRegisterHigh),
            0b110 | 0b111 => {
                log::error!("RTC test mode not implemented; ignoring command");
                None
            }
            _ => unreachable!("value & 7 is always 0-7"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum ReadTarget {
    Status,
    Year,
    Month,
    Day,
    DayOfWeek,
    Hour,
    Minute,
    Second,
    InterruptLow,
    InterruptHigh,
}

impl ReadTarget {
    fn read(self, rtc: &SeikoRealTimeClock) -> u8 {
        match self {
            Self::Status => rtc.control.read(),
            Self::Year => binary_to_bcd(rtc.datetime.year),
            Self::Month => binary_to_bcd(rtc.datetime.month),
            Self::Day => binary_to_bcd(rtc.datetime.day),
            Self::DayOfWeek => rtc.datetime.day_of_week,
            Self::Hour => {
                binary_to_bcd(rtc.datetime.hour) | ((rtc.datetime.before_noon as u8) << 7)
            }
            Self::Minute => binary_to_bcd(rtc.datetime.minute),
            Self::Second => binary_to_bcd(rtc.datetime.second),
            Self::InterruptLow => rtc.interrupt_register.lsb(),
            Self::InterruptHigh => rtc.interrupt_register.msb(),
        }
    }

    fn next(self) -> Option<Self> {
        match self {
            Self::Year => Some(Self::Month),
            Self::Month => Some(Self::Day),
            Self::Day => Some(Self::DayOfWeek),
            Self::DayOfWeek => Some(Self::Hour),
            Self::Hour => Some(Self::Minute),
            Self::Minute => Some(Self::Second),
            // TODO is this right?
            Self::InterruptLow => Some(Self::InterruptHigh),
            _ => None,
        }
    }
}

fn binary_to_bcd(value: u8) -> u8 {
    let low = value % 10;
    let high = value / 10;
    low | (high << 4)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum WriteTarget {
    Status,
    Year,
    Month,
    Day,
    DayOfWeek,
    Hour,
    Minute,
    Second,
    InterruptLow,
    InterruptHigh,
}

impl WriteTarget {
    fn write(self, rtc: &mut SeikoRealTimeClock, value: u8) {
        match self {
            Self::Status => rtc.control.write(value),
            Self::Year => {
                let year = bcd_to_binary(value);
                rtc.datetime.year = if year < 100 { year } else { 0 };
            }
            Self::Month => {
                let month = bcd_to_binary(value & 0x1F);
                rtc.datetime.month = if (1..=12).contains(&month) { month } else { 1 };
            }
            Self::Day => {
                let day = bcd_to_binary(value & 0x3F);
                if day > timeutils::days_in_month(rtc.datetime.month, rtc.datetime.year) {
                    rtc.tick_month();
                    rtc.datetime.day = 1;
                } else if day == 0 {
                    rtc.datetime.day = 1;
                } else {
                    rtc.datetime.day = day;
                }
            }
            Self::DayOfWeek => {
                rtc.datetime.day_of_week = value & 0x7;
            }
            Self::Hour => {
                let hour = bcd_to_binary(value & 0x3F);
                match rtc.control.hours {
                    Hours::Twelve => {
                        let before_noon = BeforeNoon::from_bit(value.bit(7));
                        rtc.datetime.hour = if hour < 12 { hour } else { 0 };
                        rtc.datetime.before_noon = before_noon;
                    }
                    Hours::TwentyFour => {
                        rtc.datetime.hour = if hour < 24 { hour } else { 0 };
                        rtc.datetime.before_noon = BeforeNoon::from_bit(false);
                    }
                }
            }
            Self::Minute => {
                let minute = bcd_to_binary(value & 0x7F);
                rtc.datetime.minute = if minute < 60 { minute } else { 0 };
            }
            Self::Second => {
                rtc.datetime.second = bcd_to_binary(value & 0x7F);
                // Invalid values will get cleared at the next second tick
            }
            Self::InterruptLow => {
                rtc.interrupt_register.set_lsb(value);
            }
            Self::InterruptHigh => {
                rtc.interrupt_register.set_msb(value);
            }
        }
    }

    fn next(self) -> Option<Self> {
        match self {
            Self::Year => Some(Self::Month),
            Self::Month => Some(Self::Day),
            Self::Day => Some(Self::DayOfWeek),
            Self::DayOfWeek => Some(Self::Hour),
            Self::Hour => Some(Self::Minute),
            Self::Minute => Some(Self::Second),
            // TODO is this right?
            Self::InterruptLow => Some(Self::InterruptHigh),
            _ => None,
        }
    }
}

fn bcd_to_binary(value: u8) -> u8 {
    (value & 0xF) + 10 * (value >> 4)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum CommandState {
    Idle,
    ReceivingCommand { bits: u8, remaining: u8 },
    PreparingSend { target: ReadTarget },
    SendingData { bits: u8, remaining: u8, next: Option<ReadTarget> },
    ReceivingData { destination: WriteTarget, bits: u8, remaining: u8 },
    Finished,
}

pub struct RtcWrite {
    pub chip_select: bool,
    pub clock: bool,
    pub data: bool,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct SeikoRealTimeClock {
    datetime: DateTime,
    control: Control,
    interrupt_register: u16,
    interrupt_line: bool,
    last_update_time_nanos: u128,
    command_state: CommandState,
    prev_clock: bool,
}

impl SeikoRealTimeClock {
    pub fn new() -> Self {
        Self {
            datetime: DateTime::default(),
            control: Control::default(),
            interrupt_register: 0x8000,
            interrupt_line: false,
            last_update_time_nanos: timeutils::current_time_nanos(),
            command_state: CommandState::Idle,
            prev_clock: false,
        }
    }

    pub fn read(&self) -> bool {
        log::debug!("RTC read; current command state {:?}", self.command_state);

        match self.command_state {
            CommandState::SendingData { bits, .. } => bits.bit(0),
            _ => true,
        }
    }

    pub fn write(&mut self, RtcWrite { chip_select, clock, data }: RtcWrite) {
        log::trace!(
            "RTC write: CS={}, SCK={}, SIO={}, current state {:?}",
            u8::from(chip_select),
            u8::from(clock),
            u8::from(data),
            self.command_state
        );

        let prev_clock = self.prev_clock;
        self.prev_clock = clock;

        if !chip_select {
            self.command_state = CommandState::Idle;
            return;
        }

        if self.command_state == CommandState::Finished {
            return;
        }

        if self.command_state == CommandState::Idle {
            self.command_state = CommandState::ReceivingCommand { bits: 0, remaining: 8 };
            return;
        }

        // Data reads/writes only progress on falling clock edges
        let falling_clock_edge = prev_clock && !clock;
        if !falling_clock_edge {
            return;
        }

        self.command_state = match self.command_state {
            CommandState::ReceivingCommand { mut bits, remaining: 1 } => {
                // Command bytes are received MSB first
                bits = (bits << 1) | u8::from(data);

                match Command::parse(bits) {
                    Some(Command::Reset) => {
                        log::debug!("Received RTC reset command");

                        self.reset();
                        CommandState::Finished
                    }
                    Some(command) => {
                        let direction = CommandDirection::from_bit(bits.bit(0));

                        log::debug!("Received RTC command {command:?}, direction {direction:?}");

                        match direction {
                            CommandDirection::Read => CommandState::PreparingSend {
                                target: match command {
                                    Command::Status => ReadTarget::Status,
                                    Command::DataFromYear => ReadTarget::Year,
                                    Command::DataFromHour => ReadTarget::Hour,
                                    Command::InterruptRegisterLow => ReadTarget::InterruptLow,
                                    Command::InterruptRegisterHigh => ReadTarget::InterruptHigh,
                                    Command::Reset => unreachable!("already a reset match arm"),
                                },
                            },
                            CommandDirection::Write => CommandState::ReceivingData {
                                destination: match command {
                                    Command::Status => WriteTarget::Status,
                                    Command::DataFromYear => WriteTarget::Year,
                                    Command::DataFromHour => WriteTarget::Hour,
                                    Command::InterruptRegisterLow => WriteTarget::InterruptLow,
                                    Command::InterruptRegisterHigh => WriteTarget::InterruptHigh,
                                    Command::Reset => unreachable!("already a reset match arm"),
                                },
                                bits: 0,
                                remaining: 8,
                            },
                        }
                    }
                    None => CommandState::Finished,
                }
            }
            CommandState::ReceivingCommand { mut bits, mut remaining } => {
                // Command bytes are received MSB first
                bits = (bits << 1) | u8::from(data);
                remaining -= 1;

                CommandState::ReceivingCommand { bits, remaining }
            }
            CommandState::PreparingSend { target } => CommandState::SendingData {
                bits: target.read(self),
                remaining: 8,
                next: target.next(),
            },
            CommandState::SendingData { remaining: 1, next: Some(next), .. } => {
                let bits = next.read(self);
                CommandState::SendingData { bits, remaining: 8, next: next.next() }
            }
            CommandState::SendingData { remaining: 1, next: None, .. } => CommandState::Finished,
            CommandState::SendingData { mut bits, mut remaining, next } => {
                bits >>= 1;
                remaining -= 1;

                CommandState::SendingData { bits, remaining, next }
            }
            CommandState::ReceivingData { destination, mut bits, remaining: 1 } => {
                // Data is received LSB first
                bits = (bits >> 1) | (u8::from(data) << 7);

                log::debug!("Applying write {bits:02X} to destination {destination:?}");

                destination.write(self, bits);

                match destination.next() {
                    Some(next) => {
                        CommandState::ReceivingData { destination: next, bits: 0, remaining: 8 }
                    }
                    None => CommandState::Finished,
                }
            }
            CommandState::ReceivingData { destination, mut bits, mut remaining } => {
                // Data is received LSB first
                bits = (bits >> 1) | (u8::from(data) << 7);
                remaining -= 1;

                CommandState::ReceivingData { destination, bits, remaining }
            }
            CommandState::Idle | CommandState::Finished => unreachable!(),
        };
    }

    fn reset(&mut self) {
        self.control.write(0);
        self.control.power_cycled = false;
        self.interrupt_register = 0;
        self.datetime = DateTime::default();
        self.interrupt_line = false;
    }

    pub fn update_time(&mut self, cycles: u64, interrupts: &mut InterruptRegisters) {
        let current_time_nanos = timeutils::current_time_nanos();
        let elapsed_nanos = current_time_nanos.saturating_sub(self.last_update_time_nanos);
        self.last_update_time_nanos = current_time_nanos;

        self.update_time_internal(elapsed_nanos);

        let prev_interrupt_line = self.interrupt_line;
        self.update_interrupt_line();

        // RTC interrupt line is inverted; raise interrupt when line is de-asserted
        if prev_interrupt_line && !self.interrupt_line {
            interrupts.set_flag(InterruptType::GamePak, cycles);
        }
    }

    fn update_time_internal(&mut self, elapsed_nanos: u128) {
        self.datetime.nanos += elapsed_nanos;
        let mut elapsed_seconds = (self.datetime.nanos / 1_000_000_000) as u64;
        self.datetime.nanos %= 1_000_000_000;

        if elapsed_seconds == 0 {
            return;
        }

        if self.datetime.second >= 60 {
            // Can happen if software wrote an invalid second
            self.datetime.second = 0;
            self.tick_minute();
            elapsed_seconds -= 1;
        }

        let second = u64::from(self.datetime.second) + elapsed_seconds;
        let elapsed_minutes = second / 60;
        self.datetime.second = (second % 60) as u8;
        if elapsed_minutes == 0 {
            return;
        }

        let minute = u64::from(self.datetime.minute) + elapsed_minutes;
        let elapsed_hours = minute / 60;
        self.datetime.minute = (minute % 60) as u8;

        for _ in 0..elapsed_hours {
            self.tick_hour();
        }
    }

    fn tick_minute(&mut self) {
        self.datetime.minute += 1;
        if self.datetime.minute >= 60 {
            self.datetime.minute = 0;
            self.tick_hour();
        }
    }

    fn tick_hour(&mut self) {
        self.datetime.hour += 1;

        match self.control.hours {
            Hours::Twelve => {
                if self.datetime.hour >= 12 {
                    self.datetime.hour = 0;
                    self.datetime.before_noon = self.datetime.before_noon.toggle();
                    if self.datetime.before_noon == BeforeNoon::Am {
                        self.tick_day();
                    }
                }
            }
            Hours::TwentyFour => {
                if self.datetime.hour >= 24 {
                    self.datetime.hour = 0;
                    self.tick_day();
                }
            }
        }
    }

    fn tick_day(&mut self) {
        self.datetime.day_of_week = (self.datetime.day_of_week + 1) % 7;

        self.datetime.day += 1;
        if self.datetime.day > timeutils::days_in_month(self.datetime.month, self.datetime.year) {
            self.datetime.day = 1;
            self.tick_month();
        }
    }

    fn tick_month(&mut self) {
        self.datetime.month += 1;
        if self.datetime.month > 12 {
            self.datetime.month = 1;
            self.tick_year();
        }
    }

    fn tick_year(&mut self) {
        // Chip only has a 2-digit year (represents 2000-2099)
        self.datetime.year = (self.datetime.year + 1) % 100;
    }

    fn update_interrupt_line(&mut self) {
        // TODO this is not tested - I don't think any official releases use RTC interrupts
        if self.control.per_minute_interrupt {
            if self.control.frequency_interrupt {
                // Per-minute steady interrupt: goes low at 0sec, goes high at 30sec
                match self.datetime.second {
                    0 => self.interrupt_line = true,
                    30 => self.interrupt_line = false,
                    _ => {}
                }
            } else {
                // Per-minute edge interrupt: goes low at 0sec, never goes high
                self.interrupt_line |= self.datetime.second == 0;
            }
        } else if self.control.frequency_interrupt {
            // TODO frequency steady interrupts - does anything use these?
        } else if self.control.alarm_interrupt {
            // Alarm interrupt: goes low when hour/minute matches alarm time
            let alarm_hour = bcd_to_binary(self.interrupt_register.lsb() & 0x3F);
            let alarm_before_noon = BeforeNoon::from_bit(self.interrupt_register.bit(7));
            let alarm_minute = bcd_to_binary(self.interrupt_register.msb() & 0x7F);

            self.interrupt_line = alarm_hour == self.datetime.hour
                && alarm_before_noon == self.datetime.before_noon
                && alarm_minute == self.datetime.minute;
        } else {
            // No interrupts enabled
            self.interrupt_line = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_overflows_24h() {
        let mut rtc = SeikoRealTimeClock::new();
        rtc.control.hours = Hours::TwentyFour;
        rtc.datetime = DateTime {
            year: 99,
            month: 12,
            day: 31,
            day_of_week: 6,
            hour: 23,
            before_noon: BeforeNoon::Am,
            minute: 59,
            second: 59,
            nanos: 0,
        };

        rtc.update_time_internal(1_000_000_000);

        assert_eq!(
            rtc.datetime,
            DateTime {
                year: 0,
                month: 1,
                day: 1,
                day_of_week: 0,
                hour: 0,
                before_noon: BeforeNoon::Am,
                minute: 0,
                second: 0,
                nanos: 0,
            }
        );
    }

    #[test]
    fn everything_overflows_12h() {
        let mut rtc = SeikoRealTimeClock::new();
        rtc.control.hours = Hours::Twelve;
        rtc.datetime = DateTime {
            year: 99,
            month: 12,
            day: 31,
            day_of_week: 6,
            hour: 11,
            before_noon: BeforeNoon::Pm,
            minute: 59,
            second: 59,
            nanos: 0,
        };

        rtc.update_time_internal(1_000_000_000);

        assert_eq!(
            rtc.datetime,
            DateTime {
                year: 0,
                month: 1,
                day: 1,
                day_of_week: 0,
                hour: 0,
                before_noon: BeforeNoon::Am,
                minute: 0,
                second: 0,
                nanos: 0,
            }
        );
    }

    #[test]
    fn am_to_pm_12h() {
        let mut rtc = SeikoRealTimeClock::new();
        rtc.control.hours = Hours::Twelve;
        rtc.datetime = DateTime {
            year: 0,
            month: 1,
            day: 5,
            day_of_week: 0,
            hour: 11,
            before_noon: BeforeNoon::Am,
            minute: 59,
            second: 59,
            nanos: 0,
        };

        rtc.update_time_internal(1_000_000_000);

        assert_eq!(
            rtc.datetime,
            DateTime {
                year: 0,
                month: 1,
                day: 5,
                day_of_week: 0,
                hour: 0,
                before_noon: BeforeNoon::Pm,
                minute: 0,
                second: 0,
                nanos: 0,
            }
        );
    }

    fn leap_year_test(year: u8, expected_day: u8) {
        let mut rtc = SeikoRealTimeClock::new();
        rtc.control.hours = Hours::TwentyFour;
        rtc.datetime = DateTime {
            year,
            month: 2,
            day: 28,
            day_of_week: 0,
            hour: 23,
            before_noon: BeforeNoon::Am,
            minute: 59,
            second: 59,
            nanos: 1_000_000_000 - 1,
        };

        rtc.update_time_internal(1);

        assert_eq!(rtc.datetime.day, expected_day);
    }

    #[test]
    #[rustfmt::skip]
    fn leap_years() {
        leap_year_test(0, 29);  // 2000
        leap_year_test(3, 1);   // 2003
        leap_year_test(4, 29);  // 2004
        leap_year_test(5, 1);   // 2005
        leap_year_test(95, 1);  // 2095
        leap_year_test(96, 29); // 2096
        leap_year_test(97, 1);  // 2097
    }
}
