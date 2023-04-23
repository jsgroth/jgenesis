use super::{run_test, ExpectedState};

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
