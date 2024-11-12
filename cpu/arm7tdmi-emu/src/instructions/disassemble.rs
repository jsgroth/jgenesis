use crate::instructions::{Condition, HalfwordLoadType, load_halfword};
use jgenesis_common::num::GetBit;

struct DecodeTableEntry {
    mask: u32,
    target: u32,
    decode_fn: fn(u32) -> String,
}

impl DecodeTableEntry {
    const fn new(mask: u32, target: u32, decode_fn: fn(u32) -> String) -> Self {
        Self { mask, target, decode_fn }
    }
}

const ARM_DECODE_TABLE: &[DecodeTableEntry] = &[
    DecodeTableEntry::new(0x0FFFFFF0, 0x012FFF10, arm_bx),
    DecodeTableEntry::new(0x0E000000, 0x0A000000, arm_b),
    DecodeTableEntry::new(0x0FC000F0, 0x00000090, arm_mul),
    DecodeTableEntry::new(0x0F8000F0, 0x00800090, arm_mull),
    DecodeTableEntry::new(0x0FB00FF0, 0x01000090, arm_swap),
    DecodeTableEntry::new(0x0E000090, 0x00000090, arm_ldrh),
    DecodeTableEntry::new(0x0FBF0FFF, 0x10F00000, arm_mrs),
    DecodeTableEntry::new(0x0DBEF000, 0x0128F000, arm_msr),
    DecodeTableEntry::new(0x0C000000, 0x00000000, arm_alu),
    DecodeTableEntry::new(0x0E000010, 0x06000010, |_| "Undefined".into()),
    DecodeTableEntry::new(0x0C000000, 0x04000000, arm_ldr),
    DecodeTableEntry::new(0x0E000000, 0x08000000, arm_ldm_stm),
];

pub fn arm(opcode: u32) -> String {
    for &DecodeTableEntry { mask, target, decode_fn } in ARM_DECODE_TABLE {
        if opcode & mask == target {
            return decode_fn(opcode);
        }
    }

    todo!("disassemble {opcode:08X}")
}

fn arm_bx(opcode: u32) -> String {
    let cond = Condition::from_arm_opcode(opcode).suffix();
    let rn = opcode & 0xF;
    format!("BX{cond} R{rn}")
}

fn arm_b(opcode: u32) -> String {
    let cond = Condition::from_arm_opcode(opcode).suffix();
    let link = if opcode.bit(24) { "L" } else { "" };
    let offset = (((opcode & 0xFFFFFF) as i32) << 8) >> 6;
    format!("B{link}{cond} {offset}")
}

fn arm_alu(opcode: u32) -> String {
    let operation = match (opcode >> 21) & 0xF {
        0 => "AND",
        1 => "EOR",
        2 => "SUB",
        3 => "RSB",
        4 => "ADD",
        5 => "ADC",
        6 => "SBC",
        7 => "RSC",
        8 => "TST",
        9 => "TEQ",
        10 => "CMP",
        11 => "CMN",
        12 => "ORR",
        13 => "MOV",
        14 => "BIC",
        15 => "MVN",
        _ => unreachable!(),
    };

    let set_conditions = if opcode.bit(20) { "S" } else { "" };

    let rn = (opcode >> 16) & 0xF;
    let rd = (opcode >> 12) & 0xF;

    let condition = Condition::from_arm_opcode(opcode).suffix();

    let operand2 = if opcode.bit(25) {
        let imm = opcode & 0xFF;
        let rotation = ((opcode >> 8) & 0xF) << 1;
        format!("#0x{:X}", imm.rotate_right(rotation))
    } else {
        let rm = opcode & 0xF;
        let mut s = format!("R{rm}");

        let shift = (opcode >> 4) & 0xFF;
        if shift != 0 {
            let shift_type = shift_type_str(shift >> 1);

            if shift.bit(0) {
                let rs = shift >> 4;
                s.push_str(&format!(", {shift_type} R{rs}"));
            } else {
                let mut amount = shift >> 3;
                if amount == 0 {
                    amount = 32;
                }

                s.push_str(&format!(", {shift_type} #{amount}"));
            }
        }

        s
    };

    if operation == "MOV" || operation == "MVN" {
        format!("{operation}{condition}{set_conditions} R{rd}, {operand2}")
    } else if matches!(operation, "CMP" | "CMN" | "TEQ" | "TST") {
        format!("{operation}{condition} R{rn}, {operand2}")
    } else {
        format!("{operation}{condition}{set_conditions} R{rd}, R{rn}, {operand2}")
    }
}

