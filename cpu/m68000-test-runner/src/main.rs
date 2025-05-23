//! Designed to run the 68000 tests from <https://github.com/TomHarte/ProcessorTests>

use clap::Parser;
use env_logger::Env;
use flate2::read::GzDecoder;
use m68000_emu::M68000;
use m68000_emu::bus::InMemoryBus;
use m68000_emu::traits::BusInterface;
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::mpsc;
use std::thread;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct State {
    d0: u32,
    d1: u32,
    d2: u32,
    d3: u32,
    d4: u32,
    d5: u32,
    d6: u32,
    d7: u32,
    a0: u32,
    a1: u32,
    a2: u32,
    a3: u32,
    a4: u32,
    a5: u32,
    a6: u32,
    usp: u32,
    ssp: u32,
    sr: u16,
    pc: u32,
    prefetch: [u16; 2],
    ram: Vec<(u32, u8)>,
}

macro_rules! diff_field {
    ($actual:expr, $expected:expr, $field:ident) => {
        if $actual.$field != $expected.$field {
            log::info!(
                "  {}: actual={:08X}, expected={:08X}",
                stringify!($field),
                $actual.$field,
                $expected.$field
            );
        }
    };
}

macro_rules! diff_fields {
    ($actual:expr, $expected:expr, [$($hex_field:ident),*]) => {
        $(
            diff_field!($actual, $expected, $hex_field);
        )*
    }
}

impl State {
    fn from(m68000: &M68000, bus: &mut InMemoryBus, final_state: &State) -> Self {
        let [d0, d1, d2, d3, d4, d5, d6, d7] = m68000.data_registers();
        let [a0, a1, a2, a3, a4, a5, a6] = m68000.address_registers();

        let ram =
            final_state.ram.iter().map(|&(address, _)| (address, bus.read_byte(address))).collect();

        Self {
            d0,
            d1,
            d2,
            d3,
            d4,
            d5,
            d6,
            d7,
            a0,
            a1,
            a2,
            a3,
            a4,
            a5,
            a6,
            usp: m68000.user_stack_pointer(),
            ssp: m68000.supervisor_stack_pointer(),
            sr: m68000.status_register(),
            pc: m68000.pc(),
            prefetch: final_state.prefetch,
            ram,
        }
    }

