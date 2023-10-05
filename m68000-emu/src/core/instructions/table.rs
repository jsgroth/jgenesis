use crate::core::instructions::{
    BranchCondition, Direction, Instruction, ShiftCount, ShiftDirection, UspDirection,
};
use crate::core::{AddressRegister, AddressingMode, DataRegister, OpSize};
use std::sync::OnceLock;

type InstructionTable = Box<[Instruction; 65536]>;

impl OpSize {
    fn to_bits(self) -> u16 {
        match self {
            Self::Byte => 0x0000,
            Self::Word => 0x0040,
            Self::LongWord => 0x0080,
        }
    }

    // MOVE instructions have a different mapping from bits to size
    fn to_move_bits(self) -> u16 {
        match self {
            Self::Byte => 0x1000,
            Self::Word => 0x3000,
            Self::LongWord => 0x2000,
        }
    }
}

impl AddressingMode {
    fn to_bits(self) -> u16 {
        match self {
            Self::DataDirect(register) => register.0.into(),
            Self::AddressDirect(register) => 0x0008 | u16::from(register.0),
            Self::AddressIndirect(register) => 0x0010 | u16::from(register.0),
            Self::AddressIndirectPostincrement(register) => 0x0018 | u16::from(register.0),
            Self::AddressIndirectPredecrement(register) => 0x0020 | u16::from(register.0),
            Self::AddressIndirectDisplacement(register) => 0x0028 | u16::from(register.0),
            Self::AddressIndirectIndexed(register) => 0x0030 | u16::from(register.0),
            Self::AbsoluteShort => 0x0038,
            Self::AbsoluteLong => 0x0039,
            Self::PcRelativeDisplacement => 0x003A,
            Self::PcRelativeIndexed => 0x003B,
            Self::Immediate => 0x003C,
            Self::Quick(..) => {
                panic!("Quick addressing mode does not have a standardized bit pattern")
            }
        }
    }
}

impl BranchCondition {
    fn to_bits(self) -> u16 {
        match self {
            Self::True => 0x0000,
            Self::False => 0x0100,
            Self::Higher => 0x0200,
            Self::LowerOrSame => 0x0300,
            Self::CarryClear => 0x0400,
            Self::CarrySet => 0x0500,
            Self::NotEqual => 0x0600,
            Self::Equal => 0x0700,
            Self::OverflowClear => 0x0800,
            Self::OverflowSet => 0x0900,
            Self::Plus => 0x0A00,
            Self::Minus => 0x0B00,
            Self::GreaterOrEqual => 0x0C00,
            Self::LessThan => 0x0D00,
            Self::GreaterThan => 0x0E00,
            Self::LessOrEqual => 0x0F00,
        }
    }
}

pub fn decode(opcode: u16) -> Instruction {
    static LOOKUP_TABLE: OnceLock<InstructionTable> = OnceLock::new();

    let lookup_table = LOOKUP_TABLE.get_or_init(|| {
        // Initialize table with every entry set to ILLEGAL
        let mut table = (0..=u16::MAX)
            .map(|opcode| Instruction::Illegal { opcode })
            .collect::<Vec<_>>()
            .into_boxed_slice()
            .try_into()
            .unwrap();

        populate_abcd(&mut table);
        populate_add(&mut table);
        populate_adda(&mut table);
        populate_addi(&mut table);
        populate_addq(&mut table);
        populate_addx(&mut table);
        populate_and(&mut table);
        populate_andi(&mut table);
        populate_asd(&mut table);
        populate_bcc(&mut table);
        populate_bchg(&mut table);
        populate_bclr(&mut table);
        populate_bset(&mut table);
        populate_btst(&mut table);
        populate_bsr(&mut table);
        populate_chk(&mut table);
        populate_clr(&mut table);
        populate_cmp(&mut table);
        populate_cmpa(&mut table);
        populate_cmpi(&mut table);
        populate_cmpm(&mut table);
        populate_dbcc(&mut table);
        populate_divs(&mut table);
        populate_divu(&mut table);
        populate_eor(&mut table);
        populate_eori(&mut table);
        populate_exg(&mut table);
        populate_ext(&mut table);
        populate_jmp(&mut table);
        populate_jsr(&mut table);
        populate_lea(&mut table);
        populate_link(&mut table);
        populate_lsd(&mut table);
        populate_move(&mut table);
        populate_movea(&mut table);
        populate_movem(&mut table);
        populate_movep(&mut table);
        populate_moveq(&mut table);
        populate_move_ccr_sr_usp(&mut table);
        populate_muls(&mut table);
        populate_mulu(&mut table);
        populate_nbcd(&mut table);
        populate_neg(&mut table);
        populate_negx(&mut table);
        populate_nop(&mut table);
        populate_not(&mut table);
        populate_or(&mut table);
        populate_ori(&mut table);
        populate_pea(&mut table);
        populate_reset(&mut table);
        populate_rod(&mut table);
        populate_roxd(&mut table);
        populate_rte_rtr_rts(&mut table);
        populate_sbcd(&mut table);
        populate_scc(&mut table);
        populate_stop(&mut table);
        populate_sub(&mut table);
        populate_suba(&mut table);
        populate_subi(&mut table);
        populate_subq(&mut table);
        populate_subx(&mut table);
        populate_swap(&mut table);
        populate_tas(&mut table);
        populate_trap(&mut table);
        populate_tst(&mut table);
        populate_unlk(&mut table);

        table
    });
    lookup_table[opcode as usize]
}

