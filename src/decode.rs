//! Instruction decoding for RV64IMAC + Zicsr + Zifencei.
//!
//! 32-bit encodings are decoded directly into an [`Inst`]. 16-bit compressed
//! encodings are first expanded into their canonical 32-bit equivalent by
//! [`expand_compressed`] and then decoded through the same path, so the
//! executor only ever sees one representation.

/// Operation kind of a decoded instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Op {
    Lui,
    Auipc,
    Jal,
    Jalr,
    Beq,
    Bne,
    Blt,
    Bge,
    Bltu,
    Bgeu,
    Lb,
    Lh,
    Lw,
    Ld,
    Lbu,
    Lhu,
    Lwu,
    Sb,
    Sh,
    Sw,
    Sd,
    Addi,
    Slti,
    Sltiu,
    Xori,
    Ori,
    Andi,
    Slli,
    Srli,
    Srai,
    Add,
    Sub,
    Sll,
    Slt,
    Sltu,
    Xor,
    Srl,
    Sra,
    Or,
    And,
    Addiw,
    Slliw,
    Srliw,
    Sraiw,
    Addw,
    Subw,
    Sllw,
    Srlw,
    Sraw,
    Fence,
    FenceI,
    Ecall,
    Ebreak,
    Sret,
    Mret,
    Wfi,
    SfenceVma,
    Csrrw,
    Csrrs,
    Csrrc,
    Csrrwi,
    Csrrsi,
    Csrrci,
    Mul,
    Mulh,
    Mulhsu,
    Mulhu,
    Div,
    Divu,
    Rem,
    Remu,
    Mulw,
    Divw,
    Divuw,
    Remw,
    Remuw,
    LrW,
    ScW,
    AmoswapW,
    AmoaddW,
    AmoxorW,
    AmoandW,
    AmoorW,
    AmominW,
    AmomaxW,
    AmominuW,
    AmomaxuW,
    LrD,
    ScD,
    AmoswapD,
    AmoaddD,
    AmoxorD,
    AmoandD,
    AmoorD,
    AmominD,
    AmomaxD,
    AmominuD,
    AmomaxuD,
    Illegal,
}

/// A fully decoded instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inst {
    pub op: Op,
    pub rd: u8,
    pub rs1: u8,
    pub rs2: u8,
    /// Sign-extended immediate (or shift amount / CSR zimm where relevant).
    pub imm: i64,
    /// CSR number for Zicsr instructions; aq/rl bits (aq<<1|rl) for AMOs.
    pub csr: u16,
    /// Length in bytes of the original encoding (2 or 4).
    pub len: u8,
    /// The raw (unexpanded) encoding.
    pub raw: u32,
}

impl Inst {
    pub const fn illegal(raw: u32, len: u8) -> Inst {
        Inst {
            op: Op::Illegal,
            rd: 0,
            rs1: 0,
            rs2: 0,
            imm: 0,
            csr: 0,
            len,
            raw,
        }
    }
}

#[inline]
fn bits(x: u32, hi: u32, lo: u32) -> u32 {
    (x >> lo) & ((1u32 << (hi - lo + 1)) - 1)
}

#[inline]
fn sext(value: u64, width: u32) -> i64 {
    let shift = 64 - width;
    ((value << shift) as i64) >> shift
}

fn imm_i(w: u32) -> i64 {
    (w as i32 >> 20) as i64
}
fn imm_s(w: u32) -> i64 {
    sext(((bits(w, 31, 25) << 5) | bits(w, 11, 7)) as u64, 12)
}
fn imm_b(w: u32) -> i64 {
    let v = (bits(w, 31, 31) << 12) | (bits(w, 7, 7) << 11) | (bits(w, 30, 25) << 5) | (bits(w, 11, 8) << 1);
    sext(v as u64, 13)
}
fn imm_u(w: u32) -> i64 {
    (w & 0xffff_f000) as i32 as i64
}
fn imm_j(w: u32) -> i64 {
    let v = (bits(w, 31, 31) << 20) | (bits(w, 19, 12) << 12) | (bits(w, 20, 20) << 11) | (bits(w, 30, 21) << 1);
    sext(v as u64, 21)
}

/// Decode any instruction word. If the low two bits are not `0b11` the low
/// half-word is treated as a compressed instruction.
pub fn decode(raw: u32) -> Inst {
    if raw & 3 != 3 {
        let half = raw as u16;
        match expand_compressed(half) {
            Some(w) => {
                let mut i = decode32(w);
                i.len = 2;
                i.raw = half as u32;
                if i.op == Op::Illegal {
                    return Inst::illegal(half as u32, 2);
                }
                i
            }
            None => Inst::illegal(half as u32, 2),
        }
    } else {
        decode32(raw)
    }
}

