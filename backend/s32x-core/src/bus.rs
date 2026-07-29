//! 32X memory mapping for the 68000 and SH-2s

pub mod debug;

use crate::pwm::PwmChip;
use crate::registers::{Access, SystemRegisters};
use crate::vdp::Vdp;
use crate::{WhichCpu, bootrom};
use bincode::{Decode, Encode};
use genesis_components::cartridge::Cartridge;
use jgenesis_common::boxedarray::BoxedWordArray;
use jgenesis_common::num::{GetBit, U16Ext};
use sh2_emu::Sh2;
use sh2_emu::bus::{AccessContext, BusInterface, OpSize};
use sh2_emu::debug::DummySh2Debugger;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;
use std::{array, cmp, ptr};

const SDRAM_LEN_WORDS: usize = 256 * 1024 / 2;
const SDRAM_MASK: u32 = 0x3FFFF;

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct SerialInterface {
    pub master_to_slave: Option<u8>,
    pub slave_to_master: Option<u8>,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Sega32XBus {
    pub vdp: Vdp,
    pub pwm: PwmChip,
    pub registers: SystemRegisters,
    pub sdram: BoxedWordArray<SDRAM_LEN_WORDS>,
    pub serial: SerialInterface,
}

pub struct OtherCpu {
    cpu: NonNull<Sh2>,
    cycle_counter: NonNull<u64>,
}

// SH-2 memory map
pub struct Sh2Bus {
    s32x_bus: NonNull<Sega32XBus>,
    cartridge: *mut Cartridge,
    other_sh2: Option<OtherCpu>,
    pub which: WhichCpu,
    pub cycle_counter: u64,
    pub cycle_limit: u64,
}

pub struct Sh2BusGuard<'bus, 'cartridge, 'other> {
    bus: Sh2Bus,
    _bus_marker: PhantomData<&'bus ()>,
    _cartridge_marker: PhantomData<&'cartridge ()>,
    _other_marker: PhantomData<&'other ()>,
}

impl Deref for Sh2BusGuard<'_, '_, '_> {
    type Target = Sh2Bus;

    fn deref(&self) -> &Self::Target {
        &self.bus
    }
}

impl DerefMut for Sh2BusGuard<'_, '_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.bus
    }
}

// All values are minus one because every access takes at least 1 cycle
const SH2_CARTRIDGE_CYCLES: u64 = 7;
const SH2_FRAME_BUFFER_READ_CYCLES: u64 = 4;
const SH2_VDP_CYCLES: u64 = 4;
// SDRAM burst reads take between 10 and 12 cycles; assume always 10 for simplicity
const SH2_SDRAM_READ_CYCLES: u64 = 9;
// SDRAM writes take between 1 and 3 cycles; assume always 1 for simplicity
const SH2_SDRAM_WRITE_CYCLES: u64 = 0;

macro_rules! invalid_size {
    ($size:expr) => {
        panic!("invalid size {}", $size)
    };
}

fn sh2_read_register<const SIZE: u8>(address: u32, mut read_word: impl FnMut(u32) -> u16) -> u32 {
    match SIZE {
        OpSize::BYTE => {
            let word = read_word(address & !1);
            if !address.bit(0) { word.msb().into() } else { word.lsb().into() }
        }
        OpSize::WORD => read_word(address).into(),
        OpSize::LONGWORD => {
            let high: u32 = read_word(address).into();
            let low: u32 = read_word(address | 2).into();
            low | (high << 16)
        }
        _ => invalid_size!(SIZE),
    }
}

fn sh2_write_register<const SIZE: u8>(
    bus: &mut Sega32XBus,
    address: u32,
    value: u32,
    read_word: impl Fn(&mut Sega32XBus, u32) -> u16,
    write_word: impl Fn(&mut Sega32XBus, u32, u16),
) {
    match SIZE {
        OpSize::BYTE => {
            let mut word = read_word(bus, address & !1);
            if !address.bit(0) {
                word.set_msb(value as u8);
            } else {
                word.set_lsb(value as u8);
            }
            write_word(bus, address & !1, word);
        }
        OpSize::WORD => {
            write_word(bus, address, value as u16);
        }
        OpSize::LONGWORD => {
            write_word(bus, address, (value >> 16) as u16);
            write_word(bus, address | 2, value as u16);
        }
        _ => invalid_size!(SIZE),
    }
}