fn all_addressing_modes() -> impl Iterator<Item = AddressingMode> {
    DataRegister::ALL
        .iter()
        .copied()
        .map(AddressingMode::DataDirect)
        .chain(AddressRegister::ALL.iter().copied().flat_map(|register| {
            [
                AddressingMode::AddressDirect(register),
                AddressingMode::AddressIndirect(register),
                AddressingMode::AddressIndirectPostincrement(register),
                AddressingMode::AddressIndirectPredecrement(register),
                AddressingMode::AddressIndirectDisplacement(register),
                AddressingMode::AddressIndirectIndexed(register),
            ]
        }))
        .chain([
            AddressingMode::AbsoluteShort,
            AddressingMode::AbsoluteLong,
            AddressingMode::Immediate,
            AddressingMode::PcRelativeDisplacement,
            AddressingMode::PcRelativeIndexed,
        ])
}

fn all_addressing_modes_no_address_direct() -> impl Iterator<Item = AddressingMode> {
    // Does not include address direct
    all_addressing_modes()
        .filter(|addressing_mode| !matches!(addressing_mode, AddressingMode::AddressDirect(..)))
}

fn jump_addressing_modes() -> impl Iterator<Item = AddressingMode> {
    // Does not include data direct, address direct, address indirect postincrement,
    // address indirect predecrement, or immediate
    all_addressing_modes().filter(|mode| {
        !matches!(
            mode,
            AddressingMode::DataDirect(..)
                | AddressingMode::AddressDirect(..)
                | AddressingMode::AddressIndirectPostincrement(..)
                | AddressingMode::AddressIndirectPredecrement(..)
                | AddressingMode::Immediate
        )
    })
}

fn dest_addressing_modes() -> impl Iterator<Item = AddressingMode> {
    // Does not include immediate, PC relative displacement, or PC relative indexed
    all_addressing_modes().filter(|mode| {
        !matches!(
            mode,
            AddressingMode::Immediate
                | AddressingMode::PcRelativeDisplacement
                | AddressingMode::PcRelativeIndexed
        )
    })
}

fn dest_addressing_modes_no_address_direct() -> impl Iterator<Item = AddressingMode> {
    // Does not include address direct, immediate, PC relative displacement, or PC relative indexed
    dest_addressing_modes()
        .filter(|addressing_mode| !matches!(addressing_mode, AddressingMode::AddressDirect(..)))
}

fn dest_addressing_modes_no_direct() -> impl Iterator<Item = AddressingMode> {
    // Does not include data direct, address direct, immediate, PC relative displacement, or PC relative indexed
    dest_addressing_modes().filter(|addressing_mode| {
        !matches!(
            addressing_mode,
            AddressingMode::DataDirect(..) | AddressingMode::AddressDirect(..)
        )
    })
}

fn populate_abcd(table: &mut InstructionTable) {
    for rx in 0..8_u8 {
        for ry in 0..8_u8 {
            // ABCD Dx, Dy
            let data_opcode = 0xC100 | u16::from(ry) | (u16::from(rx) << 9);
            table[data_opcode as usize] = Instruction::AddDecimal {
                source: AddressingMode::DataDirect(DataRegister(ry)),
                dest: AddressingMode::DataDirect(DataRegister(rx)),
            };

            // ABCD -(Ax), -(Ay)
            let predec_opcode = data_opcode | 0x0008;
            table[predec_opcode as usize] = Instruction::AddDecimal {
                source: AddressingMode::AddressIndirectPredecrement(AddressRegister(ry)),
                dest: AddressingMode::AddressIndirectPredecrement(AddressRegister(rx)),
            };
        }
    }
}

fn populate_add(table: &mut InstructionTable) {
    // ADD <ea>, Dn
    for source in all_addressing_modes() {
        for dest in DataRegister::ALL {
            for size in OpSize::ALL {
                let opcode = 0xD000 | size.to_bits() | source.to_bits() | (u16::from(dest.0) << 9);
                table[opcode as usize] = Instruction::Add {
                    size,
                    source,
                    dest: AddressingMode::DataDirect(dest),
                    with_extend: false,
                };
            }
        }
    }

    // ADD Dn, <ea>
    for source in DataRegister::ALL {
        for dest in dest_addressing_modes_no_direct() {
            for size in OpSize::ALL {
                let opcode = 0xD100 | size.to_bits() | dest.to_bits() | (u16::from(source.0) << 9);
                table[opcode as usize] = Instruction::Add {
                    size,
                    source: AddressingMode::DataDirect(source),
                    dest,
                    with_extend: false,
                };
            }
        }
    }
}

fn populate_adda(table: &mut InstructionTable) {
    // ADDA <ea>, An
    for source in all_addressing_modes() {
        for dest in AddressRegister::ALL {
            for size in [OpSize::Word, OpSize::LongWord] {
                let size_bit = u16::from(size == OpSize::LongWord) << 8;
                let opcode = 0xD0C0 | size_bit | source.to_bits() | (u16::from(dest.0) << 9);
                table[opcode as usize] = Instruction::Add {
                    size,
                    source,
                    dest: AddressingMode::AddressDirect(dest),
                    with_extend: false,
                };
            }
        }
    }
}