    fn diff(&self, expected: &Self) {
        diff_fields!(
            self,
            expected,
            [d0, d1, d2, d3, d4, d5, d6, d7, a0, a1, a2, a3, a4, a5, a6, usp, ssp, sr, pc]
        );

        if self.ram != expected.ram {
            log::info!("  ram:");
            for ((address, actual), (_, expected)) in
                self.ram.iter().copied().zip(expected.ram.iter().copied())
            {
                if actual != expected {
                    log::info!("    {address:08X}: actual={actual:02X}, expected={expected:02X}");
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TestDescription {
    name: String,
    initial: State,
    #[serde(rename = "final")]
    final_state: State,
    length: u32,
}

#[derive(Debug, Parser)]
struct Args {
    /// Path to a single test file to run.
    #[arg(short = 'f', long)]
    file_path: Option<String>,

    /// Path to a directory of tests to run.
    #[arg(short = 'd', long)]
    dir_path: Option<String>,

    /// Don't log details on individual test case failures
    #[arg(short = 's', long = "no-individual-logs", default_value_t = true, action = clap::ArgAction::SetFalse)]
    individual_logs: bool,
}

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info,m68000_emu::core=off"))
        .init();

    let args = Args::parse();
    match (args.file_path, args.dir_path) {
        (Some(file_path), None) => {
            run_file_test(&file_path, args.individual_logs);
        }
        (None, Some(dir_path)) => {
            run_directory_of_tests(&dir_path, args.individual_logs);
        }
        (Some(_), Some(_)) | (None, None) => {
            panic!("exactly one of file_path and dir_path must be set");
        }
    }
}

fn run_file_test(file_path: &str, individual_logs: bool) {
    let file_path = Path::new(&file_path);

    let file_ext = file_path.extension().and_then(OsStr::to_str).unwrap();
    let file = BufReader::new(File::open(file_path).unwrap());
    let file: Box<dyn Read> = match file_ext {
        "json" => Box::new(file),
        "gz" => Box::new(GzDecoder::new(file)),
        _ => panic!("unsupported file extension: {file_ext}"),
    };

    let test_descriptions: Vec<TestDescription> = serde_json::from_reader(file).unwrap();

    log::info!("Loaded {} tests", test_descriptions.len());

    let mut bus = InMemoryBus::new();
    run_single_test(&test_descriptions, &mut bus, file_path, individual_logs);
}

struct ParseResult {
    file_path: String,
    test_descriptions: Vec<TestDescription>,
}

fn run_directory_of_tests(dir_path: &str, individual_logs: bool) {
    let mut receivers = vec![];
    let read_dir = Path::new(dir_path).read_dir().expect("Unable to read directory");
    for dir_entry in read_dir {
        let dir_entry = dir_entry.expect("Unable to read directory entry");
        let metadata = dir_entry.metadata().expect("Unable to read file metadata");

        if metadata.is_file() && dir_entry.file_name().to_string_lossy().ends_with(".json.gz") {
            let (sender, receiver) = mpsc::channel();
            receivers.push(receiver);

            let file_path = dir_entry.path().to_string_lossy().to_string();
            thread::spawn(move || {
                let file = GzDecoder::new(BufReader::new(
                    File::open(Path::new(&file_path)).expect("Unable to open file"),
                ));

                let test_descriptions: Vec<TestDescription> = match serde_json::from_reader(file) {
                    Ok(descriptions) => descriptions,
                    Err(err) => {
                        log::error!("Unable to parse JSON at '{file_path}': {err}");
                        panic!("JSON parse error");
                    }
                };

                sender.send(ParseResult { file_path, test_descriptions }).unwrap();
            });
        }
    }

    let mut parse_results = vec![];
    for receiver in receivers {
        let parse_result = receiver.recv().unwrap();
        parse_results.push(parse_result);
    }

    parse_results.sort_by(|a, b| a.file_path.cmp(&b.file_path));

    let mut bus = InMemoryBus::new();
    for ParseResult { file_path, test_descriptions } in parse_results {
        run_single_test(&test_descriptions, &mut bus, Path::new(&file_path), individual_logs);
    }
}

fn run_single_test<P: AsRef<Path>>(
    test_descriptions: &[TestDescription],
    bus: &mut InMemoryBus,
    file_path: P,
    individual_logs: bool,
) {
    let mut failure_count = 0_u32;
    let mut timing_failure_count = 0_u32;
    let mut address_error_count = 0_u32;
    for test_description in test_descriptions {
        let mut m68000 = init_test_state(&test_description.initial, bus);
        let cycles = m68000.execute_instruction(bus);

        let state = State::from(&m68000, bus, &test_description.final_state);
        if state != test_description.final_state {
            if individual_logs {
                log::info!("Failed test '{}'", test_description.name);
                state.diff(&test_description.final_state);
            }

            failure_count += 1;
        }

        if cycles != test_description.length && !m68000.address_error() {
            if individual_logs {
                log::info!(
                    "Timing mismatch for test '{}'; actual={cycles}, expected={}",
                    test_description.name,
                    test_description.length
                );
            }

            timing_failure_count += 1;
        }

        if m68000.address_error() {
            address_error_count += 1;
        }
    }

    let num_tests = test_descriptions.len();
    let display_path = file_path.as_ref().display();
    log::info!("{failure_count} failed out of {num_tests} tests in {display_path}");

    let num_tests_without_address_errors = num_tests as u32 - address_error_count;
    log::info!(
        "{timing_failure_count} timing mismatches out of {num_tests_without_address_errors} tests in {display_path}"
    );
}

fn init_test_state(state: &State, bus: &mut InMemoryBus) -> M68000 {
    let mut m68000 = M68000::default();

    m68000.set_data_registers([
        state.d0, state.d1, state.d2, state.d3, state.d4, state.d5, state.d6, state.d7,
    ]);
    m68000.set_address_registers(
        [state.a0, state.a1, state.a2, state.a3, state.a4, state.a5, state.a6],
        state.usp,
        state.ssp,
    );
    m68000.set_status_register(state.sr);

    bus.write_word(state.pc, state.prefetch[0]);
    bus.write_word(state.pc.wrapping_add(2), state.prefetch[1]);

    for &(address, value) in &state.ram {
        bus.write_byte(address, value);
    }

    m68000.set_pc(state.pc, bus);

    m68000
}
