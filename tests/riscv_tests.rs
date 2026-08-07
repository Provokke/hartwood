//! Runs the official riscv-tests ISA suites (rv64ui/um/ua/uc/mi/si, "-p"
//! environment) built by `scripts/build-riscv-tests.sh`. Each test must
//! write 1 to `tohost` (pass).
//!
//! If the suite has not been built (it is fetched from GitHub), this test
//! prints a notice and checks nothing; `make test` always builds it first,
//! and setting HARTWOOD_REQUIRE_RISCV_TESTS=1 turns the notice into a failure.

use hartwood::{Config, Exit, Machine};

#[test]
fn official_riscv_tests() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("third_party/riscv-tests-build");
    let mut entries: Vec<_> = match std::fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_none())
            .collect(),
        Err(_) => Vec::new(),
    };
    if entries.is_empty() {
        let msg = format!("riscv-tests not built in {}; run scripts/build-riscv-tests.sh", dir.display());
        if std::env::var("HARTWOOD_REQUIRE_RISCV_TESTS").is_ok() {
            panic!("{msg}");
        }
        eprintln!("NOTICE: {msg} (official suite NOT run)");
        return;
    }
    entries.sort();
    let mut failures = Vec::new();
    let mut per_suite = std::collections::BTreeMap::<String, (u32, u32)>::new();
    for path in &entries {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let suite = name.split('-').next().unwrap().to_string();
        let mut m = Machine::new_captured(Config {
            ram_size: 16 << 20,
            max_cycles: Some(50_000_000),
            ..Config::default()
        });
        m.load_elf(&std::fs::read(path).unwrap()).unwrap();
        let exit = m.run();
        let e = per_suite.entry(suite).or_default();
        e.1 += 1;
        if exit == Exit::ToHost(1) {
            e.0 += 1;
        } else {
            failures.push(format!("{name}: {exit:?}"));
        }
    }
    for (suite, (pass, total)) in &per_suite {
        eprintln!("{suite:8} {pass}/{total} passed");
    }
    assert!(failures.is_empty(), "failing riscv-tests:\n{}", failures.join("\n"));
    assert!(entries.len() >= 100, "expected the full suite, found {}", entries.len());
}
