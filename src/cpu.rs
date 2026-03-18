//! The hart: architectural state, CSRs, privilege modes, traps, Sv39 MMU
//! with a TLB, and the instruction executor with a per-instruction cycle
//! cost model.

use crate::alu;
use crate::bus::Bus;
use crate::csr::{self, cause};
use crate::decode::{decode, Inst, Op};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priv {
    User = 0,
    Supervisor = 1,
    Machine = 3,
}

impl Priv {
    fn from_bits(b: u64) -> Priv {
        match b & 3 {
            0 => Priv::User,
            1 => Priv::Supervisor,
            _ => Priv::Machine,
        }
    }
}

/// A synchronous exception or interrupt to be delivered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trap {
    pub cause: u64,
    pub tval: u64,
}

impl Trap {
    pub fn new(cause: u64, tval: u64) -> Trap {
        Trap { cause, tval }
    }
}

/// A CSR access that must raise an illegal-instruction exception.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CsrError;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Access {
    Fetch,
    Load,
    Store,
}

impl Access {
    fn page_fault(self) -> u64 {
        match self {
            Access::Fetch => cause::INST_PAGE,
            Access::Load => cause::LOAD_PAGE,
            Access::Store => cause::STORE_PAGE,
        }
    }
    fn access_fault(self) -> u64 {
        match self {
            Access::Fetch => cause::INST_ACCESS,
            Access::Load => cause::LOAD_ACCESS,
            Access::Store => cause::STORE_ACCESS,
        }
    }
}

/// Cycle cost model (documented in docs/ARCHITECTURE.md).
pub mod cost {
    pub const ALU: u64 = 1;
    pub const BRANCH_NOT_TAKEN: u64 = 1;
    pub const BRANCH_TAKEN: u64 = 3;
    pub const JAL: u64 = 2;
    pub const JALR: u64 = 3;
    pub const LOAD: u64 = 3;
    pub const STORE: u64 = 2;
    pub const MUL: u64 = 3;
    pub const MULH: u64 = 4;
    pub const DIV: u64 = 34;
    pub const DIVW: u64 = 20;
    pub const CSR: u64 = 2;
    pub const ATOMIC: u64 = 5;
    pub const FENCE_I: u64 = 5;
    pub const TRAP: u64 = 5;
    pub const XRET: u64 = 3;
    pub const TLB_MISS: u64 = 10;
}

const PTE_V: u64 = 1 << 0;
const PTE_R: u64 = 1 << 1;
const PTE_W: u64 = 1 << 2;
const PTE_X: u64 = 1 << 3;
const PTE_U: u64 = 1 << 4;
const PTE_A: u64 = 1 << 6;
const PTE_D: u64 = 1 << 7;

const TLB_SIZE: usize = 256;

#[derive(Clone, Copy, Default)]
struct TlbEntry {
    /// Virtual page number + 1 (0 = invalid).
    tag: u64,
    /// Physical address of the 4 KiB page.
    ppage: u64,
    /// Leaf PTE flags.
    flags: u64,
}

/// Translation lookaside buffers caching Sv39 leaf entries at 4 KiB
/// granularity (superpages are split on fill). Split into a direct-mapped
/// instruction TLB and data TLB so code and data pages never evict each other.
pub struct Tlb {
    entries: Box<[TlbEntry; TLB_SIZE]>,
    ientries: Box<[TlbEntry; TLB_SIZE]>,
    pub hits: u64,
    pub misses: u64,
}

impl Default for Tlb {
    fn default() -> Self {
        Tlb {
            entries: Box::new([TlbEntry::default(); TLB_SIZE]),
            ientries: Box::new([TlbEntry::default(); TLB_SIZE]),
            hits: 0,
            misses: 0,
        }
    }
}

impl Tlb {
    pub fn flush(&mut self) {
        self.entries.iter_mut().for_each(|e| e.tag = 0);
        self.ientries.iter_mut().for_each(|e| e.tag = 0);
    }
    pub fn flush_page(&mut self, vaddr: u64) {
        let vpn = (vaddr >> 12) & ((1 << 27) - 1);
        for table in [&mut self.entries, &mut self.ientries] {
            let e = &mut table[vpn as usize % TLB_SIZE];
            if e.tag == vpn + 1 {
                e.tag = 0;
            }
        }
    }
}

/// Small direct-mapped cache of decoded instructions keyed by raw encoding.
/// Keyed by the instruction bits themselves, so it is always coherent with
/// memory (self-modifying code just misses).
struct DecodeCache {
    slots: Box<[(u32, Inst)]>,
}

impl DecodeCache {
    const N: usize = 4096;
    fn new() -> Self {
        // raw = 0 is never a valid key match because a 0 word decodes illegal
        // anyway; we seed with that decode so the cache is always consistent.
        DecodeCache {
            slots: vec![(0, decode(0)); Self::N].into_boxed_slice(),
        }
    }
    #[inline]
    fn get(&mut self, raw: u32) -> Inst {
        let h = (raw ^ (raw >> 12) ^ (raw >> 20)) as usize & (Self::N - 1);
        let slot = &mut self.slots[h];
        if slot.0 == raw {
            return slot.1;
        }
        let i = decode(raw);
        *slot = (raw, i);
        i
    }
}