fn populate_addi(table: &mut InstructionTable) {
    // ADDI #<d>, <ea>
    for dest in dest_addressing_modes_no_address_direct() {
        for size in OpSize::ALL {
            let opcode = 0x0600 | size.to_bits() | dest.to_bits();
            table[opcode as usize] = Instruction::Add {
                size,
                source: AddressingMode::Immediate,
                dest,
                with_extend: false,
            };
        }
    }
}

fn populate_addq(table: &mut InstructionTable) {
    // ADDQ #<d>, <ea>
    for q_value in 0..8 {
        for dest in dest_addressing_modes() {
            for size in OpSize::ALL {
                let opcode = 0x5000 | size.to_bits() | dest.to_bits() | (q_value << 9);
                let source = if q_value == 0 {
                    AddressingMode::Quick(8)
                } else {
                    AddressingMode::Quick(q_value as u8)
                };
                table[opcode as usize] =
                    Instruction::Add { size, source, dest, with_extend: false };
            }
        }
    }
}

fn populate_addx(table: &mut InstructionTable) {
    // ADDX Dy, Dx
    for source in DataRegister::ALL {
        for dest in DataRegister::ALL {
            for size in OpSize::ALL {
                let opcode =
                    0xD100 | size.to_bits() | u16::from(source.0) | (u16::from(dest.0) << 9);
                table[opcode as usize] = Instruction::Add {
                    size,
                    source: AddressingMode::DataDirect(source),
                    dest: AddressingMode::DataDirect(dest),
                    with_extend: true,
                };
            }
        }
    }

    // ADDX -(Ay), -(Ax)
    for source in AddressRegister::ALL {
        for dest in AddressRegister::ALL {
            for size in OpSize::ALL {
                let opcode =
                    0xD108 | size.to_bits() | u16::from(source.0) | (u16::from(dest.0) << 9);
                table[opcode as usize] = Instruction::Add {
                    size,
                    source: AddressingMode::AddressIndirectPredecrement(source),
                    dest: AddressingMode::AddressIndirectPredecrement(dest),
                    with_extend: true,
                };
            }
        }
    }
}

fn populate_and(table: &mut InstructionTable) {
    // AND <ea>, Dn
    for source in all_addressing_modes_no_address_direct() {
        for dest in DataRegister::ALL {
            for size in OpSize::ALL {
                let opcode = 0xC000 | size.to_bits() | source.to_bits() | (u16::from(dest.0) << 9);
                table[opcode as usize] =
                    Instruction::And { size, source, dest: AddressingMode::DataDirect(dest) };
            }
        }
    }

    // AND Dn, <ea>
    for source in DataRegister::ALL {
        for dest in dest_addressing_modes_no_direct() {
            for size in OpSize::ALL {
                let opcode = 0xC100 | size.to_bits() | dest.to_bits() | (u16::from(source.0) << 9);
                table[opcode as usize] =
                    Instruction::And { size, source: AddressingMode::DataDirect(source), dest };
            }
        }
    }
}

fn populate_andi(table: &mut InstructionTable) {
    // ANDI #<d>, <ea>
    for dest in dest_addressing_modes_no_address_direct() {
        for size in OpSize::ALL {
            let opcode = 0x0200 | size.to_bits() | dest.to_bits();
            table[opcode as usize] =
                Instruction::And { size, source: AddressingMode::Immediate, dest };
        }
    }

    // ANDI to CCR/SR
    table[0x023C] = Instruction::AndToCcr;
    table[0x027C] = Instruction::AndToSr;
}

macro_rules! impl_populate_bit_shift {
    ($name:ident, $imm_opcode_base:expr, $reg_opcode_base:expr, $mem_opcode_base:expr, $register_instr:ident, $memory_instr:ident) => {
        fn $name(table: &mut InstructionTable) {
            for dest in DataRegister::ALL {
                for size in OpSize::ALL {
                    for count_value in 0..8_u8 {
                        // xxL/xxR #<d>, Dy
                        {
                            let shift = if count_value == 0 {
                                ShiftCount::Constant(8)
                            } else {
                                ShiftCount::Constant(count_value)
                            };
                            let r_opcode = $imm_opcode_base
                                | size.to_bits()
                                | u16::from(dest.0)
                                | (u16::from(count_value) << 9);
                            let l_opcode = r_opcode | 0x0100;

                            table[r_opcode as usize] = Instruction::$register_instr(
                                size,
                                ShiftDirection::Right,
                                dest,
                                shift,
                            );
                            table[l_opcode as usize] = Instruction::$register_instr(
                                size,
                                ShiftDirection::Left,
                                dest,
                                shift,
                            );
                        }

                        // xxl/xxR Dx, Dy
                        {
                            let shift = ShiftCount::Register(DataRegister(count_value));
                            let r_opcode = $reg_opcode_base
                                | size.to_bits()
                                | u16::from(dest.0)
                                | (u16::from(count_value) << 9);
                            let l_opcode = r_opcode | 0x0100;

                            table[r_opcode as usize] = Instruction::$register_instr(
                                size,
                                ShiftDirection::Right,
                                dest,
                                shift,
                            );
                            table[l_opcode as usize] = Instruction::$register_instr(
                                size,
                                ShiftDirection::Left,
                                dest,
                                shift,
                            );
                        }
                    }
                }
            }

            // xxL/xxR <ea>
            for dest in dest_addressing_modes_no_direct() {
                let r_opcode = $mem_opcode_base | dest.to_bits();
                let l_opcode = r_opcode | 0x0100;

                table[r_opcode as usize] = Instruction::$memory_instr(ShiftDirection::Right, dest);
                table[l_opcode as usize] = Instruction::$memory_instr(ShiftDirection::Left, dest);
            }
        }
    };
}

