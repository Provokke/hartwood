//! Privileged-architecture tests: CSRs, traps, delegation, interrupts
//! (CLINT timer/software, PLIC+UART external), WFI, Sv39 paging + TLB, LR/SC.

mod common;
use common::*;
use hartwood::csr;
use hartwood::decode::enc_i;
use hartwood::Exit;

const HANDLER: u64 = BASE + 0x800;

/// Program whose M-mode trap handler (at HANDLER) just signals "pass".
fn with_handler(body: impl FnOnce(&mut Asm)) -> Asm {
    let mut a = Asm::new();
    a.li(r::T0, HANDLER).csrrw(0, csr::MTVEC, r::T0);
    body(&mut a);
    a.li(r::A0, (7 << 16) | 0x3333).finish(); // fell through: fail(7)
    assert!(a.pc() <= HANDLER, "program too long");
    while a.pc() < HANDLER {
        a.w(0x13);
    }
    a.pass();
    a
}

/// mret into `mode` at the instruction right after the mret sequence.
fn drop_to(a: &mut Asm, mpp: u64) {
    a.li(r::T0, mpp << 11).csrrw(0, csr::MSTATUS, r::T0);
    let target = a.pc() + 4 * 4; // auipc, addi, csrw, mret
    a.w(hartwood::decode::enc_u(0, r::T1, 0x17)); // auipc t1, 0
    a.addi(r::T1, r::T1, 16);
    a.csrrw(0, csr::MEPC, r::T1);
    a.mret();
    assert_eq!(a.pc(), target);
}

#[test]
fn illegal_csr_access_from_user_mode_traps_to_m() {
    let mut csr_inst = 0;
    let prog = with_handler(|a| {
        drop_to(a, 0);
        csr_inst = enc_i(csr::MSTATUS as i64, 0, 2, r::A1, 0x73);
        a.w(csr_inst); // csrr a1, mstatus  (U-mode -> illegal)
    });
    let (m, e) = run(&prog);
    assert_eq!(e, PASS);
    assert_eq!(m.cpu.mcause, csr::cause::ILLEGAL);
    assert_eq!(m.cpu.mtval, csr_inst as u64, "mtval holds the faulting encoding");
    assert_eq!((m.cpu.mstatus >> 11) & 3, 0, "MPP records U-mode");
}

#[test]
fn ecall_delegation_u_to_s_then_s_to_m() {
    // S-mode handler at BASE+0x600: records state then ecalls into M.
    let s_handler = BASE + 0x600;
    let mut prog = with_handler(|a| {
        a.li(r::T0, 1 << 8).csrrw(0, csr::MEDELEG, r::T0); // delegate U-ecall
        a.li(r::T0, s_handler).csrrw(0, csr::STVEC, r::T0);
        drop_to(a, 0);
        a.li(r::A7, 93);
        a.ecall();
    });
    let idx = ((s_handler - BASE) / 4) as usize;
    let mut s = Asm::new();
    s.csrrs(r::A2, csr::SCAUSE, 0).csrrs(r::A3, csr::SEPC, 0).ecall();
    prog.words[idx..idx + s.words.len()].copy_from_slice(&s.words);
    let (m, e) = run(&prog);
    assert_eq!(e, PASS);
    assert_eq!(m.cpu.x[12], csr::cause::ECALL_U, "scause in S handler");
    assert_eq!(m.cpu.mcause, csr::cause::ECALL_S, "S-mode ecall reached M");
    assert_eq!(m.cpu.mepc, s_handler + 8);
    assert_ne!(m.cpu.x[13], 0, "sepc recorded");
    assert_eq!(m.cpu.mstatus & csr::MSTATUS_SPP, 0, "SPP = U");
}

