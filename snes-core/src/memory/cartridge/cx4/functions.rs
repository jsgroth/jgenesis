mod rom;

use crate::memory::cartridge;
use crate::memory::cartridge::cx4::{Cx4Ram, Cx4Registers};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Default)]
struct RiscRegisters {
    a: u32,
    m: i64,
    // Used for reading data from cartridge ROM
    ext_buffer: u8,
    ext_pointer: u32,
    // Used for reading data from CX4 ROM
    rom_buffer: u32,
    // Used for reading data from CX4 RAM
    ram_buffer: u32,
    ram_pointer: u16,
    page: u16,
    zero: bool,
    negative: bool,
    carry: bool,
    call_stack: [(u16, u8); 16],
    call_stack_ptr: u8,
}

impl RiscRegisters {
    fn push_call_stack(&mut self, page: u16, pointer: u8) {
        self.call_stack[self.call_stack_ptr as usize] = (page, pointer);
        self.call_stack_ptr = (self.call_stack_ptr + 1) & 0xF;
    }

    fn pull_call_stack(&mut self) -> (u16, u8) {
        self.call_stack_ptr = self.call_stack_ptr.wrapping_sub(1) & 0xF;
        self.call_stack[self.call_stack_ptr as usize]
    }

    fn set_nz(&mut self, value: u32) {
        self.zero = value & 0xFFFFFF == 0;
        self.negative = value.bit(23);
    }
}

