//! Shared helpers for integration tests: a tiny assembler built on the
//! crate's own encoders, and machine construction utilities.
#![allow(dead_code)]

use hartwood::decode::{enc_b, enc_i, enc_j, enc_r, enc_s, enc_u};
use hartwood::{Config, Exit, Machine};

pub const BASE: u64 = 0x8000_0000;

/// Register numbers by ABI name.
pub mod r {
    pub const ZERO: u32 = 0;
    pub const RA: u32 = 1;
    pub const SP: u32 = 2;
    pub const T0: u32 = 5;
    pub const T1: u32 = 6;
    pub const T2: u32 = 7;
    pub const A0: u32 = 10;
    pub const A1: u32 = 11;
    pub const A2: u32 = 12;
    pub const A3: u32 = 13;
    pub const A7: u32 = 17;
}

/// A minimal program builder.
#[derive(Default)]
pub struct Asm {
    pub words: Vec<u32>,
}

impl Asm {
    pub fn new() -> Asm {
        Asm::default()
    }
    pub fn pc(&self) -> u64 {
        BASE + 4 * self.words.len() as u64
    }
    pub fn w(&mut self, w: u32) -> &mut Self {
        self.words.push(w);
        self
    }
    /// Load an arbitrary 64-bit constant (lui/addi/slli sequence).
    pub fn li(&mut self, rd: u32, v: u64) -> &mut Self {
        let v = v as i64;
        if (-2048..2048).contains(&v) {
            return self.w(enc_i(v, 0, 0, rd, 0x13));
        }
        if v == (v as i32) as i64 {
            let lo = (v << 52) >> 52;
            let hi = (v - lo) as u32 as i64;
            self.w(enc_u(hi, rd, 0x37));
            // addiw keeps the 32-bit sign-extension semantics.
            return self.w(enc_i(lo, rd, 0, rd, 0x1b));
        }
        // Build from the top: li(hi part) then shift in 12 bits at a time.
        let lo = (v << 52) >> 52;
        let hi = (v - lo) >> 12;
        self.li(rd, hi as u64);
        self.w(enc_i(12, rd, 1, rd, 0x13)); // slli rd, rd, 12
        if lo != 0 {
            self.w(enc_i(lo, rd, 0, rd, 0x13));
        }
        self
    }
    pub fn addi(&mut self, rd: u32, rs1: u32, imm: i64) -> &mut Self {
        self.w(enc_i(imm, rs1, 0, rd, 0x13))
    }
    pub fn r(&mut self, f7: u32, f3: u32, opc: u32, rd: u32, rs1: u32, rs2: u32) -> &mut Self {
        self.w(enc_r(f7, rs2, rs1, f3, rd, opc))
    }
    pub fn ld(&mut self, rd: u32, rs1: u32, imm: i64) -> &mut Self {
        self.w(enc_i(imm, rs1, 3, rd, 0x03))
    }
    pub fn sd(&mut self, rs2: u32, rs1: u32, imm: i64) -> &mut Self {
        self.w(enc_s(imm, rs2, rs1, 3, 0x23))
    }
    pub fn csrrw(&mut self, rd: u32, csr: u16, rs1: u32) -> &mut Self {
        self.w(enc_i(csr as i64, rs1, 1, rd, 0x73))
    }
    pub fn csrrs(&mut self, rd: u32, csr: u16, rs1: u32) -> &mut Self {
        self.w(enc_i(csr as i64, rs1, 2, rd, 0x73))
    }
    pub fn ecall(&mut self) -> &mut Self {
        self.w(0x0000_0073)
    }
    pub fn mret(&mut self) -> &mut Self {
        self.w(0x3020_0073)
    }
    pub fn sret(&mut self) -> &mut Self {
        self.w(0x1020_0073)
    }
    pub fn wfi(&mut self) -> &mut Self {
        self.w(0x1050_0073)
    }
    pub fn jal(&mut self, rd: u32, off: i64) -> &mut Self {
        self.w(enc_j(off, rd))
    }
    pub fn beq(&mut self, rs1: u32, rs2: u32, off: i64) -> &mut Self {
        self.w(enc_b(off, rs2, rs1, 0))
    }
    /// Signal pass/fail through the test finisher: writes a0 there
    /// (expected to contain 0x5555 or (code<<16)|0x3333).
    pub fn finish(&mut self) -> &mut Self {
        self.li(r::T2, 0x0010_0000);
        self.w(enc_s(0, r::A0, r::T2, 2, 0x23)); // sw a0, 0(t2)
        self.jal(0, 0) // spin
    }
    /// Write the finisher "pass" code.
    pub fn pass(&mut self) -> &mut Self {
        self.li(r::A0, 0x5555);
        self.finish()
    }
    pub fn bytes(&self) -> Vec<u8> {
        self.words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }
}

pub fn machine(asm: &Asm) -> Machine {
    let mut m = Machine::new_captured(Config {
        ram_size: 4 << 20,
        cycles_per_tick: 10,
        max_cycles: Some(10_000_000),
    });
    assert!(m.load_raw(BASE, &asm.bytes()));
    m
}

pub fn run(asm: &Asm) -> (Machine, Exit) {
    let mut m = machine(asm);
    let e = m.run();
    (m, e)
}

pub const PASS: Exit = Exit::Finisher(0x5555);
