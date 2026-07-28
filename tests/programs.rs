//! End-to-end tests: compile the bare-metal C demos with clang (via
//! `make -C programs`), run them, and compare UART output with golden files.
//! Also checks determinism and that emulated time follows the cycle model.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;

use hartwood::{Config, Exit, Machine};

static BUILD: Once = Once::new();

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn elf(name: &str) -> Vec<u8> {
    BUILD.call_once(|| {
        let st = Command::new("make")
            .arg("-s")
            .arg("-C")
            .arg(root().join("programs"))
            .status()
            .expect("failed to run make (clang + ld.lld are required)");
        assert!(st.success(), "building demo programs failed");
    });
    std::fs::read(root().join("programs/build").join(format!("{name}.elf"))).unwrap()
}

fn run_with(name: &str, cfg: Config) -> (Machine, Exit) {
    let mut m = Machine::new_captured(cfg);
    m.load_elf(&elf(name)).unwrap();
    let e = m.run();
    (m, e)
}

fn run(name: &str) -> (Machine, Exit) {
    run_with(
        name,
        Config {
            max_cycles: Some(2_000_000_000),
            ..Config::default()
        },
    )
}

fn golden(name: &str) {
    let (m, e) = run(name);
    assert_eq!(e, Exit::Finisher(0x5555), "{name} exit");
    let expect = std::fs::read_to_string(root().join("tests/golden").join(format!("{name}.txt"))).unwrap();
    assert_eq!(m.uart_output(), expect, "{name} output differs from golden");
}

#[test]
fn golden_hello() {
    golden("hello");
}
#[test]
fn golden_primes() {
    golden("primes");
}
#[test]
fn golden_mandelbrot() {
    golden("mandelbrot");
}
#[test]
fn golden_timer_interrupts() {
    golden("timer");
}
#[test]
fn golden_paging_user_mode() {
    golden("paging");
}
#[test]
fn golden_atomics() {
    golden("atomics");
}

#[test]
fn runs_are_bit_for_bit_deterministic() {
    for name in ["primes", "timer", "paging"] {
        let (a, ea) = run(name);
        let (b, eb) = run(name);
        assert_eq!(ea, eb);
        assert_eq!(a.uart_output(), b.uart_output());
        assert_eq!(
            (a.cpu.cycles, a.cpu.instret, a.cpu.pc),
            (b.cpu.cycles, b.cpu.instret, b.cpu.pc),
            "{name}"
        );
        assert_eq!(a.cpu.x, b.cpu.x);
        assert!(a.bus.ram.data == b.bus.ram.data, "{name}: RAM differs");
    }
}

#[test]
fn timer_ticks_scale_with_cycles_per_tick() {
    // With a faster timebase (fewer cycles per tick) the same number of
    // timer ticks costs proportionally fewer CPU cycles.
    let (slow, e1) = run_with(
        "timer",
        Config {
            cycles_per_tick: 20,
            ..Config::default()
        },
    );
    let (fast, e2) = run_with(
        "timer",
        Config {
            cycles_per_tick: 5,
            ..Config::default()
        },
    );
    assert_eq!(e1, Exit::Finisher(0x5555));
    assert_eq!(e2, Exit::Finisher(0x5555));
    let ratio = slow.cpu.cycles as f64 / fast.cpu.cycles as f64;
    assert!((3.8..4.2).contains(&ratio), "cycle ratio {ratio}");
    // But the program does (almost) the same amount of work.
    assert!(slow.cpu.instret.abs_diff(fast.cpu.instret) < 50);
}

#[test]
fn max_cycles_stops_execution() {
    let (m, e) = run_with(
        "primes",
        Config {
            max_cycles: Some(10_000),
            ..Config::default()
        },
    );
    assert_eq!(e, Exit::MaxCycles);
    assert!(m.cpu.cycles >= 10_000 && m.cpu.cycles < 10_100);
}

#[test]
fn cli_runs_hello_and_reports_exit_code() {
    elf("hello");
    let out = Command::new(env!("CARGO_BIN_EXE_hartwood"))
        .args(["run", root().join("programs/build/hello.elf").to_str().unwrap(), "--no-stdin"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "Hello from hartwood! (RV64IMAC, hart 0)\n");

    let out = Command::new(env!("CARGO_BIN_EXE_hartwood"))
        .args([
            "run",
            root().join("programs/build/hello.elf").to_str().unwrap(),
            "--no-stdin",
            "--trace",
            "--max-cycles",
            "100",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(124), "max-cycles exit code");
    let trace = String::from_utf8_lossy(&out.stderr);
    assert!(trace.contains("80000000"), "trace shows the entry point");
    assert!(trace.lines().count() > 10);

    let out = Command::new(env!("CARGO_BIN_EXE_hartwood"))
        .args(["disasm", root().join("programs/build/hello.elf").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("c.jal"));
}

#[test]
fn elf_loader_rejects_garbage() {
    let mut m = Machine::new_captured(Config::default());
    assert!(m
        .load_elf(b"not an elf at all, definitely not, no no no no no no no no no no no no no")
        .is_err());
    assert!(m.load_elf(&[0x7f, b'E', b'L', b'F']).is_err());
}
