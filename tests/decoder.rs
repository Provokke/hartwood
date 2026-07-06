//! Decoder and disassembler tests.
//!
//! * `disasm_corpus.tsv` holds every RV64IMA+Zicsr+Zifencei instruction form
//!   assembled by clang and disassembled by llvm-objdump (no aliases); our
//!   disassembler must reproduce llvm's text exactly.
//! * `rvc_llvm.txt` lists all 49152 16-bit encodings with llvm's verdict;
//!   our expander must agree on validity and on the base operation.

use hartwood::decode::{decode, decode32, enc_i, expand_compressed, Op};
use hartwood::disasm::disassemble;
use proptest::prelude::*;

#[test]
fn llvm_corpus_disassembles_identically() {
    let data = include_str!("data/disasm_corpus.tsv");
    let mut n = 0;
    for line in data.lines() {
        let mut f = line.split('\t');
        let pc = u64::from_str_radix(f.next().unwrap(), 16).unwrap();
        let word = u32::from_str_radix(f.next().unwrap(), 16).unwrap();
        let expect = f.next().unwrap();
        let inst = decode(word);
        assert_ne!(inst.op, Op::Illegal, "{word:08x} should be legal ({expect})");
        assert_eq!(inst.len, 4);
        assert_eq!(disassemble(&inst, pc), expect, "word {word:08x} at {pc:#x}");
        n += 1;
    }
    assert!(n >= 110, "corpus too small: {n}");
}

fn base_op_for(cmn: &str) -> Option<Op> {
    Some(match cmn {
        "c.addi4spn" | "c.nop" | "c.addi" | "c.li" | "c.addi16sp" => Op::Addi,
        "c.lw" | "c.lwsp" => Op::Lw,
        "c.ld" | "c.ldsp" => Op::Ld,
        "c.sw" | "c.swsp" => Op::Sw,
        "c.sd" | "c.sdsp" => Op::Sd,
        "c.addiw" => Op::Addiw,
        "c.lui" => Op::Lui,
        "c.srli" | "c.srli64" => Op::Srli,
        "c.srai" | "c.srai64" => Op::Srai,
        "c.slli" | "c.slli64" => Op::Slli,
        "c.andi" => Op::Andi,
        "c.sub" => Op::Sub,
        "c.xor" => Op::Xor,
        "c.or" => Op::Or,
        "c.and" => Op::And,
        "c.subw" => Op::Subw,
        "c.addw" => Op::Addw,
        "c.j" => Op::Jal,
        "c.beqz" => Op::Beq,
        "c.bnez" => Op::Bne,
        "c.jr" | "c.jalr" => Op::Jalr,
        "c.mv" | "c.add" => Op::Add,
        "c.ebreak" => Op::Ebreak,
        _ => return None,
    })
}

#[test]
fn rvc_exhaustive_matches_llvm() {
    let data = include_str!("data/rvc_llvm.txt");
    let mut checked = 0;
    let mut known_reserved = 0;
    for line in data.lines() {
        let (hex, mn) = line.split_once(' ').unwrap();
        let h = u16::from_str_radix(hex, 16).unwrap();
        let ours = decode(h as u32);
        assert_eq!(ours.len, 2);
        match (mn, base_op_for(mn)) {
            ("-", _) | ("c.unimp", _) => {
                assert_eq!(ours.op, Op::Illegal, "{h:04x}: llvm says invalid, we decoded {:?}", ours.op);
            }
            ("c.lui", _) if (h >> 12) & 1 == 0 && (h >> 2) & 0x1f == 0 => {
                // nzimm == 0 is *reserved* per the spec; llvm still prints it.
                assert_eq!(ours.op, Op::Illegal);
                known_reserved += 1;
            }
            (_, Some(op)) => assert_eq!(ours.op, op, "{h:04x} ({mn})"),
            (_, None) => panic!("unmapped llvm mnemonic {mn}"),
        }
        checked += 1;
    }
    assert_eq!(checked, 49152);
    assert_eq!(known_reserved, 31);
}