#[test]
fn breakpoint_and_misaligned_atomic_causes() {
    let (m, e) = run(&with_handler(|a| {
        a.w(0x0010_0073); // ebreak
    }));
    assert_eq!(e, PASS);
    assert_eq!(m.cpu.mcause, csr::cause::BREAKPOINT);
    assert_eq!(m.cpu.mtval, m.cpu.mepc, "tval = pc of ebreak");

    let (m, e) = run(&with_handler(|a| {
        a.li(r::A1, BASE + 0x1001);
        a.r(0x01 << 2, 3, 0x2f, r::A0, r::A1, r::A2); // amoswap.d a0, a2, (a1) misaligned
    }));
    assert_eq!(e, PASS);
    assert_eq!(m.cpu.mcause, csr::cause::STORE_MISALIGNED);
    assert_eq!(m.cpu.mtval, BASE + 0x1001);
}

#[test]
fn timer_interrupt_wakes_wfi_and_time_tracks_cycles() {
    let prog = with_handler(|a| {
        a.li(r::T0, 0x0200_4000).li(r::T1, 5000).sd(r::T1, r::T0, 0); // mtimecmp = 5000
        a.li(r::T0, csr::MIP_MTIP).csrrw(0, csr::MIE, r::T0);
        a.li(r::T0, csr::MSTATUS_MIE).csrrw(0, csr::MSTATUS, r::T0);
        a.wfi();
    });
    let (m, e) = run(&prog);
    assert_eq!(e, PASS);
    assert_eq!(m.cpu.mcause, (1 << 63) | csr::IRQ_MTI);
    assert!(m.bus.clint.mtime >= 5000);
    // mtime = cycles / cycles_per_tick (10)
    assert!((m.cpu.cycles / 10).abs_diff(m.bus.clint.mtime) <= 2);
    // WFI fast-forwards: far fewer instructions than ticks*10 cycles.
    assert!(m.cpu.instret < 100, "instret {}", m.cpu.instret);
}

#[test]
fn wfi_wakes_without_global_enable_and_does_not_trap() {
    let prog = with_handler(|a| {
        a.li(r::T0, 0x0200_4000).li(r::T1, 300).sd(r::T1, r::T0, 0);
        a.li(r::T0, csr::MIP_MTIP).csrrw(0, csr::MIE, r::T0); // MIE (global) stays 0
        a.wfi();
        a.li(r::A0, 0x5555).finish();
    });
    let (m, e) = run(&prog);
    assert_eq!(e, PASS);
    assert_eq!(m.cpu.mcause, 0, "no trap was taken");
    assert!(m.cpu.mip() & csr::MIP_MTIP != 0);
}

#[test]
fn wfi_with_no_wakeup_source_halts() {
    let prog = with_handler(|a| {
        a.wfi();
    });
    let (_, e) = run(&prog);
    assert_eq!(e, Exit::Halted);
}

#[test]
fn software_interrupt_via_clint_msip() {
    let prog = with_handler(|a| {
        a.li(r::T0, csr::MIP_MSIP).csrrw(0, csr::MIE, r::T0);
        a.li(r::T0, csr::MSTATUS_MIE).csrrw(0, csr::MSTATUS, r::T0);
        a.li(r::T0, 0x0200_0000)
            .li(r::T1, 1)
            .w(hartwood::decode::enc_s(0, r::T1, r::T0, 2, 0x23));
        a.addi(0, 0, 0);
    });
    let (m, e) = run(&prog);
    assert_eq!(e, PASS);
    assert_eq!(m.cpu.mcause, (1 << 63) | csr::IRQ_MSI);
}

