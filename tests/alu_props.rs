//! Property tests of integer + M-extension semantics: the real decoder and
//! executor are checked against an independent i128-based reference model.

use hartwood::bus::Bus;
use hartwood::cpu::Cpu;
use hartwood::decode::{decode32, enc_i, enc_r};
use hartwood::devices::uart::Uart;
use proptest::prelude::*;

fn exec_r(f7: u32, f3: u32, opc: u32, a: u64, b: u64) -> u64 {
    let mut bus = Bus::new(4096, Uart::new(Box::new(std::io::sink())));
    let mut cpu = Cpu::new();
    cpu.x[1] = a;
    cpu.x[2] = b;
    let inst = decode32(enc_r(f7, 2, 1, f3, 3, opc));
    cpu.execute(&mut bus, &inst).expect("no trap");
    cpu.x[3]
}

fn exec_i(f3: u32, opc: u32, a: u64, imm: i64) -> u64 {
    let mut bus = Bus::new(4096, Uart::new(Box::new(std::io::sink())));
    let mut cpu = Cpu::new();
    cpu.x[1] = a;
    let inst = decode32(enc_i(imm, 1, f3, 3, opc));
    cpu.execute(&mut bus, &inst).expect("no trap");
    cpu.x[3]
}

// ----------------------------- reference model -----------------------------
fn s(v: u64) -> i128 {
    v as i64 as i128
}
fn u(v: u64) -> i128 {
    v as i128
}
fn wrap(v: i128) -> u64 {
    v as u64
}
fn w32(v: i128) -> u64 {
    (v as i32) as i64 as u64
}
fn ref_div(a: i128, b: i128, min: i128) -> i128 {
    if b == 0 {
        -1
    } else if a == min && b == -1 {
        min
    } else {
        a / b // Rust i128 division truncates toward zero, as RISC-V requires
    }
}
fn ref_rem(a: i128, b: i128, min: i128) -> i128 {
    if b == 0 {
        a
    } else if a == min && b == -1 {
        0
    } else {
        a % b
    }
}