pub struct Cpu {
    pub x: [u64; 32],
    pub pc: u64,
    pub prv: Priv,
    /// Total elapsed cycles according to the cost model. Drives mtime.
    pub cycles: u64,
    pub instret: u64,
    pub wfi: bool,
    pub reservation: Option<u64>,
    pub tlb: Tlb,
    dcache: DecodeCache,

    // CSR state
    pub mstatus: u64,
    pub medeleg: u64,
    pub mideleg: u64,
    pub mie: u64,
    /// Software-writable pending bits (SSIP/STIP/SEIP); hardware bits are
    /// merged in from the CLINT/PLIC on each check.
    pub mip_sw: u64,
    /// Hardware interrupt lines (MSIP, MTIP, MEIP, external SEIP).
    pub mip_hw: u64,
    pub mtvec: u64,
    pub mepc: u64,
    pub mcause: u64,
    pub mtval: u64,
    pub mscratch: u64,
    pub mcounteren: u64,
    pub mcountinhibit: u64,
    pub menvcfg: u64,
    pub stvec: u64,
    pub sepc: u64,
    pub scause: u64,
    pub stval: u64,
    pub sscratch: u64,
    pub scounteren: u64,
    pub senvcfg: u64,
    pub satp: u64,
    pub pmpcfg: [u64; 16],
    pub pmpaddr: [u64; 64],
    mcycle_offset: u64,
    suppress_instret: bool,
    /// Value of `time` = mtime, refreshed by the machine.
    pub time: u64,
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

macro_rules! amo {
    ($self:ident, $bus:ident, $i:ident, $len:expr, $f:expr) => {{
        let addr = $self.x[$i.rs1 as usize];
        if addr % $len != 0 {
            return Err(Trap::new(cause::STORE_MISALIGNED, addr));
        }
        let pa = $self.translate($bus, addr, Access::Store)?;
        let raw = $bus.read(pa, $len).map_err(|_| Trap::new(cause::STORE_ACCESS, addr))?;
        let old = if $len == 4 { alu::sext32(raw) } else { raw };
        let src = $self.x[$i.rs2 as usize];
        let f: fn(u64, u64) -> u64 = $f;
        let new = f(old, src);
        $bus.write(pa, $len, new).map_err(|_| Trap::new(cause::STORE_ACCESS, addr))?;
        $self.set($i.rd, old);
        cost::ATOMIC
    }};
}

impl Cpu {
    pub fn new() -> Cpu {
        Cpu {
            x: [0; 32],
            pc: crate::bus::RAM_BASE,
            prv: Priv::Machine,
            cycles: 0,
            instret: 0,
            wfi: false,
            reservation: None,
            tlb: Tlb::default(),
            dcache: DecodeCache::new(),
            mstatus: csr::MSTATUS_UXL_SXL,
            medeleg: 0,
            mideleg: 0,
            mie: 0,
            mip_sw: 0,
            mip_hw: 0,
            mtvec: 0,
            mepc: 0,
            mcause: 0,
            mtval: 0,
            mscratch: 0,
            mcounteren: 0,
            mcountinhibit: 0,
            menvcfg: 0,
            stvec: 0,
            sepc: 0,
            scause: 0,
            stval: 0,
            sscratch: 0,
            scounteren: 0,
            senvcfg: 0,
            satp: 0,
            pmpcfg: [0; 16],
            pmpaddr: [0; 64],
            mcycle_offset: 0,
            suppress_instret: false,
            time: 0,
        }
    }

    #[inline]
    fn set(&mut self, rd: u8, v: u64) {
        if rd != 0 {
            self.x[rd as usize] = v;
        }
    }

    pub fn mip(&self) -> u64 {
        self.mip_sw | self.mip_hw
    }

    // ------------------------------------------------------------------
    // Address translation
    // ------------------------------------------------------------------

    fn effective_priv(&self, access: Access) -> Priv {
        if access != Access::Fetch && self.prv == Priv::Machine && self.mstatus & csr::MSTATUS_MPRV != 0 {
            Priv::from_bits(self.mstatus >> csr::MSTATUS_MPP_SHIFT)
        } else {
            self.prv
        }
    }

    #[inline]
    pub fn translate(&mut self, bus: &mut Bus, vaddr: u64, access: Access) -> Result<u64, Trap> {
        if self.satp >> 60 != 8 {
            return Ok(vaddr);
        }
        let prv = self.effective_priv(access);
        if prv == Priv::Machine {
            return Ok(vaddr);
        }
        // Canonical check: bits 63..39 must equal bit 38.
        let top = (vaddr as i64) >> 38;
        if top != 0 && top != -1 {
            return Err(Trap::new(access.page_fault(), vaddr));
        }
        let vpn = (vaddr >> 12) & ((1 << 27) - 1);
        let idx = vpn as usize % TLB_SIZE;
        let e = if access == Access::Fetch {
            self.tlb.ientries[idx]
        } else {
            self.tlb.entries[idx]
        };
        if e.tag == vpn + 1 && (access != Access::Store || e.flags & PTE_D != 0) {
            self.tlb.hits += 1;
            self.check_perm(e.flags, prv, access, vaddr)?;
            return Ok(e.ppage | (vaddr & 0xfff));
        }
        self.tlb.misses += 1;
        self.cycles += cost::TLB_MISS;
        let (ppage, flags) = self.walk(bus, vaddr, access, prv)?;
        let fill = TlbEntry {
            tag: vpn + 1,
            ppage,
            flags,
        };
        if access == Access::Fetch {
            self.tlb.ientries[idx] = fill;
        } else {
            self.tlb.entries[idx] = fill;
        }
        Ok(ppage | (vaddr & 0xfff))
    }

