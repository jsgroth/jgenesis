use crate::api::debug::{BreakWhichCpu, SegaCdDebuggerForSubCpu, SegaCdEmulatorDebugView};
use crate::memory::{ScdCpu, SegaCd, SubBus};
use genesis_core::api::debug::BaseGenesisDebugView;
use m68000_emu::debug::M68000Debugger;
use m68000_emu::{BusInterface, M68000};

pub struct DebugSubBus<'busref, 'bus, 'debug> {
    pub bus: &'busref mut SubBus<'bus>,
    pub debugger: SegaCdDebuggerForSubCpu<'debug>,
}

pub struct DebugSubBusView<'a, 'busref, 'bus, 'debug>(&'a mut DebugSubBus<'busref, 'bus, 'debug>);

impl<'busref, 'bus, 'debug> BusInterface for DebugSubBus<'busref, 'bus, 'debug> {
    type DebugView<'a>
        = DebugSubBusView<'a, 'busref, 'bus, 'debug>
    where
        Self: 'a;

    fn read_byte(&mut self, address: u32) -> u8 {
        self.bus.read_byte(address)
    }

    fn read_word(&mut self, address: u32) -> u16 {
        self.bus.read_word(address)
    }

    fn write_byte(&mut self, address: u32, value: u8) {
        self.bus.write_byte(address, value);
    }

    fn write_word(&mut self, address: u32, value: u16) {
        self.bus.write_word(address, value);
    }

    fn interrupt_level(&self) -> u8 {
        self.bus.interrupt_level()
    }

    fn acknowledge_interrupt(&mut self, interrupt_level: u8) {
        self.bus.acknowledge_interrupt(interrupt_level);
    }

    fn halt(&self) -> bool {
        self.bus.halt()
    }

    fn reset(&self) -> bool {
        self.bus.reset()
    }

    fn debug_view(&mut self) -> Option<Self::DebugView<'_>> {
        Some(DebugSubBusView(self))
    }
}

impl M68000Debugger for DebugSubBusView<'_, '_, '_, '_> {
    fn check_read<const WORD: bool>(&mut self, address: u32, cpu: &mut M68000) {
        if self.0.debugger.debugger.m68k_breakpoints(ScdCpu::Sub).check_read::<WORD>(address) {
            log::info!(
                "Sub CPU {address:06X} {} read triggered breakpoint",
                if WORD { "word" } else { "byte" }
            );
            self.handle_breakpoint(cpu);
        }
    }

    fn check_write<const WORD: bool>(&mut self, address: u32, value: u16, cpu: &mut M68000) {
        if self.0.debugger.debugger.m68k_breakpoints(ScdCpu::Sub).check_write::<WORD>(address) {
            log::info!(
                "Sub CPU {address:06X} {} write {value:04X} triggered breakpoint",
                if WORD { "word" } else { "byte" }
            );
            self.handle_breakpoint(cpu);
        }
    }

    fn check_execute(&mut self, pc: u32, cpu: &mut M68000) {
        self.0.debugger.debugger.update_68k_pc(ScdCpu::Sub, pc);

        let break_step = self.0.debugger.debugger.check_sub_break_step();
        let break_execute =
            self.0.debugger.debugger.m68k_breakpoints(ScdCpu::Sub).check_execute(pc);

        if break_step || break_execute {
            if break_execute {
                log::info!("Sub CPU PC={pc:06X} triggered execute breakpoint");
            }
            self.handle_breakpoint(cpu);
        }
    }
}

impl DebugSubBusView<'_, '_, '_, '_> {
    fn handle_breakpoint(&mut self, cpu: &mut M68000) {
        let mut debug_view = SegaCdEmulatorDebugView {
            genesis: BaseGenesisDebugView {
                m68k: self.0.debugger.main_cpu,
                z80: self.0.debugger.z80,
                memory: self.0.bus.memory.as_debug_view(SegaCd::as_debug_view),
                vdp: self.0.debugger.vdp,
            },
            sub_cpu: cpu,
            pcm: self.0.bus.pcm,
        };
        self.0.debugger.debugger.handle_breakpoint(BreakWhichCpu::Sub, &mut debug_view);
    }
}