#[test]
fn uart_rx_raises_plic_external_interrupt() {
    let plic = 0x0c00_0000u64;
    let prog = with_handler(|a| {
        let sw = |a: &mut Asm, addr: u64, v: u64| {
            a.li(r::T0, addr).li(r::T1, v).w(hartwood::decode::enc_s(0, r::T1, r::T0, 2, 0x23));
        };
        sw(a, plic + 4 * 10, 1); // priority[10] = 1
        sw(a, plic + 0x2000, 1 << 10); // enable source 10 for context 0 (M)
        sw(a, plic + 0x20_0000, 0); // threshold 0
        a.li(r::T0, 0x1000_0001)
            .li(r::T1, 1)
            .w(hartwood::decode::enc_s(0, r::T1, r::T0, 0, 0x23)); // IER = RDA
        a.li(r::T0, csr::MIP_MEIP).csrrw(0, csr::MIE, r::T0);
        a.li(r::T0, csr::MSTATUS_MIE).csrrw(0, csr::MSTATUS, r::T0);
        a.wfi();
    });
    // Replace the generic handler with one that claims the interrupt.
    let mut prog = prog;
    let idx = ((HANDLER - BASE) / 4) as usize;
    let mut h = Asm::new();
    h.li(r::T0, plic + 0x20_0004).w(enc_i(0, r::T0, 2, r::A1, 0x03)); // lw a1, claim
    h.li(r::T0, 0x1000_0000).w(enc_i(0, r::T0, 4, r::A2, 0x03)); // lbu a2, RBR
    h.pass();
    prog.words.truncate(idx);
    prog.words.extend(&h.words);
    let mut m = machine(&prog);
    m.bus.uart.rx.push_back(b'Z');
    m.bus.sync_uart_irq();
    let e = m.run();
    assert_eq!(e, PASS);
    assert_eq!(m.cpu.mcause, (1 << 63) | csr::IRQ_MEI);
    assert_eq!(m.cpu.x[11], 10, "claimed source = UART");
    assert_eq!(m.cpu.x[12], b'Z' as u64);
}

#[test]
fn lr_sc_reservation_semantics() {
    let prog = with_handler(|a| {
        a.li(r::A1, BASE + 0x2000);
        a.r(0x03 << 2, 3, 0x2f, r::A2, r::A1, r::A3); // sc.d a2, a3, (a1) without reservation -> 1
        a.r(0x02 << 2, 3, 0x2f, r::A0, r::A1, 0); // lr.d a0, (a1)
        a.li(r::A3, 77);
        a.r(0x03 << 2, 3, 0x2f, 8, r::A1, r::A3); // sc.d s0 -> 0 (success)
        a.r(0x03 << 2, 3, 0x2f, 9, r::A1, r::A3); // sc.d s1 -> 1 (reservation consumed)
        a.ld(r::A3, r::A1, 0);
        a.ecall();
    });
    let (m, e) = run(&prog);
    assert_eq!(e, PASS);
    assert_eq!(m.cpu.x[12], 1);
    assert_eq!(m.cpu.x[8], 0);
    assert_eq!(m.cpu.x[9], 1);
    assert_eq!(m.cpu.x[13], 77);
}

#[test]
fn counters_and_csr_warl_behaviour() {
    let prog = with_handler(|a| {
        a.csrrs(r::A1, csr::MISA, 0);
        a.li(r::T0, u64::MAX).csrrw(0, csr::MSTATUS, r::T0);
        a.csrrs(r::A2, csr::MSTATUS, 0);
        a.li(r::T0, u64::MAX).csrrw(0, csr::MIDELEG, r::T0);
        a.csrrs(r::A3, csr::MIDELEG, 0);
        a.csrrs(8, csr::MINSTRET, 0);
        a.csrrs(9, csr::MINSTRET, 0);
        a.ecall();
    });
    let (m, e) = run(&prog);
    assert_eq!(e, PASS);
    assert_eq!(m.cpu.x[11], csr::MISA_VALUE);
    assert_eq!(m.cpu.x[11] >> 62, 2, "RV64");
    let ms = m.cpu.x[12];
    assert_eq!((ms >> 32) & 0xf, 0xa, "UXL/SXL fixed at 64-bit");
    assert_eq!(ms & !(csr::MSTATUS_WMASK | csr::MSTATUS_UXL_SXL), 0, "only writable bits stick");
    assert_eq!(m.cpu.x[13], csr::S_INTS, "only S interrupts delegable");
    assert_eq!(m.cpu.x[9] - m.cpu.x[8], 1, "minstret counts retired instructions");
}

