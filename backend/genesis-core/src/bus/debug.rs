use crate::api::debug::{GenesisCpu, GenesisDebugView, GenesisDebugger, genesis_components};
use crate::bus::GenesisBus;
use m68000_emu::M68000;
use m68000_emu::debug::M68000Debugger;
use s32x_core::api::Sega32X;
use segacd_core::api::SegaCd;
use z80_emu::Z80;
use z80_emu::debug::Z80Debugger;
use z80_emu::traits::InterruptLine;

impl GenesisBus {
    pub fn as_debug_view<'bus, 'z80, 'm68k>(
        &'bus mut self,
        m68k: &'m68k mut M68000,
        z80: &'z80 mut Z80,
    ) -> GenesisDebugView<'m68k, 'z80, 'bus, 'bus, 'bus, 'bus> {
        let components = genesis_components!(self);

        let (s32x_debug_view, s32x_cartridge) =
            match self.sega_32x.as_mut().map(Sega32X::as_debug_view) {
                Some((debug_view, cartridge)) => (Some(debug_view), cartridge),
                None => (None, None),
            };
        let cartridge = s32x_cartridge.or(self.cartridge.as_mut());

        components.into_debug_view(
            m68k,
            z80,
            self.sega_cd.as_mut().map(SegaCd::as_debug_view),
            s32x_debug_view,
            cartridge,
        )
    }
}

pub struct Debug68000Bus<'bus, 'z80, 'debugger> {
    bus: &'bus mut GenesisBus,
    z80: &'z80 mut Z80,
    debugger: &'debugger mut GenesisDebugger,
}

impl<'bus, 'z80, 'debugger> Debug68000Bus<'bus, 'z80, 'debugger> {
    pub fn new(
        bus: &'bus mut GenesisBus,
        z80: &'z80 mut Z80,
        debugger: &'debugger mut GenesisDebugger,
    ) -> Self {
        Self { bus, z80, debugger }
    }
}

pub struct M68000DebugView<'debugbus, 'bus, 'z80, 'debugger>(
    &'debugbus mut Debug68000Bus<'bus, 'z80, 'debugger>,
);

impl M68000Debugger for M68000DebugView<'_, '_, '_, '_> {
    #[inline]
    fn check_read<const WORD: bool>(&mut self, address: u32, cpu: &mut M68000) {
        if self.0.debugger.m68k_breakpoints().check_read::<WORD>(address) {
            log::info!("68000 triggered read breakpoint: {address:06X}");

            self.0.debugger.handle_breakpoint(
                GenesisCpu::M68k,
                &mut self.0.bus.as_debug_view(cpu, self.0.z80),
            );
        }
    }

    #[inline]
    fn check_write<const WORD: bool>(&mut self, address: u32, value: u16, cpu: &mut M68000) {
        if self.0.debugger.m68k_breakpoints().check_write::<WORD>(address) {
            log::info!("68000 triggered write breakpoint: {address:06X} {value:04X}");

            self.0.debugger.handle_breakpoint(
                GenesisCpu::M68k,
                &mut self.0.bus.as_debug_view(cpu, self.0.z80),
            );
        }
    }

    #[inline]
    fn check_execute(&mut self, pc: u32, cpu: &mut M68000) {
        let execute = self.0.debugger.m68k_breakpoints().update_pc_and_check_execute(pc);
        let step = self.0.debugger.m68k_breakpoints().check_break_step();

        if execute {
            log::info!("68000 triggered execute breakpoint: PC={pc:06X}");
        }

        if execute || step {
            self.0.debugger.handle_breakpoint(
                GenesisCpu::M68k,
                &mut self.0.bus.as_debug_view(cpu, self.0.z80),
            );
        }
    }

    #[inline]
    fn check_interrupt(&mut self, interrupt_level: u8, cpu: &mut M68000) {
        if self.0.debugger.m68k_breakpoints().check_interrupt(interrupt_level) {
            log::info!("68000 triggered interrupt breakpoint; interrupt level {interrupt_level}");

            self.0.debugger.handle_breakpoint(
                GenesisCpu::M68k,
                &mut self.0.bus.as_debug_view(cpu, self.0.z80),
            );
        }
    }
}