impl_populate_bit_shift!(
    populate_asd,
    0xE000,
    0xE020,
    0xE0C0,
    ArithmeticShiftRegister,
    ArithmeticShiftMemory
);
impl_populate_bit_shift!(
    populate_lsd,
    0xE008,
    0xE028,
    0xE2C0,
    LogicalShiftRegister,
    LogicalShiftMemory
);
impl_populate_bit_shift!(populate_rod, 0xE018, 0xE038, 0xE6C0, RotateRegister, RotateMemory);
impl_populate_bit_shift!(
    populate_roxd,
    0xE010,
    0xE030,
    0xE4C0,
    RotateThruExtendRegister,
    RotateThruExtendMemory
);

fn populate_bcc(table: &mut InstructionTable) {
    // Bcc #<d>
    for condition in BranchCondition::ALL {
        for displacement in 0..=0xFF_u16 {
            let opcode = 0x6000 | displacement | condition.to_bits();
            table[opcode as usize] = Instruction::Branch(condition, displacement as i8);
        }
    }
}

macro_rules! impl_populate_bit_test {
    ($name:ident, $imm_opcode_base:expr, $reg_opcode_base:expr, $instruction:ident) => {
        fn $name(table: &mut InstructionTable) {
            for dest in dest_addressing_modes_no_address_direct() {
                // Bxxx #<d>, <ea>
                let imm_opcode = $imm_opcode_base | dest.to_bits();
                table[imm_opcode as usize] =
                    Instruction::$instruction { source: AddressingMode::Immediate, dest };

                // Bxxx Dn, <ea>
                for source in DataRegister::ALL {
                    let reg_opcode = $reg_opcode_base | dest.to_bits() | (u16::from(source.0) << 9);
                    table[reg_opcode as usize] = Instruction::$instruction {
                        source: AddressingMode::DataDirect(source),
                        dest,
                    };
                }
            }
        }
    };
}

impl_populate_bit_test!(populate_bchg, 0x0840, 0x0140, BitTestAndChange);
impl_populate_bit_test!(populate_bclr, 0x0880, 0x0180, BitTestAndClear);
impl_populate_bit_test!(populate_bset, 0x08C0, 0x01C0, BitTestAndSet);

// Not using the macro because BTST supports PC relative addressing modes
fn populate_btst(table: &mut InstructionTable) {
    for dest in all_addressing_modes_no_address_direct() {
        // BTST #<d>, <ea>
        let imm_opcode = 0x0800 | dest.to_bits();
        table[imm_opcode as usize] =
            Instruction::BitTest { source: AddressingMode::Immediate, dest };

        // BTST Dn, <ea>
        for source in DataRegister::ALL {
            let reg_opcode = 0x0100 | dest.to_bits() | (u16::from(source.0) << 9);
            table[reg_opcode as usize] =
                Instruction::BitTest { source: AddressingMode::DataDirect(source), dest };
        }
    }
}

fn populate_bsr(table: &mut InstructionTable) {
    // BSR #<d>
    for displacement in 0..=0xFF_u16 {
        let opcode = 0x6100 | displacement;
        table[opcode as usize] = Instruction::BranchToSubroutine(displacement as i8);
    }
}

fn populate_chk(table: &mut InstructionTable) {
    // CHK <ea>, Dn
    for addressing_mode in all_addressing_modes_no_address_direct() {
        for register in DataRegister::ALL {
            let opcode = 0x4180 | addressing_mode.to_bits() | (u16::from(register.0) << 9);
            table[opcode as usize] = Instruction::CheckRegister(register, addressing_mode);
        }
    }
}

fn populate_clr(table: &mut InstructionTable) {
    // CLR <ea>
    for dest in dest_addressing_modes_no_address_direct() {
        for size in OpSize::ALL {
            let opcode = 0x4200 | size.to_bits() | dest.to_bits();
            table[opcode as usize] = Instruction::Clear(size, dest);
        }
    }
}

fn populate_cmp(table: &mut InstructionTable) {
    // CMP <ea>, Dn
    for source in all_addressing_modes() {
        for dest in DataRegister::ALL {
            for size in OpSize::ALL {
                let opcode = 0xB000 | size.to_bits() | source.to_bits() | (u16::from(dest.0) << 9);
                table[opcode as usize] =
                    Instruction::Compare { size, source, dest: AddressingMode::DataDirect(dest) };
            }
        }
    }
}

fn populate_cmpa(table: &mut InstructionTable) {
    // CMPA <ea>, An
    for source in all_addressing_modes() {
        for dest in AddressRegister::ALL {
            for size in [OpSize::Word, OpSize::LongWord] {
                let size_bit = u16::from(size == OpSize::LongWord) << 8;
                let opcode = 0xB0C0 | size_bit | source.to_bits() | (u16::from(dest.0) << 9);
                table[opcode as usize] = Instruction::Compare {
                    size,
                    source,
                    dest: AddressingMode::AddressDirect(dest),
                };
            }
        }
    }
}

