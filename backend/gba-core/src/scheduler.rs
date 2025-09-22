use bincode::{Decode, Encode};
use jgenesis_proc_macros::EnumAll;
use std::array;
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, EnumAll)]
pub enum SchedulerEvent {
    VBlankIrq = 0,
    HBlankIrq,
    VCounterIrq,
    PpuEvent,
    TimerOverflow,
    Dummy,
}

impl SchedulerEvent {
    fn as_bit(self) -> u32 {
        1 << (self as u8)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
struct HeapEntry {
    event: SchedulerEvent,
    cycles: u64,
}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.cycles.cmp(&other.cycles)
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Scheduler {
    heap: [HeapEntry; SchedulerEvent::ALL.len()],
    len: usize,
    scheduled_bits: u32,
}

impl Scheduler {
    pub fn new() -> Self {
        // Initialize with a dummy event to avoid ever needing to check if the heap is empty
        Self {
            heap: array::from_fn(|_| HeapEntry { event: SchedulerEvent::Dummy, cycles: u64::MAX }),
            len: 1,
            scheduled_bits: SchedulerEvent::Dummy.as_bit(),
        }
    }

    // Insert if event is not present, update cycles if it is present
    pub fn insert_or_update(&mut self, event: SchedulerEvent, cycles: u64) {
        log::trace!("Inserting event {event:?} at cycles {cycles}");

        if self.scheduled_bits & event.as_bit() != 0 {
            for i in 0..self.len {
                if self.heap[i].event != event {
                    continue;
                }

                let old_cycles = self.heap[i].cycles;
                self.heap[i].cycles = cycles;

                match cycles.cmp(&old_cycles) {
                    Ordering::Less => self.heap_up(i),
                    Ordering::Greater => self.heap_down(i),
                    Ordering::Equal => {}
                }

                return;
            }
        }
        self.scheduled_bits |= event.as_bit();

        self.heap[self.len] = HeapEntry { event, cycles };
        self.len += 1;
        self.heap_up(self.len - 1);
    }

    pub fn remove(&mut self, event: SchedulerEvent) {
        log::trace!("Removing event {event:?}");

        if self.scheduled_bits & event.as_bit() == 0 {
            return;
        }
        self.scheduled_bits &= !event.as_bit();

        for i in 0..self.len {
            if self.heap[i].event == event {
                let old_cycles = self.heap[i].cycles;
                self.heap.swap(i, self.len - 1);
                self.len -= 1;

                match self.heap[i].cycles.cmp(&old_cycles) {
                    Ordering::Less => self.heap_up(i),
                    Ordering::Greater => self.heap_down(i),
                    Ordering::Equal => {}
                }

                return;
            }
        }
    }

    pub fn is_event_ready(&self, cycles: u64) -> bool {
        cycles >= self.heap[0].cycles
    }

    pub fn pop(&mut self, cycles: u64) -> Option<(SchedulerEvent, u64)> {
        if cycles < self.heap[0].cycles {
            return None;
        }

        let HeapEntry { event, cycles } = self.heap[0];
        self.heap.swap(0, self.len - 1);
        self.len -= 1;
        self.heap_down(0);
        self.scheduled_bits &= !event.as_bit();

        log::trace!("Popped event {event:?} at cycles {cycles}");

        Some((event, cycles))
    }

    fn heap_up(&mut self, mut i: usize) {
        while i != 0 {
            let parent = i / 2;
            if self.heap[parent] <= self.heap[i] {
                return;
            }

            self.heap.swap(i, parent);
            i = parent;
        }
    }

    fn heap_down(&mut self, mut i: usize) {
        loop {
            let left = 2 * i + 1;
            if left >= self.len {
                return;
            }
            let right = left + 1;

            if right < self.len
                && self.heap[right] < self.heap[left]
                && self.heap[right] < self.heap[i]
            {
                self.heap.swap(i, right);
                i = right;
            } else if self.heap[left] < self.heap[i] {
                self.heap.swap(i, left);
                i = left;
            } else {
                return;
            }
        }
    }
}