#[allow(clippy::match_same_arms)]
pub(super) fn execute(cx4_registers: &mut Cx4Registers, rom: &[u8], ram: &mut Cx4Ram) {
    let mut risc_registers = RiscRegisters::default();

    log::trace!(
        "Beginning execution with page {:04X} and pointer {:02X}",
        cx4_registers.instruction_page,
        cx4_registers.instruction_pointer
    );

    loop {
        let opcode_addr = cx4_registers.risc_pc();
        let rom_addr = cartridge::lorom_map_rom_address(opcode_addr, rom.len() as u32);
        let opcode = u16::from_le_bytes([rom[rom_addr as usize], rom[(rom_addr + 1) as usize]]);
        cx4_registers.increment_instruction_pointer();

        log::trace!("opcode={opcode:04X}, PC={opcode_addr:06X}");

        // The first 6 bits of opcode are enough to distinguish between instructions
        match opcode & 0xFC00 {
            0x0000 => {
                // nop; do nothing
            }
            0x0800 => jmp(cx4_registers, &risc_registers, opcode),
            0x0C00 => jz(cx4_registers, &risc_registers, opcode),
            0x1000 => jc(cx4_registers, &risc_registers, opcode),
            0x1400 => jn(cx4_registers, &risc_registers, opcode),
            0x1C00 => {
                // "finish"; effectively does nothing, exists for timing purposes only?
            }
            0x2400 => skip(cx4_registers, &risc_registers, opcode),
            0x2800 => call(cx4_registers, &mut risc_registers, opcode),
            0x2C00 => callz(cx4_registers, &mut risc_registers, opcode),
            0x3000 => callc(cx4_registers, &mut risc_registers, opcode),
            0x3400 => calln(cx4_registers, &mut risc_registers, opcode),
            0x3C00 => ret(cx4_registers, &mut risc_registers),
            0x4000 => inc_ext_ptr(&mut risc_registers),
            0x4800 => cmpr_a_op(cx4_registers, &mut risc_registers, opcode),
            0x4C00 => cmpr_a_imm(&mut risc_registers, opcode),
            0x5000 => cmp_a_op(cx4_registers, &mut risc_registers, opcode),
            0x5400 => cmp_a_imm(&mut risc_registers, opcode),
            0x5800 => sign_extend(&mut risc_registers, opcode),
            0x6000 => match opcode & 0x0300 {
                0x0000 => mov_a_op(cx4_registers, &mut risc_registers, opcode),
                0x0100 => mov_mbr_op(cx4_registers, &mut risc_registers, rom, opcode),
                0x0300 => mov_page_op(cx4_registers, &mut risc_registers, opcode),
                _ => {
                    log::warn!("Unexpected mov opcode: {opcode:04X}");
                }
            },
            0x6400 => match opcode & 0x0300 {
                0x0000 => mov_a_imm(&mut risc_registers, opcode),
                0x0300 => mov_page_imm(&mut risc_registers, opcode),
                _ => {
                    log::warn!("Unexpected mov opcode: {opcode:04X}");
                }
            },
            0x6800 => read_ram_op(cx4_registers, &mut risc_registers, ram, opcode),
            0x6C00 => read_ram_imm(&mut risc_registers, ram, opcode),
            0x7000 => read_rom(cx4_registers, &mut risc_registers, opcode),
            0x7800 => movb_page_op(cx4_registers, &mut risc_registers, opcode),
            0x7C00 => movb_page_imm(&mut risc_registers, opcode),
            0x8000 => add_a_op(cx4_registers, &mut risc_registers, opcode),
            0x8400 => add_a_imm(&mut risc_registers, opcode),
            0x8800 => subr_a_op(cx4_registers, &mut risc_registers, opcode),
            0x8C00 => subr_a_imm(&mut risc_registers, opcode),
            0x9000 => sub_a_op(cx4_registers, &mut risc_registers, opcode),
            0x9400 => sub_a_imm(&mut risc_registers, opcode),
            0x9800 => smul_op(cx4_registers, &mut risc_registers, opcode),
            0x9C00 => smul_imm(&mut risc_registers, opcode),
            0xA800 => xor_a_op(cx4_registers, &mut risc_registers, opcode),
            0xAC00 => xor_a_imm(&mut risc_registers, opcode),
            0xB000 => and_a_op(cx4_registers, &mut risc_registers, opcode),
            0xB400 => and_a_imm(&mut risc_registers, opcode),
            0xB800 => or_a_op(cx4_registers, &mut risc_registers, opcode),
            0xBC00 => or_a_imm(&mut risc_registers, opcode),
            0xC000 => shr_a_op(cx4_registers, &mut risc_registers, opcode),
            0xC400 => shr_a_imm(&mut risc_registers, opcode),
            0xC800 => sar_a_op(cx4_registers, &mut risc_registers, opcode),
            0xCC00 => sar_a_imm(&mut risc_registers, opcode),
            0xD000 => ror_a_op(cx4_registers, &mut risc_registers, opcode),
            0xD400 => ror_a_imm(&mut risc_registers, opcode),
            0xD800 => shl_a_op(cx4_registers, &mut risc_registers, opcode),
            0xDC00 => shl_a_imm(&mut risc_registers, opcode),
            0xE000 => mov_op_a(cx4_registers, &mut risc_registers, opcode),
            0xE800 => movb_ram_op(cx4_registers, &risc_registers, ram, opcode),
            0xEC00 => movb_ram_ptr(&risc_registers, ram, opcode),
            0xF000 => swap(cx4_registers, &mut risc_registers, opcode),
            0xFC00 => {
                // stop; function is complete
                return;
            }
            _ => {
                log::warn!("Unexpected opcode: {opcode:04X}");
            }
        }
    }
}

fn mov_a_imm(registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("mov A, #${:02X}", opcode & 0xFF);

    registers.a = (opcode & 0xFF).into();
}

fn mov_a_op(cx4_registers: &Cx4Registers, risc_registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("mov A, ${:02X}", opcode & 0xFF);

    risc_registers.a = read_register(cx4_registers, risc_registers, opcode) & 0xFFFFFF;
}

fn mov_mbr_op(
    cx4_registers: &Cx4Registers,
    risc_registers: &mut RiscRegisters,
    rom: &[u8],
    opcode: u16,
) {
    log::trace!("mov MBR, ${:02X}", opcode & 0xFF);

    let value = if opcode & 0xFF == 0x2E {
        // $612E seems to be the only opcode that reads "register" $2E, which reads a byte from
        // cartridge ROM using the current pointer
        let rom_addr =
            cartridge::lorom_map_rom_address(risc_registers.ext_pointer, rom.len() as u32);
        rom[rom_addr as usize]
    } else {
        read_register(cx4_registers, risc_registers, opcode) as u8
    };
    risc_registers.ext_buffer = value;
}

