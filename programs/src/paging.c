/* Builds Sv39 page tables, enables paging and drops to U-mode. The user
 * program writes through a virtual alias, makes "syscalls" with ecall and
 * finally touches an unmapped page, which the M-mode handler reports. */
#include "rt.h"

#define PTE_V 1
#define PTE_R 2
#define PTE_W 4
#define PTE_X 8
#define PTE_U 16
#define PTE_A 64
#define PTE_D 128

static u64 root[512] __attribute__((aligned(4096)));
static u64 l1[512] __attribute__((aligned(4096)));
static u64 l0[512] __attribute__((aligned(4096)));
static char shared[4096] __attribute__((aligned(4096)));
static u64 user_stack[512] __attribute__((aligned(4096)));

#define VADDR_SHARED 0x40000000UL /* -> &shared via 3-level walk */
#define VADDR_UNMAPPED 0x40001000UL

static u64 pte(u64 pa, u64 flags) { return ((pa >> 12) << 10) | flags | PTE_V; }

/* Trap frame saved by the assembly stub below. */
u64 user_regs[32];
static int syscalls;

/* Called from trap_entry with the saved user registers. */
void trap_c(void) {
  u64 cause = csr_read(mcause);
  u64 epc = csr_read(mepc);
  if (cause == 8) { /* ecall from U */
    syscalls++;
    u64 nr = user_regs[17], a0 = user_regs[10];
    if (nr == 1) {
      kputs((const char *)a0); /* identity-mapped user string */
    } else if (nr == 2) {
      kputs("user value: ");
      print_hex(a0);
      kputc('\n');
    }
    csr_write(mepc, epc + 4);
    return;
  }
  kputs("M-mode caught trap: mcause=");
  print_dec((i64)cause);
  kputs(" mtval=");
  print_hex(csr_read(mtval));
  kputs(" from priv=");
  print_dec((i64)((csr_read(mstatus) >> 11) & 3));
  kputs("\nshared page (physical view): ");
  kputs(shared);
  kputs("syscalls handled: ");
  print_dec(syscalls);
  kputc('\n');
  rt_exit(cause == 13 ? 0 : 1);
}

__attribute__((naked, aligned(4))) void trap_entry(void) {
  __asm__ volatile(
      "csrw mscratch, t0\n"
      "la t0, user_regs\n"
      "sd ra, 8(t0)\n sd sp, 16(t0)\n sd gp, 24(t0)\n sd tp, 32(t0)\n"
      "sd t1, 48(t0)\n sd t2, 56(t0)\n sd s0, 64(t0)\n sd s1, 72(t0)\n"
      "sd a0, 80(t0)\n sd a1, 88(t0)\n sd a2, 96(t0)\n sd a3, 104(t0)\n"
      "sd a4, 112(t0)\n sd a5, 120(t0)\n sd a6, 128(t0)\n sd a7, 136(t0)\n"
      "sd t3, 224(t0)\n sd t4, 232(t0)\n sd t5, 240(t0)\n sd t6, 248(t0)\n"
      "la sp, __stack_top\n"
      "call trap_c\n"
      "la t0, user_regs\n"
      "ld ra, 8(t0)\n ld sp, 16(t0)\n ld gp, 24(t0)\n ld tp, 32(t0)\n"
      "ld t1, 48(t0)\n ld t2, 56(t0)\n ld s0, 64(t0)\n ld s1, 72(t0)\n"
      "ld a0, 80(t0)\n ld a1, 88(t0)\n ld a2, 96(t0)\n ld a3, 104(t0)\n"
      "ld a4, 112(t0)\n ld a5, 120(t0)\n ld a6, 128(t0)\n ld a7, 136(t0)\n"
      "ld t3, 224(t0)\n ld t4, 232(t0)\n ld t5, 240(t0)\n ld t6, 248(t0)\n"
      "csrr t0, mscratch\n"
      "mret\n");
}

static inline u64 sys(u64 nr, u64 a0) {
  register u64 r_a0 __asm__("a0") = a0;
  register u64 r_a7 __asm__("a7") = nr;
  __asm__ volatile("ecall" : "+r"(r_a0) : "r"(r_a7) : "memory");
  return r_a0;
}

static const char msg_hello[] = "user: hello from U-mode with Sv39 enabled\n";
static const char msg_wrote[] = "user: wrote through virtual alias 0x40000000\n";

__attribute__((noreturn)) void user_main(void) {
  sys(1, (u64)msg_hello);
  volatile char *v = (volatile char *)VADDR_SHARED;
  const char *text = "written by U-mode via VA 0x40000000\n";
  for (int i = 0; text[i]; i++) v[i] = text[i];
  sys(1, (u64)msg_wrote);
  sys(2, *(volatile u64 *)VADDR_SHARED);
  /* Now touch an unmapped page: load page fault (cause 13). */
  u64 x = *(volatile u64 *)VADDR_UNMAPPED;
  sys(2, x);
  for (;;) {
  }
}

int main(void) {
  /* Gigapage identity map of RAM (0x80000000) as user RWX, so U-mode can
   * run the program image; 0x40000000 is mapped through l1/l0 to `shared`. */
  root[2] = pte(0x80000000UL, PTE_R | PTE_W | PTE_X | PTE_U | PTE_A | PTE_D);
  root[1] = pte((u64)l1, 0);
  l1[0] = pte((u64)l0, 0);
  l0[0] = pte((u64)shared, PTE_R | PTE_W | PTE_U | PTE_A);
  /* l0[1] (VADDR_UNMAPPED) intentionally left invalid. */

  csr_write(mtvec, (u64)&trap_entry);
  csr_write(satp, (8UL << 60) | ((u64)root >> 12));
  __asm__ volatile("sfence.vma zero, zero");
  kputs("M-mode: Sv39 enabled, satp=");
  print_hex(csr_read(satp));
  kputs("\n");

  /* mret into U-mode at user_main with its own stack. */
  u64 ms = csr_read(mstatus);
  ms &= ~(3UL << 11); /* MPP = U */
  csr_write(mstatus, ms);
  csr_write(mepc, (u64)&user_main);
  register u64 usp __asm__("sp");
  (void)usp;
  __asm__ volatile("mv sp, %0\n mret" ::"r"((u64)&user_stack[512]));
  return 1;
}
