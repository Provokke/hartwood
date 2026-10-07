//! Throughput benchmark: emulated MIPS (millions of retired guest
//! instructions per host second) on the demo workloads.
//! Run with `cargo bench` (requires clang to build the demo programs).

use std::process::Command;
use std::time::Instant;

use hartwood::{Config, Exit, Machine};

fn main() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let st = Command::new("make")
        .arg("-s")
        .arg("-C")
        .arg(root.join("programs"))
        .status()
        .expect("make");
    assert!(st.success());
    println!(
        "{:<12} {:>12} {:>14} {:>10} {:>9}",
        "workload", "instret", "cycles", "best ms", "MIPS"
    );
    for name in ["primes", "mandelbrot", "paging", "timer"] {
        let bytes = std::fs::read(root.join("programs/build").join(format!("{name}.elf"))).unwrap();
        let mut best = f64::MAX;
        let mut instret = 0;
        let mut cycles = 0;
        for _ in 0..7 {
            let mut m = Machine::new_captured(Config::default());
            m.load_elf(&bytes).unwrap();
            let t = Instant::now();
            let e = m.run();
            let dt = t.elapsed().as_secs_f64();
            assert_eq!(e, Exit::Finisher(0x5555));
            best = best.min(dt);
            instret = m.cpu.instret;
            cycles = m.cpu.cycles;
        }
        println!(
            "{:<12} {:>12} {:>14} {:>10.2} {:>9.1}",
            name,
            instret,
            cycles,
            best * 1e3,
            instret as f64 / best / 1e6
        );
    }
}
