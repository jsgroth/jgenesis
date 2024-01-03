use crate::vdp::dma::LineType;
use crate::vdp::registers::HorizontalDisplaySize;
use crate::vdp::DataPortLocation;
use bincode::{Decode, Encode};
use std::collections::VecDeque;

const FIFO_CAPACITY: usize = 4;

#[derive(Debug, Clone, Encode, Decode)]
pub struct FifoTracker {
    fifo: VecDeque<DataPortLocation>,
    mclk_elapsed: f64,
}

impl FifoTracker {
    pub fn new() -> Self {
        Self { fifo: VecDeque::with_capacity(FIFO_CAPACITY + 1), mclk_elapsed: 0.0 }
    }

    pub fn record_access(&mut self, line_type: LineType, data_port_location: DataPortLocation) {
        // VRAM/CRAM/VSRAM accesses can only delay the CPU during active display
        if line_type == LineType::Blanked {
            return;
        }

        self.fifo.push_back(data_port_location);
    }

    pub fn tick(
        &mut self,
        master_clock_cycles: u64,
        h_display_size: HorizontalDisplaySize,
        line_type: LineType,
    ) {
        if self.fifo.is_empty() {
            self.mclk_elapsed = 0.0;
            return;
        }

        if line_type == LineType::Blanked {
            // CPU never gets delayed during VBlank or when the display is off
            self.fifo.clear();
            self.mclk_elapsed = 0.0;
            return;
        }

        // TODO track individual slot cycles instead of doing floating-point arithmetic?

        let mclks_per_slot = match h_display_size {
            HorizontalDisplaySize::ThirtyTwoCell => {
                // 3420 mclks/line / 16 slots/line
                213.75
            }
            HorizontalDisplaySize::FortyCell => {
                // 3420 mclks/line / 18 slots/line
                190.0
            }
        };

        self.mclk_elapsed += master_clock_cycles as f64;
        while self.mclk_elapsed >= mclks_per_slot {
            let Some(&data_port_location) = self.fifo.front() else { break };

            let slots_required = match data_port_location {
                DataPortLocation::Vram => 2.0,
                DataPortLocation::Cram | DataPortLocation::Vsram => 1.0,
            };

            if self.mclk_elapsed < slots_required * mclks_per_slot {
                break;
            }

            self.mclk_elapsed -= slots_required * mclks_per_slot;
            self.fifo.pop_front();
        }
    }

    pub fn is_empty(&self) -> bool {
        self.fifo.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.fifo.len() >= FIFO_CAPACITY
    }

    pub fn should_halt_cpu(&self) -> bool {
        self.fifo.len() > FIFO_CAPACITY
    }
}
