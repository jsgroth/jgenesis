use crate::bus::Bus;
use bincode::{Decode, Encode};
use std::array;

// Prefetch holds up to 8 halfwords
const PREFETCH_LEN: u8 = 8;

#[derive(Debug, Clone, Encode, Decode)]
pub struct GamePakPrefetcher {
    buffer: [u16; PREFETCH_LEN as usize],
    read_address: u32,
    write_address: u32,
    read_idx: u8,
    write_idx: u8,
    len: u8,
    active: bool,
    fetch_cycles_remaining: u64,
}

impl GamePakPrefetcher {
    pub fn new() -> Self {
        Self {
            buffer: array::from_fn(|_| 0),
            read_address: 0,
            write_address: 0,
            read_idx: 0,
            write_idx: 0,
            len: 0,
            active: false,
            fetch_cycles_remaining: 0,
        }
    }

    pub fn empty(&self) -> bool {
        self.len == 0
    }

    pub fn full(&self) -> bool {
        self.len == PREFETCH_LEN
    }

    pub fn can_use_for(&self, address: u32) -> bool {
        self.read_address == address && (self.active || !self.empty())
    }

    fn push(&mut self, opcode: u16) {
        self.buffer[self.write_idx as usize] = opcode;
        self.write_address += 2;
        self.write_idx = (self.write_idx + 1) % PREFETCH_LEN;
        self.len += 1;
    }

    fn pop(&mut self) -> u16 {
        let opcode = self.buffer[self.read_idx as usize];
        self.read_address += 2;
        self.read_idx = (self.read_idx + 1) % PREFETCH_LEN;
        self.len -= 1;

        opcode
    }
}

impl Bus {
    pub fn prepare_prefetch_read(&mut self, address: u32) {
        if self.prefetch.can_use_for(address) {
            // Prefetch is already in the right spot
            return;
        }

        self.finish_in_progress_fetch();

        self.prefetch.read_address = address;
        self.prefetch.write_address = address;
        self.prefetch.read_idx = 0;
        self.prefetch.write_idx = 0;
        self.prefetch.len = 0;
        self.prefetch.active = true;
        self.prefetch.fetch_cycles_remaining = 1 + self.memory.control().rom_n_wait_states(address);
    }

    fn finish_in_progress_fetch(&mut self) {
        if self.prefetch.fetch_cycles_remaining == 1 {
            // 1-cycle delay when stopping prefetch during last cycle of a fetch
            self.state.cycles += 1;
        }
        self.cartridge.end_rom_burst();
    }

    pub fn prefetch_read(&mut self) -> u16 {
        if self.prefetch.empty() {
            if !self.prefetch.active {
                self.prepare_prefetch_read(self.prefetch.write_address);
            }

            // Block until the first fetch completes
            self.state.cycles += self.prefetch.fetch_cycles_remaining;
            self.advance_prefetch(self.prefetch.fetch_cycles_remaining);
        }

        self.prefetch.pop()
    }

    pub fn advance_prefetch(&mut self, mut cycles: u64) {
        if !self.prefetch.active {
            return;
        }

        while cycles != 0 {
            if cycles >= self.prefetch.fetch_cycles_remaining {
                cycles -= self.prefetch.fetch_cycles_remaining;

                let opcode = self.cartridge.read_rom(self.prefetch.write_address);
                self.prefetch.push(opcode);

                if self.prefetch.full()
                    || self.prefetch.write_address & 0x1FFFF == 0
                    || !self.memory.control().prefetch_enabled
                {
                    // When buffer fills up or prefetch crosses a 128KB page boundary, prefetch
                    // pauses until the buffer is empty
                    self.prefetch.active = false;
                    self.cartridge.end_rom_burst();
                    break;
                }

                self.prefetch.fetch_cycles_remaining =
                    self.rom_access_cycles(self.prefetch.write_address);
            } else {
                self.prefetch.fetch_cycles_remaining -= cycles;
                break;
            }
        }
    }

    pub fn stop_prefetch(&mut self) {
        if self.prefetch.active {
            self.finish_in_progress_fetch();
        }

        self.prefetch.active = false;
        self.prefetch.fetch_cycles_remaining = 0;
        self.prefetch.read_address = 0;
    }
}