fn populate_cmpi(table: &mut InstructionTable) {
    // CMPI #<d>, <ea>
    for dest in
        all_addressing_modes_no_address_direct().filter(|&mode| mode != AddressingMode::Immediate)
    {
        for size in OpSize::ALL {
            let opcode = 0x0C00 | size.to_bits() | dest.to_bits();
            table[opcode as usize] =
                Instruction::Compare { size, source: AddressingMode::Immediate, dest };
        }
    }
}

fn populate_cmpm(table: &mut InstructionTable) {
    // CMPM (Ay)+, (Ax)+
    for source in AddressRegister::ALL {
        for dest in AddressRegister::ALL {
            for size in OpSize::ALL {
                let opcode =
                    0xB108 | size.to_bits() | u16::from(source.0) | (u16::from(dest.0) << 9);
                table[opcode as usize] = Instruction::Compare {
                    size,
                    source: AddressingMode::AddressIndirectPostincrement(source),
                    dest: AddressingMode::AddressIndirectPostincrement(dest),
                };
            }
        }
    }
}

fn populate_dbcc(table: &mut InstructionTable) {
    // DBcc Dn, #<d>
    for condition in BranchCondition::ALL {
        for dest in DataRegister::ALL {
            let opcode = 0x50C8 | condition.to_bits() | u16::from(dest.0);
            table[opcode as usize] = Instruction::BranchDecrement(condition, dest);
        }
    }
}

fn populate_divs(table: &mut InstructionTable) {
    // DIVS <ea>, Dn
    for source in all_addressing_modes_no_address_direct() {
        for dest in DataRegister::ALL {
            let opcode = 0x81C0 | source.to_bits() | (u16::from(dest.0) << 9);
            table[opcode as usize] = Instruction::DivideSigned(dest, source);
        }
    }
}

fn populate_divu(table: &mut InstructionTable) {
    // DIVU <ea>, Dn
    for source in all_addressing_modes_no_address_direct() {
        for dest in DataRegister::ALL {
            let opcode = 0x80C0 | source.to_bits() | (u16::from(dest.0) << 9);
            table[opcode as usize] = Instruction::DivideUnsigned(dest, source);
        }
    }
}

fn populate_eor(table: &mut InstructionTable) {
    // EOR Dn, <ea>
    for source in DataRegister::ALL {
        for dest in dest_addressing_modes_no_address_direct() {
            for size in OpSize::ALL {
                let opcode = 0xB100 | size.to_bits() | dest.to_bits() | (u16::from(source.0) << 9);
                table[opcode as usize] = Instruction::ExclusiveOr {
                    size,
                    source: AddressingMode::DataDirect(source),
                    dest,
                };
            }
        }
    }
}

fn populate_eori(table: &mut InstructionTable) {
    // EORI #<d>, <ea>
    for dest in dest_addressing_modes_no_address_direct() {
        for size in OpSize::ALL {
            let opcode = 0x0A00 | size.to_bits() | dest.to_bits();
            table[opcode as usize] =
                Instruction::ExclusiveOr { size, source: AddressingMode::Immediate, dest };
        }
    }

    // EORI to CCR/SR
    table[0x0A3C] = Instruction::ExclusiveOrToCcr;
    table[0x0A7C] = Instruction::ExclusiveOrToSr;
}

fn populate_exg(table: &mut InstructionTable) {
    for rx in 0..8_u8 {
        for ry in 0..8_u8 {
            // EXG Dx, Dy
            let data_opcode = 0xC140 | u16::from(ry) | (u16::from(rx) << 9);
            table[data_opcode as usize] =
                Instruction::ExchangeData(DataRegister(rx), DataRegister(ry));

            // EXG Ax, Ay
            let address_opcode = data_opcode | 0x0008;
            table[address_opcode as usize] =
                Instruction::ExchangeAddress(AddressRegister(rx), AddressRegister(ry));

            // EXG Dx, Ay
            let mixed_opcode = 0xC188 | u16::from(ry) | (u16::from(rx) << 9);
            table[mixed_opcode as usize] =
                Instruction::ExchangeDataAddress(DataRegister(rx), AddressRegister(ry));
        }
    }
}

fn populate_ext(table: &mut InstructionTable) {
    // EXT Dn
    for register in DataRegister::ALL {
        for size in [OpSize::Word, OpSize::LongWord] {
            let size_bit = u16::from(size == OpSize::LongWord) << 6;
            let opcode = 0x4880 | size_bit | u16::from(register.0);
            table[opcode as usize] = Instruction::Extend(size, register);
        }
    }
}

fn populate_jmp(table: &mut InstructionTable) {
    // JMP <ea>
    for dest in jump_addressing_modes() {
        let opcode = 0x4EC0 | dest.to_bits();
        table[opcode as usize] = Instruction::Jump(dest);
    }
}

fn populate_jsr(table: &mut InstructionTable) {
    // JSR <ea>
    for dest in jump_addressing_modes() {
        let opcode = 0x4E80 | dest.to_bits();
        table[opcode as usize] = Instruction::JumpToSubroutine(dest);
    }
}

fn populate_lea(table: &mut InstructionTable) {
    // LEA <ea>, An
    for source in jump_addressing_modes() {
        for dest in AddressRegister::ALL {
            let opcode = 0x41C0 | source.to_bits() | (u16::from(dest.0) << 9);
            table[opcode as usize] = Instruction::LoadEffectiveAddress(source, dest);
        }
    }
}