    fn check_perm(&self, flags: u64, prv: Priv, access: Access, vaddr: u64) -> Result<(), Trap> {
        let fault = Trap::new(access.page_fault(), vaddr);
        let user_page = flags & PTE_U != 0;
        match prv {
            Priv::User if !user_page => return Err(fault),
            Priv::Supervisor if user_page && (access == Access::Fetch || self.mstatus & csr::MSTATUS_SUM == 0) => {
                return Err(fault);
            }
            _ => {}
        }
        let ok = match access {
            Access::Fetch => flags & PTE_X != 0,
            Access::Load => flags & PTE_R != 0 || (self.mstatus & csr::MSTATUS_MXR != 0 && flags & PTE_X != 0),
            Access::Store => flags & PTE_W != 0,
        };
        if ok {
            Ok(())
        } else {
            Err(fault)
        }
    }

    /// Sv39 page-table walk. Returns (physical 4 KiB page, leaf flags).
    fn walk(&mut self, bus: &mut Bus, vaddr: u64, access: Access, prv: Priv) -> Result<(u64, u64), Trap> {
        let pf = Trap::new(access.page_fault(), vaddr);
        let mut table = (self.satp & ((1 << 44) - 1)) << 12;
        for level in (0..3).rev() {
            let vpn_i = (vaddr >> (12 + 9 * level)) & 0x1ff;
            let pte_addr = table + vpn_i * 8;
            let mut pte = bus.read(pte_addr, 8).map_err(|_| Trap::new(access.access_fault(), vaddr))?;
            if pte & PTE_V == 0 || (pte & PTE_R == 0 && pte & PTE_W != 0) || pte >> 54 != 0 {
                return Err(pf);
            }
            let ppn = (pte >> 10) & ((1 << 44) - 1);
            if pte & (PTE_R | PTE_X) == 0 {
                if level == 0 || pte & (PTE_A | PTE_D | PTE_U) != 0 {
                    return Err(pf);
                }
                table = ppn << 12;
                continue;
            }
            // Leaf. Superpages must be aligned.
            if level > 0 && ppn & ((1 << (9 * level)) - 1) != 0 {
                return Err(pf);
            }
            self.check_perm(pte, prv, access, vaddr)?;
            // Hardware A/D update.
            let mut new = pte | PTE_A;
            if access == Access::Store {
                new |= PTE_D;
            }
            if new != pte {
                pte = new;
                bus.write(pte_addr, 8, pte).map_err(|_| Trap::new(access.access_fault(), vaddr))?;
            }
            let mask = (1u64 << (9 * level)) - 1;
            let page_ppn = (ppn & !mask) | ((vaddr >> 12) & mask);
            let mut flags = pte & 0xff;
            // Only cache D for stores-to-come if it is actually set.
            if access != Access::Store && pte & PTE_D == 0 {
                flags &= !PTE_D;
            }
            return Ok((page_ppn << 12, flags));
        }
        Err(pf)
    }

    // ------------------------------------------------------------------
    // Memory helpers
    // ------------------------------------------------------------------

    #[inline]
    fn load(&mut self, bus: &mut Bus, addr: u64, len: u64) -> Result<u64, Trap> {
        if addr & (len - 1) != 0 {
            return self.load_misaligned(bus, addr, len);
        }
        let pa = self.translate(bus, addr, Access::Load)?;
        bus.read(pa, len).map_err(|_| Trap::new(cause::LOAD_ACCESS, addr))
    }

    #[inline]
    fn store(&mut self, bus: &mut Bus, addr: u64, len: u64, val: u64) -> Result<(), Trap> {
        if addr & (len - 1) != 0 {
            return self.store_misaligned(bus, addr, len, val);
        }
        let pa = self.translate(bus, addr, Access::Store)?;
        if let Some(r) = self.reservation {
            if r & !7 == pa & !7 {
                self.reservation = None;
            }
        }
        bus.write(pa, len, val).map_err(|_| Trap::new(cause::STORE_ACCESS, addr))
    }

    /// Misaligned loads are supported in hardware, one byte at a time (each
    /// byte translated separately so page-crossing accesses work).
    #[cold]
    fn load_misaligned(&mut self, bus: &mut Bus, addr: u64, len: u64) -> Result<u64, Trap> {
        let mut pas = [0u64; 8];
        for k in 0..len {
            pas[k as usize] = self.translate(bus, addr.wrapping_add(k), Access::Load)?;
        }
        let mut v = 0u64;
        for k in 0..len {
            let b = bus.read(pas[k as usize], 1).map_err(|_| Trap::new(cause::LOAD_ACCESS, addr))?;
            v |= b << (8 * k);
        }
        self.cycles += 2 * len;
        Ok(v)
    }