fn mov_op_a(cx4_registers: &mut Cx4Registers, risc_registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("mov ${:02X}, A", opcode & 0xFF);

    let a = risc_registers.a;
    write_register(cx4_registers, risc_registers, opcode, a);
}

fn mov_page_op(cx4_registers: &Cx4Registers, risc_registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("mov page, {:02X}", opcode & 0xFF);

    let value = read_register(cx4_registers, risc_registers, opcode);
    risc_registers.page = value as u16;
}

fn mov_page_imm(registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("mov page, #${:02X}", opcode & 0xFF);

    registers.page = opcode & 0xFF;
}

fn read_rom(cx4_registers: &Cx4Registers, risc_registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("readrom ${:02X}", opcode & 0xFF);

    let address = read_register(cx4_registers, risc_registers, opcode);
    risc_registers.rom_buffer = rom::read(address);
}

fn read_ram_op(
    cx4_registers: &Cx4Registers,
    risc_registers: &mut RiscRegisters,
    ram: &Cx4Ram,
    opcode: u16,
) {
    log::trace!("readram ${:02X}", opcode & 0xFF);

    let address = read_register(cx4_registers, risc_registers, opcode);
    let value = ram.get((address & 0xFFF) as usize).copied().unwrap_or(0);
    read_ram(risc_registers, opcode, value);
}

fn read_ram_imm(risc_registers: &mut RiscRegisters, ram: &Cx4Ram, opcode: u16) {
    log::trace!("readram #${:02X}", opcode & 0xFF);

    let address = risc_registers.ram_pointer.wrapping_add(opcode & 0xFF);
    let value = ram.get(address as usize).copied().unwrap_or(0);
    read_ram(risc_registers, opcode, value);
}

fn read_ram(registers: &mut RiscRegisters, opcode: u16, ram_value: u8) {
    match opcode & 0x0300 {
        0x0000 => {
            registers.ram_buffer = (registers.ram_buffer & 0xFFFF_FF00) | u32::from(ram_value);
        }
        0x0100 => {
            registers.ram_buffer =
                (registers.ram_buffer & 0xFFFF_00FF) | (u32::from(ram_value) << 8);
        }
        0x0200 => {
            registers.ram_buffer =
                (registers.ram_buffer & 0x0000_FFFF) | (u32::from(ram_value) << 16);
        }
        _ => {
            log::warn!("Unexpected read RAM opcode: {opcode:04X}");
        }
    }
    log::trace!("  read RAM value {ram_value}; RAM buffer is {:06X}", registers.ram_buffer);
}

fn movb_ram_op(
    cx4_registers: &Cx4Registers,
    risc_registers: &RiscRegisters,
    ram: &mut Cx4Ram,
    opcode: u16,
) {
    log::trace!("movb ram[${:02X}], ram_buf", opcode & 0xFF);

    let ram_addr = (read_register(cx4_registers, risc_registers, opcode) & 0xFFF) as usize;
    if ram_addr < ram.len() {
        let value = match opcode & 0x0300 {
            0x0000 => risc_registers.ram_buffer as u8,
            0x0100 => (risc_registers.ram_buffer >> 8) as u8,
            0x0200 => (risc_registers.ram_buffer >> 16) as u8,
            _ => {
                log::warn!("Unexpected movb RAM[..] opcode: {opcode:02X}");
                0x00
            }
        };
        ram[ram_addr] = value;
    }
}

fn movb_ram_ptr(registers: &RiscRegisters, ram: &mut Cx4Ram, opcode: u16) {
    log::trace!("movbram[ptr+#${:02X}], ram_buf", opcode & 0xFF);

    let ram_addr = (registers.ram_pointer.wrapping_add(opcode & 0xFF) & 0xFFF) as usize;
    if ram_addr < ram.len() {
        let value = match opcode & 0x0300 {
            0x0000 => registers.ram_buffer as u8,
            0x0100 => (registers.ram_buffer >> 8) as u8,
            0x0200 => (registers.ram_buffer >> 16) as u8,
            _ => {
                log::warn!("Unexpected movb RAM[..] opcode: {opcode:02X}");
                0x00
            }
        };
        ram[ram_addr] = value;
        log::trace!("  wrote {value:02X} to {ram_addr:03X}");
    }
}

