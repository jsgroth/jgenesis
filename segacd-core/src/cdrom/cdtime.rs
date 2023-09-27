use bincode::{Decode, Encode};
use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::ops::{Add, AddAssign, Sub, SubAssign};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub struct CdTime {
    pub minutes: u8,
    pub seconds: u8,
    pub frames: u8,
}

impl CdTime {
    pub const ZERO: Self = Self { minutes: 0, seconds: 0, frames: 0 };
    pub const DISC_END: Self = Self { minutes: 60, seconds: 3, frames: 74 };

    pub const MAX_MINUTES: u8 = 61;
    pub const SECONDS_PER_MINUTE: u8 = 60;
    pub const FRAMES_PER_SECOND: u8 = 75;

    pub const MAX_SECTORS: u32 = 270000;

    pub fn new(minutes: u8, seconds: u8, frames: u8) -> Self {
        assert!(minutes < Self::MAX_MINUTES, "Minutes must be less than {}", Self::MAX_MINUTES);
        assert!(
            seconds < Self::SECONDS_PER_MINUTE,
            "Seconds must be less than {}",
            Self::SECONDS_PER_MINUTE
        );
        assert!(
            frames < Self::FRAMES_PER_SECOND,
            "Frames must be less than {}",
            Self::FRAMES_PER_SECOND
        );

        Self { minutes, seconds, frames }
    }

    pub fn new_checked(minutes: u8, seconds: u8, frames: u8) -> Option<Self> {
        (minutes < Self::MAX_MINUTES
            && seconds < Self::SECONDS_PER_MINUTE
            && frames < Self::FRAMES_PER_SECOND)
            .then_some(Self { minutes, seconds, frames })
    }

    pub fn to_sector_number(self) -> u32 {
        (u32::from(Self::SECONDS_PER_MINUTE) * u32::from(self.minutes) + u32::from(self.seconds))
            * u32::from(Self::FRAMES_PER_SECOND)
            + u32::from(self.frames)
    }

    pub fn from_sector_number(sector_number: u32) -> Self {
        // All Sega CD sector numbers are less than 270,000
        assert!(sector_number < Self::MAX_SECTORS, "Invalid sector number: {sector_number}");

        let frames = sector_number % u32::from(Self::FRAMES_PER_SECOND);
        let seconds = (sector_number / u32::from(Self::FRAMES_PER_SECOND))
            % u32::from(Self::SECONDS_PER_MINUTE);
        let minutes = sector_number
            / (u32::from(Self::FRAMES_PER_SECOND) * u32::from(Self::SECONDS_PER_MINUTE));

        Self::new(minutes as u8, seconds as u8, frames as u8)
    }

    pub fn to_frames(self) -> u32 {
        let seconds: u32 = self.seconds.into();
        let minutes: u32 = self.minutes.into();
        let frames: u32 = self.frames.into();

        let frames_per_second: u32 = Self::FRAMES_PER_SECOND.into();
        let seconds_per_minute: u32 = Self::SECONDS_PER_MINUTE.into();

        frames + frames_per_second * (seconds + seconds_per_minute * minutes)
    }

    pub fn from_frames(frames: u32) -> Self {
        let minutes =
            frames / (u32::from(Self::FRAMES_PER_SECOND) * u32::from(Self::SECONDS_PER_MINUTE));
        let seconds =
            (frames / u32::from(Self::FRAMES_PER_SECOND)) % u32::from(Self::SECONDS_PER_MINUTE);
        let frames = frames % u32::from(Self::FRAMES_PER_SECOND);

        Self::new(minutes as u8, seconds as u8, frames as u8)
    }
}

impl Add for CdTime {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        let (frames, carried) = add(self.frames, rhs.frames, false, Self::FRAMES_PER_SECOND);
        let (seconds, carried) = add(self.seconds, rhs.seconds, carried, Self::SECONDS_PER_MINUTE);
        let (minutes, _) = add(self.minutes, rhs.minutes, carried, Self::MAX_MINUTES);

        Self { minutes, seconds, frames }
    }
}

impl AddAssign for CdTime {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sub for CdTime {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        let (frames, borrowed) = sub(self.frames, rhs.frames, false, Self::FRAMES_PER_SECOND);
        let (seconds, borrowed) =
            sub(self.seconds, rhs.seconds, borrowed, Self::SECONDS_PER_MINUTE);
        let (minutes, _) = sub(self.minutes, rhs.minutes, borrowed, Self::MAX_MINUTES);

        Self { minutes, seconds, frames }
    }
}

impl SubAssign for CdTime {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl PartialOrd for CdTime {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CdTime {
    fn cmp(&self, other: &Self) -> Ordering {
        self.minutes
            .cmp(&other.minutes)
            .then(self.seconds.cmp(&other.seconds))
            .then(self.frames.cmp(&other.frames))
    }
}

fn add(a: u8, b: u8, overflow: bool, base: u8) -> (u8, bool) {
    let sum = a + b + u8::from(overflow);
    (sum % base, sum >= base)
}

fn sub(a: u8, b: u8, overflow: bool, base: u8) -> (u8, bool) {
    let operand_r = b + u8::from(overflow);
    if a < operand_r { (base - (operand_r - a), true) } else { (a - operand_r, false) }
}

impl FromStr for CdTime {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = s.as_bytes();
        if bytes.len() != 8 {
            return Err(format!("Invalid time length: {}", bytes.len()));
        }

        if bytes[2] != b':' || bytes[5] != b':' {
            return Err(format!("Unexpected time format: {s}"));
        }

        let err_fn = |_err| format!("Invalid time string: {s}");
        let minutes: u8 = s[0..2].parse().map_err(err_fn)?;
        let seconds: u8 = s[3..5].parse().map_err(err_fn)?;
        let frames: u8 = s[6..8].parse().map_err(err_fn)?;

        Ok(CdTime { minutes, seconds, frames })
    }
}

impl Display for CdTime {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:02}:{:02}:{:02}", self.minutes, self.seconds, self.frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cd_time_add() {
        // No carries
        assert_eq!(CdTime::new(10, 20, 30) + CdTime::new(15, 25, 35), CdTime::new(25, 45, 65));

        // Frames carry
        assert_eq!(CdTime::new(10, 20, 30) + CdTime::new(15, 25, 55), CdTime::new(25, 46, 10));

        // Seconds carry
        assert_eq!(CdTime::new(10, 20, 30) + CdTime::new(15, 55, 35), CdTime::new(26, 15, 65));
    }

    #[test]
    fn cd_time_sub() {
        // No borrows
        assert_eq!(CdTime::new(12, 13, 14) - CdTime::new(7, 7, 7), CdTime::new(5, 6, 7));

        // Frames borrow
        assert_eq!(CdTime::new(5, 4, 3) - CdTime::new(1, 1, 10), CdTime::new(4, 2, 68));

        // Seconds borrow
        assert_eq!(CdTime::new(15, 5, 39) - CdTime::new(13, 16, 25), CdTime::new(1, 49, 14));
    }
}