/// Hand-checked expansions with full operand verification.
#[test]
fn rvc_expansions_operands() {
    // (halfword, expected disassembly of expansion at pc 0x1000)
    let cases: &[(u16, &str)] = &[
        (0x1141, "c.addi sp, sp, -0x10"),
        (0xe406, "c.sd ra, 0x8(sp)"),
        (0x60a2, "c.ld ra, 0x8(sp)"),
        (0x8082, "c.jalr zero, 0x0(ra)"),
        (0x852e, "c.add a0, zero, a1"),
        (0x952e, "c.add a0, a0, a1"),
        (0x4505, "c.addi a0, zero, 0x1"),
        (0x0028, "c.addi a0, sp, 0x8"),
        (0x6505, "c.lui a0, 0x1"),
        (0x8d0d, "c.sub a0, a0, a1"),
        (0x9d2d, "c.addw a0, a0, a1"),
        (0x8905, "c.andi a0, a0, 0x1"),
        (0x8105, "c.srli a0, a0, 0x1"),
        (0x8505, "c.srai a0, a0, 0x1"),
        (0x050a, "c.slli a0, a0, 0x2"),
        (0xa001, "c.jal zero, 0x1000"),
        (0xc111, "c.beq a0, zero, 0x1004"),
        (0xe111, "c.bne a0, zero, 0x1004"),
        (0x9002, "c.ebreak"),
        (0x4108, "c.lw a0, 0x0(a0)"),
        (0xc10c, "c.sw a1, 0x0(a0)"),
        (0x2505, "c.addiw a0, a0, 0x1"),
        (0x6121, "c.addi sp, sp, 0x40"),
        (0x4502, "c.lw a0, 0x0(sp)"),
        (0xc02a, "c.sw a0, 0x0(sp)"),
    ];
    for &(h, expect) in cases {
        let i = decode(h as u32);
        assert_eq!(disassemble(&i, 0x1000), expect, "halfword {h:04x}");
    }
}

/// Every opcode/funct combination we implement, built from the encoders.
#[test]
fn every_32bit_opcode_decodes() {
    use hartwood::decode::{enc_r, enc_s};
    let r = |f7, f3, opc| enc_r(f7, 3, 2, f3, 1, opc);
    let table: Vec<(u32, Op)> = vec![
        (r(0, 0, 0x33), Op::Add),
        (r(0x20, 0, 0x33), Op::Sub),
        (r(0, 1, 0x33), Op::Sll),
        (r(0, 2, 0x33), Op::Slt),
        (r(0, 3, 0x33), Op::Sltu),
        (r(0, 4, 0x33), Op::Xor),
        (r(0, 5, 0x33), Op::Srl),
        (r(0x20, 5, 0x33), Op::Sra),
        (r(0, 6, 0x33), Op::Or),
        (r(0, 7, 0x33), Op::And),
        (r(1, 0, 0x33), Op::Mul),
        (r(1, 1, 0x33), Op::Mulh),
        (r(1, 2, 0x33), Op::Mulhsu),
        (r(1, 3, 0x33), Op::Mulhu),
        (r(1, 4, 0x33), Op::Div),
        (r(1, 5, 0x33), Op::Divu),
        (r(1, 6, 0x33), Op::Rem),
        (r(1, 7, 0x33), Op::Remu),
        (r(0, 0, 0x3b), Op::Addw),
        (r(0x20, 0, 0x3b), Op::Subw),
        (r(0, 1, 0x3b), Op::Sllw),
        (r(0, 5, 0x3b), Op::Srlw),
        (r(0x20, 5, 0x3b), Op::Sraw),
        (r(1, 0, 0x3b), Op::Mulw),
        (r(1, 4, 0x3b), Op::Divw),
        (r(1, 5, 0x3b), Op::Divuw),
        (r(1, 6, 0x3b), Op::Remw),
        (r(1, 7, 0x3b), Op::Remuw),
        (enc_i(5, 2, 0, 1, 0x13), Op::Addi),
        (enc_i(5, 2, 2, 1, 0x13), Op::Slti),
        (enc_i(5, 2, 3, 1, 0x13), Op::Sltiu),
        (enc_i(5, 2, 4, 1, 0x13), Op::Xori),
        (enc_i(5, 2, 6, 1, 0x13), Op::Ori),
        (enc_i(5, 2, 7, 1, 0x13), Op::Andi),
        (enc_i(63, 2, 1, 1, 0x13), Op::Slli),
        (enc_i(63, 2, 5, 1, 0x13), Op::Srli),
        (enc_i(0x400 | 63, 2, 5, 1, 0x13), Op::Srai),
        (enc_i(-1, 2, 0, 1, 0x1b), Op::Addiw),
        (enc_i(31, 2, 1, 1, 0x1b), Op::Slliw),
        (enc_i(31, 2, 5, 1, 0x1b), Op::Srliw),
        (enc_i(0x400 | 31, 2, 5, 1, 0x1b), Op::Sraiw),
    ];
    let mut table = table;
    for (f3, op) in [
        (0, Op::Lb),
        (1, Op::Lh),
        (2, Op::Lw),
        (3, Op::Ld),
        (4, Op::Lbu),
        (5, Op::Lhu),
        (6, Op::Lwu),
    ] {
        table.push((enc_i(-8, 2, f3, 1, 0x03), op));
    }
    for (f3, op) in [(0, Op::Sb), (1, Op::Sh), (2, Op::Sw), (3, Op::Sd)] {
        table.push((enc_s(-8, 3, 2, f3, 0x23), op));
    }
    let amo = [
        (0x00, Op::AmoaddW, Op::AmoaddD),
        (0x01, Op::AmoswapW, Op::AmoswapD),
        (0x03, Op::ScW, Op::ScD),
        (0x04, Op::AmoxorW, Op::AmoxorD),
        (0x08, Op::AmoorW, Op::AmoorD),
        (0x0c, Op::AmoandW, Op::AmoandD),
        (0x10, Op::AmominW, Op::AmominD),
        (0x14, Op::AmomaxW, Op::AmomaxD),
        (0x18, Op::AmominuW, Op::AmominuD),
        (0x1c, Op::AmomaxuW, Op::AmomaxuD),
    ];
    for (f5, w, d) in amo {
        for aqrl in 0..4 {
            table.push((enc_r((f5 << 2) | aqrl, 3, 2, 2, 1, 0x2f), w));
            table.push((enc_r((f5 << 2) | aqrl, 3, 2, 3, 1, 0x2f), d));
        }
    }
    table.push((enc_r(0x02 << 2, 0, 2, 2, 1, 0x2f), Op::LrW));
    table.push((enc_r(0x02 << 2, 0, 2, 3, 1, 0x2f), Op::LrD));
    for (f3, op) in [
        (1, Op::Csrrw),
        (2, Op::Csrrs),
        (3, Op::Csrrc),
        (5, Op::Csrrwi),
        (6, Op::Csrrsi),
        (7, Op::Csrrci),
    ] {
        table.push((enc_i(0x300, 2, f3, 1, 0x73), op));
    }
    table.extend([
        (0x0000_0073, Op::Ecall),
        (0x0010_0073, Op::Ebreak),
        (0x1020_0073, Op::Sret),
        (0x3020_0073, Op::Mret),
        (0x1050_0073, Op::Wfi),
        (0x1200_0073, Op::SfenceVma),
        (0x0ff0_000f, Op::Fence),
        (0x0000_100f, Op::FenceI),
        (0x0000_0537, Op::Lui),
        (0x0000_0517, Op::Auipc),
        (0x0000_006f, Op::Jal),
        (0x0000_0067, Op::Jalr),
    ]);
    for (f3, op) in [(0, Op::Beq), (1, Op::Bne), (4, Op::Blt), (5, Op::Bge), (6, Op::Bltu), (7, Op::Bgeu)] {
        table.push((hartwood::decode::enc_b(16, 3, 2, f3), op));
    }
    for (w, op) in table {
        assert_eq!(decode32(w).op, op, "{w:08x}");
    }
}