fn shift_type_str(shift_type: u32) -> &'static str {
    match shift_type & 3 {
        0 => "LSL",
        1 => "LSR",
        2 => "ASR",
        3 => "ROR",
        _ => unreachable!(),
    }
}

fn arm_mrs(opcode: u32) -> String {
    let cond = Condition::from_arm_opcode(opcode).suffix();
    let psr = if opcode.bit(22) { "SPSR" } else { "CPSR" };
    let rd = (opcode >> 12) & 0xF;

    format!("MRS{cond} R{rd}, {psr}")
}

fn arm_msr(opcode: u32) -> String {
    let cond = Condition::from_arm_opcode(opcode).suffix();
    let psr = if opcode.bit(22) { "SPSR" } else { "CPSR" };
    let flags_only = if opcode.bit(16) { "" } else { "_flg" };

    let expression = if !opcode.bit(16) && opcode.bit(25) {
        let immediate = opcode & 0xFF;
        let rotation = ((opcode >> 8) & 0xF) << 1;
        let value = immediate.rotate_right(rotation);
        format!("#0x{value:X}")
    } else {
        let rm = opcode & 0xF;
        format!("R{rm}")
    };

    format!("MSR{cond} {psr}{flags_only}, {expression}")
}

fn arm_mul(opcode: u32) -> String {
    let cond = Condition::from_arm_opcode(opcode).suffix();
    let rm = opcode & 0xF;
    let rs = (opcode >> 8) & 0xF;
    let rn = (opcode >> 12) & 0xF;
    let rd = (opcode >> 16) & 0xF;
    let s = if opcode.bit(20) { "S" } else { "" };
    let accumulate = opcode.bit(21);

    if accumulate {
        format!("MLA{cond}{s} R{rd}, R{rm}, R{rs}, R{rn}")
    } else {
        format!("MUL{cond}{s} R{rd}, R{rm}, R{rs}")
    }
}

fn arm_mull(opcode: u32) -> String {
    let cond = Condition::from_arm_opcode(opcode).suffix();
    let rm = opcode & 0xF;
    let rs = (opcode >> 8) & 0xF;
    let rdlo = (opcode >> 12) & 0xF;
    let rdhi = (opcode >> 16) & 0xF;
    let s = if opcode.bit(20) { "S" } else { "" };
    let accumulate = opcode.bit(21);
    let signed = if opcode.bit(22) { "S" } else { "U" };

    let op = if accumulate { "MLAL" } else { "MULL" };
    format!("{signed}{op}{cond}{s} R{rdlo}, R{rdhi}, R{rm}, R{rs}")
}

fn arm_ldr(opcode: u32) -> String {
    let cond = Condition::from_arm_opcode(opcode).suffix();

    let rn = (opcode >> 16) & 0xF;
    let rd = (opcode >> 12) & 0xF;
    let load = opcode.bit(20);
    let write_back = opcode.bit(21);
    let byte = opcode.bit(22);
    let add = opcode.bit(23);
    let pre_indexed = opcode.bit(24);
    let immediate = !opcode.bit(25);

    let add_str = if add { "" } else { "-" };

    let offset = if immediate {
        let offset = opcode & 0xFFF;
        format!("#{add_str}0x{offset:X}")
    } else {
        let rm = opcode & 0xF;
        let shift = (opcode >> 4) & 0xFF;
        let shift_type = shift_type_str(shift >> 1);
        let mut shift_amount = shift >> 3;
        if shift_type != "LSL" && shift_amount == 0 {
            shift_amount = 32;
        }

        if shift == 0 {
            format!("{add_str}R{rm}")
        } else {
            format!("{add_str}R{rm}, {shift_type} {shift_amount}")
        }
    };

    let address = if pre_indexed {
        let write_back_str = if write_back { "!" } else { "" };
        format!("[R{rn}, {offset}]{write_back_str}")
    } else {
        format!("[R{rn}], {offset}")
    };

    let operation = if load { "LDR" } else { "STR" };
    let byte_suffix = if byte { "B" } else { "" };

    format!("{operation}{cond}{byte_suffix} R{rd}, {address}")
}

