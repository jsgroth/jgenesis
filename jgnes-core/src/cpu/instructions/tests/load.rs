use super::{hash_map, run_test, ExpectedState};

#[test]
fn lda_immediate() {
    run_test(
        // LDA #$78
        "A978",
        ExpectedState {
            a: Some(0x78),
            p: Some(0x34),
            cycles: Some(3),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$DD
        "A9DD",
        ExpectedState {
            a: Some(0xDD),
            p: Some(0xB4),
            cycles: Some(3),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$00
        "A900",
        ExpectedState {
            a: Some(0x00),
            p: Some(0x36),
            cycles: Some(3),
            ..ExpectedState::default()
        },
    );
}

#[test]
fn ldx_immediate() {
    run_test(
        // LDX #$78
        "A278",
        ExpectedState {
            x: Some(0x78),
            p: Some(0x34),
            cycles: Some(3),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDX #$DD
        "A2DD",
        ExpectedState {
            x: Some(0xDD),
            p: Some(0xB4),
            cycles: Some(3),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDX #$00
        "A200",
        ExpectedState {
            x: Some(0x00),
            p: Some(0x36),
            cycles: Some(3),
            ..ExpectedState::default()
        },
    );
}

#[test]
fn ldy_immediate() {
    run_test(
        // LDY #$78
        "A078",
        ExpectedState {
            y: Some(0x78),
            p: Some(0x34),
            cycles: Some(3),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDY #$DD
        "A0DD",
        ExpectedState {
            y: Some(0xDD),
            p: Some(0xB4),
            cycles: Some(3),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDY #$00
        "A000",
        ExpectedState {
            y: Some(0x00),
            p: Some(0x36),
            cycles: Some(3),
            ..ExpectedState::default()
        },
    );
}

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
        "A945AA",
        ExpectedState {
            a: Some(0x45),
            x: Some(0x45),
            p: Some(0x34),
            cycles: Some(5),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$CD; TAX
        "A9CDAA",
        ExpectedState {
            a: Some(0xCD),
            x: Some(0xCD),
            p: Some(0xB4),
            cycles: Some(5),
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
        "A945A8",
        ExpectedState {
            a: Some(0x45),
            y: Some(0x45),
            p: Some(0x34),
            cycles: Some(5),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$CD; TAY
        "A9CDA8",
        ExpectedState {
            a: Some(0xCD),
            y: Some(0xCD),
            p: Some(0xB4),
            cycles: Some(5),
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
        "A2FFA9019A",
        ExpectedState {
            a: Some(0x01),
            x: Some(0xFF),
            s: Some(0xFF),
            p: Some(0x34),
            cycles: Some(8),
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
        "9ABAA2FFBA",
        ExpectedState {
            x: Some(0x00),
            s: Some(0x00),
            p: Some(0x36),
            cycles: Some(9),
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
        "A245A9008A",
        ExpectedState {
            a: Some(0x45),
            x: Some(0x45),
            p: Some(0x34),
            cycles: Some(8),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDX #$EE; LDA #$00; TXA
        "A2EEA9008A",
        ExpectedState {
            a: Some(0xEE),
            x: Some(0xEE),
            p: Some(0xB4),
            cycles: Some(8),
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
        "A045A90098",
        ExpectedState {
            a: Some(0x45),
            y: Some(0x45),
            p: Some(0x34),
            cycles: Some(8),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDY #$EE; LDA #$00; TYA
        "A0EEA90098",
        ExpectedState {
            a: Some(0xEE),
            y: Some(0xEE),
            p: Some(0xB4),
            cycles: Some(8),
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
        "A9BA8545",
        ExpectedState {
            a: Some(0xBA),
            p: Some(0xB4),
            memory: hash_map! { 0x0045: 0xBA },
            cycles: Some(6),
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
        "A978A2059545",
        ExpectedState {
            a: Some(0x78),
            x: Some(0x05),
            p: Some(0x34),
            memory: hash_map! { 0x0045: 0x00, 0x004A: 0x78 },
            cycles: Some(10),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$78; LDX #$10; STA $F5,X
        "A978A21095F5",
        ExpectedState {
            a: Some(0x78),
            x: Some(0x10),
            p: Some(0x34),
            memory: hash_map! { 0x0005: 0x78, 0x0105: 0x00 },
            cycles: Some(10),
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
        // LDA #$85; STA 6578
        "A9858D7865",
        ExpectedState {
            a: Some(0x85),
            p: Some(0xB4),
            memory: hash_map! { 0x6578: 0x85 },
            cycles: Some(7),
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
        "A985A2349D7865",
        ExpectedState {
            a: Some(0x85),
            x: Some(0x34),
            p: Some(0x34),
            memory: hash_map! { 0x6578: 0x00, 0x65AC: 0x85 },
            cycles: Some(11),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$85; LDX #$34; STA $65F0,X
        "A985A2349DF065",
        ExpectedState {
            a: Some(0x85),
            x: Some(0x34),
            p: Some(0x34),
            memory: hash_map! { 0x65F0: 0x00, 0x6624: 0x85 },
            cycles: Some(11),
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
        "A985A034997865",
        ExpectedState {
            a: Some(0x85),
            y: Some(0x34),
            p: Some(0x34),
            memory: hash_map! { 0x6578: 0x00, 0x65AC: 0x85 },
            cycles: Some(11),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$85; LDY #$34; STA $65F0,Y
        "A985A03499F065",
        ExpectedState {
            a: Some(0x85),
            y: Some(0x34),
            p: Some(0x34),
            memory: hash_map! { 0x65F0: 0x00, 0x6624: 0x85 },
            cycles: Some(11),
            ..ExpectedState::default()
        },
    );
}
