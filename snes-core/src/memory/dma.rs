use crate::bus::Bus;
use crate::memory::{DmaDirection, DmaIncrementMode, HdmaAddressingMode};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use wdc65816_emu::traits::BusInterface;

// Bus B (8-bit) is mapped to $2100-$21FF in Bus A (24-bit)
const BUS_B_BASE_ADDRESS: u32 = 0x002100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum GpDmaState {
    Idle,
    Pending,
    Copying { channel: u8, bytes_copied: u16 },
}

impl Default for GpDmaState {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum HDmaState {
    Idle,
    Copying { channel: u8 },
}

impl Default for HDmaState {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaStatus {
    None,
    InProgress { master_cycles_elapsed: u64 },
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct DmaUnit {
    gpdma_state: GpDmaState,
    hdma_state: HDmaState,
    hdma_do_transfer: [bool; 8],
    hdma_prev_scanline_mclk: u64,
}

impl DmaUnit {
    pub fn new() -> Self {
        Self {
            gpdma_state: GpDmaState::default(),
            hdma_state: HDmaState::default(),
            hdma_do_transfer: [false; 8],
            hdma_prev_scanline_mclk: 0,
        }
    }

    #[must_use]
    pub fn tick(&mut self, bus: &mut Bus<'_>, total_master_cycles: u64) -> DmaStatus {
        // HDMA takes priority over GPDMA
        let hdma_status = self.tick_hdma(bus);
        self.hdma_prev_scanline_mclk = bus.ppu.scanline_master_cycles();

        if let Some(status) = hdma_status {
            return status;
        }

        self.tick_gpdma(bus, total_master_cycles)
    }

    fn tick_hdma(&mut self, bus: &mut Bus<'_>) -> Option<DmaStatus> {
        let scanline_mclk = bus.ppu.scanline_master_cycles();

        let any_channels_active =
            bus.cpu_registers.active_hdma_channels.iter().copied().any(|active| active);

        // Check if HDMA registers need to be reloaded (V=0 + H=6)
        if bus.ppu.scanline() == 0
            && scanline_mclk >= 24
            && (self.hdma_prev_scanline_mclk < 24 || self.hdma_prev_scanline_mclk > scanline_mclk)
        {
            return if any_channels_active {
                let master_cycles_elapsed = self.hdma_reload(bus);
                Some(DmaStatus::InProgress { master_cycles_elapsed })
            } else {
                // If no HDMA channels are active, clear all do_transfer flags at the end of VBlank;
                // Super Ghouls 'n Ghosts depends on this to render graphics during gameplay
                self.hdma_do_transfer.fill(false);
                None
            };
        }

        // Bail out early if no HDMA channels are active
        if !any_channels_active {
            return None;
        }

        match self.hdma_state {
            HDmaState::Idle => {
                // Check if HDMA transfer should start (V=0-224, H=278)
                if !bus.ppu.vblank_flag()
                    && scanline_mclk >= 4 * 278
                    && self.hdma_prev_scanline_mclk < 4 * 278
                {
                    // Find the first channel that is active *and* has a non-zero line counter
                    let Some((first_active_channel, _)) =
                        bus.cpu_registers.active_hdma_channels.iter().copied().enumerate().find(
                            |&(i, active)| active && bus.cpu_registers.hdma_line_counter[i] != 0,
                        )
                    else {
                        // Either no channels are active or every channel is done for the frame
                        return None;
                    };

                    self.hdma_state = HDmaState::Copying { channel: first_active_channel as u8 };

                    // HDMA incurs an 18-cycle overhead for each active scanline
                    return Some(DmaStatus::InProgress { master_cycles_elapsed: 18 });
                }
            }
            HDmaState::Copying { channel } => {
                let (next_state, master_cycles_elapsed) =
                    self.hdma_process_channel(bus, channel as usize);
                self.hdma_state = next_state;

                return Some(DmaStatus::InProgress { master_cycles_elapsed });
            }
        }

        None
    }

