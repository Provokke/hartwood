//! CSR addresses and bit definitions.

pub const SSTATUS: u16 = 0x100;
pub const SIE: u16 = 0x104;
pub const STVEC: u16 = 0x105;
pub const SCOUNTEREN: u16 = 0x106;
pub const SENVCFG: u16 = 0x10a;
pub const SSCRATCH: u16 = 0x140;
pub const SEPC: u16 = 0x141;
pub const SCAUSE: u16 = 0x142;
pub const STVAL: u16 = 0x143;
pub const SIP: u16 = 0x144;
pub const SATP: u16 = 0x180;

pub const MSTATUS: u16 = 0x300;
pub const MISA: u16 = 0x301;
pub const MEDELEG: u16 = 0x302;
pub const MIDELEG: u16 = 0x303;
pub const MIE: u16 = 0x304;
pub const MTVEC: u16 = 0x305;
pub const MCOUNTEREN: u16 = 0x306;
pub const MENVCFG: u16 = 0x30a;
pub const MCOUNTINHIBIT: u16 = 0x320;
pub const MSCRATCH: u16 = 0x340;
pub const MEPC: u16 = 0x341;
pub const MCAUSE: u16 = 0x342;
pub const MTVAL: u16 = 0x343;
pub const MIP: u16 = 0x344;
pub const PMPCFG0: u16 = 0x3a0;
pub const PMPADDR0: u16 = 0x3b0;
pub const TSELECT: u16 = 0x7a0;
pub const TDATA1: u16 = 0x7a1;
pub const TDATA2: u16 = 0x7a2;
pub const TDATA3: u16 = 0x7a3;
pub const MCYCLE: u16 = 0xb00;
pub const MINSTRET: u16 = 0xb02;
pub const CYCLE: u16 = 0xc00;
pub const TIME: u16 = 0xc01;
pub const INSTRET: u16 = 0xc02;
pub const MVENDORID: u16 = 0xf11;
pub const MARCHID: u16 = 0xf12;
pub const MIMPID: u16 = 0xf13;
pub const MHARTID: u16 = 0xf14;
pub const MCONFIGPTR: u16 = 0xf15;

// mstatus bits
pub const MSTATUS_SIE: u64 = 1 << 1;
pub const MSTATUS_MIE: u64 = 1 << 3;
pub const MSTATUS_SPIE: u64 = 1 << 5;
pub const MSTATUS_MPIE: u64 = 1 << 7;
pub const MSTATUS_SPP: u64 = 1 << 8;
pub const MSTATUS_MPP_SHIFT: u64 = 11;
pub const MSTATUS_MPP: u64 = 3 << 11;
pub const MSTATUS_MPRV: u64 = 1 << 17;
pub const MSTATUS_SUM: u64 = 1 << 18;
pub const MSTATUS_MXR: u64 = 1 << 19;
pub const MSTATUS_TVM: u64 = 1 << 20;
pub const MSTATUS_TW: u64 = 1 << 21;
pub const MSTATUS_TSR: u64 = 1 << 22;
pub const MSTATUS_UXL_SXL: u64 = (2 << 32) | (2 << 34);

pub const MSTATUS_WMASK: u64 = MSTATUS_SIE
    | MSTATUS_MIE
    | MSTATUS_SPIE
    | MSTATUS_MPIE
    | MSTATUS_SPP
    | MSTATUS_MPP
    | MSTATUS_MPRV
    | MSTATUS_SUM
    | MSTATUS_MXR
    | MSTATUS_TVM
    | MSTATUS_TW
    | MSTATUS_TSR;
pub const SSTATUS_MASK: u64 = MSTATUS_SIE | MSTATUS_SPIE | MSTATUS_SPP | MSTATUS_SUM | MSTATUS_MXR | (3 << 32);

// Interrupt bits (mip/mie)
pub const IRQ_SSI: u64 = 1;
pub const IRQ_MSI: u64 = 3;
pub const IRQ_STI: u64 = 5;
pub const IRQ_MTI: u64 = 7;
pub const IRQ_SEI: u64 = 9;
pub const IRQ_MEI: u64 = 11;
pub const MIP_SSIP: u64 = 1 << IRQ_SSI;
pub const MIP_MSIP: u64 = 1 << IRQ_MSI;
pub const MIP_STIP: u64 = 1 << IRQ_STI;
pub const MIP_MTIP: u64 = 1 << IRQ_MTI;
pub const MIP_SEIP: u64 = 1 << IRQ_SEI;
pub const MIP_MEIP: u64 = 1 << IRQ_MEI;
pub const S_INTS: u64 = MIP_SSIP | MIP_STIP | MIP_SEIP;
pub const ALL_INTS: u64 = S_INTS | MIP_MSIP | MIP_MTIP | MIP_MEIP;

/// misa: RV64 + A, C, I, M, S, U.
pub const MISA_VALUE: u64 = (2 << 62) | (1 << 0) | (1 << 2) | (1 << 8) | (1 << 12) | (1 << 18) | (1 << 20);

/// Exception cause codes.
pub mod cause {
    pub const INST_MISALIGNED: u64 = 0;
    pub const INST_ACCESS: u64 = 1;
    pub const ILLEGAL: u64 = 2;
    pub const BREAKPOINT: u64 = 3;
    pub const LOAD_MISALIGNED: u64 = 4;
    pub const LOAD_ACCESS: u64 = 5;
    pub const STORE_MISALIGNED: u64 = 6;
    pub const STORE_ACCESS: u64 = 7;
    pub const ECALL_U: u64 = 8;
    pub const ECALL_S: u64 = 9;
    pub const ECALL_M: u64 = 11;
    pub const INST_PAGE: u64 = 12;
    pub const LOAD_PAGE: u64 = 13;
    pub const STORE_PAGE: u64 = 15;
}

/// Human-readable CSR names for the disassembler.
pub fn name(csr: u16) -> Option<&'static str> {
    Some(match csr {
        SSTATUS => "sstatus",
        SIE => "sie",
        STVEC => "stvec",
        SCOUNTEREN => "scounteren",
        SENVCFG => "senvcfg",
        SSCRATCH => "sscratch",
        SEPC => "sepc",
        SCAUSE => "scause",
        STVAL => "stval",
        SIP => "sip",
        SATP => "satp",
        MSTATUS => "mstatus",
        MISA => "misa",
        MEDELEG => "medeleg",
        MIDELEG => "mideleg",
        MIE => "mie",
        MTVEC => "mtvec",
        MCOUNTEREN => "mcounteren",
        MENVCFG => "menvcfg",
        MCOUNTINHIBIT => "mcountinhibit",
        MSCRATCH => "mscratch",
        MEPC => "mepc",
        MCAUSE => "mcause",
        MTVAL => "mtval",
        MIP => "mip",
        PMPCFG0 => "pmpcfg0",
        PMPADDR0 => "pmpaddr0",
        TSELECT => "tselect",
        TDATA1 => "tdata1",
        MCYCLE => "mcycle",
        MINSTRET => "minstret",
        CYCLE => "cycle",
        TIME => "time",
        INSTRET => "instret",
        MVENDORID => "mvendorid",
        MARCHID => "marchid",
        MIMPID => "mimpid",
        MHARTID => "mhartid",
        _ => return None,
    })
}