    #[cold]
    fn store_misaligned(&mut self, bus: &mut Bus, addr: u64, len: u64, val: u64) -> Result<(), Trap> {
        let mut pas = [0u64; 8];
        // Translate every byte first so a fault leaves memory untouched.
        for k in 0..len {
            pas[k as usize] = self.translate(bus, addr.wrapping_add(k), Access::Store)?;
        }
        self.reservation = None;
        for k in 0..len {
            bus.write(pas[k as usize], 1, (val >> (8 * k)) & 0xff)
                .map_err(|_| Trap::new(cause::STORE_ACCESS, addr))?;
        }
        self.cycles += 2 * len;
        Ok(())
    }

    #[inline]
    fn fetch(&mut self, bus: &mut Bus) -> Result<u32, Trap> {
        let pc = self.pc;
        let pa = self.translate(bus, pc, Access::Fetch)?;
        if pc & 0xfff != 0xffe && bus.ram.contains(pa, 4) {
            return Ok(bus.ram.read(pa, 4) as u32);
        }
        let lo = bus.read(pa, 2).map_err(|_| Trap::new(cause::INST_ACCESS, pc))? as u32;
        if lo & 3 != 3 {
            return Ok(lo);
        }
        let pc2 = pc.wrapping_add(2);
        let pa2 = self.translate(bus, pc2, Access::Fetch)?;
        let hi = bus.read(pa2, 2).map_err(|_| Trap::new(cause::INST_ACCESS, pc2))? as u32;
        Ok(lo | (hi << 16))
    }

    // ------------------------------------------------------------------
    // Traps and interrupts
    // ------------------------------------------------------------------

    /// Deliver a trap (exception or interrupt) at the current pc.
    pub fn take_trap(&mut self, trap: Trap, interrupt: bool) {
        self.cycles += cost::TRAP;
        self.wfi = false;
        let deleg = if interrupt { self.mideleg } else { self.medeleg };
        let cause_val = trap.cause | if interrupt { 1 << 63 } else { 0 };
        if self.prv <= Priv::Supervisor && (deleg >> trap.cause) & 1 == 1 {
            self.sepc = self.pc;
            self.scause = cause_val;
            self.stval = trap.tval;
            let sie = self.mstatus & csr::MSTATUS_SIE != 0;
            self.mstatus &= !(csr::MSTATUS_SPIE | csr::MSTATUS_SIE | csr::MSTATUS_SPP);
            if sie {
                self.mstatus |= csr::MSTATUS_SPIE;
            }
            if self.prv == Priv::Supervisor {
                self.mstatus |= csr::MSTATUS_SPP;
            }
            self.prv = Priv::Supervisor;
            let base = self.stvec & !3;
            self.pc = if interrupt && self.stvec & 1 == 1 {
                base + 4 * trap.cause
            } else {
                base
            };
        } else {
            self.mepc = self.pc;
            self.mcause = cause_val;
            self.mtval = trap.tval;
            let mie = self.mstatus & csr::MSTATUS_MIE != 0;
            self.mstatus &= !(csr::MSTATUS_MPIE | csr::MSTATUS_MIE | csr::MSTATUS_MPP);
            if mie {
                self.mstatus |= csr::MSTATUS_MPIE;
            }
            self.mstatus |= (self.prv as u64) << csr::MSTATUS_MPP_SHIFT;
            self.prv = Priv::Machine;
            let base = self.mtvec & !3;
            self.pc = if interrupt && self.mtvec & 1 == 1 {
                base + 4 * trap.cause
            } else {
                base
            };
        }
    }

    /// Highest-priority interrupt that should be taken now, if any.
    pub fn pending_interrupt(&self) -> Option<u64> {
        let pending = self.mip() & self.mie;
        if pending == 0 {
            return None;
        }
        let m_enabled = self.prv < Priv::Machine || self.mstatus & csr::MSTATUS_MIE != 0;
        let s_enabled = self.prv < Priv::Supervisor || (self.prv == Priv::Supervisor && self.mstatus & csr::MSTATUS_SIE != 0);
        let m_pending = if m_enabled { pending & !self.mideleg } else { 0 };
        let s_pending = if s_enabled { pending & self.mideleg } else { 0 };
        const ORDER: [u64; 6] = [csr::IRQ_MEI, csr::IRQ_MSI, csr::IRQ_MTI, csr::IRQ_SEI, csr::IRQ_SSI, csr::IRQ_STI];
        for set in [m_pending, s_pending] {
            if set != 0 {
                for irq in ORDER {
                    if set & (1 << irq) != 0 {
                        return Some(irq);
                    }
                }
            }
        }
        None
    }

    // ------------------------------------------------------------------
    // CSRs
    // ------------------------------------------------------------------

    fn counter_allowed(&self, idx: u16) -> bool {
        let bit = 1u64 << (idx & 31);
        match self.prv {
            Priv::Machine => true,
            Priv::Supervisor => self.mcounteren & bit != 0,
            Priv::User => self.mcounteren & bit != 0 && self.scounteren & bit != 0,
        }
    }