    fn tick_gpdma(&mut self, bus: &mut Bus<'_>, total_master_cycles: u64) -> DmaStatus {
        match self.gpdma_state {
            GpDmaState::Idle => {
                if bus.cpu_registers.active_gpdma_channels.iter().copied().any(|active| active) {
                    self.gpdma_state = GpDmaState::Pending;
                }
                DmaStatus::None
            }
            GpDmaState::Pending => {
                let Some(first_active_channel) = bus
                    .cpu_registers
                    .active_gpdma_channels
                    .iter()
                    .copied()
                    .position(|active| active)
                else {
                    log::warn!("GPDMA somehow started with no active channels; not running DMA");

                    self.gpdma_state = GpDmaState::Idle;
                    return DmaStatus::None;
                };

                if log::log_enabled!(log::Level::Trace) {
                    gpdma_start_log(bus);
                }

                self.gpdma_state =
                    GpDmaState::Copying { channel: first_active_channel as u8, bytes_copied: 0 };

                let initial_wait_cycles = compute_gpdma_initial_wait_cycles(total_master_cycles);
                DmaStatus::InProgress { master_cycles_elapsed: initial_wait_cycles }
            }
            GpDmaState::Copying { channel, bytes_copied } => {
                let next_state = gpdma_copy_byte(bus, channel, bytes_copied);

                let master_cycles_elapsed = match next_state {
                    GpDmaState::Idle => 8,
                    GpDmaState::Copying { channel: next_channel, .. }
                        if channel == next_channel =>
                    {
                        8
                    }
                    GpDmaState::Copying { .. } => {
                        // Include the 8-cycle overhead for starting the new channel
                        16
                    }
                    GpDmaState::Pending => panic!("next GPDMA state should never be pending"),
                };

                self.gpdma_state = next_state;
                DmaStatus::InProgress { master_cycles_elapsed }
            }
        }
    }

    fn hdma_reload(&mut self, bus: &mut Bus<'_>) -> u64 {
        // TODO don't do this all at once?
        // HDMA reload always has an 18-cycle overhead
        let mut cycles = 18;

        for channel in 0..8 {
            if !bus.cpu_registers.active_hdma_channels[channel] {
                self.hdma_do_transfer[channel] = false;
                continue;
            }

            log::trace!("Reloading HDMA channel {channel}");

            // Each active channel adds an 8-cycle overhead
            cycles += 8;

            // Reload HDMA table address (start address in same register as GPDMA current address)
            bus.cpu_registers.hdma_table_current_address[channel] =
                bus.cpu_registers.gpdma_current_address[channel];

            // Load line counter
            cycles += self.hdma_reload_line_counter(bus, channel);
        }

        cycles
    }

    fn hdma_reload_line_counter(&mut self, bus: &mut Bus<'_>, channel: usize) -> u64 {
        log::trace!("Reloading HDMA line counter for channel {channel}");

        let table_bank = bus.cpu_registers.dma_bank[channel];
        let mut table_current_addr = bus.cpu_registers.hdma_table_current_address[channel];

        log::trace!("  HDMA table bank={table_bank:02X}, current address={table_current_addr:04X}");

        // Read the first value from the HDMA table
        let line_counter = bus.read(u24_address(table_bank, table_current_addr));
        bus.cpu_registers.hdma_line_counter[channel] = line_counter;
        table_current_addr = table_current_addr.wrapping_add(1);

        log::trace!("  HDMA line counter: {line_counter:02X}");

        // If necessary, read the indirect address from the table
        let mut extra_cycles = 0;
        if bus.cpu_registers.hdma_addressing_mode[channel] == HdmaAddressingMode::Indirect {
            // Indirect addressing mode adds 16 cycles to reload time
            extra_cycles += 16;

            let address_lsb = bus.read(u24_address(table_bank, table_current_addr));
            let address_msb = bus.read(u24_address(table_bank, table_current_addr.wrapping_add(1)));
            table_current_addr = table_current_addr.wrapping_add(2);

            // HDMA indirect address is stored in the same register as GPDMA byte counter
            let address = u16::from_le_bytes([address_lsb, address_msb]);
            bus.cpu_registers.gpdma_byte_counter[channel] = address;

            log::trace!(
                "  HDMA indirect bank = {:02X}, indirect address: {address:04X}",
                bus.cpu_registers.hdma_indirect_bank[channel]
            );
        }

        bus.cpu_registers.hdma_table_current_address[channel] = table_current_addr;
        self.hdma_do_transfer[channel] = true;

        extra_cycles
    }