fn movb_page_op(cx4_registers: &Cx4Registers, risc_registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("movb P, ${:02X}", opcode & 0xFF);

    let value = read_register(cx4_registers, risc_registers, opcode);
    mov_page(risc_registers, opcode, value as u8);
}

fn movb_page_imm(registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("movb P, #${:02X}", opcode & 0xFF);

    mov_page(registers, opcode, opcode as u8);
}

fn mov_page(registers: &mut RiscRegisters, opcode: u16, value: u8) {
    if !opcode.bit(8) {
        registers.page = (registers.page & 0xFF00) | u16::from(value);
    } else {
        registers.page = (registers.page & 0x00FF) | (u16::from(value) << 8);
    }
}

fn swap(cx4_registers: &mut Cx4Registers, risc_registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("swap A, ${:02}", opcode & 0xFF);

    let old_a = risc_registers.a;
    risc_registers.a = read_register(cx4_registers, risc_registers, opcode);
    write_register(cx4_registers, risc_registers, opcode, old_a);
}

fn jmp(cx4_registers: &mut Cx4Registers, risc_registers: &RiscRegisters, opcode: u16) {
    log::trace!("jmp {:02X}", opcode & 0xFF);

    execute_jump(cx4_registers, risc_registers, opcode);
}

fn jz(cx4_registers: &mut Cx4Registers, risc_registers: &RiscRegisters, opcode: u16) {
    log::trace!("jz {:02X}", opcode & 0xFF);

    if risc_registers.zero {
        execute_jump(cx4_registers, risc_registers, opcode);
    }
}

fn jc(cx4_registers: &mut Cx4Registers, risc_registers: &RiscRegisters, opcode: u16) {
    log::trace!("jc {:02X}", opcode & 0xFF);

    if risc_registers.carry {
        execute_jump(cx4_registers, risc_registers, opcode);
    }
}

fn jn(cx4_registers: &mut Cx4Registers, risc_registers: &RiscRegisters, opcode: u16) {
    log::trace!("jn {:02X}", opcode & 0xFF);

    if risc_registers.negative {
        execute_jump(cx4_registers, risc_registers, opcode);
    }
}

fn call(cx4_registers: &mut Cx4Registers, risc_registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("call {:02X}", opcode & 0xFF);

    risc_registers
        .push_call_stack(cx4_registers.instruction_page, cx4_registers.instruction_pointer);
    execute_jump(cx4_registers, risc_registers, opcode);
}

fn callz(cx4_registers: &mut Cx4Registers, risc_registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("callz {:02X}", opcode & 0xFF);

    if risc_registers.zero {
        risc_registers
            .push_call_stack(cx4_registers.instruction_page, cx4_registers.instruction_pointer);
        execute_jump(cx4_registers, risc_registers, opcode);
    }
}

fn callc(cx4_registers: &mut Cx4Registers, risc_registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("callc {:02X}", opcode & 0xFF);

    if risc_registers.carry {
        risc_registers
            .push_call_stack(cx4_registers.instruction_page, cx4_registers.instruction_pointer);
        execute_jump(cx4_registers, risc_registers, opcode);
    }
}

fn calln(cx4_registers: &mut Cx4Registers, risc_registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("calln {:02X}", opcode & 0xFF);

    if risc_registers.negative {
        risc_registers
            .push_call_stack(cx4_registers.instruction_page, cx4_registers.instruction_pointer);
        execute_jump(cx4_registers, risc_registers, opcode);
    }
}

fn ret(cx4_registers: &mut Cx4Registers, risc_registers: &mut RiscRegisters) {
    log::trace!("ret");

    let (page, pointer) = risc_registers.pull_call_stack();
    cx4_registers.instruction_page = page;
    cx4_registers.instruction_pointer = pointer;
}

fn execute_jump(cx4_registers: &mut Cx4Registers, risc_registers: &RiscRegisters, opcode: u16) {
    cx4_registers.instruction_pointer = opcode as u8;
    if opcode.bit(9) {
        cx4_registers.instruction_page = risc_registers.page;
    }
}