/// Decode a 32-bit (non-compressed) instruction word.
pub fn decode32(w: u32) -> Inst {
    let rd = bits(w, 11, 7) as u8;
    let rs1 = bits(w, 19, 15) as u8;
    let rs2 = bits(w, 24, 20) as u8;
    let f3 = bits(w, 14, 12);
    let f7 = bits(w, 31, 25);
    let mk = |op: Op, imm: i64| Inst {
        op,
        rd,
        rs1,
        rs2,
        imm,
        csr: 0,
        len: 4,
        raw: w,
    };
    let ill = Inst::illegal(w, 4);
    if w & 3 != 3 {
        return ill;
    }
    match bits(w, 6, 0) {
        0x37 => mk(Op::Lui, imm_u(w)),
        0x17 => mk(Op::Auipc, imm_u(w)),
        0x6f => mk(Op::Jal, imm_j(w)),
        0x67 if f3 == 0 => mk(Op::Jalr, imm_i(w)),
        0x63 => {
            let op = match f3 {
                0 => Op::Beq,
                1 => Op::Bne,
                4 => Op::Blt,
                5 => Op::Bge,
                6 => Op::Bltu,
                7 => Op::Bgeu,
                _ => return ill,
            };
            mk(op, imm_b(w))
        }
        0x03 => {
            let op = match f3 {
                0 => Op::Lb,
                1 => Op::Lh,
                2 => Op::Lw,
                3 => Op::Ld,
                4 => Op::Lbu,
                5 => Op::Lhu,
                6 => Op::Lwu,
                _ => return ill,
            };
            mk(op, imm_i(w))
        }
        0x23 => {
            let op = match f3 {
                0 => Op::Sb,
                1 => Op::Sh,
                2 => Op::Sw,
                3 => Op::Sd,
                _ => return ill,
            };
            mk(op, imm_s(w))
        }
        0x13 => {
            let shamt = bits(w, 25, 20) as i64;
            match f3 {
                0 => mk(Op::Addi, imm_i(w)),
                2 => mk(Op::Slti, imm_i(w)),
                3 => mk(Op::Sltiu, imm_i(w)),
                4 => mk(Op::Xori, imm_i(w)),
                6 => mk(Op::Ori, imm_i(w)),
                7 => mk(Op::Andi, imm_i(w)),
                1 if bits(w, 31, 26) == 0 => mk(Op::Slli, shamt),
                5 if bits(w, 31, 26) == 0 => mk(Op::Srli, shamt),
                5 if bits(w, 31, 26) == 0x10 => mk(Op::Srai, shamt),
                _ => ill,
            }
        }
        0x1b => {
            let shamt = bits(w, 24, 20) as i64;
            match (f3, f7) {
                (0, _) => mk(Op::Addiw, imm_i(w)),
                (1, 0) => mk(Op::Slliw, shamt),
                (5, 0) => mk(Op::Srliw, shamt),
                (5, 0x20) => mk(Op::Sraiw, shamt),
                _ => ill,
            }
        }
        0x33 => {
            let op = match (f7, f3) {
                (0, 0) => Op::Add,
                (0x20, 0) => Op::Sub,
                (0, 1) => Op::Sll,
                (0, 2) => Op::Slt,
                (0, 3) => Op::Sltu,
                (0, 4) => Op::Xor,
                (0, 5) => Op::Srl,
                (0x20, 5) => Op::Sra,
                (0, 6) => Op::Or,
                (0, 7) => Op::And,
                (1, 0) => Op::Mul,
                (1, 1) => Op::Mulh,
                (1, 2) => Op::Mulhsu,
                (1, 3) => Op::Mulhu,
                (1, 4) => Op::Div,
                (1, 5) => Op::Divu,
                (1, 6) => Op::Rem,
                (1, 7) => Op::Remu,
                _ => return ill,
            };
            mk(op, 0)
        }
        0x3b => {
            let op = match (f7, f3) {
                (0, 0) => Op::Addw,
                (0x20, 0) => Op::Subw,
                (0, 1) => Op::Sllw,
                (0, 5) => Op::Srlw,
                (0x20, 5) => Op::Sraw,
                (1, 0) => Op::Mulw,
                (1, 4) => Op::Divw,
                (1, 5) => Op::Divuw,
                (1, 6) => Op::Remw,
                (1, 7) => Op::Remuw,
                _ => return ill,
            };
            mk(op, 0)
        }
        0x0f => match f3 {
            0 => mk(Op::Fence, imm_i(w)),
            1 => mk(Op::FenceI, 0),
            _ => ill,
        },
        0x73 => {
            let csr = bits(w, 31, 20) as u16;
            let mut i = match f3 {
                0 => {
                    if rd != 0 {
                        return ill;
                    }
                    return match (f7, rs2, rs1) {
                        (0, 0, 0) => mk(Op::Ecall, 0),
                        (0, 1, 0) => mk(Op::Ebreak, 0),
                        (0x08, 2, 0) => mk(Op::Sret, 0),
                        (0x18, 2, 0) => mk(Op::Mret, 0),
                        (0x08, 5, 0) => mk(Op::Wfi, 0),
                        (0x09, _, _) => mk(Op::SfenceVma, 0),
                        _ => ill,
                    };
                }
                1 => mk(Op::Csrrw, 0),
                2 => mk(Op::Csrrs, 0),
                3 => mk(Op::Csrrc, 0),
                5 => mk(Op::Csrrwi, rs1 as i64),
                6 => mk(Op::Csrrsi, rs1 as i64),
                7 => mk(Op::Csrrci, rs1 as i64),
                _ => return ill,
            };
            i.csr = csr;
            i
        }
        0x2f => {
            let f5 = bits(w, 31, 27);
            let aqrl = bits(w, 26, 25) as u16;
            let op = match (f3, f5) {
                (2, 0x02) if rs2 == 0 => Op::LrW,
                (2, 0x03) => Op::ScW,
                (2, 0x01) => Op::AmoswapW,
                (2, 0x00) => Op::AmoaddW,
                (2, 0x04) => Op::AmoxorW,
                (2, 0x0c) => Op::AmoandW,
                (2, 0x08) => Op::AmoorW,
                (2, 0x10) => Op::AmominW,
                (2, 0x14) => Op::AmomaxW,
                (2, 0x18) => Op::AmominuW,
                (2, 0x1c) => Op::AmomaxuW,
                (3, 0x02) if rs2 == 0 => Op::LrD,
                (3, 0x03) => Op::ScD,
                (3, 0x01) => Op::AmoswapD,
                (3, 0x00) => Op::AmoaddD,
                (3, 0x04) => Op::AmoxorD,
                (3, 0x0c) => Op::AmoandD,
                (3, 0x08) => Op::AmoorD,
                (3, 0x10) => Op::AmominD,
                (3, 0x14) => Op::AmomaxD,
                (3, 0x18) => Op::AmominuD,
                (3, 0x1c) => Op::AmomaxuD,
                _ => return ill,
            };
            let mut i = mk(op, 0);
            i.csr = aqrl;
            i
        }
        _ => ill,
    }
}