    pub fn csr_read(&mut self, addr: u16) -> Result<u64, CsrError> {
        if (addr >> 8) & 3 > self.prv as u16 {
            return Err(CsrError);
        }
        use csr::*;
        Ok(match addr {
            SSTATUS => self.mstatus & SSTATUS_MASK,
            SIE => self.mie & self.mideleg,
            STVEC => self.stvec,
            SCOUNTEREN => self.scounteren,
            SENVCFG => self.senvcfg,
            SSCRATCH => self.sscratch,
            SEPC => self.sepc,
            SCAUSE => self.scause,
            STVAL => self.stval,
            SIP => self.mip() & self.mideleg,
            SATP => {
                if self.prv == Priv::Supervisor && self.mstatus & MSTATUS_TVM != 0 {
                    return Err(CsrError);
                }
                self.satp
            }
            MSTATUS => self.mstatus,
            MISA => MISA_VALUE,
            MEDELEG => self.medeleg,
            MIDELEG => self.mideleg,
            MIE => self.mie,
            MTVEC => self.mtvec,
            MCOUNTEREN => self.mcounteren,
            MENVCFG => self.menvcfg,
            MCOUNTINHIBIT => self.mcountinhibit,
            MSCRATCH => self.mscratch,
            MEPC => self.mepc,
            MCAUSE => self.mcause,
            MTVAL => self.mtval,
            MIP => self.mip(),
            0x3a0..=0x3af => {
                if addr & 1 == 1 {
                    return Err(CsrError); // odd pmpcfg registers do not exist on RV64
                }
                self.pmpcfg[(addr - PMPCFG0) as usize]
            }
            0x3b0..=0x3ef => self.pmpaddr[(addr - PMPADDR0) as usize],
            TSELECT => 0,
            TDATA1..=TDATA3 => 0,
            MCYCLE => self.cycles.wrapping_add(self.mcycle_offset),
            MINSTRET => self.instret,
            0xb03..=0xb1f | 0x323..=0x33f => 0,
            CYCLE | TIME | INSTRET | 0xc03..=0xc1f => {
                if !self.counter_allowed(addr - CYCLE) {
                    return Err(CsrError);
                }
                match addr {
                    CYCLE => self.cycles.wrapping_add(self.mcycle_offset),
                    TIME => self.time,
                    INSTRET => self.instret,
                    _ => 0,
                }
            }
            MVENDORID | MARCHID | MIMPID | MHARTID | MCONFIGPTR => 0,
            _ => return Err(CsrError),
        })
    }