fn populate_link(table: &mut InstructionTable) {
    // LINK An, #<d>
    for source in AddressRegister::ALL {
        let opcode = 0x4E50 | u16::from(source.0);
        table[opcode as usize] = Instruction::Link(source);
    }
}

fn populate_move(table: &mut InstructionTable) {
    // MOVE <ea>, <ea>
    for source in all_addressing_modes() {
        for dest in dest_addressing_modes_no_address_direct() {
            for size in OpSize::ALL {
                // Dest bits are shifted left 6, and mode/register are flipped
                let raw_dest_bits = dest.to_bits();
                let dest_bits = ((raw_dest_bits & 0x07) << 9) | ((raw_dest_bits & 0x38) << 3);

                let opcode = size.to_move_bits() | source.to_bits() | dest_bits;
                table[opcode as usize] = Instruction::Move { size, source, dest };
            }
        }
    }
}

fn populate_movea(table: &mut InstructionTable) {
    // MOVEA <ea>, An
    for source in all_addressing_modes() {
        for dest in AddressRegister::ALL {
            for size in [OpSize::Word, OpSize::LongWord] {
                let opcode =
                    0x0040 | size.to_move_bits() | source.to_bits() | (u16::from(dest.0) << 9);
                table[opcode as usize] =
                    Instruction::Move { size, source, dest: AddressingMode::AddressDirect(dest) };
            }
        }
    }
}

fn populate_movem(table: &mut InstructionTable) {
    // MOVEM <registers>, <ea>
    // Register-to-memory MOVEM does not support data/address direct, address indirect postincrement,
    // immediate, or PC relative displacement/indexed
    for dest in dest_addressing_modes_no_direct()
        .filter(|mode| !matches!(mode, AddressingMode::AddressIndirectPostincrement(..)))
    {
        for size in [OpSize::Word, OpSize::LongWord] {
            let size_bit = u16::from(size == OpSize::LongWord) << 6;
            let opcode = 0x4880 | size_bit | dest.to_bits();
            table[opcode as usize] =
                Instruction::MoveMultiple(size, dest, Direction::RegisterToMemory);
        }
    }

    // MOVEM <ea>, <registers>
    // Memory-to-register MOVEM does not support data/address direct, address indirect predecrement,
    // or immediate
    for source in all_addressing_modes().filter(|mode| {
        !matches!(
            mode,
            AddressingMode::DataDirect(..)
                | AddressingMode::AddressDirect(..)
                | AddressingMode::AddressIndirectPredecrement(..)
                | AddressingMode::Immediate
        )
    }) {
        for size in [OpSize::Word, OpSize::LongWord] {
            let size_bit = u16::from(size == OpSize::LongWord) << 6;
            let opcode = 0x4C80 | size_bit | source.to_bits();
            table[opcode as usize] =
                Instruction::MoveMultiple(size, source, Direction::MemoryToRegister);
        }
    }
}

fn populate_movep(table: &mut InstructionTable) {
    for d_register in DataRegister::ALL {
        for a_register in AddressRegister::ALL {
            for size in [OpSize::Word, OpSize::LongWord] {
                let size_bit = u16::from(size == OpSize::LongWord) << 6;
                let to_register_opcode =
                    0x0108 | size_bit | u16::from(a_register.0) | (u16::from(d_register.0) << 9);
                let from_register_opcode = to_register_opcode | 0x0080;

                // MOVEP (d, Ay), Dx
                table[to_register_opcode as usize] = Instruction::MovePeripheral(
                    size,
                    d_register,
                    a_register,
                    Direction::MemoryToRegister,
                );

                // MOVEP Dx, (D, Ay)
                table[from_register_opcode as usize] = Instruction::MovePeripheral(
                    size,
                    d_register,
                    a_register,
                    Direction::RegisterToMemory,
                );
            }
        }
    }
}

fn populate_moveq(table: &mut InstructionTable) {
    // MOVEQ #<d>, Dn
    for dest in DataRegister::ALL {
        for immediate_value in 0..=0xFF_u16 {
            let opcode = 0x7000 | immediate_value | (u16::from(dest.0) << 9);
            table[opcode as usize] = Instruction::MoveQuick(immediate_value as i8, dest);
        }
    }
}

fn populate_move_ccr_sr_usp(table: &mut InstructionTable) {
    // MOVE <ea>, CCR
    for source in all_addressing_modes_no_address_direct() {
        let opcode = 0x44C0 | source.to_bits();
        table[opcode as usize] = Instruction::MoveToCcr(source);
    }

    // MOVE SR, <ea>
    for dest in dest_addressing_modes_no_address_direct() {
        let opcode = 0x40C0 | dest.to_bits();
        table[opcode as usize] = Instruction::MoveFromSr(dest);
    }

    // MOVE <ea>, SR
    for source in all_addressing_modes_no_address_direct() {
        let opcode = 0x46C0 | source.to_bits();
        table[opcode as usize] = Instruction::MoveToSr(source);
    }

    // MOVE An, USP
    // MOVE USP, An
    for register in AddressRegister::ALL {
        let to_usp_opcode = 0x4E60 | u16::from(register.0);
        let from_usp_opcode = to_usp_opcode | 0x0008;

        table[to_usp_opcode as usize] = Instruction::MoveUsp(UspDirection::RegisterToUsp, register);
        table[from_usp_opcode as usize] =
            Instruction::MoveUsp(UspDirection::UspToRegister, register);
    }
}

