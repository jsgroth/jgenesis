//! RTC-4513 real-time clock chip, used by Tengai Makyou Zero

use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use jgenesis_common::timeutils;
use std::cmp::Ordering;
use std::fmt::{Display, Formatter};

// Bit 7 indicates status, 0=busy and 1=ready; hardcode to 1
pub const STATUS_BYTE: u8 = 0x80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum HourType {
    #[default]
    Am,
    Pm,
}

impl HourType {
    #[must_use]
    const fn toggle(self) -> Self {
        match self {
            Self::Am => Self::Pm,
            Self::Pm => Self::Am,
        }
    }

    const fn from_bit(bit: bool) -> Self {
        if bit { Self::Pm } else { Self::Am }
    }

    fn to_bit(self) -> bool {
        self == Self::Pm
    }
}

impl Display for HourType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Am => write!(f, "AM"),
            Self::Pm => write!(f, "PM"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum ClockHours {
    #[default]
    Twelve,
    TwentyFour,
}

impl ClockHours {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::TwentyFour } else { Self::Twelve }
    }

    fn to_bit(self) -> bool {
        self == Self::TwentyFour
    }
}

impl Display for ClockHours {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Twelve => write!(f, "12-hour"),
            Self::TwentyFour => write!(f, "24-hour"),
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct RtcTime {
    last_update_nanos: u128,
    nanos: u32,
    seconds: u8,
    minutes: u8,
    hours: u8,
    hour_type: HourType,
    day: u8,
    month: u8,
    year: u8,
    day_of_week: u8,
    clock_hours: ClockHours,
    calendar_enabled: bool,
}

impl RtcTime {
    fn new(now_nanos: u128) -> Self {
        Self {
            last_update_nanos: now_nanos,
            nanos: 0,
            seconds: 0,
            minutes: 0,
            hours: 0,
            hour_type: HourType::default(),
            day: 0,
            month: 0,
            year: 0,
            day_of_week: 0,
            clock_hours: ClockHours::default(),
            calendar_enabled: false,
        }
    }

    fn increment_seconds(&mut self) {
        self.seconds += 1;
        if self.seconds >= 60 {
            self.seconds = 0;
            self.increment_minutes();
        }

        log::trace!("Incremented seconds, new time: {self:#?}");
    }

    fn increment_minutes(&mut self) {
        self.minutes += 1;
        if self.minutes >= 60 {
            self.minutes = 0;
            self.increment_hours();
        }
    }

    fn increment_hours(&mut self) {
        self.hours += 1;

        match self.clock_hours {
            ClockHours::TwentyFour => {
                if self.hours >= 24 {
                    self.hours = 0;
                    self.increment_day();
                }
            }
            ClockHours::Twelve => match self.hours.cmp(&12) {
                Ordering::Equal => {
                    self.hour_type = self.hour_type.toggle();
                    if self.hour_type == HourType::Am {
                        self.increment_day();
                    }
                }
                Ordering::Greater => {
                    self.hours = 1;
                }
                Ordering::Less => {}
            },
        }
    }

    fn increment_day(&mut self) {
        if !self.calendar_enabled {
            self.day_of_week = (self.day_of_week + 1) % 7;
            return;
        }

        self.day += 1;
        self.day_of_week = (self.day_of_week + 1) % 7;

        if self.day > timeutils::days_in_month(self.month, self.year) {
            self.day = 1;
            self.increment_month();
        }
    }

    fn increment_month(&mut self) {
        self.month += 1;
        if self.month > 12 {
            self.month = 1;
            self.increment_year();
        }
    }

