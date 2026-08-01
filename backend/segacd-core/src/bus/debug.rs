use crate::api::debug::{SegaCdDebugView, SegaCdDebugger};
use crate::bus::SegaCdBus;
use m68000_emu::debug::M68000Debugger;
use m68000_emu::{BusInterface, M68000};

pub struct DebugSegaCdBus<'bus, 'debugger, Debugger> {
    bus: &'bus mut SegaCdBus,
    debugger: &'debugger mut Debugger,
}

impl<'bus, 'debugger, Debugger> DebugSegaCdBus<'bus, 'debugger, Debugger> {
    pub fn new(bus: &'bus mut SegaCdBus, debugger: &'debugger mut Debugger) -> Self {
        Self { bus, debugger }
    }
}

impl SegaCdBus {
    pub fn as_debug_view<'slf, 'cpu, 'ret>(
        &'slf mut self,
        sub_cpu: &'cpu mut M68000,
    ) -> SegaCdDebugView<'ret>
    where
        'slf: 'ret,
        'cpu: 'ret,
    {
        SegaCdDebugView {
            sub_cpu,
            bios_rom: self.bios.0.as_mut(),
            prg_ram: self.prg_ram.as_mut_slice(),
            word_ram: &mut self.word_ram,
            pcm: &mut self.pcm,
            cdc: self.disc_drive.cdc_mut(),
            prg_ram_bank: self.registers.prg_ram_bank,
        }
    }
}

pub struct BusDebugView<'debugbus, 'bus, 'debugger, Debugger>(
    &'debugbus mut DebugSegaCdBus<'bus, 'debugger, Debugger>,
);

impl<Debugger: SegaCdDebugger> M68000Debugger for BusDebugView<'_, '_, '_, Debugger> {
    fn check_read<const WORD: bool>(&mut self, address: u32, cpu: &mut M68000) {
        if self.0.debugger.check_sub_read_breakpoint::<WORD>(address) {
            log::info!("Sub 68000 triggered read breakpoint: {address:06X}");

            let debug_view = self.0.bus.as_debug_view(cpu);
            self.0.debugger.handle_sub_breakpoint(debug_view);
        }
    }

    fn check_write<const WORD: bool>(&mut self, address: u32, value: u16, cpu: &mut M68000) {
        if self.0.debugger.check_sub_write_breakpoint::<WORD>(address, value) {
            log::info!("Sub 68000 triggered write breakpoint: {address:06X} {value:04X}");

            let debug_view = self.0.bus.as_debug_view(cpu);
            self.0.debugger.handle_sub_breakpoint(debug_view);
        }
    }

    fn check_execute(&mut self, pc: u32, cpu: &mut M68000) {
        if self.0.debugger.check_sub_execute_breakpoint(pc) {
            log::info!("Sub 68000 triggered execute breakpoint: PC={pc:06X}");

            let debug_view = self.0.bus.as_debug_view(cpu);
            self.0.debugger.handle_sub_breakpoint(debug_view);
        }
    }

    fn check_interrupt(&mut self, interrupt_level: u8, cpu: &mut M68000) {
        if self.0.debugger.check_sub_interrupt_breakpoint(interrupt_level) {
            log::info!(
                "Sub 68000 triggered interrupt breakpoint, interrupt level {interrupt_level}"
            );

            let debug_view = self.0.bus.as_debug_view(cpu);
            self.0.debugger.handle_sub_breakpoint(debug_view);
        }
    }
}

impl<'bus, 'debugger, Debugger: SegaCdDebugger> BusInterface
    for DebugSegaCdBus<'bus, 'debugger, Debugger>
{
    type DebugView<'a>
        = BusDebugView<'a, 'bus, 'debugger, Debugger>
    where
        Self: 'a;

    #[inline]
    fn read_byte(&mut self, address: u32) -> u8 {
        self.bus.read_byte(address)
    }

    #[inline]
    fn read_word(&mut self, address: u32) -> u16 {
        self.bus.read_word(address)
    }

    #[inline]
    fn write_byte(&mut self, address: u32, value: u8) {
        self.bus.write_byte(address, value);
    }

    #[inline]
    fn write_word(&mut self, address: u32, value: u16) {
        self.bus.write_word(address, value);
    }

    #[inline]
    fn interrupt_level(&self) -> u8 {
        self.bus.interrupt_level()
    }

    #[inline]
    fn acknowledge_interrupt(&mut self, interrupt_level: u8) {
        self.bus.acknowledge_interrupt(interrupt_level);
    }

    #[inline]
    fn halt(&self) -> bool {
        self.bus.halt()
    }

    #[inline]
    fn reset(&self) -> bool {
        <SegaCdBus as BusInterface>::reset(self.bus)
    }

    #[inline]
    fn debug_view(&mut self) -> Option<Self::DebugView<'_>> {
        Some(BusDebugView(self))
    }
}