fn skip(cx4_registers: &mut Cx4Registers, risc_registers: &RiscRegisters, opcode: u16) {
    log::trace!("skip, opcode={opcode:04X}");

    let value = opcode.bit(0);
    let flag = match opcode & 0x0300 {
        0x0100 => risc_registers.carry,
        0x0200 => risc_registers.zero,
        0x0300 => risc_registers.negative,
        _ => {
            log::warn!("Unexpected skip opcode: {opcode:02X}");
            false
        }
    };
    if flag == value {
        cx4_registers.instruction_pointer = cx4_registers.instruction_pointer.wrapping_add(1);
    }
}

fn add_a_op(cx4_registers: &Cx4Registers, risc_registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("add A, ${:02X}", opcode & 0xFF);

    let value = read_register(cx4_registers, risc_registers, opcode);
    add(risc_registers, opcode, value);
}

fn add_a_imm(registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("add A, #${:02X}", opcode & 0xFF);

    add(registers, opcode, (opcode & 0xFF).into());
}

fn add(registers: &mut RiscRegisters, opcode: u16, value: u32) {
    let result = apply_a_shift(registers.a, opcode) + value;
    registers.set_nz(result);
    registers.carry = result > 0xFFFFFF;

    registers.a = result & 0xFFFFFF;
}

fn subr_a_op(cx4_registers: &Cx4Registers, risc_registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("subr A, ${:02X}", opcode & 0xFF);

    let value = read_register(cx4_registers, risc_registers, opcode);
    sub(risc_registers, opcode, value, true);
}

fn subr_a_imm(registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("subr A, #${:02X}", opcode & 0xFF);

    sub(registers, opcode, (opcode & 0xFF).into(), true);
}

fn sub_a_op(cx4_registers: &Cx4Registers, risc_registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("sub A, ${:02X}", opcode & 0xFF);

    let value = read_register(cx4_registers, risc_registers, opcode);
    sub(risc_registers, opcode, value, false);
}

fn sub_a_imm(registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("sub A, #${:02X}", opcode & 0xFF);

    sub(registers, opcode, (opcode & 0xFF).into(), false);
}

fn sub(registers: &mut RiscRegisters, opcode: u16, value: u32, reverse: bool) {
    let a = apply_a_shift(registers.a, opcode);
    let result = if reverse { value.wrapping_sub(a) } else { a.wrapping_sub(value) };
    registers.set_nz(result);
    registers.carry = result <= 0xFFFFFF;

    registers.a = result & 0xFFFFFF;
}

fn cmpr_a_op(cx4_registers: &Cx4Registers, risc_registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("cmpr A, ${:02X}", opcode & 0xFF);

    let value = read_register(cx4_registers, risc_registers, opcode);
    cmp(risc_registers, opcode, value, true);
}

fn cmpr_a_imm(registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("cmpr A, #${:02X}", opcode & 0xFF);

    cmp(registers, opcode, (opcode & 0xFF).into(), true);
}

fn cmp_a_op(cx4_registers: &Cx4Registers, risc_registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("cmp A, ${:02X}", opcode & 0xFF);

    let value = read_register(cx4_registers, risc_registers, opcode);
    cmp(risc_registers, opcode, value, false);
}

fn cmp_a_imm(registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("cmp A, #${:02X}", opcode & 0xFF);

    cmp(registers, opcode, (opcode & 0xFF).into(), false);
}

fn cmp(registers: &mut RiscRegisters, opcode: u16, value: u32, reverse: bool) {
    let a = apply_a_shift(registers.a, opcode);
    let result = if reverse { value.wrapping_sub(a) } else { a.wrapping_sub(value) };
    registers.set_nz(result);
    registers.carry = result <= 0xFFFFFF;
}

fn smul_op(cx4_registers: &Cx4Registers, risc_registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("smul ${:02X}", opcode & 0xFF);

    let value = read_register(cx4_registers, risc_registers, opcode);
    smul(risc_registers, value);
}

fn smul_imm(registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("smul #${:02X}", opcode & 0xFF);

    let value = (opcode & 0xFF).into();
    smul(registers, value);
}