fn sh2_read_memory<const SIZE: u8, const N: usize>(memory: &[u8; N], address: u32) -> u32 {
    match SIZE {
        OpSize::BYTE => read_u8(memory, address).into(),
        OpSize::WORD => read_u16(memory, address).into(),
        OpSize::LONGWORD => read_u32(memory, address),
        _ => invalid_size!(SIZE),
    }
}

fn sh2_read_memory_u16<const SIZE: u8, const N: usize>(memory: &[u16; N], address: u32) -> u32 {
    match SIZE {
        OpSize::BYTE => {
            let memory_addr = ((address >> 1) as usize) & (N - 1);
            let word = memory[memory_addr];
            if !address.bit(0) { word.msb().into() } else { word.lsb().into() }
        }
        OpSize::WORD => {
            let memory_addr = ((address >> 1) as usize) & (N - 1);
            memory[memory_addr].into()
        }
        OpSize::LONGWORD => {
            let memory_addr = ((address >> 1) as usize) & (N - 1) & !1;
            let high: u32 = memory[memory_addr].into();
            let low: u32 = memory[memory_addr + 1].into();
            low | (high << 16)
        }
        _ => invalid_size!(SIZE),
    }
}

fn sh2_write_memory_u16<const SIZE: u8, const N: usize>(
    memory: &mut [u16; N],
    address: u32,
    value: u32,
) {
    match SIZE {
        OpSize::BYTE => {
            let memory_addr = ((address >> 1) as usize) & (N - 1);
            if !address.bit(0) {
                memory[memory_addr].set_msb(value as u8);
            } else {
                memory[memory_addr].set_lsb(value as u8);
            }
        }
        OpSize::WORD => {
            let memory_addr = ((address >> 1) as usize) & (N - 1);
            memory[memory_addr] = value as u16;
        }
        OpSize::LONGWORD => {
            let memory_addr = ((address >> 1) as usize) & (N - 1) & !1;
            memory[memory_addr] = (value >> 16) as u16;
            memory[memory_addr + 1] = value as u16;
        }
        _ => invalid_size!(SIZE),
    }
}

fn sh2_vdp_cycles<const SIZE: u8>() -> u64 {
    match SIZE {
        OpSize::BYTE | OpSize::WORD => SH2_VDP_CYCLES,
        OpSize::LONGWORD => 2 * SH2_VDP_CYCLES,
        _ => invalid_size!(SIZE),
    }
}