// ----------------------------- Sv39 paging ---------------------------------

const ROOT: u64 = 0x8020_0000;
const L1: u64 = 0x8020_1000;
const L0: u64 = 0x8020_2000;
const DATA_RW: u64 = 0x8030_0000;
const DATA_RO: u64 = 0x8030_1000;
const DATA_U: u64 = 0x8030_2000;
const V: u64 = 1;
const R: u64 = 2;
const W: u64 = 4;
const X: u64 = 8;
const U: u64 = 16;
const A: u64 = 64;
const D: u64 = 128;

fn pte(pa: u64, flags: u64) -> u64 {
    ((pa >> 12) << 10) | flags | V
}

fn paging_machine(prog: &Asm, mstatus_extra: u64) -> hartwood::Machine {
    let mut m = machine(prog);
    let ram = &mut m.bus.ram;
    ram.write(ROOT + 2 * 8, 8, pte(0x8000_0000, R | W | X | A | D)); // gigapage identity (S only)
    ram.write(ROOT + 8, 8, pte(L1, 0));
    ram.write(L1, 8, pte(L0, 0));
    ram.write(L0, 8, pte(DATA_RW, R | W)); // VA 0x4000_0000, A/D clear
    ram.write(L0 + 8, 8, pte(DATA_RO, R | A)); // VA 0x4000_1000
    ram.write(L0 + 16, 8, pte(DATA_U, R | W | U | A | D)); // VA 0x4000_2000
    let _ = mstatus_extra;
    m
}

fn enter_s_with_paging(a: &mut Asm, mstatus_extra: u64) {
    a.li(r::T0, (8 << 60) | (ROOT >> 12)).csrrw(0, csr::SATP, r::T0);
    a.li(r::T0, (1 << 11) | mstatus_extra).csrrw(0, csr::MSTATUS, r::T0);
    let target = a.pc() + 16;
    a.w(hartwood::decode::enc_u(0, r::T1, 0x17));
    a.addi(r::T1, r::T1, 16);
    a.csrrw(0, csr::MEPC, r::T1);
    a.mret();
    assert_eq!(a.pc(), target);
}

#[test]
fn sv39_translation_ad_bits_and_store_fault() {
    let prog = with_handler(|a| {
        enter_s_with_paging(a, 0);
        a.li(r::A1, 0x4000_0000).li(r::A2, 0x1234).sd(r::A2, r::A1, 8);
        a.ld(r::A3, r::A1, 8);
        a.li(r::A1, 0x4000_1000).ld(15, r::A1, 0); // read-only page: load OK
        a.sd(r::A2, r::A1, 0); // store -> page fault
    });
    let mut m = paging_machine(&prog, 0);
    m.bus.ram.write(DATA_RO, 8, 0xfeed);
    let e = m.run();
    assert_eq!(e, PASS);
    assert_eq!(m.cpu.x[13], 0x1234, "load through translation");
    assert_eq!(m.bus.ram.read(DATA_RW + 8, 8), 0x1234, "store hit the right physical page");
    assert_eq!(m.cpu.x[15], 0xfeed);
    assert_eq!(m.cpu.mcause, csr::cause::STORE_PAGE);
    assert_eq!(m.cpu.mtval, 0x4000_1000);
    let pte_rw = m.bus.ram.read(L0, 8);
    assert_eq!(pte_rw & (A | D), A | D, "hardware set A and D");
    assert_eq!(m.bus.ram.read(L0 + 8, 8) & D, 0, "faulting store did not set D");
    assert!(m.cpu.tlb.misses > 0);
}