    fn hdma_process_channel(&mut self, bus: &mut Bus<'_>, channel: usize) -> (HDmaState, u64) {
        // 8-cycle overhead per active channel
        let mut cycles = 8;

        // Copy a single unit if do_transfer=true
        if self.hdma_do_transfer[channel] {
            cycles += hdma_copy_unit(bus, channel);
        }

        // Decrement line counter
        let line_counter = bus.cpu_registers.hdma_line_counter[channel].wrapping_sub(1);
        bus.cpu_registers.hdma_line_counter[channel] = line_counter;

        // Set do_transfer to repeat (highest bit of line counter)
        self.hdma_do_transfer[channel] = line_counter.bit(7);

        // Check if $43xA needs to be reloaded (when lowest 7 bits of line counter == 0)
        if line_counter & 0x7F == 0 {
            cycles += self.hdma_reload_line_counter(bus, channel);
        }

        let next_state = match bus.cpu_registers.active_hdma_channels[channel + 1..]
            .iter()
            .copied()
            .enumerate()
            .find(|&(i, active)| {
                active && bus.cpu_registers.hdma_line_counter[channel + 1 + i] != 0
            }) {
            Some((next_channel_offset, _)) => {
                let next_channel = channel + 1 + next_channel_offset;
                HDmaState::Copying { channel: next_channel as u8 }
            }
            None => HDmaState::Idle,
        };

        (next_state, cycles)
    }
}

fn u24_address(bank: u8, offset: u16) -> u32 {
    (u32::from(bank) << 16) | u32::from(offset)
}

// Returns number of cycles (8 * bytes copied)
fn hdma_copy_unit(bus: &mut Bus<'_>, channel: usize) -> u64 {
    let (bus_a_bank, mut bus_a_offset) = match bus.cpu_registers.hdma_addressing_mode[channel] {
        HdmaAddressingMode::Direct => (
            bus.cpu_registers.dma_bank[channel],
            bus.cpu_registers.hdma_table_current_address[channel],
        ),
        HdmaAddressingMode::Indirect => (
            bus.cpu_registers.hdma_indirect_bank[channel],
            bus.cpu_registers.gpdma_byte_counter[channel],
        ),
    };

    let bus_b_address = bus.cpu_registers.dma_bus_b_address[channel];
    let direction = bus.cpu_registers.dma_direction[channel];

    log::trace!(
        "HDMA channel {channel}: ABank={bus_a_bank:02X}, AAddr={bus_a_offset:04X}, BAddr={bus_b_address:02X}, Unit={}, Direction={direction:?}, AddrMode={:?}",
        bus.cpu_registers.dma_transfer_unit[channel],
        bus.cpu_registers.hdma_addressing_mode[channel]
    );

    let bytes_copied = match bus.cpu_registers.dma_transfer_unit[channel] {
        0 => {
            // 1 byte, 1 register
            hdma_copy_byte(direction, bus, bus_a_bank, &mut bus_a_offset, bus_b_address);

            1
        }
        1 => {
            // 2 bytes, 2 registers
            hdma_copy_byte(direction, bus, bus_a_bank, &mut bus_a_offset, bus_b_address);
            hdma_copy_byte(
                direction,
                bus,
                bus_a_bank,
                &mut bus_a_offset,
                bus_b_address.wrapping_add(1),
            );

            2
        }
        2 | 6 => {
            // 2 bytes, 1 register
            hdma_copy_byte(direction, bus, bus_a_bank, &mut bus_a_offset, bus_b_address);
            hdma_copy_byte(direction, bus, bus_a_bank, &mut bus_a_offset, bus_b_address);

            2
        }
        3 | 7 => {
            // 4 bytes, 2 registers (serial)
            hdma_copy_byte(direction, bus, bus_a_bank, &mut bus_a_offset, bus_b_address);
            hdma_copy_byte(direction, bus, bus_a_bank, &mut bus_a_offset, bus_b_address);
            hdma_copy_byte(
                direction,
                bus,
                bus_a_bank,
                &mut bus_a_offset,
                bus_b_address.wrapping_add(1),
            );
            hdma_copy_byte(
                direction,
                bus,
                bus_a_bank,
                &mut bus_a_offset,
                bus_b_address.wrapping_add(1),
            );

            4
        }
        4 => {
            // 4 bytes, 4 registers
            for i in 0..4 {
                hdma_copy_byte(
                    direction,
                    bus,
                    bus_a_bank,
                    &mut bus_a_offset,
                    bus_b_address.wrapping_add(i),
                );
            }

            4
        }
        5 => {
            // 4 bytes, 2 registers (alternating)
            for _ in 0..2 {
                hdma_copy_byte(direction, bus, bus_a_bank, &mut bus_a_offset, bus_b_address);
                hdma_copy_byte(
                    direction,
                    bus,
                    bus_a_bank,
                    &mut bus_a_offset,
                    bus_b_address.wrapping_add(1),
                );
            }

            4
        }
        _ => panic!("invalid DMA transfer unit: {}", bus.cpu_registers.dma_transfer_unit[channel]),
    };

    log::trace!("  Copied {bytes_copied} bytes, new AAddress: {bus_a_offset:04X}");

    // Write back incremented bus A address
    match bus.cpu_registers.hdma_addressing_mode[channel] {
        HdmaAddressingMode::Direct => {
            bus.cpu_registers.hdma_table_current_address[channel] = bus_a_offset;
        }
        HdmaAddressingMode::Indirect => {
            bus.cpu_registers.gpdma_byte_counter[channel] = bus_a_offset;
        }
    }

    // Each byte copy takes 8 cycles
    8 * bytes_copied
}

fn hdma_copy_byte(
    direction: DmaDirection,
    bus: &mut Bus<'_>,
    bus_a_bank: u8,
    bus_a_offset: &mut u16,
    bus_b_address: u8,
) {
    let bus_a_full_address = u24_address(bus_a_bank, *bus_a_offset);
    *bus_a_offset = bus_a_offset.wrapping_add(1);

    let bus_b_full_address = BUS_B_BASE_ADDRESS | u32::from(bus_b_address);

    match direction {
        DmaDirection::AtoB => {
            let byte = bus.read(bus_a_full_address);
            bus.write(bus_b_full_address, byte);
        }
        DmaDirection::BtoA => {
            let byte = bus.read(bus_b_full_address);
            bus.write(bus_a_full_address, byte);
        }
    }
}

fn compute_gpdma_initial_wait_cycles(total_master_cycles: u64) -> u64 {
    // Wait until a multiple of 8 master cycles, waiting at least 1 cycle
    let alignment_cycles = 8 - (total_master_cycles & 0x07);

    // Overhead of 8 cycles for GPDMA init, plus 8 cycles for first channel init
    8 + 8 + alignment_cycles
}

fn gpdma_copy_byte(bus: &mut Bus<'_>, channel: u8, bytes_copied: u16) -> GpDmaState {
    let channel = channel as usize;

    let bus_a_bank = bus.cpu_registers.dma_bank[channel];
    let bus_a_address = bus.cpu_registers.gpdma_current_address[channel];
    let bus_a_full_address = (u32::from(bus_a_bank) << 16) | u32::from(bus_a_address);

    // Transfer units (0-7):
    //   0: 1 byte, 1 register
    //   1: 2 bytes, 2 registers
    //   2: 2 bytes, 1 register (functionally same as 0 for GPDMA)
    //   3: 4 bytes, 2 registers (xx, xx, xx+1, xx+1)
    //   4: 4 bytes, 4 registers
    //   5: 4 bytes, 2 registers alternating (xx, xx+1, xx, xx+1) (functionally same as 1 for GPDMA)
    //   6: Same as 2
    //   7: Same as 3
    let transfer_unit = bus.cpu_registers.dma_transfer_unit[channel];
    let bus_b_adjustment = match transfer_unit {
        0 | 2 | 6 => 0,
        1 | 5 => (bytes_copied & 0x01) as u8,
        3 | 7 => ((bytes_copied >> 1) & 0x01) as u8,
        4 => (bytes_copied & 0x03) as u8,
        _ => panic!("invalid transfer unit: {transfer_unit}"),
    };

    let bus_b_address = BUS_B_BASE_ADDRESS
        | u32::from(bus.cpu_registers.dma_bus_b_address[channel].wrapping_add(bus_b_adjustment));

    // TODO handle disallowed accesses, e.g. CPU internal registers and WRAM-to-WRAM DMA
    match bus.cpu_registers.dma_direction[channel] {
        DmaDirection::AtoB => {
            let byte = bus.read(bus_a_full_address);
            bus.write(bus_b_address, byte);
        }
        DmaDirection::BtoA => {
            let byte = bus.read(bus_b_address);
            bus.write(bus_a_full_address, byte);
        }
    }

    match bus.cpu_registers.dma_increment_mode[channel] {
        DmaIncrementMode::Fixed0 | DmaIncrementMode::Fixed1 => {}
        DmaIncrementMode::Increment => {
            bus.cpu_registers.gpdma_current_address[channel] = bus_a_address.wrapping_add(1);
        }
        DmaIncrementMode::Decrement => {
            bus.cpu_registers.gpdma_current_address[channel] = bus_a_address.wrapping_sub(1);
        }
    }

    let byte_counter = bus.cpu_registers.gpdma_byte_counter[channel];
    bus.cpu_registers.gpdma_byte_counter[channel] = byte_counter.wrapping_sub(1);

    // Channel is done when byte counter decrements to 0
    if byte_counter == 1 {
        bus.cpu_registers.active_gpdma_channels[channel] = false;

        return match bus.cpu_registers.active_gpdma_channels[channel + 1..]
            .iter()
            .copied()
            .position(|active| active)
        {
            Some(next_active_channel) => {
                let next_active_channel = (channel + 1 + next_active_channel) as u8;
                GpDmaState::Copying { channel: next_active_channel, bytes_copied: 0 }
            }
            None => GpDmaState::Idle,
        };
    }

    GpDmaState::Copying { channel: channel as u8, bytes_copied: bytes_copied.wrapping_add(1) }
}

fn gpdma_start_log(bus: &Bus<'_>) {
    log::trace!("GPDMA started");
    for (i, active) in bus.cpu_registers.active_gpdma_channels.iter().copied().enumerate() {
        if !active {
            continue;
        }

        log::trace!("  Channel {i} bus A bank: {:02X}", bus.cpu_registers.dma_bank[i]);
        log::trace!(
            "  Channel {i} bus A address: {:04X}",
            bus.cpu_registers.gpdma_current_address[i]
        );
        log::trace!("  Channel {i} bus B address: {:02X}", bus.cpu_registers.dma_bus_b_address[i]);
        log::trace!("  Channel {i} byte counter: {:04X}", bus.cpu_registers.gpdma_byte_counter[i]);
        log::trace!("  Channel {i} direction: {:?}", bus.cpu_registers.dma_direction[i]);
        log::trace!("  Channel {i} transfer unit: {}", bus.cpu_registers.dma_transfer_unit[i]);
        log::trace!("  Channel {i} increment mode: {:?}", bus.cpu_registers.dma_increment_mode[i]);
    }
}
