//! SNES general-purpose DMA and HBlank DMA code

use crate::bus::Bus;
use crate::memory::{DmaDirection, DmaIncrementMode, HdmaAddressingMode};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use wdc65816_emu::traits::BusInterface;

const CHANNELS: usize = 8;

// Bus B (8-bit) is mapped to $2100-$21FF in Bus A (24-bit)
const BUS_B_BASE_ADDRESS: u32 = 0x002100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum HDmaState {
    Idle,
    ReloadPending,
    Pending,
    Transfer { channel: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum GpDmaState {
    Idle,
    Pending,
    Transfer { channel: u8, bytes_copied: u16 },
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct DmaUnit {
    hdma: HDmaState,
    gpdma: GpDmaState,
    hdma_do_transfer: [bool; CHANNELS],
    last_scanline_mclk: u64,
    dma_active: bool,
    dma_start_mclk: u64,
}

impl DmaUnit {
    pub fn new() -> Self {
        Self {
            hdma: HDmaState::Idle,
            gpdma: GpDmaState::Idle,
            hdma_do_transfer: [false; CHANNELS],
            last_scanline_mclk: 0,
            dma_active: false,
            dma_start_mclk: 0,
        }
    }

    pub fn tick(
        &mut self,
        bus: &mut Bus<'_>,
        total_master_cycles: u64,
        next_cpu_cycle_mclk: impl Fn(&Bus<'_>) -> u64 + Copy,
    ) -> DmaStatus {
        let last_scanline_mclk = self.last_scanline_mclk;
        self.last_scanline_mclk = bus.ppu.scanline_master_cycles();

        // HDMA takes priority over GPDMA if both are active
        if let Some(status) =
            self.tick_hdma(bus, last_scanline_mclk, total_master_cycles, next_cpu_cycle_mclk)
        {
            return status;
        }

        self.tick_gpdma(bus, total_master_cycles, next_cpu_cycle_mclk)
    }

    fn tick_hdma(
        &mut self,
        bus: &mut Bus<'_>,
        last_scanline_mclk: u64,
        total_master_cycles: u64,
        next_cpu_cycle_mclk: impl Fn(&Bus<'_>) -> u64,
    ) -> Option<DmaStatus> {
        match self.hdma {
            HDmaState::Idle => {
                if Self::check_hdma_reload(bus, last_scanline_mclk) {
                    if self.dma_active {
                        return self.hdma_reload(bus, total_master_cycles, next_cpu_cycle_mclk);
                    }
                    self.hdma = HDmaState::ReloadPending;
                } else if Self::check_hdma_start(bus, last_scanline_mclk) {
                    if self.dma_active {
                        return self.hdma_start(bus, total_master_cycles);
                    }
                    self.hdma = HDmaState::Pending;
                }

                None
            }
            HDmaState::ReloadPending => {
                self.hdma_reload(bus, total_master_cycles, next_cpu_cycle_mclk)
            }
            HDmaState::Pending => self.hdma_start(bus, total_master_cycles),
            HDmaState::Transfer { channel } => Some(self.hdma_progress_transfer(
                channel.into(),
                bus,
                total_master_cycles,
                next_cpu_cycle_mclk,
            )),
        }
    }

    fn check_hdma_reload(bus: &mut Bus<'_>, last_scanline_mclk: u64) -> bool {
        // HDMA reload begins at H=4 V=0
        const RELOAD_SCANLINE_MCLK: u64 = 4 * 4;

        let scanline_mclk = bus.ppu.scanline_master_cycles();
        bus.ppu.scanline() == 0
            && scanline_mclk >= RELOAD_SCANLINE_MCLK
            && (last_scanline_mclk < RELOAD_SCANLINE_MCLK || last_scanline_mclk > scanline_mclk)
    }

    fn check_hdma_start(bus: &mut Bus<'_>, last_scanline_mclk: u64) -> bool {
        // HDMA begins at H=276, V in 0-224
        const START_SCANLINE_MCLK: u64 = 276 * 4;

        if !Self::any_hdma_active(bus) {
            return false;
        }

        !bus.ppu.vblank_flag()
            && bus.ppu.scanline_master_cycles() >= START_SCANLINE_MCLK
            && last_scanline_mclk < START_SCANLINE_MCLK
    }

    fn hdma_channel_active(bus: &Bus<'_>, channel: usize) -> bool {
        // HDMA channels go inactive for the rest of the frame when line counter is loaded with 0
        bus.cpu_registers.active_hdma_channels[channel]
            && bus.cpu_registers.hdma_line_counter[channel] != 0
    }

    fn any_hdma_active(bus: &mut Bus<'_>) -> bool {
        (0..CHANNELS).any(|channel| Self::hdma_channel_active(bus, channel))
    }

    fn hdma_reload(
        &mut self,
        bus: &mut Bus<'_>,
        total_master_cycles: u64,
        next_cpu_cycle_mclk: impl Fn(&Bus<'_>) -> u64,
    ) -> Option<DmaStatus> {
        if !bus.cpu_registers.active_hdma_channels.into_iter().any(|b| b) {
            // Reset all do_transfer flags at reload time when no HDMA channels are active
            // Super Ghouls 'N Ghosts depends on this because it enables HDMA mid-frame
            self.hdma_do_transfer.fill(false);
            self.hdma = HDmaState::Idle;
            return None;
        }

        // 8-cycle overhead for HDMA reload if any channels are active
        let mut cycles = 8 + self.start_dma_if_inactive(total_master_cycles);

        for channel in 0..CHANNELS {
            if !bus.cpu_registers.active_hdma_channels[channel] {
                self.hdma_do_transfer[channel] = false;
                continue;
            }

            log::trace!("Reloading HDMA channel {channel}");

            // Active HDMA cancels any active GPDMA on the same channel
            bus.cpu_registers.active_gpdma_channels[channel] = false;

            bus.cpu_registers.hdma_table_current_address[channel] =
                bus.cpu_registers.gpdma_current_address[channel];

            // 8 cycles for each active channel, +16 if in indirect mode
            cycles += 8 + self.hdma_reload_line_counter(bus, channel);
        }

        self.hdma = HDmaState::Idle;
        cycles += self.end_dma_if_done(bus, total_master_cycles + cycles, next_cpu_cycle_mclk);

        Some(DmaStatus::InProgress { master_cycles_elapsed: cycles })
    }

    #[must_use]
    fn hdma_reload_line_counter(&mut self, bus: &mut Bus<'_>, channel: usize) -> u64 {
        log::trace!("Reloading HDMA line counter for channel {channel}");

        let bank = bus.cpu_registers.dma_bank[channel];
        let mut current_addr = bus.cpu_registers.hdma_table_current_address[channel];

        log::trace!("  HDMA table bank={bank:02X}, current address={current_addr:04X}");

        let line_counter = bus.read(u24(bank, current_addr));
        bus.cpu_registers.hdma_line_counter[channel] = line_counter;
        current_addr = current_addr.wrapping_add(1);

        log::trace!("  HDMA line counter: {line_counter:02X}");

        let mut cycles = 0;
        if bus.cpu_registers.hdma_addressing_mode[channel] == HdmaAddressingMode::Indirect {
            cycles = 16;

            let address_lsb = bus.read(u24(bank, current_addr));
            current_addr = current_addr.wrapping_add(1);
            let address_msb = bus.read(u24(bank, current_addr));
            current_addr = current_addr.wrapping_add(1);

            // Same register is used for GPDMA byte counter and HDMA indirect address
            let address = u16::from_le_bytes([address_lsb, address_msb]);
            bus.cpu_registers.gpdma_byte_counter[channel] = address;

            log::trace!(
                "  HDMA indirect bank = {:02X}, indirect address: {address:04X}",
                bus.cpu_registers.hdma_indirect_bank[channel]
            );
        }

        bus.cpu_registers.hdma_table_current_address[channel] = current_addr;
        self.hdma_do_transfer[channel] = true;

        cycles
    }

    fn hdma_start(&mut self, bus: &mut Bus<'_>, total_master_cycles: u64) -> Option<DmaStatus> {
        if !Self::any_hdma_active(bus) {
            self.hdma = HDmaState::Idle;
            return None;
        }

        // 8-cycle overhead at HDMA start if any channels are active
        let cycles = 8 + self.start_dma_if_inactive(total_master_cycles);
        self.hdma = HDmaState::Transfer { channel: 0 };

        Some(DmaStatus::InProgress { master_cycles_elapsed: cycles })
    }

    fn hdma_progress_transfer(
        &mut self,
        mut channel: usize,
        bus: &mut Bus<'_>,
        total_master_cycles: u64,
        next_cpu_cycle_mclk: impl Fn(&Bus<'_>) -> u64,
    ) -> DmaStatus {
        while channel < CHANNELS {
            if !Self::hdma_channel_active(bus, channel) {
                channel += 1;
                continue;
            }

            // Active HDMA cancels any active GPDMA on the same channel
            bus.cpu_registers.active_gpdma_channels[channel] = false;

            if !self.hdma_do_transfer[channel] {
                channel += 1;
                continue;
            }

            // 8 cycles per byte copied (up to 4 bytes / 32 cycles)
            let cycles = hdma_copy_unit(bus, channel);
            self.hdma = HDmaState::Transfer { channel: (channel + 1) as u8 };
            return DmaStatus::InProgress { master_cycles_elapsed: cycles };
        }

        self.hdma_finish(bus, total_master_cycles, next_cpu_cycle_mclk)
    }

    fn hdma_finish(
        &mut self,
        bus: &mut Bus<'_>,
        total_master_cycles: u64,
        next_cpu_cycle_mclk: impl Fn(&Bus<'_>) -> u64,
    ) -> DmaStatus {
        let mut cycles = 0;

        for channel in 0..CHANNELS {
            if !Self::hdma_channel_active(bus, channel) {
                continue;
            }

            let line_counter = bus.cpu_registers.hdma_line_counter[channel].wrapping_sub(1);
            bus.cpu_registers.hdma_line_counter[channel] = line_counter;

            // Highest bit of line counter functions as a repeat flag
            self.hdma_do_transfer[channel] = line_counter.bit(7);

            // 8 cycles per active channel
            // +16 for each channel in indirect mode that reloads its line counter
            cycles += 8;

            // Line counter reloads when lowest 7 bits are 0
            if line_counter & 0x7F == 0 {
                cycles += self.hdma_reload_line_counter(bus, channel);
            }
        }

        self.hdma = HDmaState::Idle;
        cycles += self.end_dma_if_done(bus, total_master_cycles + cycles, next_cpu_cycle_mclk);

        debug_assert_ne!(cycles, 0);
        DmaStatus::InProgress { master_cycles_elapsed: cycles }
    }

    fn tick_gpdma(
        &mut self,
        bus: &mut Bus<'_>,
        total_master_cycles: u64,
        next_cpu_cycle_mclk: impl Fn(&Bus<'_>) -> u64,
    ) -> DmaStatus {
        match self.gpdma {
            GpDmaState::Idle => {
                if bus.cpu_registers.active_gpdma_channels.into_iter().any(|b| b) {
                    self.gpdma = GpDmaState::Pending;
                }
                DmaStatus::None
            }
            GpDmaState::Pending => self.gpdma_start(bus, total_master_cycles),
            GpDmaState::Transfer { channel, bytes_copied } => self.gpdma_progress_transfer(
                channel.into(),
                bytes_copied,
                bus,
                total_master_cycles,
                next_cpu_cycle_mclk,
            ),
        }
    }

    fn gpdma_start(&mut self, bus: &mut Bus<'_>, total_master_cycles: u64) -> DmaStatus {
        if !bus.cpu_registers.active_gpdma_channels.into_iter().any(|b| b) {
            self.gpdma = GpDmaState::Idle;
            return DmaStatus::None;
        }

        if log::log_enabled!(log::Level::Trace) {
            gpdma_start_log(bus);
        }

        // 8-cycle overhead when starting GPDMA
        let cycles = 8 + self.start_dma_if_inactive(total_master_cycles);
        self.gpdma = GpDmaState::Transfer { channel: 0, bytes_copied: 0 };
        DmaStatus::InProgress { master_cycles_elapsed: cycles }
    }

    fn gpdma_progress_transfer(
        &mut self,
        mut channel: usize,
        mut bytes_copied: u16,
        bus: &mut Bus<'_>,
        total_master_cycles: u64,
        next_cpu_cycle_mclk: impl Fn(&Bus<'_>) -> u64,
    ) -> DmaStatus {
        while channel < CHANNELS {
            if !bus.cpu_registers.active_gpdma_channels[channel] {
                channel += 1;
                bytes_copied = 0;
                continue;
            }

            // 8 cycles per byte
            let mut cycles = 8;

            if bytes_copied == 0 {
                // 8-cycle overhead per active channel
                cycles += 8;

                // Notify coprocessor when GPDMA starts a new channel; needed by S-DD1 and SA-1
                let start_address = u24(
                    bus.cpu_registers.dma_bank[channel],
                    bus.cpu_registers.gpdma_current_address[channel],
                );
                bus.memory.notify_dma_start(channel as u8, start_address);
            }

            gpdma_copy_byte(bus, channel, bytes_copied);

            if bus.cpu_registers.gpdma_byte_counter[channel] == 0 {
                bus.cpu_registers.active_gpdma_channels[channel] = false;
                channel += 1;
                bytes_copied = 0;
            } else {
                bytes_copied = bytes_copied.wrapping_add(1);
            }

            self.gpdma = GpDmaState::Transfer { channel: channel as u8, bytes_copied };
            return DmaStatus::InProgress { master_cycles_elapsed: cycles };
        }

        bus.memory.notify_dma_end();

        self.gpdma = GpDmaState::Idle;
        let cycles = self.end_dma_if_done(bus, total_master_cycles, next_cpu_cycle_mclk);
        DmaStatus::InProgress { master_cycles_elapsed: cycles }
    }

    #[must_use]
    fn start_dma_if_inactive(&mut self, total_master_cycles: u64) -> u64 {
        if self.dma_active {
            return 0;
        }

        self.dma_active = true;
        self.dma_start_mclk = total_master_cycles;

        // DMA can only begin at a multiple of 8 cycles since power-on
        8 - (total_master_cycles % 8)
    }

    #[must_use]
    fn end_dma_if_done(
        &mut self,
        bus: &mut Bus<'_>,
        total_master_cycles: u64,
        next_cpu_cycle_mclk: impl Fn(&Bus<'_>) -> u64,
    ) -> u64 {
        if self.hdma != HDmaState::Idle || self.gpdma != GpDmaState::Idle {
            return 0;
        }

        self.dma_active = false;

        // After DMA ends, must wait until a whole number of CPU cycles have elapsed since DMA began
        // CPU clock timing is based on the cycle that will execute after DMA ends
        let dma_elapsed_mclk = total_master_cycles - self.dma_start_mclk;
        let next_cpu_cycle_mclk = next_cpu_cycle_mclk(bus);
        next_cpu_cycle_mclk - (dma_elapsed_mclk % next_cpu_cycle_mclk)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaStatus {
    None,
    InProgress { master_cycles_elapsed: u64 },
}

fn u24(bank: u8, offset: u16) -> u32 {
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
    let bus_a_full_address = u24(bus_a_bank, *bus_a_offset);
    *bus_a_offset = bus_a_offset.wrapping_add(1);

    let bus_b_full_address = BUS_B_BASE_ADDRESS | u32::from(bus_b_address);

    match direction {
        DmaDirection::AtoB => {
            let byte = dma_read_bus_a(bus, bus_a_full_address);
            bus.apply_write(bus_b_full_address, byte);
        }
        DmaDirection::BtoA => {
            let byte = bus.read(bus_b_full_address);
            dma_write_bus_a(bus, bus_a_full_address, byte);
        }
    }
}

fn dma_read_bus_a(bus: &mut Bus<'_>, bus_a_address: u32) -> u8 {
    let bank = (bus_a_address >> 16) & 0xFF;
    let offset = bus_a_address & 0xFFFF;
    match (bank, offset) {
        // DMA cannot read bus B or DMA registers through bus A
        // Krusty's Super Fun House depends on this or else it will write incorrect BG color
        // palettes to CGRAM
        (0x00..=0x3F | 0x80..=0xBF, 0x2100..=0x21FF | 0x4300..=0x43FF) => bus.memory.cpu_open_bus(),
        _ => bus.read(bus_a_address),
    }
}

fn dma_write_bus_a(bus: &mut Bus<'_>, bus_a_address: u32, value: u8) {
    let bank = (bus_a_address >> 16) & 0xFF;
    let offset = bus_a_address & 0xFFFF;
    match (bank, offset) {
        // DMA cannot write to bus B or DMA registers through bus A
        (0x00..=0x3F | 0x80..=0xBF, 0x2100..=0x21FF | 0x4300..=0x43FF) => {}
        _ => bus.apply_write(bus_a_address, value),
    }
}

fn gpdma_copy_byte(bus: &mut Bus<'_>, channel: usize, bytes_copied: u16) {
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

    match bus.cpu_registers.dma_direction[channel] {
        DmaDirection::AtoB => {
            let byte = dma_read_bus_a(bus, bus_a_full_address);
            bus.apply_write(bus_b_address, byte);
        }
        DmaDirection::BtoA => {
            let byte = bus.read(bus_b_address);
            dma_write_bus_a(bus, bus_a_full_address, byte);
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

    bus.cpu_registers.gpdma_byte_counter[channel] =
        bus.cpu_registers.gpdma_byte_counter[channel].wrapping_sub(1);
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