    pub fn csr_write(&mut self, addr: u16, val: u64) -> Result<(), CsrError> {
        if (addr >> 8) & 3 > self.prv as u16 || addr >> 10 == 3 {
            return Err(CsrError);
        }
        use csr::*;
        match addr {
            SSTATUS => self.mstatus = (self.mstatus & !SSTATUS_MASK) | (val & SSTATUS_MASK & !(3 << 32)),
            SIE => self.mie = (self.mie & !self.mideleg) | (val & self.mideleg),
            STVEC => self.stvec = val & !2,
            SCOUNTEREN => self.scounteren = val & 0xffff_ffff,
            SENVCFG => self.senvcfg = val & 1,
            SSCRATCH => self.sscratch = val,
            SEPC => self.sepc = val & !1,
            SCAUSE => self.scause = val,
            STVAL => self.stval = val,
            SIP => {
                let m = self.mideleg & MIP_SSIP;
                self.mip_sw = (self.mip_sw & !m) | (val & m);
            }
            SATP => {
                if self.prv == Priv::Supervisor && self.mstatus & MSTATUS_TVM != 0 {
                    return Err(CsrError);
                }
                let mode = val >> 60;
                if mode == 0 || mode == 8 {
                    self.satp = val & !(0xffffu64 << 44); // ASIDLEN = 0
                    self.tlb.flush();
                }
            }
            MSTATUS => {
                let mut v = (self.mstatus & !MSTATUS_WMASK) | (val & MSTATUS_WMASK);
                if (v >> MSTATUS_MPP_SHIFT) & 3 == 2 {
                    v &= !MSTATUS_MPP; // reserved encoding -> U (WARL)
                }
                self.mstatus = v;
            }
            MISA => {}
            MEDELEG => self.medeleg = val & 0xb3ff,
            MIDELEG => self.mideleg = val & S_INTS,
            MIE => self.mie = val & ALL_INTS,
            MTVEC => self.mtvec = val & !2,
            MCOUNTEREN => self.mcounteren = val & 0xffff_ffff,
            MENVCFG => self.menvcfg = val & 1,
            MCOUNTINHIBIT => self.mcountinhibit = val & 0xffff_fffd,
            MSCRATCH => self.mscratch = val,
            MEPC => self.mepc = val & !1,
            MCAUSE => self.mcause = val,
            MTVAL => self.mtval = val,
            MIP => self.mip_sw = (self.mip_sw & !S_INTS) | (val & S_INTS),
            0x3a0..=0x3af => {
                if addr & 1 == 1 {
                    return Err(CsrError);
                }
                self.pmpcfg[(addr - PMPCFG0) as usize] = val;
            }
            0x3b0..=0x3ef => self.pmpaddr[(addr - PMPADDR0) as usize] = val & ((1 << 54) - 1),
            TSELECT | TDATA1..=TDATA3 => {}
            MCYCLE => self.mcycle_offset = val.wrapping_sub(self.cycles),
            MINSTRET => {
                self.instret = val;
                self.suppress_instret = true;
            }
            0xb03..=0xb1f | 0x323..=0x33f => {}
            _ => return Err(CsrError),
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // Execution
    // ------------------------------------------------------------------

    /// Execute one instruction (or take one pending interrupt).
    /// Returns the decoded instruction that retired, if any.
    pub fn step(&mut self, bus: &mut Bus) -> Option<Inst> {
        if let Some(irq) = self.pending_interrupt() {
            self.take_trap(Trap::new(irq, 0), true);
            return None;
        }
        if self.wfi {
            self.cycles += 1;
            return None;
        }
        let raw = match self.fetch(bus) {
            Ok(r) => r,
            Err(t) => {
                self.take_trap(t, false);
                return None;
            }
        };
        let inst = self.dcache.get(raw);
        match self.execute(bus, &inst) {
            Ok(c) => {
                self.cycles += c;
                if !self.suppress_instret && self.mcountinhibit & 4 == 0 {
                    self.instret = self.instret.wrapping_add(1);
                }
                self.suppress_instret = false;
                Some(inst)
            }
            Err(t) => {
                self.take_trap(t, false);
                None
            }
        }
    }

    fn illegal(inst: &Inst) -> Trap {
        Trap::new(cause::ILLEGAL, inst.raw as u64)
    }

    /// Execute a decoded instruction, returning its cycle cost.
    pub fn execute(&mut self, bus: &mut Bus, i: &Inst) -> Result<u64, Trap> {
        let rs1 = self.x[i.rs1 as usize];
        let rs2 = self.x[i.rs2 as usize];
        let imm = i.imm as u64;
        let next = self.pc.wrapping_add(i.len as u64);
        let mut cyc = cost::ALU;
        let rd = i.rd;
        macro_rules! wr {
            ($v:expr) => {{
                let v = $v;
                self.set(rd, v);
            }};
        }
        macro_rules! branch {
            ($cond:expr) => {{
                if $cond {
                    self.pc = self.pc.wrapping_add(imm);
                    return Ok(cost::BRANCH_TAKEN);
                }
                cyc = cost::BRANCH_NOT_TAKEN;
            }};
        }
        match i.op {
            Op::Lui => wr!(imm),
            Op::Auipc => wr!(self.pc.wrapping_add(imm)),
            Op::Jal => {
                let target = self.pc.wrapping_add(imm);
                wr!(next);
                self.pc = target;
                return Ok(cost::JAL);
            }
            Op::Jalr => {
                let target = rs1.wrapping_add(imm) & !1;
                wr!(next);
                self.pc = target;
                return Ok(cost::JALR);
            }
            Op::Beq => branch!(rs1 == rs2),
            Op::Bne => branch!(rs1 != rs2),
            Op::Blt => branch!((rs1 as i64) < (rs2 as i64)),
            Op::Bge => branch!((rs1 as i64) >= (rs2 as i64)),
            Op::Bltu => branch!(rs1 < rs2),
            Op::Bgeu => branch!(rs1 >= rs2),
            Op::Lb | Op::Lh | Op::Lw | Op::Ld | Op::Lbu | Op::Lhu | Op::Lwu => {
                let addr = rs1.wrapping_add(imm);
                let v = match i.op {
                    Op::Lb => self.load(bus, addr, 1)? as i8 as i64 as u64,
                    Op::Lh => self.load(bus, addr, 2)? as i16 as i64 as u64,
                    Op::Lw => self.load(bus, addr, 4)? as i32 as i64 as u64,
                    Op::Ld => self.load(bus, addr, 8)?,
                    Op::Lbu => self.load(bus, addr, 1)?,
                    Op::Lhu => self.load(bus, addr, 2)?,
                    _ => self.load(bus, addr, 4)?,
                };
                wr!(v);
                cyc = cost::LOAD;
            }
            Op::Sb => {
                self.store(bus, rs1.wrapping_add(imm), 1, rs2)?;
                cyc = cost::STORE;
            }
            Op::Sh => {
                self.store(bus, rs1.wrapping_add(imm), 2, rs2)?;
                cyc = cost::STORE;
            }
            Op::Sw => {
                self.store(bus, rs1.wrapping_add(imm), 4, rs2)?;
                cyc = cost::STORE;
            }
            Op::Sd => {
                self.store(bus, rs1.wrapping_add(imm), 8, rs2)?;
                cyc = cost::STORE;
            }
            Op::Addi => wr!(rs1.wrapping_add(imm)),
            Op::Slti => wr!(alu::slt(rs1, imm)),
            Op::Sltiu => wr!(alu::sltu(rs1, imm)),
            Op::Xori => wr!(rs1 ^ imm),
            Op::Ori => wr!(rs1 | imm),
            Op::Andi => wr!(rs1 & imm),
            Op::Slli => wr!(alu::sll(rs1, imm)),
            Op::Srli => wr!(alu::srl(rs1, imm)),
            Op::Srai => wr!(alu::sra(rs1, imm)),
            Op::Add => wr!(rs1.wrapping_add(rs2)),
            Op::Sub => wr!(rs1.wrapping_sub(rs2)),
            Op::Sll => wr!(alu::sll(rs1, rs2)),
            Op::Slt => wr!(alu::slt(rs1, rs2)),
            Op::Sltu => wr!(alu::sltu(rs1, rs2)),
            Op::Xor => wr!(rs1 ^ rs2),
            Op::Srl => wr!(alu::srl(rs1, rs2)),
            Op::Sra => wr!(alu::sra(rs1, rs2)),
            Op::Or => wr!(rs1 | rs2),
            Op::And => wr!(rs1 & rs2),
            Op::Addiw => wr!(alu::sext32(rs1.wrapping_add(imm))),
            Op::Slliw => wr!(alu::sllw(rs1, imm)),
            Op::Srliw => wr!(alu::srlw(rs1, imm)),
            Op::Sraiw => wr!(alu::sraw(rs1, imm)),
            Op::Addw => wr!(alu::sext32(rs1.wrapping_add(rs2))),
            Op::Subw => wr!(alu::sext32(rs1.wrapping_sub(rs2))),
            Op::Sllw => wr!(alu::sllw(rs1, rs2)),
            Op::Srlw => wr!(alu::srlw(rs1, rs2)),
            Op::Sraw => wr!(alu::sraw(rs1, rs2)),
            Op::Mul => {
                wr!(rs1.wrapping_mul(rs2));
                cyc = cost::MUL;
            }
            Op::Mulh => {
                wr!(alu::mulh(rs1, rs2));
                cyc = cost::MULH;
            }
            Op::Mulhsu => {
                wr!(alu::mulhsu(rs1, rs2));
                cyc = cost::MULH;
            }
            Op::Mulhu => {
                wr!(alu::mulhu(rs1, rs2));
                cyc = cost::MULH;
            }
            Op::Mulw => {
                wr!(alu::mulw(rs1, rs2));
                cyc = cost::MUL;
            }
            Op::Div => {
                wr!(alu::div(rs1, rs2));
                cyc = cost::DIV;
            }
            Op::Divu => {
                wr!(alu::divu(rs1, rs2));
                cyc = cost::DIV;
            }
            Op::Rem => {
                wr!(alu::rem(rs1, rs2));
                cyc = cost::DIV;
            }
            Op::Remu => {
                wr!(alu::remu(rs1, rs2));
                cyc = cost::DIV;
            }
            Op::Divw => {
                wr!(alu::divw(rs1, rs2));
                cyc = cost::DIVW;
            }
            Op::Divuw => {
                wr!(alu::divuw(rs1, rs2));
                cyc = cost::DIVW;
            }
            Op::Remw => {
                wr!(alu::remw(rs1, rs2));
                cyc = cost::DIVW;
            }
            Op::Remuw => {
                wr!(alu::remuw(rs1, rs2));
                cyc = cost::DIVW;
            }
            Op::Fence => {}
            Op::FenceI => cyc = cost::FENCE_I,
            Op::Ecall => {
                let c = match self.prv {
                    Priv::User => cause::ECALL_U,
                    Priv::Supervisor => cause::ECALL_S,
                    Priv::Machine => cause::ECALL_M,
                };
                return Err(Trap::new(c, 0));
            }
            Op::Ebreak => return Err(Trap::new(cause::BREAKPOINT, self.pc)),
            Op::Mret => {
                if self.prv != Priv::Machine {
                    return Err(Self::illegal(i));
                }
                let mpp = Priv::from_bits(self.mstatus >> csr::MSTATUS_MPP_SHIFT);
                let mpie = self.mstatus & csr::MSTATUS_MPIE != 0;
                self.mstatus &= !(csr::MSTATUS_MIE | csr::MSTATUS_MPP);
                if mpie {
                    self.mstatus |= csr::MSTATUS_MIE;
                }
                self.mstatus |= csr::MSTATUS_MPIE;
                if mpp != Priv::Machine {
                    self.mstatus &= !csr::MSTATUS_MPRV;
                }
                self.prv = mpp;
                self.pc = self.mepc;
                return Ok(cost::XRET);
            }
            Op::Sret => {
                if self.prv < Priv::Supervisor || (self.prv == Priv::Supervisor && self.mstatus & csr::MSTATUS_TSR != 0) {
                    return Err(Self::illegal(i));
                }
                let spp = if self.mstatus & csr::MSTATUS_SPP != 0 {
                    Priv::Supervisor
                } else {
                    Priv::User
                };
                let spie = self.mstatus & csr::MSTATUS_SPIE != 0;
                self.mstatus &= !(csr::MSTATUS_SIE | csr::MSTATUS_SPP | csr::MSTATUS_MPRV);
                if spie {
                    self.mstatus |= csr::MSTATUS_SIE;
                }
                self.mstatus |= csr::MSTATUS_SPIE;
                self.prv = spp;
                self.pc = self.sepc;
                return Ok(cost::XRET);
            }
            Op::Wfi => {
                if self.prv < Priv::Machine && self.mstatus & csr::MSTATUS_TW != 0 {
                    return Err(Self::illegal(i));
                }
                if self.prv == Priv::User {
                    return Err(Self::illegal(i));
                }
                if self.mip() & self.mie == 0 {
                    self.wfi = true;
                }
            }
            Op::SfenceVma => {
                if self.prv < Priv::Supervisor || (self.prv == Priv::Supervisor && self.mstatus & csr::MSTATUS_TVM != 0) {
                    return Err(Self::illegal(i));
                }
                if i.rs1 == 0 {
                    self.tlb.flush();
                } else {
                    self.tlb.flush_page(rs1);
                }
            }
            Op::Csrrw | Op::Csrrs | Op::Csrrc | Op::Csrrwi | Op::Csrrsi | Op::Csrrci => {
                let src = match i.op {
                    Op::Csrrw | Op::Csrrs | Op::Csrrc => rs1,
                    _ => i.imm as u64,
                };
                let write = !matches!(i.op, Op::Csrrs | Op::Csrrc | Op::Csrrsi | Op::Csrrci) || i.rs1 != 0;
                let read = !(matches!(i.op, Op::Csrrw | Op::Csrrwi) && rd == 0);
                let old = if read || write {
                    match self.csr_read(i.csr) {
                        Ok(v) => v,
                        Err(CsrError) => return Err(Self::illegal(i)),
                    }
                } else {
                    0
                };
                if write {
                    let new = match i.op {
                        Op::Csrrw | Op::Csrrwi => src,
                        Op::Csrrs | Op::Csrrsi => old | src,
                        _ => old & !src,
                    };
                    if self.csr_write(i.csr, new).is_err() {
                        return Err(Self::illegal(i));
                    }
                }
                wr!(old);
                cyc = cost::CSR;
            }
            Op::LrW | Op::LrD => {
                let len = if i.op == Op::LrW { 4 } else { 8 };
                if !rs1.is_multiple_of(len) {
                    return Err(Trap::new(cause::LOAD_MISALIGNED, rs1));
                }
                let pa = self.translate(bus, rs1, Access::Load)?;
                let v = bus.read(pa, len).map_err(|_| Trap::new(cause::LOAD_ACCESS, rs1))?;
                wr!(if len == 4 { alu::sext32(v) } else { v });
                self.reservation = Some(pa);
                cyc = cost::ATOMIC;
            }
            Op::ScW | Op::ScD => {
                let len = if i.op == Op::ScW { 4 } else { 8 };
                if !rs1.is_multiple_of(len) {
                    return Err(Trap::new(cause::STORE_MISALIGNED, rs1));
                }
                let pa = self.translate(bus, rs1, Access::Store)?;
                if self.reservation == Some(pa) {
                    bus.write(pa, len, rs2).map_err(|_| Trap::new(cause::STORE_ACCESS, rs1))?;
                    wr!(0);
                } else {
                    wr!(1);
                }
                self.reservation = None;
                cyc = cost::ATOMIC;
            }
            Op::AmoswapW => cyc = amo!(self, bus, i, 4, |_, b| b),
            Op::AmoaddW => cyc = amo!(self, bus, i, 4, |a, b| a.wrapping_add(b)),
            Op::AmoxorW => cyc = amo!(self, bus, i, 4, |a, b| a ^ b),
            Op::AmoandW => cyc = amo!(self, bus, i, 4, |a, b| a & b),
            Op::AmoorW => cyc = amo!(self, bus, i, 4, |a, b| a | b),
            Op::AmominW => cyc = amo!(self, bus, i, 4, |a, b| (a as i32).min(b as i32) as u64),
            Op::AmomaxW => cyc = amo!(self, bus, i, 4, |a, b| (a as i32).max(b as i32) as u64),
            Op::AmominuW => cyc = amo!(self, bus, i, 4, |a, b| (a as u32).min(b as u32) as u64),
            Op::AmomaxuW => cyc = amo!(self, bus, i, 4, |a, b| (a as u32).max(b as u32) as u64),
            Op::AmoswapD => cyc = amo!(self, bus, i, 8, |_, b| b),
            Op::AmoaddD => cyc = amo!(self, bus, i, 8, |a, b| a.wrapping_add(b)),
            Op::AmoxorD => cyc = amo!(self, bus, i, 8, |a, b| a ^ b),
            Op::AmoandD => cyc = amo!(self, bus, i, 8, |a, b| a & b),
            Op::AmoorD => cyc = amo!(self, bus, i, 8, |a, b| a | b),
            Op::AmominD => cyc = amo!(self, bus, i, 8, |a, b| (a as i64).min(b as i64) as u64),
            Op::AmomaxD => cyc = amo!(self, bus, i, 8, |a, b| (a as i64).max(b as i64) as u64),
            Op::AmominuD => cyc = amo!(self, bus, i, 8, |a, b| a.min(b)),
            Op::AmomaxuD => cyc = amo!(self, bus, i, 8, |a, b| a.max(b)),
            Op::Illegal => return Err(Self::illegal(i)),
        }
        self.pc = next;
        Ok(cyc)
    }
}