fn arm_ldrh(opcode: u32) -> String {
    let cond = Condition::from_arm_opcode(opcode).suffix();

    let data_type = match (opcode >> 5) & 3 {
        0 => "SWP",
        1 => "H",
        2 => "SB",
        3 => "SH",
        _ => unreachable!(),
    };

    let operation = if opcode.bit(20) { "LDR" } else { "STR" };

    let rd = (opcode >> 12) & 0xF;
    let rn = (opcode >> 16) & 0xF;

    let offset = if opcode.bit(22) {
        format!("#0x{:X}", (opcode >> 4) & 0xF0 | (opcode & 0xF))
    } else {
        format!("R{}", opcode & 0xF)
    };

    let sign = if opcode.bit(23) { "+" } else { "-" };
    let write_back = if opcode.bit(21) { "!" } else { "" };

    let address = if opcode.bit(24) {
        format!("[R{rn}, {sign}{offset}]{write_back}")
    } else {
        format!("[R{rn}], {sign}{offset}{write_back}")
    };

    format!("{operation}{cond}{data_type} R{rd}, {address}")
}

fn arm_ldm_stm(opcode: u32) -> String {
    let cond = Condition::from_arm_opcode(opcode).suffix();

    let rlist = (0..16)
        .filter_map(|i| opcode.bit(i).then(|| format!("R{i}")))
        .collect::<Vec<_>>()
        .join(", ");

    let rn = (opcode >> 16) & 0xF;
    let load = opcode.bit(20);
    let write_back = opcode.bit(21);
    let s_bit = opcode.bit(22);
    let up = opcode.bit(23);
    let pre = opcode.bit(24);

    let write_back_str = if write_back { "!" } else { "" };
    let s_bit_str = if s_bit { "^" } else { "" };
    let up_str = if up { "I" } else { "D" };
    let pre_str = if pre { "B" } else { "A" };

    let op = if load { "LDM" } else { "STM" };

    format!("{op}{cond}{up_str}{pre_str} R{rn}{write_back_str} {{{rlist}}}{s_bit_str}")
}

fn arm_swap(opcode: u32) -> String {
    let cond = Condition::from_arm_opcode(opcode).suffix();

    let rm = opcode & 0xF;
    let rd = (opcode >> 12) & 0xF;
    let rn = (opcode >> 16) & 0xF;
    let byte = if opcode.bit(22) { "B" } else { "" };

    format!("SWP{cond}{byte} R{rd}, R{rm}, [R{rn}]")
}

struct ThumbDecodeEntry {
    mask: u16,
    target: u16,
    decode_fn: fn(u16) -> String,
}

impl ThumbDecodeEntry {
    const fn new(mask: u16, target: u16, decode_fn: fn(u16) -> String) -> Self {
        Self { mask, target, decode_fn }
    }
}

const THUMB_DECODE_TABLE: &[ThumbDecodeEntry] = &[
    ThumbDecodeEntry::new(0xF800, 0x1800, thumb_2),
    ThumbDecodeEntry::new(0xE000, 0x0000, thumb_1),
    ThumbDecodeEntry::new(0xE000, 0x2000, thumb_3),
    ThumbDecodeEntry::new(0xFC00, 0x4000, thumb_4),
    ThumbDecodeEntry::new(0xFC00, 0x4400, thumb_5),
    ThumbDecodeEntry::new(0xF800, 0x4800, thumb_6),
    ThumbDecodeEntry::new(0xF200, 0x5000, thumb_7),
    ThumbDecodeEntry::new(0xF200, 0x5200, thumb_8),
    ThumbDecodeEntry::new(0xE000, 0x6000, thumb_9),
    ThumbDecodeEntry::new(0xF000, 0x8000, thumb_10),
    ThumbDecodeEntry::new(0xF000, 0x9000, thumb_11),
    ThumbDecodeEntry::new(0xF000, 0xA000, thumb_12),
    ThumbDecodeEntry::new(0xFF00, 0xB000, thumb_13),
    ThumbDecodeEntry::new(0xF600, 0xB400, thumb_14),
    ThumbDecodeEntry::new(0xF000, 0xC000, thumb_15),
    ThumbDecodeEntry::new(0xFF00, 0xDF00, |opcode| todo!("Thumb format 17 {opcode:04X}")),
    ThumbDecodeEntry::new(0xF000, 0xD000, thumb_16),
    ThumbDecodeEntry::new(0xF800, 0xE000, thumb_18),
    ThumbDecodeEntry::new(0xF000, 0xF000, thumb_19),
];