// ---------------------------------------------------------------------------
// 32-bit encoders used by the compressed expander (and by tests).
// ---------------------------------------------------------------------------

pub fn enc_r(f7: u32, rs2: u32, rs1: u32, f3: u32, rd: u32, opc: u32) -> u32 {
    (f7 << 25) | (rs2 << 20) | (rs1 << 15) | (f3 << 12) | (rd << 7) | opc
}
pub fn enc_i(imm: i64, rs1: u32, f3: u32, rd: u32, opc: u32) -> u32 {
    (((imm as u32) & 0xfff) << 20) | (rs1 << 15) | (f3 << 12) | (rd << 7) | opc
}
pub fn enc_s(imm: i64, rs2: u32, rs1: u32, f3: u32, opc: u32) -> u32 {
    let imm = imm as u32;
    (bits(imm, 11, 5) << 25) | (rs2 << 20) | (rs1 << 15) | (f3 << 12) | (bits(imm, 4, 0) << 7) | opc
}
pub fn enc_b(imm: i64, rs2: u32, rs1: u32, f3: u32) -> u32 {
    let imm = imm as u32;
    (bits(imm, 12, 12) << 31)
        | (bits(imm, 10, 5) << 25)
        | (rs2 << 20)
        | (rs1 << 15)
        | (f3 << 12)
        | (bits(imm, 4, 1) << 8)
        | (bits(imm, 11, 11) << 7)
        | 0x63
}
pub fn enc_u(imm: i64, rd: u32, opc: u32) -> u32 {
    ((imm as u32) & 0xffff_f000) | (rd << 7) | opc
}
pub fn enc_j(imm: i64, rd: u32) -> u32 {
    let imm = imm as u32;
    (bits(imm, 20, 20) << 31) | (bits(imm, 10, 1) << 21) | (bits(imm, 11, 11) << 20) | (bits(imm, 19, 12) << 12) | (rd << 7) | 0x6f
}