impl Sh2Bus {
    #[inline]
    pub fn create<'bus, 'cartridge, 'other>(
        s32x_bus: &'bus mut Sega32XBus,
        cartridge: Option<&'cartridge mut Cartridge>,
        which: WhichCpu,
        cycle_counter: u64,
        cycle_limit: u64,
        other_sh2: Option<(&'other mut Sh2, &'other mut u64)>,
    ) -> Sh2BusGuard<'bus, 'cartridge, 'other> {
        // SAFETY: Sh2Bus contains raw pointers that are created from mutable references here. The
        // returned bus is only accessible through a guard so that the caller cannot reborrow or
        // move the underlying values until after dropping the guard.
        let cartridge = cartridge.map(ptr::from_mut).unwrap_or(ptr::null_mut());
        let other_sh2 = other_sh2.map(|(other_cpu, other_cycles)| OtherCpu {
            cpu: other_cpu.into(),
            cycle_counter: other_cycles.into(),
        });

        Sh2BusGuard {
            bus: Sh2Bus {
                s32x_bus: s32x_bus.into(),
                cartridge,
                other_sh2,
                which,
                cycle_counter,
                cycle_limit,
            },
            _bus_marker: PhantomData,
            _cartridge_marker: PhantomData,
            _other_marker: PhantomData,
        }
    }

    fn s32x_bus(&mut self) -> &mut Sega32XBus {
        // SAFETY: Mutable reference created from a raw pointer that was originally created from a
        // mutable reference.
        // This method mutably borrows the Sh2Bus so only one mutable reference can be created this
        // way at a time. Other code should not create mutable references directly, and must not
        // touch the pointer while a mutable reference is alive.
        unsafe { self.s32x_bus.as_mut() }
    }

    fn s32x_bus_shared(&self) -> &Sega32XBus {
        // SAFETY: Same as above but returns a shared reference from a shared Sh2Bus borrow
        unsafe { self.s32x_bus.as_ref() }
    }

    fn cartridge(&mut self) -> Option<&mut Cartridge> {
        // SAFETY: Similar to above but with a nullable pointer, hence this returns an Option
        unsafe { self.cartridge.as_mut() }
    }

    // Brutal Unleashed: Above the Claw requires fairly close synchronization to prevent
    // the game from freezing due to the master SH-2 missing a communication port write from
    // the slave SH-2. After the slave SH-2 sees a specific value from the master SH-2, it
    // writes to the communication port twice in quick succession, and the master SH-2 must
    // read the first value before it's overwritten
    fn need_to_sync_other(&self, address: u32) -> bool {
        (0x4020..0x4030).contains(&address) && self.other_sh2.is_some()
    }

    fn maybe_sync_other_cpu(&mut self, address: u32) {
        if !self.need_to_sync_other(address) {
            return;
        }

        let Some(OtherCpu { mut cpu, cycle_counter }) = self.other_sh2 else { return };

        // SAFETY: All raw pointers used here were created from mutable references and are
        // guaranteed non-null (except for the cartridge).
        // The original Sh2Bus instance is not used while the other CPU is executing against the
        // bus copy.
        unsafe {
            let limit = cmp::min(self.cycle_limit, self.cycle_counter);
            let mut bus = Sh2Bus {
                s32x_bus: self.s32x_bus,
                cartridge: self.cartridge,
                which: self.which.other(),
                cycle_counter: cycle_counter.read(),
                cycle_limit: limit,
                other_sh2: None,
            };

            while bus.cycle_counter < bus.cycle_limit {
                cpu.as_mut().execute(crate::api::SH2_EXECUTION_SLICE_LEN, &mut bus);
            }
            cycle_counter.write(bus.cycle_counter);
        }
    }

    // $00000000-$01FFFFFF: Boot ROM, 32X registers, 32X CRAM
    fn read_00<const SIZE: u8>(&mut self, address: u32, ctx: AccessContext) -> u32 {
        self.cycle_counter += if SIZE == OpSize::LONGWORD { 2 } else { 1 };

        match address {
            0x4000..=0x402F => {
                // 32X system registers
                log::trace!(
                    "SH-2 {:?} read {} {address:08X}",
                    self.which,
                    OpSize::display::<SIZE>()
                );

                self.maybe_sync_other_cpu(address);

                let which = self.which;
                sh2_read_register::<SIZE>(address, |address| {
                    let bus = self.s32x_bus();
                    bus.registers.sh2_read(address, which, &bus.vdp)
                })
            }
            0x4030..=0x403F => {
                // 32X PWM registers
                log::trace!(
                    "SH-2 {:?} PWM register {} read {address:08X}",
                    self.which,
                    OpSize::display::<SIZE>()
                );

                sh2_read_register::<SIZE>(address, |address| {
                    self.s32x_bus().pwm.read_register(address)
                })
            }
            0x4100..=0x41FF => {
                // 32X VDP registers
                self.cycle_counter += sh2_vdp_cycles::<SIZE>();

                if self.s32x_bus().registers.vdp_access == Access::Sh2 {
                    sh2_read_register::<SIZE>(address, |address| {
                        self.s32x_bus().vdp.read_register(address)
                    })
                } else {
                    log::warn!(
                        "VDP register {} read with FM=0: {address:08X}",
                        OpSize::display::<SIZE>()
                    );
                    // TODO open bus?
                    OpSize::mask::<SIZE>()
                }
            }
            0x4200..=0x43FF => {
                // 32X CRAM
                self.cycle_counter += sh2_vdp_cycles::<SIZE>();

                if self.s32x_bus().registers.vdp_access == Access::Sh2 {
                    sh2_read_register::<SIZE>(address, |address| {
                        self.s32x_bus().vdp.read_cram(address)
                    })
                } else {
                    log::warn!("CRAM {} read with FM=0: {address:08X}", OpSize::display::<SIZE>());
                    // TODO open bus?
                    OpSize::mask::<SIZE>()
                }
            }
            0x0000..=0x3FFF => {
                // Boot ROM
                match self.which {
                    WhichCpu::Master => sh2_read_memory::<SIZE, _>(bootrom::SH2_MASTER, address),
                    WhichCpu::Slave => sh2_read_memory::<SIZE, _>(bootrom::SH2_SLAVE, address),
                }
            }
            _ => {
                log::debug!(
                    "SH-2 {:?} invalid address {} read {address:08X}, ctx: {ctx}",
                    self.which,
                    OpSize::display::<SIZE>()
                );

                // TODO open bus?
                0
            }
        }
    }

    // $02000000-$03FFFFFF: Cartridge
    fn read_02<const SIZE: u8>(&mut self, address: u32) -> u32 {
        self.cycle_counter += if SIZE == OpSize::LONGWORD {
            2 * (1 + SH2_CARTRIDGE_CYCLES)
        } else {
            1 + SH2_CARTRIDGE_CYCLES
        };

        let Some(cartridge) = self.cartridge() else {
            // TODO open bus?
            return !0;
        };

        match SIZE {
            OpSize::BYTE => cartridge.read_byte(address & 0x7FFFFF, !0).into(),
            OpSize::WORD => cartridge.read_word(address & 0x7FFFFF, !0).into(),
            OpSize::LONGWORD => {
                let rom_addr = address & 0x7FFFFF & !3;
                let high: u32 = cartridge.read_word(rom_addr, !0).into();
                let low: u32 = cartridge.read_word(rom_addr | 2, !0).into();
                low | (high << 16)
            }
            _ => invalid_size!(SIZE),
        }
    }

    // $04000000-$05FFFFFF: Frame buffer
    fn read_04<const SIZE: u8>(&mut self, address: u32, ctx: AccessContext) -> u32 {
        self.cycle_counter += if SIZE == OpSize::LONGWORD {
            2 * (1 + SH2_FRAME_BUFFER_READ_CYCLES)
        } else {
            1 + SH2_FRAME_BUFFER_READ_CYCLES
        };

        if self.s32x_bus().registers.vdp_access == Access::Sh2 {
            sh2_read_register::<SIZE>(address, |address| {
                self.s32x_bus().vdp.read_frame_buffer(address)
            })
        } else {
            log::warn!(
                "SH-2 {:?} frame buffer {} read with FM=0: {address:08X}, ctx: {ctx}",
                self.which,
                OpSize::display::<SIZE>()
            );

            // TODO open bus?
            OpSize::mask::<SIZE>()
        }
    }

    // $06000000-$07FFFFFF: SDRAM
    fn read_06<const SIZE: u8>(&mut self, address: u32, ctx: AccessContext) -> u32 {
        if address >= 0x06040000 {
            log::debug!(
                "SH-2 {:?} invalid {} read {address:08X}, ctx: {ctx}",
                self.which,
                OpSize::display::<SIZE>()
            );
            return 0;
        }

        // SDRAM access times are not doubled for longword reads
        self.cycle_counter += 1 + SH2_SDRAM_READ_CYCLES;

        sh2_read_memory_u16::<SIZE, _>(&self.s32x_bus().sdram, address)
    }

    // $00000000-$01FFFFFF: Boot ROM, 32X registers, 32X CRAM
    fn write_00<const SIZE: u8>(&mut self, address: u32, value: u32, ctx: AccessContext) {
        self.cycle_counter += if SIZE == OpSize::LONGWORD { 2 } else { 1 };

        match address {
            0x4000..=0x402F => {
                // 32X system registers
                log::trace!(
                    "SH-2 {:?} {} write {address:08X} {value:08X}",
                    self.which,
                    OpSize::display::<SIZE>()
                );

                self.maybe_sync_other_cpu(address);

                let which = self.which;
                sh2_write_register::<SIZE>(
                    self.s32x_bus(),
                    address,
                    value,
                    |bus, address| bus.registers.sh2_read(address, which, &bus.vdp),
                    |bus, address, word| {
                        bus.registers.sh2_write(address, word, which, &mut bus.vdp);
                    },
                );
            }
            0x4030..=0x403F => {
                // 32X PWM registers
                log::trace!(
                    "SH-2 {:?} PWM register {} write {address:08X} {value:08X}",
                    self.which,
                    OpSize::display::<SIZE>()
                );

                sh2_write_register::<SIZE>(
                    self.s32x_bus(),
                    address,
                    value,
                    |bus, address| bus.pwm.read_register(address),
                    |bus, address, word| bus.pwm.sh2_write_register(address, word),
                );
            }
            0x4100..=0x41FF => {
                // 32X VDP registers
                self.cycle_counter += sh2_vdp_cycles::<SIZE>();

                if self.s32x_bus().registers.vdp_access == Access::Sh2 {
                    sh2_write_register::<SIZE>(
                        self.s32x_bus(),
                        address,
                        value,
                        |bus, address| bus.vdp.read_register(address),
                        |bus, address, word| bus.vdp.write_register(address, word),
                    );
                } else {
                    log::warn!(
                        "VDP register {} write with FM=0: {address:08X} {value:08X}",
                        OpSize::display::<SIZE>()
                    );
                }
            }
            0x4200..=0x43FF => {
                // 32X CRAM
                self.cycle_counter += sh2_vdp_cycles::<SIZE>();

                if self.s32x_bus().registers.vdp_access == Access::Sh2 {
                    sh2_write_register::<SIZE>(
                        self.s32x_bus(),
                        address,
                        value,
                        |bus, address| bus.vdp.read_cram(address),
                        |bus, address, word| bus.vdp.write_cram(address, word),
                    );
                } else {
                    log::warn!(
                        "CRAM {} write with FM=0: {address:08X} {value:08X}",
                        OpSize::display::<SIZE>()
                    );
                }
            }
            _ => {
                log::debug!(
                    "SH-2 {:?} invalid address {} write: {address:08X} {value:08X}, ctx: {ctx}",
                    self.which,
                    OpSize::display::<SIZE>()
                );
            }
        }
    }

    // $04000000-$05FFFFFF: Frame buffer
    fn write_04<const SIZE: u8>(&mut self, address: u32, value: u32, ctx: AccessContext) {
        if self.s32x_bus().registers.vdp_access != Access::Sh2 {
            log::warn!(
                "SH-2 {:?} frame buffer {} write with FM=0: {address:08X} {value:08X}, ctx: {ctx}",
                self.which,
                OpSize::display::<SIZE>()
            );
            return;
        }

        let cycle_counter = self.cycle_counter;
        self.cycle_counter += self.s32x_bus().vdp.frame_buffer_write_latency(cycle_counter);
        if SIZE == OpSize::LONGWORD {
            let cycle_counter = self.cycle_counter;
            self.cycle_counter += self.s32x_bus().vdp.frame_buffer_write_latency(cycle_counter);
        }

        if SIZE == OpSize::BYTE {
            // Treat normal mapping and overwrite image identically because 0 bytes are never
            // written in either case
            self.s32x_bus().vdp.write_frame_buffer_byte(address, value as u8);
            return;
        }

        sh2_write_register::<SIZE>(
            self.s32x_bus(),
            address,
            value,
            |_, _| panic!("read_word should never be called for frame buffer writes"),
            |bus, address, word| {
                if address & 0x20000 == 0 {
                    // Normal frame buffer mapping
                    bus.vdp.write_frame_buffer_word(address, word);
                } else {
                    // Overwrite image
                    bus.vdp.frame_buffer_overwrite_word(address, word);
                }
            },
        );
    }

    // $06000000-$07FFFFFF: SDRAM
    fn write_06<const SIZE: u8>(&mut self, address: u32, value: u32, ctx: AccessContext) {
        if address >= 0x06040000 {
            log::debug!(
                "SH-2 {:?} invalid {} write {address:08X} {value:08X}, ctx: {ctx}",
                self.which,
                OpSize::display::<SIZE>()
            );
            return;
        }

        // No latency difference between 16-bit SDRAM writes and 32-bit SDRAM writes
        self.cycle_counter += 1 + SH2_SDRAM_WRITE_CYCLES;

        sh2_write_memory_u16::<SIZE, _>(&mut self.s32x_bus().sdram, address, value);
    }
}

