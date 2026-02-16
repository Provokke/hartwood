/* Timer-interrupt-driven program: the main loop sleeps in WFI and a machine
 * timer interrupt handler re-arms mtimecmp each tick. */
#include "rt.h"

#define INTERVAL 10000 /* mtime ticks between interrupts */
#define TICKS 5

static volatile int ticks;
static volatile u64 stamps[TICKS];

__attribute__((interrupt("machine"))) static void trap_handler(void) {
  u64 cause = csr_read(mcause);
  if (cause == ((1UL << 63) | 7)) {
    u64 now = CLINT_MTIME;
    if (ticks < TICKS) stamps[ticks] = now;
    ticks++;
    CLINT_MTIMECMP = now + INTERVAL;
  } else {
    kputs("unexpected trap, mcause=");
    print_hex(cause);
    kputc('\n');
    rt_exit(1);
  }
}

int main(void) {
  csr_write(mtvec, (u64)&trap_handler);
  u64 start = CLINT_MTIME;
  CLINT_MTIMECMP = start + INTERVAL;
  csr_set(mie, 1UL << 7);    /* MTIE */
  csr_set(mstatus, 1UL << 3); /* MIE */
  int seen = 0;
  while (ticks < TICKS) {
    __asm__ volatile("wfi");
    if (ticks != seen) {
      seen = ticks;
      kputs("tick ");
      print_dec(seen);
      kputs(" at mtime=");
      print_udec(stamps[seen - 1] - start);
      kputc('\n');
    }
  }
  csr_clear(mie, 1UL << 7);
  kputs("timer demo done: ");
  print_dec(TICKS);
  kputs(" interrupts, cycle=");
  print_udec(csr_read(mcycle));
  kputs(" instret=");
  print_udec(csr_read(minstret));
  kputc('\n');
  return 0;
}
