<div align="center">

# hartwood

A from-scratch RISC-V system emulator written in Rust (no dependencies outside the dev-only `proptest`). It interprets **RV64IMAC + Zicsr + Zifencei** with M/S/U privilege modes, traps and interrupts, **Sv39 virtual memory with TLBs**, LR/SC + AMO...

![License](https://img.shields.io/badge/LICENSE-MIT-blue?style=for-the-badge)  ![Language](https://img.shields.io/badge/RUST-dea584?style=for-the-badge&logo=rust&logoColor=white)

Refer to [LICENSE](LICENSE) for the legal bits.

</div>

## 🏗️ Architecture

```text
                 +--------------------------- Machine (machine.rs) --------------------------+
  ELF file ----> | elf.rs loader      run loop: step -> sync devices -> check tohost/finisher |
                 |                    WFI fast-forward to next mtimecmp; mtime = cycles/N     |
                 |  +----------------------- Cpu (cpu.rs) -----------------------+           |
                 |  | fetch --> decode cache --> decode.rs (RVC expanded to 32b) |           |
                 |  |   |                          |                              |           |
                 |  |   v                          v                              |           |
                 |  | Sv39 MMU: I-TLB / D-TLB   execute (alu.rs, CSRs, traps,   |           |
                 |  | page walk, A/D update     priv modes, LR/SC, AMO, costs)  |           |
                 |  +----------------------------+-------------------------------+           |
                 |                               | physical accesses                         |
                 |  +----------------------------v-------------------------------+           |
                 |  |                       Bus (bus.rs)                          |           |
                 |  | 0x0010_0000 finisher | 0x0200_0000 CLINT | 0x0c00_0000 PLIC |           |
                 |  | 0x1000_0000 UART16550 (irq 10) | 0x8000_0000 RAM            |           |
                 |  +-------------------------------------------------------------+           |
                 +---------------------------------------------------------------------------+
```

More detail (cycle cost table, trap/interrupt rules, TLB design) is in
.

## 🚀 Quick start

```sh
cargo build --release
make -C programs                       # bare-metal demos (clang + ld.lld)
./target/release/hartwood run programs/build/hello.elf
./target/release/hartwood run prog.elf --trace --max-cycles 1000000 --stats
./target/release/hartwood disasm programs/build/hello.elf
```

Options: `--trace` (per-instruction trace to stderr), `--max-cycles N`
(exit code 124 when hit), `--cycles-per-tick N` (CPU cycles per mtime tick,
default 10), `--ram-mb N`, `--stats` (instret/cycles/MIPS/TLB stats),
`--no-stdin` (don't feed stdin to the UART). Programs end by writing to the
test finisher (`0x5555` = pass) or riscv-tests `tohost`; the process exit code
reflects the result.

### Sample output (real)

```text
$ hartwood run programs/build/paging.elf --stats
M-mode: Sv39 enabled, satp=0x8000000000080004
user: hello from U-mode with Sv39 enabled
user: wrote through virtual alias 0x40000000
user value: 0x206e657474697277
M-mode caught trap: mcause=13 mtval=0x0000000040001000 from priv=0
shared page (physical view): written by U-mode via VA 0x40000000
syscalls handled: 3
[hartwood] exit=Finisher(21845) instret=17463 cycles=28159 mtime=2815 ... tlb_hits=197 tlb_misses=76

$ hartwood run programs/build/timer.elf
tick 1 at mtime=10004
tick 2 at mtime=20008
...
timer demo done: 5 interrupts, cycle=501432 instret=2303

$ hartwood run prog.elf --trace
         1 3 0000000080000000: 00011117  auipc sp, 0x11
         2 3 0000000080000004: 00010113  addi sp, sp, 0x0
         9 3 0000000080000018: 0062f663  bgeu t0, t1, 0x80000024
```

Demo programs (`programs/src`, with a tiny runtime in `programs/rt`): `hello`,
`primes` (sieve + trial-division cross-check), `mandelbrot` (fixed-point ASCII
art), `timer` (WFI + machine-timer interrupt handler), `paging` (builds Sv39
tables, `mret`s to U-mode, ecall syscalls, catches a page fault), `atomics`
(AMOs + LR/SC CAS loop).

## 🧪 Testing

`make test` runs everything (builds demos, fetches+builds riscv-tests, then
`cargo test --workspace` with the official suite required). `make lint` runs
`cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` (both clean).

| suite | what it checks | count |
|---|---|---|
| `tests/decoder.rs` | 114-instruction corpus disassembled **identically to llvm-objdump**; **all 49,152** 16-bit encodings agree with llvm on validity + base op; hand-checked RVC operand expansions; every opcode/funct; proptests (no panics, RVC expansion consistency) | 7 tests |
| `tests/alu_props.rs` | decoder+executor vs. an independent i128 reference model for RV64I/M incl. W-forms, overflow, div-by-zero (2000 cases each) | 4 tests |
| `tests/system.rs` | illegal CSR from U, ecall delegation U->S->M, ebreak/misaligned-AMO causes, timer IRQ + WFI fast-forward, WFI wake without MIE, halt detection, CLINT MSIP, UART->PLIC external IRQ + claim, LR/SC, CSR WARL/counters, Sv39 translation + hardware A/D, store page fault, SUM, unmapped/non-canonical faults, TLB staleness until `sfence.vma`, MPRV, misaligned superpage | 16 tests |
| `tests/programs.rs` | golden UART output for all 6 C demos, bit-for-bit determinism (cycles, regs, RAM), timer scaling with `--cycles-per-tick`, `--max-cycles`, CLI/trace/disasm, ELF rejection | 11 tests |
| `tests/riscv_tests.rs` | official riscv-tests: rv64ui 54/54, rv64um 13/13, rv64ua 19/19, rv64uc 1/1, rv64mi 17/17, rv64si 7/7 | **111/111** |

As a sanity check that the suite has teeth, deliberately breaking `sub` makes
`rv64ui-p-sub` and `rv64ui-p-auipc` fail.

The riscv-tests are built by `scripts/build-riscv-tests.sh` with
`clang --target=riscv64-unknown-elf -march=rv64ima_zicsr_zifencei` and ld.lld.
Two clang-compat tweaks are applied after preprocessing (the sources are not
modified): the `.global mtvec_handler/stvec_handler` lines that clash with the
env's `.weak` are dropped, and the `tcontrol` CSR name is replaced by `0x7a5`.
If the suite isn't built, plain `cargo test` prints a NOTICE and skips that
one test (`make test` sets `HARTWOOD_REQUIRE_RISCV_TESTS=1` so it cannot skip).

## ⏱️ Benchmarks

`make bench` (release build, best of 7, one core of a shared 4-core VM):

| workload | guest instret | modelled cycles | host time | MIPS |
|---|---|---|---|---|
| primes | 626,301 | 1,377,683 | 6.67 ms | 94.0 |
| mandelbrot | 522,502 | 804,019 | 5.21 ms | 100.3 |
| paging (Sv39, U-mode) | 17,463 | 28,149 | 0.20 ms | 88.1 |
| timer (mostly WFI) | 2,421 | 501,992 | 0.04 ms | 67.9 |

Workloads are short, so treat these as ballpark (~90-100 MIPS).

## ⚠️ Honest limits

* Single hart; no F/D, no PMP enforcement (pmp CSRs are plain storage), no
  debug triggers (tselect/tdata read as 0), no ASIDs (satp ASID is WARL 0).
* Misaligned loads/stores are done in "hardware" (byte-wise); misaligned AMOs/LR/SC trap.
* The cycle model is a simple per-instruction-class cost table (no pipeline or
  cache simulation); "cycle-accurate" means time is a deterministic function of it.
* virtio-blk was not implemented; xv6/OpenSBI boot was not attempted.
* UART input from stdin is polled every 4096 instructions (non-deterministic by
  nature; tests preload the RX queue instead).
