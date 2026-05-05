//! Disassembler producing GNU-style assembly text (ABI register names).
//! Compressed instructions are shown as their 32-bit expansion with a `c.`
//! prefix, e.g. `c.addi sp, sp, -16`.

use crate::csr;
use crate::decode::{Inst, Op};

pub const ABI: [&str; 32] = [
    "zero", "ra", "sp", "gp", "tp", "t0", "t1", "t2", "s0", "s1", "a0", "a1", "a2", "a3", "a4", "a5", "a6", "a7", "s2", "s3", "s4", "s5",
    "s6", "s7", "s8", "s9", "s10", "s11", "t3", "t4", "t5", "t6",
];

pub fn mnemonic(op: Op) -> &'static str {
    use Op::*;
    match op {
        Lui => "lui",
        Auipc => "auipc",
        Jal => "jal",
        Jalr => "jalr",
        Beq => "beq",
        Bne => "bne",
        Blt => "blt",
        Bge => "bge",
        Bltu => "bltu",
        Bgeu => "bgeu",
        Lb => "lb",
        Lh => "lh",
        Lw => "lw",
        Ld => "ld",
        Lbu => "lbu",
        Lhu => "lhu",
        Lwu => "lwu",
        Sb => "sb",
        Sh => "sh",
        Sw => "sw",
        Sd => "sd",
        Addi => "addi",
        Slti => "slti",
        Sltiu => "sltiu",
        Xori => "xori",
        Ori => "ori",
        Andi => "andi",
        Slli => "slli",
        Srli => "srli",
        Srai => "srai",
        Add => "add",
        Sub => "sub",
        Sll => "sll",
        Slt => "slt",
        Sltu => "sltu",
        Xor => "xor",
        Srl => "srl",
        Sra => "sra",
        Or => "or",
        And => "and",
        Addiw => "addiw",
        Slliw => "slliw",
        Srliw => "srliw",
        Sraiw => "sraiw",
        Addw => "addw",
        Subw => "subw",
        Sllw => "sllw",
        Srlw => "srlw",
        Sraw => "sraw",
        Fence => "fence",
        FenceI => "fence.i",
        Ecall => "ecall",
        Ebreak => "ebreak",
        Sret => "sret",
        Mret => "mret",
        Wfi => "wfi",
        SfenceVma => "sfence.vma",
        Csrrw => "csrrw",
        Csrrs => "csrrs",
        Csrrc => "csrrc",
        Csrrwi => "csrrwi",
        Csrrsi => "csrrsi",
        Csrrci => "csrrci",
        Mul => "mul",
        Mulh => "mulh",
        Mulhsu => "mulhsu",
        Mulhu => "mulhu",
        Div => "div",
        Divu => "divu",
        Rem => "rem",
        Remu => "remu",
        Mulw => "mulw",
        Divw => "divw",
        Divuw => "divuw",
        Remw => "remw",
        Remuw => "remuw",
        LrW => "lr.w",
        ScW => "sc.w",
        AmoswapW => "amoswap.w",
        AmoaddW => "amoadd.w",
        AmoxorW => "amoxor.w",
        AmoandW => "amoand.w",
        AmoorW => "amoor.w",
        AmominW => "amomin.w",
        AmomaxW => "amomax.w",
        AmominuW => "amominu.w",
        AmomaxuW => "amomaxu.w",
        LrD => "lr.d",
        ScD => "sc.d",
        AmoswapD => "amoswap.d",
        AmoaddD => "amoadd.d",
        AmoxorD => "amoxor.d",
        AmoandD => "amoand.d",
        AmoorD => "amoor.d",
        AmominD => "amomin.d",
        AmomaxD => "amomax.d",
        AmominuD => "amominu.d",
        AmomaxuD => "amomaxu.d",
        Illegal => "unimp",
    }
}

fn csr_name(c: u16) -> String {
    csr::name(c).map(str::to_string).unwrap_or_else(|| format!("{c:#x}"))
}

/// Disassemble an instruction located at `pc`.
pub fn disassemble(i: &Inst, pc: u64) -> String {
    use Op::*;
    let r = |n: u8| ABI[n as usize];
    let prefix = if i.len == 2 { "c." } else { "" };
    let m = format!("{prefix}{}", mnemonic(i.op));
    let (rd, rs1, rs2) = (r(i.rd), r(i.rs1), r(i.rs2));
    let target = pc.wrapping_add(i.imm as u64);
    match i.op {
        Lui | Auipc => format!("{m} {rd}, {:#x}", (i.imm as u64 >> 12) & 0xfffff),
        Jal => format!("{m} {rd}, {target:#x}"),
        Jalr => format!("{m} {rd}, {}({rs1})", hx(i.imm)),
        Beq | Bne | Blt | Bge | Bltu | Bgeu => format!("{m} {rs1}, {rs2}, {target:#x}"),
        Lb | Lh | Lw | Ld | Lbu | Lhu | Lwu => format!("{m} {rd}, {}({rs1})", hx(i.imm)),
        Sb | Sh | Sw | Sd => format!("{m} {rs2}, {}({rs1})", hx(i.imm)),
        Addi | Slti | Sltiu | Xori | Ori | Andi | Slli | Srli | Srai | Addiw | Slliw | Srliw | Sraiw => {
            format!("{m} {rd}, {rs1}, {}", hx(i.imm))
        }
        Fence => format!("{m} {}, {}", fence_set((i.imm >> 4) & 15), fence_set(i.imm & 15)),
        FenceI | Ecall | Ebreak | Sret | Mret | Wfi | Illegal => m,
        SfenceVma => format!("{m} {rs1}, {rs2}"),
        Csrrw | Csrrs | Csrrc => format!("{m} {rd}, {}, {rs1}", csr_name(i.csr)),
        Csrrwi | Csrrsi | Csrrci => format!("{m} {rd}, {}, {}", csr_name(i.csr), hx(i.imm)),
        LrW | LrD => format!("{m}{} {rd}, ({rs1})", aqrl(i.csr)),
        ScW | ScD | AmoswapW | AmoaddW | AmoxorW | AmoandW | AmoorW | AmominW | AmomaxW | AmominuW | AmomaxuW | AmoswapD | AmoaddD
        | AmoxorD | AmoandD | AmoorD | AmominD | AmomaxD | AmominuD | AmomaxuD => {
            format!("{m}{} {rd}, {rs2}, ({rs1})", aqrl(i.csr))
        }
        _ => format!("{m} {rd}, {rs1}, {rs2}"),
    }
}

/// Signed hexadecimal immediate in the style of llvm-objdump (`-0x10`).
fn hx(v: i64) -> String {
    if v < 0 {
        format!("-{:#x}", v.unsigned_abs())
    } else {
        format!("{v:#x}")
    }
}

fn fence_set(bits: i64) -> String {
    let mut s = String::new();
    for (b, c) in [(8, 'i'), (4, 'o'), (2, 'r'), (1, 'w')] {
        if bits & b != 0 {
            s.push(c);
        }
    }
    if s.is_empty() {
        s.push('0');
    }
    s
}

fn aqrl(bits: u16) -> &'static str {
    match bits & 3 {
        0 => "",
        1 => ".rl",
        2 => ".aq",
        _ => ".aqrl",
    }
}