pub fn thumb(opcode: u16) -> String {
    for &ThumbDecodeEntry { mask, target, decode_fn } in THUMB_DECODE_TABLE {
        if opcode & mask == target {
            return decode_fn(opcode);
        }
    }

    todo!("disassemble Thumb {opcode:04X}")
}

// Move shifted register
fn thumb_1(opcode: u16) -> String {
    let rd = opcode & 7;
    let rs = (opcode >> 3) & 7;
    let offset = (opcode >> 6) & 0x1F;

    let op = match (opcode >> 11) & 3 {
        0 => "LSL",
        1 => "LSR",
        2 => "ASR",
        _ => panic!("invalid Thumb format 1 opcode: {opcode:04X}"),
    };

    format!("{op} R{rd}, R{rs} #{offset}")
}

// Add/subtract
fn thumb_2(opcode: u16) -> String {
    let rd = opcode & 7;
    let rs = (opcode >> 3) & 7;
    let op = if opcode.bit(9) { "SUB" } else { "ADD" };

    let immediate = opcode.bit(10);
    if immediate {
        let operand = (opcode >> 6) & 7;
        format!("{op} R{rd}, R{rs}, #{operand}")
    } else {
        let rn = (opcode >> 6) & 7;
        format!("{op} R{rd}, R{rs}, R{rn}")
    }
}

// Move/compare/add/subtract immediate
fn thumb_3(opcode: u16) -> String {
    let immediate = opcode & 0xFF;
    let rd = (opcode >> 8) & 7;
    let op = match (opcode >> 11) & 3 {
        0 => "MOV",
        1 => "CMP",
        2 => "ADD",
        3 => "SUB",
        _ => unreachable!(),
    };

    format!("{op} R{rd}, #0x{immediate:X}")
}

// ALU operations
fn thumb_4(opcode: u16) -> String {
    let rd = opcode & 7;
    let rs = (opcode >> 3) & 7;
    let op = match (opcode >> 6) & 0xF {
        0x0 => "AND",
        0x1 => "EOR",
        0x2 => "LSL",
        0x3 => "LSR",
        0x4 => "ASR",
        0x5 => "ADC",
        0x6 => "SBC",
        0x7 => "ROR",
        0x8 => "TST",
        0x9 => "NEG",
        0xA => "CMP",
        0xB => "CMN",
        0xC => "ORR",
        0xD => "MUL",
        0xE => "BIC",
        0xF => "MVN",
        _ => unreachable!(),
    };

    format!("{op} R{rd}, R{rs}")
}

// Hi register operations / branch exchange
fn thumb_5(opcode: u16) -> String {
    let mut rd = opcode & 7;
    let mut rs = (opcode >> 3) & 7;

    let h1 = opcode.bit(7);
    let h2 = opcode.bit(6);

    if h1 {
        rd += 8;
    }

    if h2 {
        rs += 8;
    }

    match (opcode >> 8) & 3 {
        0 => format!("ADD R{rd}, R{rs}"),
        1 => format!("CMP R{rd}, R{rs}"),
        2 => format!("MOV R{rd}, R{rs}"),
        3 => format!("BX R{rs}"),
        _ => unreachable!(),
    }
}

// PC-relative load
fn thumb_6(opcode: u16) -> String {
    let offset = (opcode & 0xFF) << 2;
    let rd = (opcode >> 8) & 7;

    format!("LDR R{rd}, [PC, #0x{offset:X}]")
}

// Load/store with register offset
fn thumb_7(opcode: u16) -> String {
    let rd = opcode & 7;
    let rb = (opcode >> 3) & 7;
    let ro = (opcode >> 6) & 7;

    let byte_suffix = if opcode.bit(10) { "B" } else { "" };
    let op = if opcode.bit(11) { "LDR" } else { "STR" };

    format!("{op}{byte_suffix} R{rd}, [R{rb}, R{ro}]")
}