fn populate_muls(table: &mut InstructionTable) {
    // MULS <ea>, Dn
    for source in all_addressing_modes_no_address_direct() {
        for dest in DataRegister::ALL {
            let opcode = 0xC1C0 | source.to_bits() | (u16::from(dest.0) << 9);
            table[opcode as usize] = Instruction::MultiplySigned(dest, source);
        }
    }
}

fn populate_mulu(table: &mut InstructionTable) {
    // MULU <ea>, Dn
    for source in all_addressing_modes_no_address_direct() {
        for dest in DataRegister::ALL {
            let opcode = 0xC0C0 | source.to_bits() | (u16::from(dest.0) << 9);
            table[opcode as usize] = Instruction::MultiplyUnsigned(dest, source);
        }
    }
}

fn populate_nbcd(table: &mut InstructionTable) {
    // NBCD <ea>
    for dest in dest_addressing_modes_no_address_direct() {
        let opcode = 0x4800 | dest.to_bits();
        table[opcode as usize] = Instruction::NegateDecimal(dest);
    }
}

fn populate_neg(table: &mut InstructionTable) {
    // NEG <ea>
    for dest in dest_addressing_modes_no_address_direct() {
        for size in OpSize::ALL {
            let opcode = 0x4400 | size.to_bits() | dest.to_bits();
            table[opcode as usize] = Instruction::Negate { size, dest, with_extend: false };
        }
    }
}

fn populate_negx(table: &mut InstructionTable) {
    // NEGX <ea>
    for dest in dest_addressing_modes_no_address_direct() {
        for size in OpSize::ALL {
            let opcode = 0x4000 | size.to_bits() | dest.to_bits();
            table[opcode as usize] = Instruction::Negate { size, dest, with_extend: true };
        }
    }
}

fn populate_nop(table: &mut InstructionTable) {
    // NOP
    table[0x4E71] = Instruction::NoOp;
}

fn populate_not(table: &mut InstructionTable) {
    // NOT <ea>
    for dest in dest_addressing_modes_no_address_direct() {
        for size in OpSize::ALL {
            let opcode = 0x4600 | size.to_bits() | dest.to_bits();
            table[opcode as usize] = Instruction::Not(size, dest);
        }
    }
}

fn populate_or(table: &mut InstructionTable) {
    // OR <ea>, Dn
    for source in all_addressing_modes_no_address_direct() {
        for dest in DataRegister::ALL {
            for size in OpSize::ALL {
                let opcode = 0x8000 | size.to_bits() | source.to_bits() | (u16::from(dest.0) << 9);
                table[opcode as usize] =
                    Instruction::Or { size, source, dest: AddressingMode::DataDirect(dest) };
            }
        }
    }

    // OR Dn, <ea>
    for source in DataRegister::ALL {
        for dest in dest_addressing_modes_no_direct() {
            for size in OpSize::ALL {
                let opcode = 0x8100 | size.to_bits() | dest.to_bits() | (u16::from(source.0) << 9);
                table[opcode as usize] =
                    Instruction::Or { size, source: AddressingMode::DataDirect(source), dest };
            }
        }
    }
}

fn populate_ori(table: &mut InstructionTable) {
    // ORI #<d>, <ea>
    for dest in dest_addressing_modes_no_address_direct() {
        for size in OpSize::ALL {
            let opcode = size.to_bits() | dest.to_bits();
            table[opcode as usize] =
                Instruction::Or { size, source: AddressingMode::Immediate, dest };
        }
    }

    // ORI to CCR/SR
    table[0x003C] = Instruction::OrToCcr;
    table[0x007C] = Instruction::OrToSr;
}

fn populate_pea(table: &mut InstructionTable) {
    for source in jump_addressing_modes() {
        let opcode = 0x4840 | source.to_bits();
        table[opcode as usize] = Instruction::PushEffectiveAddress(source);
    }
}

fn populate_reset(table: &mut InstructionTable) {
    // RESET
    table[0x4E70] = Instruction::Reset;
}

fn populate_rte_rtr_rts(table: &mut InstructionTable) {
    // RTE
    table[0x4E73] = Instruction::ReturnFromException;

    // RTR
    table[0x4E77] = Instruction::Return { restore_ccr: true };

    // RTS
    table[0x4E75] = Instruction::Return { restore_ccr: false };
}

fn populate_sbcd(table: &mut InstructionTable) {
    for rx in 0..8_u8 {
        for ry in 0..8_u8 {
            // SBCD Dx, Dy
            let data_opcode = 0x8100 | u16::from(ry) | (u16::from(rx) << 9);
            table[data_opcode as usize] = Instruction::SubtractDecimal {
                source: AddressingMode::DataDirect(DataRegister(ry)),
                dest: AddressingMode::DataDirect(DataRegister(rx)),
            };

            // SBCD -(Ax), -(Ay)
            let predec_opcode = data_opcode | 0x0008;
            table[predec_opcode as usize] = Instruction::SubtractDecimal {
                source: AddressingMode::AddressIndirectPredecrement(AddressRegister(ry)),
                dest: AddressingMode::AddressIndirectPredecrement(AddressRegister(rx)),
            };
        }
    }
}