    fn increment_year(&mut self) {
        // This chip only stores years in the range 0-99
        self.year = (self.year + 1) % 100;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum Command {
    #[default]
    None,
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum InterruptDuty {
    #[default]
    FixedTime,
    UntilAck,
}

impl InterruptDuty {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::UntilAck } else { Self::FixedTime }
    }

    fn to_bit(self) -> bool {
        self == Self::UntilAck
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Rtc4513 {
    command: Command,
    register: Option<u8>,
    time: RtcTime,
    selected: bool,
    wrapped: bool,
    paused: bool,
    pending_seconds_increment: bool,
    stopped: bool,
    reset: bool,
    time_lost: bool,
    irq: bool,
    irq_enabled: bool,
    irq_rate_bits: u8,
    irq_rate_nanos: u128,
    last_irq_nanos: u128,
    irq_duty: InterruptDuty,
    last_day_write_high: u8,
    last_month_write_high: u8,
}

impl Rtc4513 {
    pub fn new() -> Self {
        let now_nanos = timeutils::current_time_nanos();
        Self {
            command: Command::default(),
            register: None,
            time: RtcTime::new(now_nanos),
            selected: false,
            wrapped: false,
            paused: false,
            pending_seconds_increment: false,
            stopped: false,
            reset: false,
            time_lost: true,
            irq: false,
            irq_enabled: true,
            irq_rate_bits: 0,
            irq_rate_nanos: irq_rate_nanos_from_bits(0),
            last_irq_nanos: now_nanos,
            irq_duty: InterruptDuty::default(),
            last_day_write_high: 0,
            last_month_write_high: 0,
        }
    }

    pub fn read_chip_select(&self) -> u8 {
        log::trace!("Chip select read");
        self.selected.into()
    }

    pub fn write_chip_select(&mut self, value: u8) {
        log::trace!("Chip select write {value:02X}");

        let prev_selected = self.selected;
        self.selected = value.bit(0);

        if prev_selected && !self.selected {
            self.wrapped = false;
            self.reset = false;
            self.command = Command::None;
            self.register = None;
        }
    }

    pub fn read_data_port(&mut self) -> u8 {
        if !self.selected || self.command != Command::Read {
            return 0;
        }

        self.update_time();

        let Some(register) = self.register else { return 0 };

        let byte = self.read_register(register);
        log::trace!(
            "RTC data port read, current register is {:X?}, returning {byte:X}",
            self.register
        );

        self.register = Some((register + 1) & 0x0F);
        byte
    }

    pub fn write_data_port(&mut self, value: u8) {
        if !self.selected {
            return;
        }

        self.update_time();

        log::trace!(
            "RTC data port write {value:X}, current register is {:X?} and command is {:?}",
            self.register,
            self.command
        );

        match (self.command, self.register) {
            (Command::None, _) => {
                // First write since select: set command ($03 = write, $0C = read)
                self.command = match value {
                    0x03 => Command::Write,
                    0x0C => Command::Read,
                    _ => Command::None,
                };
            }
            (_, None) => {
                // Second write since select: set register index
                self.register = Some(value & 0x0F);
            }
            (Command::Write, Some(register)) => {
                // Write to selected register and increment register index
                self.write_register(register, value);
                self.register = Some((register + 1) & 0x0F);
            }
            (Command::Read, Some(_)) => {}
        }
    }

    fn read_register(&mut self, register: u8) -> u8 {
        match register {
            0x0 => self.read_seconds_low(),
            0x1 => self.read_seconds_high(),
            0x2 => self.read_minutes_low(),
            0x3 => self.read_minutes_high(),
            0x4 => self.read_hours_low(),
            0x5 => self.read_hours_high(),
            0x6 => self.read_day_low(),
            0x7 => self.read_day_high(),
            0x8 => self.read_month_low(),
            0x9 => self.read_month_high(),
            0xA => self.read_year_low(),
            0xB => self.read_year_high(),
            0xC => self.read_day_of_week(),
            0xD => self.read_control_1(),
            0xE => self.read_control_2(),
            0xF => self.read_control_3(),
            _ => panic!("Invalid RTC-4513 register: {register:02X}"),
        }
    }

    fn write_register(&mut self, register: u8, value: u8) {
        match register {
            0x0 => self.write_seconds_low(value),
            0x1 => self.write_seconds_high(value),
            0x2 => self.write_minutes_low(value),
            0x3 => self.write_minutes_high(value),
            0x4 => self.write_hours_low(value),
            0x5 => self.write_hours_high(value),
            0x6 => self.write_day_low(value),
            0x7 => self.write_day_high(value),
            0x8 => self.write_month_low(value),
            0x9 => self.write_month_high(value),
            0xA => self.write_year_low(value),
            0xB => self.write_year_high(value),
            0xC => self.write_day_of_week(value),
            0xD => self.write_control_1(value),
            0xE => self.write_control_2(value),
            0xF => self.write_control_3(value),
            _ => panic!("Invalid RTC-4513 register: {register:02X}"),
        }
    }

    fn read_seconds_low(&self) -> u8 {
        self.time.seconds % 10
    }

    fn read_seconds_high(&self) -> u8 {
        let lost = u8::from(self.time_lost) << 3;
        lost | (self.time.seconds / 10)
    }

    fn write_seconds_low(&mut self, value: u8) {
        self.time.seconds = self.time.seconds / 10 * 10 + (value & 0x0F);
        log::trace!("  seconds={}", self.time.seconds);
    }

    fn write_seconds_high(&mut self, value: u8) {
        self.time.seconds = 10 * (value & 0x07) + (self.time.seconds % 10);
        self.time_lost = value.bit(3);
        log::trace!("  seconds={}, time_lost={}", self.time.seconds, self.time_lost);
    }

    fn read_minutes_low(&self) -> u8 {
        self.time.minutes % 10
    }

    fn read_minutes_high(&self) -> u8 {
        let wrapped = u8::from(self.wrapped) << 3;
        wrapped | (self.time.minutes / 10)
    }

    fn write_minutes_low(&mut self, value: u8) {
        self.time.minutes = self.time.minutes / 10 * 10 + (value & 0x0F);
        log::trace!("  minutes={}", self.time.minutes);
    }

    fn write_minutes_high(&mut self, value: u8) {
        self.time.minutes = 10 * (value & 0x07) + (self.time.minutes % 10);
        log::trace!("  minutes={}", self.time.minutes);
    }

    fn read_hours_low(&self) -> u8 {
        self.time.hours % 10
    }

    fn read_hours_high(&self) -> u8 {
        let wrapped = u8::from(self.wrapped) << 3;
        let am_pm_bit = u8::from(self.time.hour_type.to_bit()) << 2;
        wrapped | am_pm_bit | (self.time.hours / 10)
    }

    fn write_hours_low(&mut self, value: u8) {
        self.time.hours = self.time.hours / 10 * 10 + (value & 0x0F);
        log::trace!("  hours={}", self.time.hours);
    }

    fn write_hours_high(&mut self, value: u8) {
        let mask = match self.time.clock_hours {
            ClockHours::TwentyFour => 0x03,
            ClockHours::Twelve => 0x01,
        };
        self.time.hours = 10 * (value & mask) + (self.time.hours % 10);

        if self.time.clock_hours == ClockHours::Twelve {
            self.time.hour_type = HourType::from_bit(value.bit(2));
        }

        log::trace!("  hours={}, hour_type={}", self.time.hours, self.time.hour_type);
    }

    fn read_day_low(&self) -> u8 {
        self.time.day % 10
    }

    fn read_day_high(&self) -> u8 {
        let wrapped = u8::from(self.wrapped) << 3;
        let ram_bit = self.last_day_write_high & 0x04;
        wrapped | ram_bit | (self.time.day / 10)
    }

    fn write_day_low(&mut self, value: u8) {
        self.time.day = self.time.day / 10 * 10 + (value & 0x0F);
        log::trace!("  day={}", self.time.day);
    }

    fn write_day_high(&mut self, value: u8) {
        self.time.day = 10 * (value & 0x03) + (self.time.day % 10);
        self.last_day_write_high = value & 0x0F;
        log::trace!("  day={}", self.time.day);
    }

    fn read_month_low(&self) -> u8 {
        self.time.month % 10
    }

    fn read_month_high(&self) -> u8 {
        let wrapped = u8::from(self.wrapped) << 3;
        let ram_bits = self.last_month_write_high & 0x06;
        wrapped | ram_bits | (self.time.month / 10)
    }

    fn write_month_low(&mut self, value: u8) {
        self.time.month = self.time.month / 10 * 10 + (value & 0x0F);
        log::trace!("  month={}", self.time.month);
    }

    fn write_month_high(&mut self, value: u8) {
        self.time.month = 10 * (value & 0x01) + (self.time.month % 10);
        self.last_month_write_high = value & 0x0F;
        log::trace!("  month={}", self.time.month);
    }

    fn read_year_low(&self) -> u8 {
        self.time.year % 10
    }

    fn read_year_high(&self) -> u8 {
        self.time.year / 10
    }

    fn write_year_low(&mut self, value: u8) {
        self.time.year = self.time.year / 10 * 10 + (value & 0x0F);
        log::trace!("  year={}", self.time.year);
    }

    fn write_year_high(&mut self, value: u8) {
        self.time.year = 10 * (value & 0x0F) + (self.time.year % 10);
        log::trace!("  year={}", self.time.year);
    }

    fn read_day_of_week(&self) -> u8 {
        let wrapped = u8::from(self.wrapped) << 3;
        wrapped | self.time.day_of_week
    }

    fn write_day_of_week(&mut self, value: u8) {
        self.time.day_of_week = value & 0x07;
        log::trace!("  day_of_week={}", self.time.day_of_week);
    }

    fn read_control_1(&mut self) -> u8 {
        let irq = self.irq;
        self.irq = false;

        u8::from(self.paused)
            | (u8::from(self.time.calendar_enabled) << 1)
            | (u8::from(irq && self.irq_enabled) << 2)
    }

    fn write_control_1(&mut self, value: u8) {
        // HOLD: Pause clock while set
        self.paused = value.bit(0);

        // CAL/HW: Calendar enabled (day/month/year)
        self.time.calendar_enabled = value.bit(1);

        // 30ADJ: Clear seconds, and increment minutes if seconds was >= 30
        if value.bit(3) {
            let prev_seconds = self.time.seconds;
            self.time.seconds = 0;

            if prev_seconds >= 30 {
                self.time.increment_minutes();
            }
        }

        if !self.paused && self.pending_seconds_increment {
            self.pending_seconds_increment = false;
            self.time.increment_seconds();
        }

        log::trace!(
            "  HOLD={}, CAL/HW={}, 30ADJ={}",
            self.paused,
            self.time.calendar_enabled,
            value.bit(3)
        );
    }

    fn read_control_2(&self) -> u8 {
        u8::from(!self.irq_enabled)
            | (u8::from(self.irq_duty.to_bit()) << 1)
            | (self.irq_rate_bits << 2)
    }

    fn write_control_2(&mut self, value: u8) {
        // MASK: Mask interrupts
        self.irq_enabled = !value.bit(0);

        // DUTY: Interrupt duty
        self.irq_duty = InterruptDuty::from_bit(value.bit(1));

        // RATE: Interrupt rate
        self.irq_rate_bits = (value >> 2) & 0x03;
        self.irq_rate_nanos = irq_rate_nanos_from_bits(self.irq_rate_bits);

        log::trace!(
            "  MASK={}, DUTY={:?}, RATE={}",
            value.bit(0),
            self.irq_duty,
            (value >> 2) & 0x03
        );
    }

    fn read_control_3(&self) -> u8 {
        u8::from(self.reset)
            | (u8::from(self.stopped) << 1)
            | (u8::from(self.time.clock_hours.to_bit()) << 2)
    }

    fn write_control_3(&mut self, value: u8) {
        // RESET: Clear seconds and stop clock (automatically cleared on chip deselect)
        self.reset = value.bit(0);
        if self.reset {
            self.time.seconds = 0;
            self.pending_seconds_increment = false;
        }

        // STOP: Stop clock
        self.stopped = value.bit(1);

        // 24/12: Select 12-hour clock vs. 24-hour clock
        self.time.clock_hours = ClockHours::from_bit(value.bit(2));
        if self.time.clock_hours == ClockHours::TwentyFour {
            self.time.hour_type = HourType::from_bit(false);
        }

        log::trace!(
            "  RESET={}, STOP={}, 24/12={}",
            self.reset,
            self.stopped,
            self.time.clock_hours
        );
    }

    fn update_time(&mut self) {
        if self.stopped || self.reset {
            self.time.last_update_nanos = timeutils::current_time_nanos();
            return;
        }

        let now_nanos = timeutils::current_time_nanos();
        let elapsed = now_nanos.saturating_sub(self.time.last_update_nanos);
        let new_time_nanos = u128::from(self.time.nanos) + elapsed;
        self.time.nanos = (new_time_nanos % 1_000_000_000) as u32;
        self.time.last_update_nanos = now_nanos;

        for _ in 0..new_time_nanos / 1_000_000_000 {
            if self.paused {
                self.pending_seconds_increment = true;
                break;
            }

            self.time.increment_seconds();
            if self.command != Command::None {
                self.wrapped = true;
            }
        }

        if now_nanos - self.last_irq_nanos >= self.irq_rate_nanos {
            if self.irq_enabled {
                log::trace!("Flagging IRQ; now = {now_nanos}, last_irq = {}", self.last_irq_nanos);
                self.irq = true;
            }

            while self.last_irq_nanos + self.irq_rate_nanos <= now_nanos {
                self.last_irq_nanos += self.irq_rate_nanos;
            }
        }

        // Interrupts with fixed duty expire after 7.8ms
        if self.irq_duty == InterruptDuty::FixedTime && now_nanos - self.last_irq_nanos >= 7_800_000
        {
            self.irq = false;
        }
    }
}

impl Default for Rtc4513 {
    fn default() -> Self {
        Self::new()
    }
}

fn irq_rate_nanos_from_bits(rate: u8) -> u128 {
    match rate {
        // 64 times per second
        0 => 1_000_000_000 / 64,
        // Once per second
        1 => 1_000_000_000,
        // Once per minute
        2 => 60 * 1_000_000_000,
        // Once per hour
        3 => 60 * 60 * 1_000_000_000,
        _ => panic!("Invalid RTC-4513 interrupt rate: {rate}"),
    }
}