impl BusInterface for Sh2Bus {
    type DebugView<'a>
        = DummySh2Debugger
    where
        Self: 'a;

    #[inline]
    fn read<const SIZE: u8>(&mut self, address: u32, ctx: AccessContext) -> u32 {
        const BYTE_FNS: [fn(&mut Sh2Bus, u32, AccessContext) -> u32; 4] = [
            |bus, address, ctx| bus.read_00::<{ OpSize::BYTE }>(address, ctx),
            |bus, address, _ctx| bus.read_02::<{ OpSize::BYTE }>(address),
            |bus, address, ctx| bus.read_04::<{ OpSize::BYTE }>(address, ctx),
            |bus, address, ctx| bus.read_06::<{ OpSize::BYTE }>(address, ctx),
        ];

        const WORD_FNS: [fn(&mut Sh2Bus, u32, AccessContext) -> u32; 4] = [
            |bus, address, ctx| bus.read_00::<{ OpSize::WORD }>(address, ctx),
            |bus, address, _ctx| bus.read_02::<{ OpSize::WORD }>(address),
            |bus, address, ctx| bus.read_04::<{ OpSize::WORD }>(address, ctx),
            |bus, address, ctx| bus.read_06::<{ OpSize::WORD }>(address, ctx),
        ];

        const LONGWORD_FNS: [fn(&mut Sh2Bus, u32, AccessContext) -> u32; 4] = [
            |bus, address, ctx| bus.read_00::<{ OpSize::LONGWORD }>(address, ctx),
            |bus, address, _ctx| bus.read_02::<{ OpSize::LONGWORD }>(address),
            |bus, address, ctx| bus.read_04::<{ OpSize::LONGWORD }>(address, ctx),
            |bus, address, ctx| bus.read_06::<{ OpSize::LONGWORD }>(address, ctx),
        ];

        let idx = ((address >> 25) & 3) as usize;
        match SIZE {
            OpSize::BYTE => BYTE_FNS[idx](self, address, ctx),
            OpSize::WORD => WORD_FNS[idx](self, address, ctx),
            OpSize::LONGWORD => LONGWORD_FNS[idx](self, address, ctx),
            _ => invalid_size!(SIZE),
        }
    }