/// Expand a 16-bit RVC instruction into its 32-bit equivalent.
/// Returns `None` for reserved / illegal encodings (RV64C only; no F/D forms).
pub fn expand_compressed(h: u16) -> Option<u32> {
    let c = h as u32;
    let op = c & 3;
    let f3 = bits(c, 15, 13);
    let rd = bits(c, 11, 7);
    let rs2 = bits(c, 6, 2);
    let rdp = bits(c, 4, 2) + 8; // rd'/rs2'
    let rs1p = bits(c, 9, 7) + 8; // rs1'/rd'
    match (op, f3) {
        (0, 0) => {
            // C.ADDI4SPN
            let imm = (bits(c, 10, 7) << 6) | (bits(c, 12, 11) << 4) | (bits(c, 5, 5) << 3) | (bits(c, 6, 6) << 2);
            if imm == 0 {
                return None; // includes the all-zero illegal instruction
            }
            Some(enc_i(imm as i64, 2, 0, rdp, 0x13))
        }
        (0, 2) => {
            // C.LW
            let imm = (bits(c, 5, 5) << 6) | (bits(c, 12, 10) << 3) | (bits(c, 6, 6) << 2);
            Some(enc_i(imm as i64, rs1p, 2, rdp, 0x03))
        }
        (0, 3) => {
            // C.LD
            let imm = (bits(c, 6, 5) << 6) | (bits(c, 12, 10) << 3);
            Some(enc_i(imm as i64, rs1p, 3, rdp, 0x03))
        }
        (0, 6) => {
            // C.SW
            let imm = (bits(c, 5, 5) << 6) | (bits(c, 12, 10) << 3) | (bits(c, 6, 6) << 2);
            Some(enc_s(imm as i64, rdp, rs1p, 2, 0x23))
        }
        (0, 7) => {
            // C.SD
            let imm = (bits(c, 6, 5) << 6) | (bits(c, 12, 10) << 3);
            Some(enc_s(imm as i64, rdp, rs1p, 3, 0x23))
        }
        (1, 0) => {
            // C.ADDI / C.NOP
            let imm = sext(((bits(c, 12, 12) << 5) | bits(c, 6, 2)) as u64, 6);
            Some(enc_i(imm, rd, 0, rd, 0x13))
        }
        (1, 1) => {
            // C.ADDIW (rd != 0)
            if rd == 0 {
                return None;
            }
            let imm = sext(((bits(c, 12, 12) << 5) | bits(c, 6, 2)) as u64, 6);
            Some(enc_i(imm, rd, 0, rd, 0x1b))
        }
        (1, 2) => {
            // C.LI
            let imm = sext(((bits(c, 12, 12) << 5) | bits(c, 6, 2)) as u64, 6);
            Some(enc_i(imm, 0, 0, rd, 0x13))
        }
        (1, 3) => {
            if rd == 2 {
                // C.ADDI16SP
                let imm =
                    (bits(c, 12, 12) << 9) | (bits(c, 4, 3) << 7) | (bits(c, 5, 5) << 6) | (bits(c, 2, 2) << 5) | (bits(c, 6, 6) << 4);
                if imm == 0 {
                    return None;
                }
                Some(enc_i(sext(imm as u64, 10), 2, 0, 2, 0x13))
            } else {
                // C.LUI
                let imm = sext(((bits(c, 12, 12) << 17) | (bits(c, 6, 2) << 12)) as u64, 18);
                if imm == 0 {
                    return None;
                }
                Some(enc_u(imm, rd, 0x37))
            }
        }
        (1, 4) => {
            let f2 = bits(c, 11, 10);
            let shamt = (bits(c, 12, 12) << 5) | bits(c, 6, 2);
            match f2 {
                0 => Some(enc_i(shamt as i64, rs1p, 5, rs1p, 0x13)),           // C.SRLI
                1 => Some(enc_i((shamt | 0x400) as i64, rs1p, 5, rs1p, 0x13)), // C.SRAI
                2 => {
                    let imm = sext(shamt as u64, 6);
                    Some(enc_i(imm, rs1p, 7, rs1p, 0x13)) // C.ANDI
                }
                _ => {
                    let sel = (bits(c, 12, 12) << 2) | bits(c, 6, 5);
                    match sel {
                        0 => Some(enc_r(0x20, rdp, rs1p, 0, rs1p, 0x33)), // C.SUB
                        1 => Some(enc_r(0, rdp, rs1p, 4, rs1p, 0x33)),    // C.XOR
                        2 => Some(enc_r(0, rdp, rs1p, 6, rs1p, 0x33)),    // C.OR
                        3 => Some(enc_r(0, rdp, rs1p, 7, rs1p, 0x33)),    // C.AND
                        4 => Some(enc_r(0x20, rdp, rs1p, 0, rs1p, 0x3b)), // C.SUBW
                        5 => Some(enc_r(0, rdp, rs1p, 0, rs1p, 0x3b)),    // C.ADDW
                        _ => None,
                    }
                }
            }
        }
        (1, 5) => {
            // C.J
            let imm = (bits(c, 12, 12) << 11)
                | (bits(c, 8, 8) << 10)
                | (bits(c, 10, 9) << 8)
                | (bits(c, 6, 6) << 7)
                | (bits(c, 7, 7) << 6)
                | (bits(c, 2, 2) << 5)
                | (bits(c, 11, 11) << 4)
                | (bits(c, 5, 3) << 1);
            Some(enc_j(sext(imm as u64, 12), 0))
        }
        (1, 6) | (1, 7) => {
            // C.BEQZ / C.BNEZ
            let imm = (bits(c, 12, 12) << 8) | (bits(c, 6, 5) << 6) | (bits(c, 2, 2) << 5) | (bits(c, 11, 10) << 3) | (bits(c, 4, 3) << 1);
            Some(enc_b(sext(imm as u64, 9), 0, rs1p, f3 - 6))
        }
        (2, 0) => {
            // C.SLLI
            let shamt = (bits(c, 12, 12) << 5) | bits(c, 6, 2);
            Some(enc_i(shamt as i64, rd, 1, rd, 0x13))
        }
        (2, 2) => {
            // C.LWSP
            if rd == 0 {
                return None;
            }
            let imm = (bits(c, 3, 2) << 6) | (bits(c, 12, 12) << 5) | (bits(c, 6, 4) << 2);
            Some(enc_i(imm as i64, 2, 2, rd, 0x03))
        }
        (2, 3) => {
            // C.LDSP
            if rd == 0 {
                return None;
            }
            let imm = (bits(c, 4, 2) << 6) | (bits(c, 12, 12) << 5) | (bits(c, 6, 5) << 3);
            Some(enc_i(imm as i64, 2, 3, rd, 0x03))
        }
        (2, 4) => {
            let b12 = bits(c, 12, 12);
            match (b12, rd, rs2) {
                (0, 0, 0) => None,
                (0, _, 0) => Some(enc_i(0, rd, 0, 0, 0x67)),      // C.JR
                (0, _, _) => Some(enc_r(0, rs2, 0, 0, rd, 0x33)), // C.MV
                (1, 0, 0) => Some(0x0010_0073),                   // C.EBREAK
                (1, _, 0) => Some(enc_i(0, rd, 0, 1, 0x67)),      // C.JALR
                _ => Some(enc_r(0, rs2, rd, 0, rd, 0x33)),        // C.ADD
            }
        }
        (2, 6) => {
            // C.SWSP
            let imm = (bits(c, 8, 7) << 6) | (bits(c, 12, 9) << 2);
            Some(enc_s(imm as i64, rs2, 2, 2, 0x23))
        }
        (2, 7) => {
            // C.SDSP
            let imm = (bits(c, 9, 7) << 6) | (bits(c, 12, 10) << 3);
            Some(enc_s(imm as i64, rs2, 2, 3, 0x23))
        }
        _ => None, // floating-point forms and op==3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immediates_roundtrip_through_encoders() {
        for imm in [-4096i64, -2, 0, 2, 4094, 2048, -2048] {
            let w = enc_b(imm, 3, 4, 0);
            assert_eq!(decode32(w).imm, imm);
        }
        for imm in [-(1i64 << 20), -2, 0, 2, (1 << 20) - 2] {
            let w = enc_j(imm, 1);
            assert_eq!(decode32(w).imm, imm);
        }
        for imm in [-2048i64, -1, 0, 1, 2047] {
            assert_eq!(decode32(enc_s(imm, 1, 2, 3, 0x23)).imm, imm);
            assert_eq!(decode32(enc_i(imm, 1, 0, 2, 0x13)).imm, imm);
        }
    }
}