fn smul(registers: &mut RiscRegisters, value: u32) {
    // smul multiplies two signed 24-bit integers to produce a signed 48-bit result
    // Simulate this by sign extending the 24-bit numbers to 64 bits and doing the multiplication
    // in 64 bits
    let a = ((registers.a as i32) << 8) >> 8;
    let b = ((value as i32) << 8) >> 8;
    registers.m = i64::from(a) * i64::from(b);
}

fn and_a_op(cx4_registers: &Cx4Registers, risc_registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("and A, ${:02X}", opcode & 0xFF);

    let value = read_register(cx4_registers, risc_registers, opcode);
    and(risc_registers, opcode, value);
}

fn and_a_imm(registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("and A, #${:02X}", opcode & 0xFF);

    and(registers, opcode, (opcode & 0xFF).into());
}

fn and(registers: &mut RiscRegisters, opcode: u16, value: u32) {
    let result = apply_a_shift(registers.a, opcode) & value;
    registers.set_nz(result);

    registers.a = result & 0xFFFFFF;
}

fn or_a_op(cx4_registers: &Cx4Registers, risc_registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("or A, ${:02X}", opcode & 0xFF);

    let value = read_register(cx4_registers, risc_registers, opcode);
    or(risc_registers, opcode, value);
}

fn or_a_imm(registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("or A, #${:02X}", opcode & 0xFF);

    or(registers, opcode, (opcode & 0xFF).into());
}

fn or(registers: &mut RiscRegisters, opcode: u16, value: u32) {
    let result = apply_a_shift(registers.a, opcode) | value;
    registers.set_nz(result);

    registers.a = result & 0xFFFFFF;
}

fn xor_a_op(cx4_registers: &Cx4Registers, risc_registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("xor A, ${:02X}", opcode & 0xFF);

    let value = read_register(cx4_registers, risc_registers, opcode);
    xor(risc_registers, opcode, value);
}

fn xor_a_imm(risc_registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("xor A, #${:02X}", opcode & 0xFF);

    xor(risc_registers, opcode, (opcode & 0xFF).into());
}

fn xor(registers: &mut RiscRegisters, opcode: u16, value: u32) {
    let result = apply_a_shift(registers.a, opcode) ^ value;
    registers.set_nz(result);

    registers.a = result & 0xFFFFFF;
}

fn shr_a_op(cx4_registers: &Cx4Registers, risc_registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("shr ${:02X}", opcode & 0xFF);

    let value = read_register(cx4_registers, risc_registers, opcode);
    shr(risc_registers, value);
}

fn shr_a_imm(registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("shr #${:02X}", opcode & 0xFF);

    shr(registers, (opcode & 0x1F).into());
}

fn shr(registers: &mut RiscRegisters, value: u32) {
    if value <= 24 {
        registers.a >>= value;
    }
    registers.set_nz(registers.a);
}

fn sar_a_op(cx4_registers: &Cx4Registers, risc_registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("sar ${:02X}", opcode & 0xFF);

    let value = read_register(cx4_registers, risc_registers, opcode);
    sar(risc_registers, value);
}

fn sar_a_imm(registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("sar #${:02X}", opcode & 0xFF);

    sar(registers, (opcode & 0x1F).into());
}

fn sar(registers: &mut RiscRegisters, value: u32) {
    if value <= 24 {
        registers.a = ((((registers.a as i32) << 8) >> (8 + value)) as u32) & 0xFFFFFF;
    }
    registers.set_nz(registers.a);
}

fn shl_a_op(cx4_registers: &Cx4Registers, risc_registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("shl ${:02X}", opcode & 0xFF);

    let value = read_register(cx4_registers, risc_registers, opcode);
    shl(risc_registers, value);
}

fn shl_a_imm(registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("shl #${:02X}", opcode & 0xFF);

    shl(registers, (opcode & 0x1F).into());
}

fn shl(registers: &mut RiscRegisters, value: u32) {
    if value <= 24 {
        registers.a = (registers.a << value) & 0xFFFFFF;
    }
    registers.set_nz(registers.a);
}

fn ror_a_op(cx4_registers: &Cx4Registers, risc_registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("ror ${:02X}", opcode & 0xFF);

    let value = read_register(cx4_registers, risc_registers, opcode);
    ror(risc_registers, value);
}