// Load/store sign-extended byte/halfword
fn thumb_8(opcode: u16) -> String {
    let rd = opcode & 7;
    let rb = (opcode >> 3) & 7;
    let ro = (opcode >> 6) & 7;

    let sh_bits = (opcode >> 10) & 3;
    let op = match sh_bits {
        0 => "STRH",
        1 => "LDSB",
        2 => "LDRH",
        3 => "LDSH",
        _ => unreachable!(),
    };

    format!("{op} R{rd}, [R{rb}, R{ro}]")
}

// Load/store with immediate offset
fn thumb_9(opcode: u16) -> String {
    let rd = opcode & 7;
    let rb = (opcode >> 3) & 7;

    let mut offset = (opcode >> 6) & 0x1F;

    let byte = opcode.bit(12);
    if !byte {
        offset <<= 2;
    }

    let byte_suffix = if byte { "B" } else { "" };
    let op = if opcode.bit(11) { "LDR" } else { "STR" };

    format!("{op}{byte_suffix} R{rd}, [R{rb}, #{offset}]")
}

// Load/store halfword
fn thumb_10(opcode: u16) -> String {
    let rd = opcode & 7;
    let rb = (opcode >> 3) & 7;
    let offset = ((opcode >> 6) & 0x1F) << 1;
    let load = opcode.bit(11);

    let op = if load { "LDRH" } else { "STRH" };
    format!("{op} R{rd}, [R{rb}, #0x{offset:X}]")
}

// SP-relative load/store
fn thumb_11(opcode: u16) -> String {
    let offset = (opcode & 0xFF) << 2;
    let rd = (opcode >> 8) & 7;
    let load = opcode.bit(11);

    let op = if load { "LDR" } else { "STR" };
    format!("{op} R{rd}, [SP, #0x{offset:X}]")
}

// Load address
fn thumb_12(opcode: u16) -> String {
    let immediate = (opcode & 0xFF) << 2;
    let rd = (opcode >> 8) & 7;
    let source = if opcode.bit(11) { "SP" } else { "PC" };

    format!("ADD R{rd}, {source}, #0x{immediate:X}")
}

// Add offset to stack pointer
fn thumb_13(opcode: u16) -> String {
    let offset = (opcode & 0x7F) << 2;
    let sign = opcode.bit(7);

    let sign_str = if sign { "-" } else { "" };

    format!("ADD SP, #{sign}0x{offset:X}")
}

// Push/pop registers
fn thumb_14(opcode: u16) -> String {
    let lr_pc_bit = opcode.bit(8);
    let load = opcode.bit(11);

    let op = if load { "POP" } else { "PUSH" };

    let mut rlist = thumb_rlist_vec(opcode);
    if lr_pc_bit {
        rlist.push(if load { "PC".into() } else { "LR".into() });
    }

    format!("{op} {{{}}}", rlist.join(", "))
}

// Multiple load/store
fn thumb_15(opcode: u16) -> String {
    let op = if opcode.bit(11) { "LDMIA" } else { "STMIA" };
    let rb = (opcode >> 8) & 7;

    let rlist = thumb_rlist_vec(opcode).join(",");

    format!("{op} R{rb}! {{{rlist}}}")
}

fn thumb_rlist_vec(opcode: u16) -> Vec<String> {
    (0..8).filter_map(|i| opcode.bit(i).then(|| format!("R{i}"))).collect()
}

// Conditional branch
fn thumb_16(opcode: u16) -> String {
    let cond = Condition::from_bits((opcode >> 8).into()).suffix();
    let offset = i16::from(opcode as i8) << 1;

    format!("B{cond} {offset}")
}

// Unconditional branch
fn thumb_18(opcode: u16) -> String {
    let offset = (((opcode & 0x3FF) as i16) << 5) >> 4;
    format!("B {offset}")
}

// Long branch with link
fn thumb_19(opcode: u16) -> String {
    let which = if opcode.bit(11) { "(low)" } else { "(high)" };
    let offset = opcode & 0x7FF;

    format!("BL {which} #0x{offset:X}")
}
