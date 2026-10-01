//! PCE CD seek time estimation, based on this research and implementation by Dave Shadoff:
//!   <https://github.com/pce-devel/PCECD_seek>
//!
//! Original C implementation is MIT-licensed, Copyright (c) 2019 David Shadoff

use cdrom::cdtime::CdTime;
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy)]
struct SectorGroup {
    sectors_per_revolution: f64,
    start_sector: i32,
    end_sector: i32,
    rotation_ms: f64,
}

impl SectorGroup {
    const fn new(
        sectors_per_revolution: i32,
        start_sector: i32,
        end_sector: i32,
        rotation_ms: f64,
    ) -> Self {
        Self {
            sectors_per_revolution: sectors_per_revolution as f64,
            start_sector,
            end_sector,
            rotation_ms,
        }
    }
}

const SECTOR_GROUPS: &[SectorGroup] = &[
    SectorGroup::new(10, 0, 12572, 133.47),
    SectorGroup::new(11, 12573, 30244, 146.82), // Except for the first and last groups,
    SectorGroup::new(12, 30245, 49523, 160.17), // there are 1606.5 tracks in each range
    SectorGroup::new(13, 49524, 70408, 173.51),
    SectorGroup::new(14, 70409, 92900, 186.86),
    SectorGroup::new(15, 92901, 116998, 200.21),
    SectorGroup::new(16, 116999, 142703, 213.56),
    SectorGroup::new(17, 142704, 170014, 226.90),
    SectorGroup::new(18, 170015, 198932, 240.25),
    SectorGroup::new(19, 198933, 229456, 253.60),
    SectorGroup::new(20, 229457, 261587, 266.95),
    SectorGroup::new(21, 261588, 295324, 280.29),
    SectorGroup::new(22, 295325, 330668, 293.64),
    SectorGroup::new(23, 330669, 333012, 306.99),
];

const DUMMY_END_SECTOR: SectorGroup =
    SectorGroup::new(23, 333013, CdTime::MAX_SECTORS as i32, 306.99);

fn find_sector_group(sector: i32) -> (usize, SectorGroup) {
    SECTOR_GROUPS
        .iter()
        .copied()
        .enumerate()
        .find(|(_, group)| (group.start_sector..=group.end_sector).contains(&sector))
        .unwrap_or((SECTOR_GROUPS.len(), DUMMY_END_SECTOR))
}

pub fn estimate_ms(from: CdTime, to: CdTime) -> f64 {
    let from_sector = from.to_sector_number() as i32;
    let to_sector = to.to_sector_number() as i32;

    // First, we identify which group the start and end are in
    let (from_group_idx, from_group) = find_sector_group(from_sector);
    let (to_group_idx, to_group) = find_sector_group(to_sector);

    // Now we find the track difference
    //
    // Note: except for the first and last sector groups, all groups are 1606.48 tracks per group.
    let track_difference = match to_group_idx.cmp(&from_group_idx) {
        Ordering::Equal => {
            f64::from((to_sector - from_sector).abs()) / to_group.sectors_per_revolution
        }
        Ordering::Greater => {
            f64::from(from_group.end_sector - from_sector) / from_group.sectors_per_revolution
                + f64::from(to_sector - to_group.start_sector) / to_group.sectors_per_revolution
                + 1606.48 * (to_group_idx - from_group_idx - 1) as f64
        }
        Ordering::Less => {
            f64::from(from_sector - from_group.start_sector) / from_group.sectors_per_revolution
                + f64::from(to_group.end_sector - to_sector) / to_group.sectors_per_revolution
                + 1606.48 * (from_group_idx - to_group_idx - 1) as f64
        }
    };

    // Now, we use the algorithm to determine how long to wait
    if (to_sector - from_sector).abs() < 2 {
        3.0 * 1000.0 / 60.0
    } else if (to_sector - from_sector).abs() < 5 {
        9.0 * 1000.0 / 60.0 + to_group.rotation_ms / 2.0
    } else if track_difference <= 80.0 {
        16.0 * 1000.0 / 60.0 + to_group.rotation_ms / 2.0
    } else if track_difference <= 160.0 {
        22.0 * 1000.0 / 60.0 + to_group.rotation_ms / 2.0
    } else if track_difference <= 644.0 {
        22.0 * 1000.0 / 60.0
            + to_group.rotation_ms / 2.0
            + (track_difference - 161.0) * 16.66 / 80.0
    } else {
        36.0 * 1000.0 / 60.0 + (track_difference - 644.0) * 16.66 / 195.0
    }
}

pub fn estimate_clocks(from: CdTime, to: CdTime) -> u32 {
    let seek_time_ms = estimate_ms(from, to);
    (seek_time_ms / 1000.0 * 44100.0).ceil() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_80_minute_disc() {
        // The algorithm above only covers 74-minute discs, so this tests that something sort of
        // reasonable happens with times that go beyond that
        let a = CdTime::new(78, 30, 71);
        let b = CdTime::new(2, 3, 4);

        assert!(estimate_ms(a, b) >= 1500.0);
        assert!(estimate_ms(b, a) >= 1500.0);
    }
}