fn edge() -> impl Strategy<Value = u64> {
    prop_oneof![
        Just(0u64),
        Just(1),
        Just(u64::MAX),
        Just(i64::MIN as u64),
        Just(i64::MAX as u64),
        Just(0x8000_0000),
        Just(0x7fff_ffff),
        Just(0xffff_ffff),
        Just(0xffff_ffff_8000_0000),
        any::<u64>(),
        any::<u32>().prop_map(|x| x as u64),
        any::<i32>().prop_map(|x| x as i64 as u64),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn rv64i_register_ops(a in edge(), b in edge()) {
        prop_assert_eq!(exec_r(0, 0, 0x33, a, b), wrap(s(a) + s(b)));
        prop_assert_eq!(exec_r(0x20, 0, 0x33, a, b), wrap(s(a) - s(b)));
        prop_assert_eq!(exec_r(0, 1, 0x33, a, b), wrap(u(a) << (b & 63)));
        prop_assert_eq!(exec_r(0, 2, 0x33, a, b), (s(a) < s(b)) as u64);
        prop_assert_eq!(exec_r(0, 3, 0x33, a, b), (u(a) < u(b)) as u64);
        prop_assert_eq!(exec_r(0, 4, 0x33, a, b), a ^ b);
        prop_assert_eq!(exec_r(0, 5, 0x33, a, b), wrap(u(a) >> (b & 63)));
        prop_assert_eq!(exec_r(0x20, 5, 0x33, a, b), wrap(s(a) >> (b & 63)));
        prop_assert_eq!(exec_r(0, 6, 0x33, a, b), a | b);
        prop_assert_eq!(exec_r(0, 7, 0x33, a, b), a & b);
        // 32-bit forms
        let (a32, b32) = (a as u32 as i128, b as u32 as i128);
        let sa32 = a as i32 as i128;
        prop_assert_eq!(exec_r(0, 0, 0x3b, a, b), w32(a32 + b32));
        prop_assert_eq!(exec_r(0x20, 0, 0x3b, a, b), w32(a32 - b32));
        prop_assert_eq!(exec_r(0, 1, 0x3b, a, b), w32(a32 << (b & 31)));
        prop_assert_eq!(exec_r(0, 5, 0x3b, a, b), w32(a32 >> (b & 31)));
        prop_assert_eq!(exec_r(0x20, 5, 0x3b, a, b), w32(sa32 >> (b & 31)));
    }

    #[test]
    fn m_extension(a in edge(), b in edge()) {
        let min64 = i64::MIN as i128;
        prop_assert_eq!(exec_r(1, 0, 0x33, a, b), wrap(s(a) * s(b)));
        prop_assert_eq!(exec_r(1, 1, 0x33, a, b), wrap((s(a) * s(b)) >> 64));
        prop_assert_eq!(exec_r(1, 2, 0x33, a, b), wrap((s(a) * u(b)) >> 64));
        prop_assert_eq!(exec_r(1, 3, 0x33, a, b), ((a as u128 * b as u128) >> 64) as u64);
        prop_assert_eq!(exec_r(1, 4, 0x33, a, b), wrap(ref_div(s(a), s(b), min64)));
        prop_assert_eq!(exec_r(1, 5, 0x33, a, b), a.checked_div(b).unwrap_or(u64::MAX));
        prop_assert_eq!(exec_r(1, 6, 0x33, a, b), wrap(ref_rem(s(a), s(b), min64)));
        prop_assert_eq!(exec_r(1, 7, 0x33, a, b), if b == 0 { a } else { a % b });
        // W forms
        let min32 = i32::MIN as i128;
        let (sa, sb) = (a as i32 as i128, b as i32 as i128);
        let (ua, ub) = (a as u32 as i128, b as u32 as i128);
        prop_assert_eq!(exec_r(1, 0, 0x3b, a, b), w32(sa * sb));
        prop_assert_eq!(exec_r(1, 4, 0x3b, a, b), w32(ref_div(sa, sb, min32)));
        prop_assert_eq!(exec_r(1, 5, 0x3b, a, b), w32(if ub == 0 { -1 } else { ua / ub }));
        prop_assert_eq!(exec_r(1, 6, 0x3b, a, b), w32(ref_rem(sa, sb, min32)));
        prop_assert_eq!(exec_r(1, 7, 0x3b, a, b), w32(if ub == 0 { ua } else { ua % ub }));
    }

    #[test]
    fn immediate_ops(a in edge(), imm in -2048i64..2048, sh in 0i64..64) {
        prop_assert_eq!(exec_i(0, 0x13, a, imm), wrap(s(a) + imm as i128));
        prop_assert_eq!(exec_i(2, 0x13, a, imm), (s(a) < imm as i128) as u64);
        prop_assert_eq!(exec_i(3, 0x13, a, imm), (a < imm as u64) as u64);
        prop_assert_eq!(exec_i(4, 0x13, a, imm), a ^ imm as u64);
        prop_assert_eq!(exec_i(6, 0x13, a, imm), a | imm as u64);
        prop_assert_eq!(exec_i(7, 0x13, a, imm), a & imm as u64);
        prop_assert_eq!(exec_i(1, 0x13, a, sh), a << sh);
        prop_assert_eq!(exec_i(5, 0x13, a, sh), a >> sh);
        prop_assert_eq!(exec_i(5, 0x13, a, sh | 0x400), ((a as i64) >> sh) as u64);
        prop_assert_eq!(exec_i(0, 0x1b, a, imm), w32(a as i32 as i128 + imm as i128));
        let sh = sh & 31;
        prop_assert_eq!(exec_i(1, 0x1b, a, sh), w32((a as u32 as i128) << sh));
        prop_assert_eq!(exec_i(5, 0x1b, a, sh), w32((a as u32 as i128) >> sh));
        prop_assert_eq!(exec_i(5, 0x1b, a, sh | 0x400), w32((a as i32 as i128) >> sh));
    }
}

#[test]
fn division_edge_cases_explicit() {
    let min = i64::MIN as u64;
    assert_eq!(exec_r(1, 4, 0x33, min, u64::MAX), min, "div overflow");
    assert_eq!(exec_r(1, 6, 0x33, min, u64::MAX), 0, "rem overflow");
    assert_eq!(exec_r(1, 4, 0x33, 42, 0), u64::MAX, "div by zero");
    assert_eq!(exec_r(1, 5, 0x33, 42, 0), u64::MAX, "divu by zero");
    assert_eq!(exec_r(1, 6, 0x33, 42, 0), 42, "rem by zero");
    assert_eq!(exec_r(1, 7, 0x33, 42, 0), 42, "remu by zero");
    assert_eq!(exec_r(1, 4, 0x3b, 0x8000_0000, u64::MAX), 0xffff_ffff_8000_0000, "divw overflow");
    assert_eq!(exec_r(1, 6, 0x3b, 0x8000_0000, u64::MAX), 0, "remw overflow");
    assert_eq!(exec_r(1, 4, 0x33, (-7i64) as u64, 2), (-3i64) as u64, "truncates toward zero");
    assert_eq!(
        exec_r(1, 6, 0x33, (-7i64) as u64, 2),
        (-1i64) as u64,
        "remainder sign follows dividend"
    );
    // x0 is hard-wired to zero
    let mut bus = Bus::new(4096, Uart::new(Box::new(std::io::sink())));
    let mut cpu = Cpu::new();
    cpu.x[1] = 5;
    cpu.execute(&mut bus, &decode32(enc_i(7, 1, 0, 0, 0x13))).unwrap();
    assert_eq!(cpu.x[0], 0);
}
