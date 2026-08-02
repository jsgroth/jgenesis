//! Genesis+32X debugger code, in a submodule to limit the blast radius of unsafe usage here

use crate::api::debug::{
    GenesisComponents, GenesisCpu, GenesisDebugView, GenesisDebugger, GenesisDebuggerWithCpus,
};
use crate::bus::PendingWrites;
use genesis_components::cartridge::Cartridge;
use genesis_components::vdp::Vdp;
use genesis_components::ym2612::Ym2612;
use m68000_emu::M68000;
use s32x_core::WhichCpu;
use s32x_core::api::debug::{Sega32XDebugView, Sega32XDebugger};
use segacd_core::api::SegaCd;
use sh2_emu::bus::OpSizeEnum;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::ptr;
use std::ptr::NonNull;
use ti_sn76489::Sn76489;
use z80_emu::Z80;

pub(crate) struct GenesisDebuggerFor32X {
    debugger: NonNull<GenesisDebugger>,
    m68k: NonNull<M68000>,
    z80: NonNull<Z80>,
    sega_cd: *mut SegaCd,
    working_ram: NonNull<[u16]>,
    audio_ram: NonNull<[u8]>,
    z80_bank_number: u32,
    pending_writes: NonNull<PendingWrites>,
    vdp: NonNull<Vdp>,
    ym2612: NonNull<Ym2612>,
    psg: NonNull<Sn76489>,
}

pub(crate) struct GenesisDebuggerFor32XGuard<'debugger, 'scd, 'genesis> {
    debugger: GenesisDebuggerFor32X,
    _debugger_marker: PhantomData<&'debugger ()>,
    _scd_marker: PhantomData<&'scd ()>,
    _genesis_marker: PhantomData<&'genesis ()>,
}

impl Deref for GenesisDebuggerFor32XGuard<'_, '_, '_> {
    type Target = GenesisDebuggerFor32X;

    fn deref(&self) -> &Self::Target {
        // SAFETY: Code outside of this module cannot do anything with a shared reference to a
        // GenesisDebuggerFor32X; all Sega32XDebugger trait methods take a &mut self receiver, and
        // no GenesisDebuggerFor32X fields are visible outside of this module
        &self.debugger
    }
}

impl DerefMut for GenesisDebuggerFor32XGuard<'_, '_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.debugger
    }
}

impl GenesisDebuggerWithCpus<'_, '_> {
    pub(crate) fn for_32x<'scd, 'genesis>(
        &mut self,
        sega_cd: Option<&'scd mut SegaCd>,
        genesis_components: GenesisComponents<'genesis>,
    ) -> GenesisDebuggerFor32XGuard<'_, 'scd, 'genesis> {
        // SAFETY: Creates raw pointers from mutable references here. The returned GenesisDebuggerFor32X
        // is behind a guard so that the caller cannot access any of these mutable references until
        // after dropping the guard
        GenesisDebuggerFor32XGuard {
            debugger: GenesisDebuggerFor32X {
                debugger: self.debugger.into(),
                m68k: self.m68k.into(),
                z80: self.z80.into(),
                sega_cd: sega_cd.map(ptr::from_mut).unwrap_or(ptr::null_mut()),
                working_ram: genesis_components.working_ram.into(),
                audio_ram: genesis_components.audio_ram.into(),
                z80_bank_number: genesis_components.z80_bank_number,
                pending_writes: genesis_components.pending_writes.into(),
                vdp: genesis_components.vdp.into(),
                ym2612: genesis_components.ym2612.into(),
                psg: genesis_components.psg.into(),
            },
            _debugger_marker: PhantomData,
            _scd_marker: PhantomData,
            _genesis_marker: PhantomData,
        }
    }
}

impl Sega32XDebugger for GenesisDebuggerFor32X {
    fn check_sh2_read_breakpoint(
        &mut self,
        which: WhichCpu,
        address: u32,
        size: OpSizeEnum,
    ) -> bool {
        // SAFETY: Debugger pointer was created from a mutable reference, and the debugger is only
        // accessible through a guard
        unsafe {
            self.debugger.as_mut().sh2_breakpoints.breakpoints[which as usize]
                .should_break_read(address, size)
        }
    }

    fn check_sh2_write_breakpoint(
        &mut self,
        which: WhichCpu,
        address: u32,
        _value: u32,
        size: OpSizeEnum,
    ) -> bool {
        // SAFETY: Debugger pointer was created from a mutable reference, and the debugger is only
        // accessible through a guard
        unsafe {
            self.debugger.as_mut().sh2_breakpoints.breakpoints[which as usize]
                .should_break_write(address, size)
        }
    }

    fn check_sh2_execute_breakpoint(&mut self, which: WhichCpu, pc: u32, _opcode: u16) -> bool {
        // SAFETY: Debugger pointer was created from a mutable reference, and the debugger is only
        // accessible through a guard
        unsafe {
            let sh2_breakpoints = &mut self.debugger.as_mut().sh2_breakpoints;

            let execute = sh2_breakpoints.update_pc_and_check_execute(which, pc);
            let step = sh2_breakpoints.check_break_step(which);

            execute || step
        }
    }

    fn check_sh2_interrupt_breakpoint(&mut self, which: WhichCpu, interrupt_level: u8) -> bool {
        // SAFETY: Debugger pointer was created from a mutable reference, and the debugger is only
        // accessible through a guard
        unsafe {
            self.debugger.as_mut().sh2_breakpoints.breakpoints[which as usize]
                .should_break_interrupt(interrupt_level)
        }
    }

    fn handle_sh2_breakpoint(
        &mut self,
        which: WhichCpu,
        cartridge: Option<&mut Cartridge>,
        debug_view: Sega32XDebugView<'_>,
    ) {
        // SAFETY: All of these raw pointers were created from mutable references in
        // GenesisDebuggerWithCpus::for_32x above, and the caller cannot access the underlying
        // values while GenesisDebuggerFor32XGuard is alive, so it is safe to create mutable
        // references from these pointers
        unsafe {
            let mut genesis_debug_view = GenesisDebugView {
                sega_cd: self.sega_cd.as_mut().map(SegaCd::as_debug_view),
                sega_32x: Some(debug_view),
                m68k: self.m68k.as_mut(),
                z80: self.z80.as_mut(),
                cartridge,
                working_ram: self.working_ram.as_mut(),
                audio_ram: self.audio_ram.as_mut(),
                z80_bank_number: self.z80_bank_number,
                pending_writes: self.pending_writes.as_mut(),
                vdp: self.vdp.as_mut(),
                ym2612: self.ym2612.as_mut(),
                psg: self.psg.as_mut(),
            };
            self.debugger
                .as_mut()
                .handle_breakpoint(GenesisCpu::Sh2(which), &mut genesis_debug_view);
        }
    }
}
