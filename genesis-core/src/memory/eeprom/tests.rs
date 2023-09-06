use super::*;
use test_log::test;

macro_rules! test_op {
    ($chip:expr, read expect=$sda:literal) => {
        assert_eq!(
            $sda != 0,
            $chip.handle_read(),
            "Expected SDA {}, got {}",
            $sda,
            u8::from($chip.handle_read())
        );
    };
    ($chip:expr, write sda=$sda:literal scl=$scl:literal) => {
        $chip.handle_dual_write($sda != 0, $scl != 0);
    };
}

macro_rules! run_test {
    ($chip:expr, [$(($($t:tt)* $(,)?)),* $(,)?]) => {
        $(
            test_op!($chip, $($t)*);
        )*
    };
}

// Reads in the same style as Wonder Boy in Monster World
#[test]
fn x24c01_basic_read() {
    let mut bytes = vec![0_u8; 128];
    bytes[0] = 0b0110_1001;
    bytes[1] = 0b1100_0011;
    let mut x24c01 = X24C01Chip::new(Some(&bytes));

    run_test!(x24c01, [
        // Initialize to standby
        (write sda=1 scl=1),
        (write sda=0 scl=1),
        // Send address
        (write sda=0 scl=0),
        (write sda=0 scl=0),
        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),
        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),
        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),
        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),
        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),
        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),
        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=1 scl=0),
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        // Address ACK
        (read expect=0),
        // Send data at address 0
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=0),
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=1),
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=1),
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=0),
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=1),
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=0),
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=0),
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=1),
        (write sda=1 scl=0),
        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),
        // Send data at address 1
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=1),
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=1),
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=0),
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=0),
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=0),
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=0),
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=1),
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=1),
    ]);
}

// Writes in the same style as Wonder Boy in Monster World
#[test]
fn x24c01_basic_write() {
    let mut x24c01 = X24C01Chip::new(None);

    run_test!(x24c01, [
        (write sda=1 scl=1),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        // Send address (2)
        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=1 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        // Address ACK
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=0),
        (write sda=1 scl=0),

        // Send data (1100 0011)
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=1 scl=0),

        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=1 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=1 scl=0),

        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=1 scl=0),

        // Write ACK
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=0),
        (write sda=1 scl=0),

        // Send data (1001 0110)
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=1 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=1 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=1 scl=0),

        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=1 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        // Write ACK
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=0),
        (write sda=1 scl=0),
    ]);

    let memory = x24c01.get_memory();
    assert_eq!(memory[2], 0b1100_0011, "byte at address 6");
    assert_eq!(memory[3], 0b1001_0110, "byte at address 7");
}

// Reads in the same style as NBA Jam
#[test]
fn x24c02_read_dual() {
    let mut bytes = vec![0_u8; 256];
    bytes[6] = 0b0110_1001;
    bytes[7] = 0b1100_0011;
    let mut x24c02 = X24C02Chip::new(Some(&bytes));

    run_test!(x24c02, [
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        // Send device address
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=1 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=1 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        // Device address ACK
        (write sda=1 scl=0),
        (read expect=0),
        (write sda=1 scl=1),

        // Send write address
        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=1 scl=0),

        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=1 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        // Write address ACK
        (write sda=1 scl=0),
        (read expect=0),
        (write sda=1 scl=1),

        // Restart
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        // Send device address (again), but with R flag set
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=1 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=1 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=1 scl=0),

        // Device address ACK
        (write sda=1 scl=0),
        (read expect=0),

        // Read data (0110 1001)
        (write sda=1 scl=1),
        (write sda=1 scl=0),
        (read expect=0),

        (write sda=1 scl=1),
        (write sda=1 scl=0),
        (read expect=1),

        (write sda=1 scl=1),
        (write sda=1 scl=0),
        (read expect=1),

        (write sda=1 scl=1),
        (write sda=1 scl=0),
        (read expect=0),

        (write sda=1 scl=1),
        (write sda=1 scl=0),
        (read expect=1),

        (write sda=1 scl=1),
        (write sda=1 scl=0),
        (read expect=0),

        (write sda=1 scl=1),
        (write sda=1 scl=0),
        (read expect=0),

        (write sda=1 scl=1),
        (write sda=1 scl=0),
        (read expect=1),

        // Begin next send
        (write sda=1 scl=1),
        (write sda=1 scl=0),
        (write sda=0 scl=0),

        // Read data (1100 0011)
        (write sda=0 scl=1),
        (write sda=1 scl=0),
        (read expect=1),

        (write sda=1 scl=1),
        (write sda=1 scl=0),
        (read expect=1),

        (write sda=1 scl=1),
        (write sda=1 scl=0),
        (read expect=0),

        (write sda=1 scl=1),
        (write sda=1 scl=0),
        (read expect=0),

        (write sda=1 scl=1),
        (write sda=1 scl=0),
        (read expect=0),

        (write sda=1 scl=1),
        (write sda=1 scl=0),
        (read expect=0),

        (write sda=1 scl=1),
        (write sda=1 scl=0),
        (read expect=1),

        (write sda=1 scl=1),
        (write sda=1 scl=0),
        (read expect=1),
    ]);
}

// NBA Jam Tournament Edition and NFL Quarterback Club do this on startup, seemingly to test
// that the EEPROM chip is present?
#[test]
fn x24c02_toggle_read_while_stopped() {
    let mut x24c02 = X24C02Chip::new(None);

    run_test!(x24c02, [
        (write sda=0 scl=0),
        (write sda=1 scl=0),
        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=1 scl=1),
        (write sda=1 scl=0),
        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=1 scl=1),
        (write sda=1 scl=0),
        (write sda=1 scl=0),
        (read expect=1),
        (write sda=0 scl=0),
        (read expect=0),
        (write sda=1 scl=0),
        (read expect=1),
        (write sda=0 scl=0),
        (read expect=0),
        (write sda=1 scl=0),
        (read expect=1),
    ]);
}

// Reads in the style of NFL Quarterback Club
#[test]
fn x24c08_read_individual() {
    let mut bytes = vec![0_u8; 1024];
    bytes[512 + 6] = 0b1100_0011;
    bytes[512 + 7] = 0b0110_1001;
    let mut x24c08 = X24C08Chip::new(Some(&bytes));

    run_test!(x24c08, [
        // Initialize
        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=1 scl=1),
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        // Send device address
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=1 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=1 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        // Device address ACK
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=0),
        (write sda=1 scl=0),

        // Send write address
        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=1 scl=0),

        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=1 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        // Write address ACK
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=0),
        (write sda=1 scl=0),

        // Restart
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        // Send device address (again), with R flag and A9 set
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=1 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=1 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=1 scl=0),

        (write sda=0 scl=0),
        (write sda=0 scl=1),
        (write sda=0 scl=0),

        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (write sda=1 scl=0),

        // Device address ACK
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=0),
        (write sda=1 scl=0),

        // Read data at address (1100 0011)
        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=1),

        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=1),

        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=0),

        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=0),

        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=0),

        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=0),

        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=1),

        (write sda=1 scl=0),
        (write sda=1 scl=1),
        (read expect=1),
    ]);
}
