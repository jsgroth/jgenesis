//! Serialize SH-2 communication port accesses by time (roughly). This is part of the fix for Brutal Unleashed:
//! Above the Claw. The other part of the fix is preventing one SH-2 from getting too far ahead of the other.
//!
//! 68000 accesses do not need to be serialized in the same way because the 68000 runs so much slower
//! than the SH-2s.
//!
//! Using a Vec for this is very inefficient on paper, but each individual Vec will never grow
//! larger than 3, so a fancier data structure would very likely be less efficient in practice.

use bincode::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct CommunicationPort(Vec<(u64, u16)>);

impl CommunicationPort {
    pub fn new() -> Self {
        Self(vec![(0, 0)])
    }

    pub fn m68k_read(&self) -> u16 {
        self.0.last().unwrap().1
    }

    pub fn m68k_write(&mut self, value: u16) {
        self.0.last_mut().unwrap().1 = value;
    }

    pub fn sh2_read(&self, cycle_counter: u64) -> u16 {
        self.0
            .iter()
            .copied()
            .rev()
            .find_map(|(time, value)| (time <= cycle_counter).then_some(value))
            .unwrap_or(0)
    }

    pub fn sh2_write(&mut self, value: u16, cycle_counter: u64) {
        let i = self
            .0
            .iter()
            .copied()
            .position(|(time, _)| time > cycle_counter)
            .unwrap_or(self.0.len());
        self.0.insert(i, (cycle_counter, value));

        // Arbitrary threshold - prevent the Vecs from growing forever
        while self.0.len() > 3 {
            self.0.remove(0);
        }
    }
}