#[test]
fn sv39_sum_controls_supervisor_access_to_user_pages() {
    let body = |a: &mut Asm, extra| {
        enter_s_with_paging(a, extra);
        a.li(r::A1, 0x4000_2000).ld(15, r::A1, 0);
        a.ecall();
    };
    let (prog_nosum, prog_sum) = (with_handler(|a| body(a, 0)), with_handler(|a| body(a, csr::MSTATUS_SUM)));
    let mut m = paging_machine(&prog_nosum, 0);
    assert_eq!(m.run(), PASS);
    assert_eq!(m.cpu.mcause, csr::cause::LOAD_PAGE);
    let mut m = paging_machine(&prog_sum, 0);
    m.bus.ram.write(DATA_U, 8, 99);
    assert_eq!(m.run(), PASS);
    assert_eq!(m.cpu.mcause, csr::cause::ECALL_S);
    assert_eq!(m.cpu.x[15], 99);
}

#[test]
fn sv39_unmapped_and_noncanonical_addresses_fault() {
    for (va, cause) in [
        (0x4000_3000u64, csr::cause::LOAD_PAGE),
        (0x0000_0080_0000_0000, csr::cause::LOAD_PAGE),
    ] {
        let prog = with_handler(|a| {
            enter_s_with_paging(a, 0);
            a.li(r::A1, va).ld(r::A0, r::A1, 0);
        });
        let mut m = paging_machine(&prog, 0);
        assert_eq!(m.run(), PASS);
        assert_eq!(m.cpu.mcause, cause, "va {va:#x}");
        assert_eq!(m.cpu.mtval, va);
    }
}

#[test]
fn tlb_is_stale_until_sfence_vma() {
    // S-mode: load VA (fills TLB); change PTE to point elsewhere; load again
    // (stale); sfence.vma; load again (new mapping).
    let prog = with_handler(|a| {
        enter_s_with_paging(a, 0);
        a.li(r::A1, 0x4000_0000).ld(r::A2, r::A1, 0);
        a.li(r::T0, L0).li(r::T1, pte(DATA_RO, R | W | A | D)).sd(r::T1, r::T0, 0);
        a.ld(r::A3, r::A1, 0);
        a.w(0x1200_0073); // sfence.vma zero, zero
        a.ld(14, r::A1, 0); // a4
        a.ecall();
    });
    let mut m = paging_machine(&prog, 0);
    m.bus.ram.write(DATA_RW, 8, 111);
    m.bus.ram.write(DATA_RO, 8, 222);
    assert_eq!(m.run(), PASS);
    assert_eq!(m.cpu.x[12], 111);
    assert_eq!(m.cpu.x[13], 111, "stale TLB entry used before sfence");
    assert_eq!(m.cpu.x[14], 222, "new mapping after sfence.vma");
}

#[test]
fn mprv_makes_m_mode_loads_translate() {
    let prog = with_handler(|a| {
        a.li(r::T0, (8 << 60) | (ROOT >> 12)).csrrw(0, csr::SATP, r::T0);
        a.li(r::T0, (1 << 11) | csr::MSTATUS_MPRV).csrrw(0, csr::MSTATUS, r::T0);
        a.li(r::A1, 0x4000_1000).ld(15, r::A1, 0);
        a.ecall();
    });
    let mut m = paging_machine(&prog, 0);
    m.bus.ram.write(DATA_RO, 8, 4242);
    assert_eq!(m.run(), PASS);
    assert_eq!(m.cpu.x[15], 4242);
}

#[test]
fn misaligned_superpage_is_a_page_fault() {
    let prog = with_handler(|a| {
        enter_s_with_paging(a, 0);
        a.li(r::A1, 0xc000_0000).ld(r::A0, r::A1, 0);
    });
    let mut m = paging_machine(&prog, 0);
    // root[3] maps a gigapage at a non-1GiB-aligned PA -> invalid.
    m.bus.ram.write(ROOT + 3 * 8, 8, pte(0x8020_0000, R | W | A | D));
    assert_eq!(m.run(), PASS);
    assert_eq!(m.cpu.mcause, csr::cause::LOAD_PAGE);
}