fn ror_a_imm(registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("ror #${:02X}", opcode & 0xFF);

    ror(registers, (opcode & 0x1F).into());
}

fn ror(registers: &mut RiscRegisters, value: u32) {
    if value <= 24 {
        registers.a = ((registers.a >> value) | (registers.a << (24 - value))) & 0xFFFFFF;
    }
    registers.set_nz(registers.a);
}

fn sign_extend(registers: &mut RiscRegisters, opcode: u16) {
    log::trace!("exts, opcode={opcode:04X}");

    match opcode & 0x0300 {
        0x0100 => {
            registers.a = ((registers.a as i8) as u32) & 0xFFFFFF;
        }
        0x0200 => {
            registers.a = ((registers.a as i16) as u32) & 0xFFFFFF;
        }
        _ => {
            log::warn!("Unexpected sign extension opcode: {opcode:04X}");
        }
    }
    registers.set_nz(registers.a);
}

fn inc_ext_ptr(registers: &mut RiscRegisters) {
    log::trace!("inc MAR");

    registers.ext_pointer = (registers.ext_pointer + 1) & 0xFFFFFF;
}

fn apply_a_shift(a: u32, opcode: u16) -> u32 {
    match opcode & 0x0300 {
        0x0000 => a,
        0x0100 => (a << 1) & 0xFFFFFF,
        0x0200 => (a << 8) & 0xFFFFFF,
        0x0300 => (a << 16) & 0xFFFFFF,
        _ => unreachable!("value & 0x0300 is always one of the above values"),
    }
}

const CONSTANT_REGISTERS: &[u32; 16] = &[
    0x000000, 0xFFFFFF, 0x00FF00, 0xFF0000, 0x00FFFF, 0xFFFF00, 0x800000, 0x7FFFFF, 0x008000,
    0x007FFF, 0xFF7FFF, 0xFFFF7F, 0x010000, 0xFEFFFF, 0x000100, 0x00FEFF,
];

fn read_register(cx4_registers: &Cx4Registers, risc_registers: &RiscRegisters, opcode: u16) -> u32 {
    match opcode & 0xFF {
        0x00 => risc_registers.a,
        0x01 => ((risc_registers.m >> 24) as u32) & 0xFFFFFF,
        0x02 => (risc_registers.m as u32) & 0xFFFFFF,
        0x03 => risc_registers.ext_buffer.into(),
        0x08 => risc_registers.rom_buffer,
        0x0C => risc_registers.ram_buffer,
        0x13 => risc_registers.ext_pointer,
        0x1C => risc_registers.ram_pointer.into(),
        0x20 => cx4_registers.instruction_pointer.into(),
        0x28 => risc_registers.page.into(),
        // $50-$5F are "fake" registers that always read constant values
        0x50..=0x5F => CONSTANT_REGISTERS[(opcode & 0xF) as usize],
        0x60..=0x6F => cx4_registers.gpr[(opcode & 0xF) as usize],
        _ => {
            log::warn!("Unhandled CX4 coprocessor register read: {:02X}", opcode & 0xFF);
            0x00
        }
    }
}

fn write_register(
    cx4_registers: &mut Cx4Registers,
    risc_registers: &mut RiscRegisters,
    opcode: u16,
    value: u32,
) {
    match opcode & 0xFF {
        0x00 => {
            risc_registers.a = value;
        }
        0x03 => {
            risc_registers.ext_buffer = value as u8;
        }
        0x0C => {
            risc_registers.ram_buffer = value & 0xFFFFFF;
        }
        0x13 => {
            risc_registers.ext_pointer = value & 0xFFFFFF;
        }
        0x1C => {
            risc_registers.ram_pointer = (value & 0xFFF) as u16;
        }
        0x20 => {
            cx4_registers.instruction_pointer = value as u8;
        }
        0x28 => {
            risc_registers.page = value as u16;
        }
        0x60..=0x6F => {
            let register_idx = (opcode & 0xF) as usize;
            cx4_registers.gpr[register_idx] = value & 0xFFFFFF;
        }
        _ => {
            log::warn!("Unhandled CX4 coprocessor register write: {:02X}", opcode & 0xFF);
        }
    }
}