impl<'bus, 'z80, 'debugger> m68000_emu::BusInterface for Debug68000Bus<'bus, 'z80, 'debugger> {
    type DebugView<'a>
        = M68000DebugView<'a, 'bus, 'z80, 'debugger>
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
        <GenesisBus as m68000_emu::BusInterface>::reset(self.bus)
    }

    #[inline]
    fn debug_view(&mut self) -> Option<Self::DebugView<'_>> {
        Some(M68000DebugView(self))
    }
}

pub struct DebugZ80Bus<'bus, 'm68k, 'debugger> {
    bus: &'bus mut GenesisBus,
    m68k: &'m68k mut M68000,
    debugger: &'debugger mut GenesisDebugger,
}

impl<'bus, 'm68k, 'debugger> DebugZ80Bus<'bus, 'm68k, 'debugger> {
    pub fn new(
        bus: &'bus mut GenesisBus,
        m68k: &'m68k mut M68000,
        debugger: &'debugger mut GenesisDebugger,
    ) -> Self {
        Self { bus, m68k, debugger }
    }
}

pub struct Z80DebugView<'debugbus, 'bus, 'm68k, 'debugger>(
    &'debugbus mut DebugZ80Bus<'bus, 'm68k, 'debugger>,
);

impl Z80Debugger for Z80DebugView<'_, '_, '_, '_> {
    #[inline]
    fn check_read_memory(&mut self, address: u16, cpu: &mut Z80) {
        if self.0.debugger.z80_breakpoints().check_read(address) {
            log::info!("Z80 triggered read breakpoint: {address:04X}");

            self.0.debugger.handle_breakpoint(
                GenesisCpu::Z80,
                &mut self.0.bus.as_debug_view(self.0.m68k, cpu),
            );
        }
    }

    #[inline(always)]
    #[allow(unused_variables)]
    fn check_read_io(&mut self, address: u16, cpu: &mut Z80) {}

    #[inline]
    fn check_write_memory(&mut self, address: u16, value: u8, cpu: &mut Z80) {
        if self.0.debugger.z80_breakpoints().check_write(address) {
            log::info!("Z80 triggered write breakpoint: {address:04X} {value:02X}");

            self.0.debugger.handle_breakpoint(
                GenesisCpu::Z80,
                &mut self.0.bus.as_debug_view(self.0.m68k, cpu),
            );
        }
    }

    #[inline(always)]
    #[allow(unused_variables)]
    fn check_write_io(&mut self, address: u16, value: u8, cpu: &mut Z80) {}

    #[inline]
    fn check_execute(&mut self, pc: u16, cpu: &mut Z80) {
        let execute = self.0.debugger.z80_breakpoints().update_pc_and_check_execute(pc);
        let step = self.0.debugger.z80_breakpoints().check_break_step();

        if execute {
            log::info!("Z80 triggered execute breakpoint: PC={pc:04X}");
        }

        if execute || step {
            self.0.debugger.handle_breakpoint(
                GenesisCpu::Z80,
                &mut self.0.bus.as_debug_view(self.0.m68k, cpu),
            );
        }
    }
}

impl<'bus, 'm68k, 'debugger> z80_emu::BusInterface for DebugZ80Bus<'bus, 'm68k, 'debugger> {
    type DebugView<'a>
        = Z80DebugView<'a, 'bus, 'm68k, 'debugger>
    where
        Self: 'a;

    #[inline]
    fn read_memory(&mut self, address: u16) -> u8 {
        self.bus.read_memory(address)
    }

    #[inline]
    fn write_memory(&mut self, address: u16, value: u8) {
        self.bus.write_memory(address, value);
    }

    #[inline]
    fn read_io(&mut self, address: u16) -> u8 {
        self.bus.read_io(address)
    }

    #[inline]
    fn write_io(&mut self, address: u16, value: u8) {
        self.bus.write_io(address, value);
    }

    #[inline]
    fn nmi(&self) -> InterruptLine {
        self.bus.nmi()
    }

    #[inline]
    fn int(&self) -> InterruptLine {
        self.bus.int()
    }

    #[inline]
    fn busreq(&self) -> bool {
        self.bus.busreq()
    }

    #[inline]
    fn reset(&self) -> bool {
        <GenesisBus as z80_emu::BusInterface>::reset(self.bus)
    }

    #[inline]
    fn debug_view(&mut self) -> Option<Self::DebugView<'_>> {
        Some(Z80DebugView(self))
    }
}