    #[inline]
    fn read_cache_line(&mut self, address: u32, ctx: AccessContext) -> [u16; 8] {
        if (0x06000000..0x06040000).contains(&address) {
            // The SH-2s can read a full 16-byte cache line in 12 cycles
            self.cycle_counter += SH2_SDRAM_READ_CYCLES + 1;

            let base_addr = ((address & SDRAM_MASK & !0xF) >> 1) as usize;
            let sdram = &self.s32x_bus().sdram;
            return array::from_fn(|i| sdram[base_addr + i]);
        }

        array::from_fn(|i| self.read_word(address | ((i as u32) << 1), ctx))
    }

    #[inline]
    fn write<const SIZE: u8>(&mut self, address: u32, value: u32, ctx: AccessContext) {
        const BYTE_FNS: [fn(&mut Sh2Bus, u32, u32, AccessContext); 4] = [
            |bus, address, value, ctx| bus.write_00::<{ OpSize::BYTE }>(address, value, ctx),
            |_, _, _, _| {}, // SH-2s cannot write to the cartridge
            |bus, address, value, ctx| bus.write_04::<{ OpSize::BYTE }>(address, value, ctx),
            |bus, address, value, ctx| bus.write_06::<{ OpSize::BYTE }>(address, value, ctx),
        ];

        const WORD_FNS: [fn(&mut Sh2Bus, u32, u32, AccessContext); 4] = [
            |bus, address, value, ctx| bus.write_00::<{ OpSize::WORD }>(address, value, ctx),
            |_, _, _, _| {}, // SH-2s cannot write to the cartridge
            |bus, address, value, ctx| bus.write_04::<{ OpSize::WORD }>(address, value, ctx),
            |bus, address, value, ctx| bus.write_06::<{ OpSize::WORD }>(address, value, ctx),
        ];

        const LONGWORD_FNS: [fn(&mut Sh2Bus, u32, u32, AccessContext); 4] = [
            |bus, address, value, ctx| bus.write_00::<{ OpSize::LONGWORD }>(address, value, ctx),
            |_, _, _, _| {}, // SH-2s cannot write to the cartridge
            |bus, address, value, ctx| bus.write_04::<{ OpSize::LONGWORD }>(address, value, ctx),
            |bus, address, value, ctx| bus.write_06::<{ OpSize::LONGWORD }>(address, value, ctx),
        ];

        let idx = ((address >> 25) & 3) as usize;
        match SIZE {
            OpSize::BYTE => BYTE_FNS[idx](self, address, value, ctx),
            OpSize::WORD => WORD_FNS[idx](self, address, value, ctx),
            OpSize::LONGWORD => LONGWORD_FNS[idx](self, address, value, ctx),
            _ => invalid_size!(SIZE),
        }
    }

