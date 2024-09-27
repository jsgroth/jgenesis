use crate::superfx::gsu::instructions::{
    MemoryType, clear_prefix_flags, read_register, write_register,
};
use crate::superfx::gsu::{GraphicsSupportUnit, MultiplierSpeed};
use jgenesis_common::num::{GetBit, SignBit};

pub(super) fn add(
    opcode: u8,
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // ADD/ADC: Add / Add with carry
    // ALT1 controls ADD vs. ADC
    // ALT2 controls register operand vs. immediate operand
    let operand =
        if gsu.alt2 { u16::from(opcode & 0x0F) } else { read_register(gsu, opcode & 0x0F) };

    let existing_carry = if gsu.alt1 { u16::from(gsu.carry_flag) } else { 0 };

    let source = read_register(gsu, gsu.sreg);
    let (partial_sum, carry1) = source.overflowing_add(operand);
    let (sum, carry2) = partial_sum.overflowing_add(existing_carry);
    let carry = carry1 || carry2;

    let bit_14_carry = (source & 0x7FFF) + (operand & 0x7FFF) + existing_carry >= 0x8000;
    let overflow = bit_14_carry != carry;

    gsu.zero_flag = sum == 0;
    gsu.carry_flag = carry;
    gsu.sign_flag = sum.sign_bit();
    gsu.overflow_flag = overflow;

    let cycles = write_register(gsu, gsu.dreg, sum, rom, ram);

    clear_prefix_flags(gsu);
    cycles + memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn sub(
    opcode: u8,
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // SUB/SBC/CMP: Subtract / Subtract with carry / Compare
    // ALT1 controls SUB vs. SBC/CMP
    // For SUB, ALT2 controls register operand vs. immediate operand
    // For SBC/CMP, ALT2 controls SBC vs. CMP
    let operand = if !gsu.alt1 && gsu.alt2 {
        u16::from(opcode & 0x0F)
    } else {
        read_register(gsu, opcode & 0x0F)
    };

    let sbc = gsu.alt1 && !gsu.alt2;
    let existing_borrow = if sbc { u16::from(!gsu.carry_flag) } else { 0 };

    let source = read_register(gsu, gsu.sreg);
    let (partial_diff, borrow1) = source.overflowing_sub(operand);
    let (difference, borrow2) = partial_diff.overflowing_sub(existing_borrow);
    let borrow = borrow1 || borrow2;

    let bit_14_borrow = source & 0x7FFF < (operand & 0x7FFF) + existing_borrow;
    let overflow = bit_14_borrow != borrow;

    gsu.zero_flag = difference == 0;
    gsu.carry_flag = !borrow;
    gsu.sign_flag = difference.sign_bit();
    gsu.overflow_flag = overflow;

    let mut cycles = 0;
    if !(gsu.alt1 && gsu.alt2) {
        // Only write if not CMP
        cycles = write_register(gsu, gsu.dreg, difference, rom, ram);
    }

    clear_prefix_flags(gsu);
    cycles + memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn fmult(
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // FMULT/LMULT: Fractional signed multiplication / Long signed multiplication
    // ALT1 controls FMULT vs. LMULT
    let source: i32 = (read_register(gsu, gsu.sreg) as i16).into();
    let operand: i32 = (gsu.r[6] as i16).into();
    let product = source * operand;
    let high_word = (product >> 16) as u16;

    if gsu.alt1 {
        // LMULT: Write low word to R4
        gsu.r[4] = product as u16;
    }

    let cycles = write_register(gsu, gsu.dreg, high_word, rom, ram);

    gsu.zero_flag = high_word == 0;
    gsu.carry_flag = product.bit(15);
    gsu.sign_flag = high_word.sign_bit();

    clear_prefix_flags(gsu);
    cycles
        + match (memory_type, gsu.multiplier_speed) {
            (MemoryType::CodeCache, MultiplierSpeed::Standard) => 8,
            (MemoryType::CodeCache, MultiplierSpeed::High) => 4,
            (MemoryType::Rom | MemoryType::Ram, MultiplierSpeed::Standard) => 11,
            (MemoryType::Rom | MemoryType::Ram, MultiplierSpeed::High) => 7,
        }
}

pub(super) fn mult(
    opcode: u8,
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // MULT/UMULT: Signed multiplication / Unsigned multiplication
    // ALT1 controls MULT vs. UMULT
    // ALT2 controls register operand vs. immediate operand
    let operand =
        if gsu.alt2 { u16::from(opcode & 0x0F) } else { read_register(gsu, opcode & 0x0F) & 0xFF };

    let source = read_register(gsu, gsu.sreg) & 0xFF;

    let product = if gsu.alt1 {
        // UMULT: Unsigned 8-bit x 8-bit -> 16-bit
        source * operand
    } else {
        // MULT: Signed 8-bit x 8-bit -> 16-bit
        (i16::from(source as i8) * i16::from(operand as i8)) as u16
    };

    let cycles = write_register(gsu, gsu.dreg, product, rom, ram);

    gsu.zero_flag = product == 0;
    gsu.sign_flag = product.sign_bit();

    clear_prefix_flags(gsu);
    // TODO ROM/RAM should take more cycles at 21.47 MHz?
    cycles
        + match (memory_type, gsu.multiplier_speed) {
            (MemoryType::CodeCache, MultiplierSpeed::Standard) => 2,
            (MemoryType::CodeCache, MultiplierSpeed::High) => 1,
            (MemoryType::Rom | MemoryType::Ram, MultiplierSpeed::Standard) => 5,
            (MemoryType::Rom | MemoryType::Ram, MultiplierSpeed::High) => 3,
        }
}

pub(super) fn inc(
    opcode: u8,
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // INC Rn: Increment register
    let register = opcode & 0x0F;
    let incremented = gsu.r[register as usize].wrapping_add(1);
    let cycles = write_register(gsu, register, incremented, rom, ram);

    gsu.zero_flag = incremented == 0;
    gsu.sign_flag = incremented.sign_bit();

    clear_prefix_flags(gsu);
    cycles + memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn dec(
    opcode: u8,
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // DEC Rn: Decrement register
    let register = opcode & 0x0F;
    let decremented = gsu.r[register as usize].wrapping_sub(1);
    let cycles = write_register(gsu, register, decremented, rom, ram);

    gsu.zero_flag = decremented == 0;
    gsu.sign_flag = decremented.sign_bit();

    clear_prefix_flags(gsu);
    cycles + memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn and(
    opcode: u8,
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // AND/BIC: And / And with complement
    // ALT1 controls AND vs. BIC
    // ALT2 controls register operand vs. immediate operand
    let operand =
        if gsu.alt2 { u16::from(opcode & 0x0F) } else { read_register(gsu, opcode & 0x0F) };

    let source = read_register(gsu, gsu.sreg);
    let result = if gsu.alt1 { source & !operand } else { source & operand };

    let cycles = write_register(gsu, gsu.dreg, result, rom, ram);

    gsu.zero_flag = result == 0;
    gsu.sign_flag = result.sign_bit();

    clear_prefix_flags(gsu);
    cycles + memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn or(
    opcode: u8,
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // OR/XOR: Or / Exclusive or
    // ALT1 controls OR vs. XOR
    // ALT2 controls register operand vs. immediate operand
    let operand =
        if gsu.alt2 { u16::from(opcode & 0x0F) } else { read_register(gsu, opcode & 0x0F) };

    let source = read_register(gsu, gsu.sreg);
    let result = if gsu.alt1 { source ^ operand } else { source | operand };

    let cycles = write_register(gsu, gsu.dreg, result, rom, ram);

    gsu.zero_flag = result == 0;
    gsu.sign_flag = result.sign_bit();

    clear_prefix_flags(gsu);
    cycles + memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn not(
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // NOT: Bitwise not
    let source = read_register(gsu, gsu.sreg);
    let inverted = !source;
    let cycles = write_register(gsu, gsu.dreg, inverted, rom, ram);

    gsu.zero_flag = inverted == 0;
    gsu.sign_flag = inverted.sign_bit();

    clear_prefix_flags(gsu);
    cycles + memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn asr(
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // ASR/DIV2: Arithmetic shift right / Divide by 2
    // ALT1 controls ASR vs. DIV2
    let source = read_register(gsu, gsu.sreg);

    // DIV2 is equivalent to ASR unless Sreg == -1, in which case DIV2 produces 0 and ASR produces -1
    let shifted =
        if gsu.alt1 && source == u16::MAX { 0 } else { (source >> 1) | (source & 0x8000) };

    let cycles = write_register(gsu, gsu.dreg, shifted, rom, ram);

    gsu.zero_flag = shifted == 0;
    gsu.carry_flag = source.bit(0);
    gsu.sign_flag = shifted.sign_bit();

    clear_prefix_flags(gsu);
    cycles + memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn lsr(
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // LSR: Logical shift right
    let source = read_register(gsu, gsu.sreg);
    let shifted = source >> 1;
    let cycles = write_register(gsu, gsu.dreg, shifted, rom, ram);

    gsu.zero_flag = shifted == 0;
    gsu.carry_flag = source.bit(0);
    gsu.sign_flag = false;

    clear_prefix_flags(gsu);
    cycles + memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn rol(
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // ROL: Rotate left
    let source = read_register(gsu, gsu.sreg);
    let rotated = (source << 1) | u16::from(gsu.carry_flag);
    let cycles = write_register(gsu, gsu.dreg, rotated, rom, ram);

    gsu.zero_flag = rotated == 0;
    gsu.carry_flag = source.sign_bit();
    gsu.sign_flag = rotated.sign_bit();

    clear_prefix_flags(gsu);
    cycles + memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn ror(
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // ROR: Rotate right
    let source = read_register(gsu, gsu.sreg);
    let rotated = (source >> 1) | (u16::from(gsu.carry_flag) << 15);
    let cycles = write_register(gsu, gsu.dreg, rotated, rom, ram);

    gsu.zero_flag = rotated == 0;
    gsu.carry_flag = source.bit(0);
    gsu.sign_flag = rotated.sign_bit();

    clear_prefix_flags(gsu);
    cycles + memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn sex(
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &[u8],
) -> u8 {
    // SEX: Sign extend
    let source = read_register(gsu, gsu.sreg);
    let extended = (source as i8) as u16;
    let cycles = write_register(gsu, gsu.dreg, extended, rom, ram);

    gsu.zero_flag = extended == 0;
    gsu.sign_flag = extended.sign_bit();

    clear_prefix_flags(gsu);
    cycles + memory_type.access_cycles(gsu.clock_speed)
}