#[test]
fn reserved_encodings_are_illegal() {
    for w in [
        0x0000_0000u32,                                       // all zeros (compressed illegal)
        0xffff_ffff,                                          // all ones
        hartwood::decode::enc_r(0x40, 3, 2, 0, 1, 0x33),      // bad funct7
        enc_i(5, 2, 2, 1, 0x1b),                              // OP-IMM-32 funct3=2
        hartwood::decode::enc_r(0x02 << 2, 5, 2, 2, 1, 0x2f), // LR with rs2 != 0
    ]
    .iter()
    {
        assert_eq!(decode(*w).op, Op::Illegal, "{w:08x}");
    }
    assert_eq!(expand_compressed(0x0000), None);
    assert_eq!(expand_compressed(0x6101 & !0x107c), None, "c.addi16sp with imm 0");
}

proptest! {
    #[test]
    fn decode_and_disasm_never_panic(w in any::<u32>()) {
        let i = decode(w);
        let _ = disassemble(&i, 0x8000_0000);
        prop_assert!(i.len == 2 || i.len == 4);
        prop_assert_eq!(i.len == 4, w & 3 == 3);
    }

    #[test]
    fn compressed_expansion_is_32bit_and_consistent(h in any::<u16>()) {
        if let Some(w) = expand_compressed(h) {
            prop_assert_eq!(w & 3, 3);
            let a = decode(h as u32);
            let b = decode32(w);
            prop_assert_eq!((a.op, a.rd, a.rs1, a.rs2, a.imm), (b.op, b.rd, b.rs1, b.rs2, b.imm));
        }
    }
}
