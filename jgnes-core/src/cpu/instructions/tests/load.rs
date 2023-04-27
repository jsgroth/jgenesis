use super::{hash_map, run_test, ExpectedState};

#[test]
fn lda_immediate() {
    run_test(
        // LDA #$78
        "A978",
        ExpectedState {
            a: Some(0x78),
            p: Some(0x34),
            cycles: Some(2),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$DD
        "A9DD",
        ExpectedState {
            a: Some(0xDD),
            p: Some(0xB4),
            cycles: Some(2),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$00
        "A900",
        ExpectedState {
            a: Some(0x00),
            p: Some(0x36),
            cycles: Some(2),
            ..ExpectedState::default()
        },
    );
}

// TODO LDA zero page / zero page X / absolute / absolute X / absolute Y / indexed indirect / indirect indexed

#[test]
fn ldx_immediate() {
    run_test(
        // LDX #$78
        "A278",
        ExpectedState {
            x: Some(0x78),
            p: Some(0x34),
            cycles: Some(2),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDX #$DD
        "A2DD",
        ExpectedState {
            x: Some(0xDD),
            p: Some(0xB4),
            cycles: Some(2),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDX #$00
        "A200",
        ExpectedState {
            x: Some(0x00),
            p: Some(0x36),
            cycles: Some(2),
            ..ExpectedState::default()
        },
    );
}

// TODO LDX zero page / zero page Y / absolute / absolute Y

#[test]
fn ldy_immediate() {
    run_test(
        // LDY #$78
        "A078",
        ExpectedState {
            y: Some(0x78),
            p: Some(0x34),
            cycles: Some(2),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDY #$DD
        "A0DD",
        ExpectedState {
            y: Some(0xDD),
            p: Some(0xB4),
            cycles: Some(2),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDY #$00
        "A000",
        ExpectedState {
            y: Some(0x00),
            p: Some(0x36),
            cycles: Some(2),
            ..ExpectedState::default()
        },
    );
}

// TODO LDY zero page / zero page X / absolute / absolute X

#[test]
fn tax() {
    run_test(
        // TAX
        "AA",
        ExpectedState {
            a: Some(0x00),
            x: Some(0x00),
            p: Some(0x36),
            cycles: Some(2),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$45; TAX
        concat!("A945", "AA"),
        ExpectedState {
            a: Some(0x45),
            x: Some(0x45),
            p: Some(0x34),
            cycles: Some(4),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$CD; TAX
        concat!("A9CD", "AA"),
        ExpectedState {
            a: Some(0xCD),
            x: Some(0xCD),
            p: Some(0xB4),
            cycles: Some(4),
            ..ExpectedState::default()
        },
    );
}

#[test]
fn tay() {
    run_test(
        // TAY
        "A8",
        ExpectedState {
            a: Some(0x00),
            y: Some(0x00),
            p: Some(0x36),
            cycles: Some(2),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$45; TAY
        concat!("A945", "A8"),
        ExpectedState {
            a: Some(0x45),
            y: Some(0x45),
            p: Some(0x34),
            cycles: Some(4),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$CD; TAY
        concat!("A9CD", "A8"),
        ExpectedState {
            a: Some(0xCD),
            y: Some(0xCD),
            p: Some(0xB4),
            cycles: Some(4),
            ..ExpectedState::default()
        },
    );
}

#[test]
fn txs() {
    run_test(
        // TXS
        "9A",
        ExpectedState {
            x: Some(0x00),
            s: Some(0x00),
            p: Some(0x34),
            cycles: Some(2),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDX #$FF; LDA #$01; TXS
        concat!("A2FF", "A901", "9A"),
        ExpectedState {
            a: Some(0x01),
            x: Some(0xFF),
            s: Some(0xFF),
            p: Some(0x34),
            cycles: Some(6),
            ..ExpectedState::default()
        },
    );
}

#[test]
fn tsx() {
    run_test(
        // TSX
        "BA",
        ExpectedState {
            x: Some(0xFD),
            s: Some(0xFD),
            p: Some(0xB4),
            cycles: Some(2),
            ..ExpectedState::default()
        },
    );

    run_test(
        // TXS; TSX; LDX #$FF; TSX
        concat!("9A", "BA", "A2FF", "BA"),
        ExpectedState {
            x: Some(0x00),
            s: Some(0x00),
            p: Some(0x36),
            cycles: Some(8),
            ..ExpectedState::default()
        },
    );
}

#[test]
fn txa() {
    run_test(
        // TXA
        "8A",
        ExpectedState {
            a: Some(0x00),
            x: Some(0x00),
            p: Some(0x36),
            cycles: Some(2),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDX #$45; LDA #$00; TXA
        concat!("A245", "A900", "8A"),
        ExpectedState {
            a: Some(0x45),
            x: Some(0x45),
            p: Some(0x34),
            cycles: Some(6),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDX #$EE; LDA #$00; TXA
        concat!("A2EE", "A900", "8A"),
        ExpectedState {
            a: Some(0xEE),
            x: Some(0xEE),
            p: Some(0xB4),
            cycles: Some(6),
            ..ExpectedState::default()
        },
    );
}

#[test]
fn tya() {
    run_test(
        // TYA
        "98",
        ExpectedState {
            a: Some(0x00),
            y: Some(0x00),
            p: Some(0x36),
            cycles: Some(2),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDY #$45; LDA #$00; TYA
        concat!("A045", "A900", "98"),
        ExpectedState {
            a: Some(0x45),
            y: Some(0x45),
            p: Some(0x34),
            cycles: Some(6),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDY #$EE; LDA #$00; TYA
        concat!("A0EE", "A900", "98"),
        ExpectedState {
            a: Some(0xEE),
            y: Some(0xEE),
            p: Some(0xB4),
            cycles: Some(6),
            ..ExpectedState::default()
        },
    );
}

#[test]
fn sta_zero_page() {
    run_test(
        // STA $45
        "8545",
        ExpectedState {
            a: Some(0x00),
            p: Some(0x34),
            memory: hash_map! { 0x0045: 0x00 },
            cycles: Some(3),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$BA; STA $45
        concat!("A9BA", "8545"),
        ExpectedState {
            a: Some(0xBA),
            p: Some(0xB4),
            memory: hash_map! { 0x0045: 0xBA },
            cycles: Some(5),
            ..ExpectedState::default()
        },
    );
}

#[test]
fn sta_zero_page_indexed() {
    run_test(
        // STA $45,X
        "9545",
        ExpectedState {
            a: Some(0x00),
            x: Some(0x00),
            p: Some(0x34),
            memory: hash_map! { 0x0045: 0x00 },
            cycles: Some(4),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$78; LDX #$05; STA $45,X
        concat!("A978", "A205", "9545"),
        ExpectedState {
            a: Some(0x78),
            x: Some(0x05),
            p: Some(0x34),
            memory: hash_map! { 0x004A: 0x78 },
            cycles: Some(8),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$78; LDX #$10; STA $F5,X
        concat!("A978", "A210", "95F5"),
        ExpectedState {
            a: Some(0x78),
            x: Some(0x10),
            p: Some(0x34),
            memory: hash_map! { 0x0005: 0x78 },
            cycles: Some(8),
            ..ExpectedState::default()
        },
    );
}

#[test]
fn sta_absolute() {
    run_test(
        // STA $6578
        "8D7865",
        ExpectedState {
            a: Some(0x00),
            p: Some(0x34),
            memory: hash_map! { 0x6578: 0x00 },
            cycles: Some(4),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$85; STA $6578
        concat!("A985", "8D7865"),
        ExpectedState {
            a: Some(0x85),
            p: Some(0xB4),
            memory: hash_map! { 0x6578: 0x85 },
            cycles: Some(6),
            ..ExpectedState::default()
        },
    );
}

#[test]
fn sta_absolute_x() {
    run_test(
        // STA $6578,X
        "9D7865",
        ExpectedState {
            a: Some(0x00),
            x: Some(0x00),
            p: Some(0x34),
            memory: hash_map! { 0x6578: 0x00 },
            cycles: Some(5),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$85; LDX #$34; STA $6578,X
        concat!("A985", "A234", "9D7865"),
        ExpectedState {
            a: Some(0x85),
            x: Some(0x34),
            p: Some(0x34),
            memory: hash_map! { 0x65AC: 0x85 },
            cycles: Some(9),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$85; LDX #$34; STA $65F0,X
        concat!("A985", "A234", "9DF065"),
        ExpectedState {
            a: Some(0x85),
            x: Some(0x34),
            p: Some(0x34),
            memory: hash_map! { 0x6624: 0x85 },
            cycles: Some(9),
            ..ExpectedState::default()
        },
    );
}

#[test]
fn sta_absolute_y() {
    run_test(
        // STA $6578,Y
        "997865",
        ExpectedState {
            a: Some(0x00),
            y: Some(0x00),
            p: Some(0x34),
            memory: hash_map! { 0x6578: 0x00 },
            cycles: Some(5),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$85; LDY #$34; STA $6578,Y
        concat!("A985", "A034", "997865"),
        ExpectedState {
            a: Some(0x85),
            y: Some(0x34),
            p: Some(0x34),
            memory: hash_map! { 0x65AC: 0x85 },
            cycles: Some(9),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$85; LDY #$34; STA $65F0,Y
        concat!("A985", "A034", "99F065"),
        ExpectedState {
            a: Some(0x85),
            y: Some(0x34),
            p: Some(0x34),
            memory: hash_map! { 0x6624: 0x85 },
            cycles: Some(9),
            ..ExpectedState::default()
        },
    );
}

// TODO STA indexed indirect / indirect indexed

#[test]
fn stx_zero_page() {
    run_test(
        // STX $45
        "8645",
        ExpectedState {
            x: Some(0x00),
            p: Some(0x34),
            memory: hash_map! { 0x0045: 0x00 },
            cycles: Some(3),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDX #$BA; STX $45
        concat!("A2BA", "8645"),
        ExpectedState {
            x: Some(0xBA),
            p: Some(0xB4),
            memory: hash_map! { 0x0045: 0xBA },
            cycles: Some(5),
            ..ExpectedState::default()
        },
    );
}

#[test]
fn stx_zero_page_indexed() {
    run_test(
        // STX $45,Y
        "9645",
        ExpectedState {
            x: Some(0x00),
            y: Some(0x00),
            p: Some(0x34),
            memory: hash_map! { 0x0045: 0x00 },
            cycles: Some(4),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDX #$78; LDY #$05; STX $45,Y
        concat!("A278", "A005", "9645"),
        ExpectedState {
            x: Some(0x78),
            y: Some(0x05),
            p: Some(0x34),
            memory: hash_map! { 0x004A: 0x78 },
            cycles: Some(8),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDX #$78; LDY #$10; STX $F5,Y
        concat!("A278", "A010", "96F5"),
        ExpectedState {
            x: Some(0x78),
            y: Some(0x10),
            p: Some(0x34),
            memory: hash_map! { 0x0005: 0x78 },
            cycles: Some(8),
            ..ExpectedState::default()
        },
    );
}

#[test]
fn stx_absolute() {
    run_test(
        // STX $6578
        "8E7865",
        ExpectedState {
            x: Some(0x00),
            p: Some(0x34),
            memory: hash_map! { 0x6578: 0x00 },
            cycles: Some(4),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDX #$85; STX $6578
        concat!("A285", "8E7865"),
        ExpectedState {
            x: Some(0x85),
            p: Some(0xB4),
            memory: hash_map! { 0x6578: 0x85 },
            cycles: Some(6),
            ..ExpectedState::default()
        },
    );
}

#[test]
fn sty_zero_page() {
    run_test(
        // STY $45
        "8445",
        ExpectedState {
            y: Some(0x00),
            p: Some(0x34),
            memory: hash_map! { 0x0045: 0x00 },
            cycles: Some(3),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDY #$BA; STY $45
        concat!("A0BA", "8445"),
        ExpectedState {
            y: Some(0xBA),
            p: Some(0xB4),
            memory: hash_map! { 0x0045: 0xBA },
            cycles: Some(5),
            ..ExpectedState::default()
        },
    );
}

#[test]
fn sty_zero_page_indexed() {
    run_test(
        // STY $45,X
        "9445",
        ExpectedState {
            y: Some(0x00),
            x: Some(0x00),
            p: Some(0x34),
            memory: hash_map! { 0x0045: 0x00 },
            cycles: Some(4),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDY #$78; LDX #$05; STY $45,X
        concat!("A078", "A205", "9445"),
        ExpectedState {
            y: Some(0x78),
            x: Some(0x05),
            p: Some(0x34),
            memory: hash_map! { 0x004A: 0x78 },
            cycles: Some(8),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDY #$78; LDX #$10; STY $F5,X
        concat!("A078", "A210", "94F5"),
        ExpectedState {
            y: Some(0x78),
            x: Some(0x10),
            p: Some(0x34),
            memory: hash_map! { 0x0005: 0x78 },
            cycles: Some(8),
            ..ExpectedState::default()
        },
    );
}

#[test]
fn sty_absolute() {
    run_test(
        // STY $6578
        "8C7865",
        ExpectedState {
            y: Some(0x00),
            p: Some(0x34),
            memory: hash_map! { 0x6578: 0x00 },
            cycles: Some(4),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDY #$85; STY $6578
        concat!("A085", "8C7865"),
        ExpectedState {
            y: Some(0x85),
            p: Some(0xB4),
            memory: hash_map! { 0x6578: 0x85 },
            cycles: Some(6),
            ..ExpectedState::default()
        },
    );
}