    #[inline]
    fn reset(&self) -> bool {
        self.s32x_bus_shared().registers.reset_sh2
    }

    #[inline]
    fn interrupt_level(&self) -> u8 {
        match self.which {
            WhichCpu::Master => {
                self.s32x_bus_shared().registers.master_interrupts.current_interrupt_level
            }
            WhichCpu::Slave => {
                self.s32x_bus_shared().registers.slave_interrupts.current_interrupt_level
            }
        }
    }

    #[inline]
    fn dma_request_0(&self) -> bool {
        !self.s32x_bus_shared().registers.dma.fifo.sh2_is_empty()
    }

    #[inline]
    fn dma_request_1(&self) -> bool {
        self.s32x_bus_shared().pwm.dma_request_1()
    }

    #[inline]
    fn acknowledge_dreq_1(&mut self) {
        self.s32x_bus().pwm.acknowledge_dreq_1();
    }

    #[inline]
    fn serial_rx(&mut self) -> Option<u8> {
        match self.which {
            WhichCpu::Master => self.s32x_bus().serial.slave_to_master.take(),
            WhichCpu::Slave => self.s32x_bus().serial.master_to_slave.take(),
        }
    }

    #[inline]
    fn serial_tx(&mut self, value: u8) {
        match self.which {
            WhichCpu::Master => self.s32x_bus().serial.master_to_slave = Some(value),
            WhichCpu::Slave => self.s32x_bus().serial.slave_to_master = Some(value),
        }
    }

    #[inline]
    fn increment_cycle_counter(&mut self, cycles: u64) {
        self.cycle_counter += cycles;
    }

    #[inline]
    fn should_stop_execution(&self) -> bool {
        self.cycle_counter >= self.cycle_limit
    }

    sh2_emu::impl_sh2_opcode_table!(Sh2Bus);
}

#[inline]
fn read_u8<const LEN: usize>(slice: &[u8; LEN], address: u32) -> u8 {
    slice[(address as usize) & (LEN - 1)]
}

#[inline]
fn read_u16<const LEN: usize>(slice: &[u8; LEN], address: u32) -> u16 {
    let address = (address as usize) & (LEN - 1) & !1;
    u16::from_be_bytes([slice[address], slice[address + 1]])
}

#[inline]
fn read_u32<const LEN: usize>(slice: &[u8; LEN], address: u32) -> u32 {
    let address = (address as usize) & (LEN - 1) & !3;
    u32::from_be_bytes(slice[address..address + 4].try_into().unwrap())
}