fn populate_scc(table: &mut InstructionTable) {
    // Scc <ea>
    for dest in dest_addressing_modes_no_address_direct() {
        for condition in BranchCondition::ALL {
            let opcode = 0x50C0 | condition.to_bits() | dest.to_bits();
            table[opcode as usize] = Instruction::Set(condition, dest);
        }
    }
}

fn populate_stop(table: &mut InstructionTable) {
    // STOP
    table[0x4E72] = Instruction::Stop;
}

fn populate_sub(table: &mut InstructionTable) {
    // SUB <ea>, Dn
    for source in all_addressing_modes() {
        for dest in DataRegister::ALL {
            for size in OpSize::ALL {
                let opcode = 0x9000 | size.to_bits() | source.to_bits() | (u16::from(dest.0) << 9);
                table[opcode as usize] = Instruction::Subtract {
                    size,
                    source,
                    dest: AddressingMode::DataDirect(dest),
                    with_extend: false,
                };
            }
        }
    }

    // SUB Dn, <ea>
    for source in DataRegister::ALL {
        for dest in dest_addressing_modes_no_direct() {
            for size in OpSize::ALL {
                let opcode = 0x9100 | size.to_bits() | dest.to_bits() | (u16::from(source.0) << 9);
                table[opcode as usize] = Instruction::Subtract {
                    size,
                    source: AddressingMode::DataDirect(source),
                    dest,
                    with_extend: false,
                };
            }
        }
    }
}

fn populate_suba(table: &mut InstructionTable) {
    // SUBA <ea>, An
    for source in all_addressing_modes() {
        for dest in AddressRegister::ALL {
            for size in [OpSize::Word, OpSize::LongWord] {
                let size_bit = u16::from(size == OpSize::LongWord) << 8;
                let opcode = 0x90C0 | size_bit | source.to_bits() | (u16::from(dest.0) << 9);
                table[opcode as usize] = Instruction::Subtract {
                    size,
                    source,
                    dest: AddressingMode::AddressDirect(dest),
                    with_extend: false,
                };
            }
        }
    }
}

fn populate_subi(table: &mut InstructionTable) {
    // SUBI #<d>, <ea>
    for dest in dest_addressing_modes_no_address_direct() {
        for size in OpSize::ALL {
            let opcode = 0x0400 | size.to_bits() | dest.to_bits();
            table[opcode as usize] = Instruction::Subtract {
                size,
                source: AddressingMode::Immediate,
                dest,
                with_extend: false,
            };
        }
    }
}

fn populate_subq(table: &mut InstructionTable) {
    // SUBQ #<d>, <ea>
    for dest in dest_addressing_modes() {
        for size in OpSize::ALL {
            for q_value in 0..8_u16 {
                let opcode = 0x5100 | size.to_bits() | dest.to_bits() | (q_value << 9);
                let source = if q_value == 0 {
                    AddressingMode::Quick(8)
                } else {
                    AddressingMode::Quick(q_value as u8)
                };

                table[opcode as usize] =
                    Instruction::Subtract { size, source, dest, with_extend: false };
            }
        }
    }
}

fn populate_subx(table: &mut InstructionTable) {
    for rx in 0..8_u8 {
        for ry in 0..8_u8 {
            for size in OpSize::ALL {
                // SUBX Dx, Dy
                let data_opcode = 0x9100 | size.to_bits() | u16::from(rx) | (u16::from(ry) << 9);
                table[data_opcode as usize] = Instruction::Subtract {
                    size,
                    source: AddressingMode::DataDirect(DataRegister(rx)),
                    dest: AddressingMode::DataDirect(DataRegister(ry)),
                    with_extend: true,
                };

                // SUBX -(Ax), -(Ay)
                let predec_opcode = data_opcode | 0x0008;
                table[predec_opcode as usize] = Instruction::Subtract {
                    size,
                    source: AddressingMode::AddressIndirectPredecrement(AddressRegister(rx)),
                    dest: AddressingMode::AddressIndirectPredecrement(AddressRegister(ry)),
                    with_extend: true,
                };
            }
        }
    }
}

fn populate_swap(table: &mut InstructionTable) {
    // SWAP Dn
    for dest in DataRegister::ALL {
        let opcode = 0x4840 | u16::from(dest.0);
        table[opcode as usize] = Instruction::Swap(dest);
    }
}

fn populate_tas(table: &mut InstructionTable) {
    // TAS <ea>
    for dest in dest_addressing_modes_no_address_direct() {
        let opcode = 0x4AC0 | dest.to_bits();
        table[opcode as usize] = Instruction::TestAndSet(dest);
    }
}

fn populate_trap(table: &mut InstructionTable) {
    // TRAP #<vector>
    for vector in 0..=0xF_u16 {
        let opcode = 0x4E40 | vector;
        table[opcode as usize] = Instruction::Trap(vector.into());
    }

    // TRAPV
    table[0x4E76] = Instruction::TrapOnOverflow;
}

fn populate_tst(table: &mut InstructionTable) {
    // TST <ea>
    for source in all_addressing_modes() {
        for size in OpSize::ALL {
            let opcode = 0x4A00 | size.to_bits() | source.to_bits();
            table[opcode as usize] = Instruction::Test(size, source);
        }
    }
}

fn populate_unlk(table: &mut InstructionTable) {
    // UNLK An
    for register in AddressRegister::ALL {
        let opcode = 0x4E58 | u16::from(register.0);
        table[opcode as usize] = Instruction::Unlink(register);
    }
}
