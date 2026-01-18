/* Exercises LR/SC and AMO instructions through C11-style builtins. */
#include "rt.h"

static volatile u64 counter;
static volatile u32 word;

static u64 cas64(volatile u64 *p, u64 expected, u64 desired) {
  u64 prev;
  u32 fail;
  __asm__ volatile(
      "1: lr.d.aqrl %0, (%2)\n"
      "   bne %0, %3, 2f\n"
      "   sc.d.aqrl %1, %4, (%2)\n"
      "   bnez %1, 1b\n"
      "2:\n"
      : "=&r"(prev), "=&r"(fail)
      : "r"(p), "r"(expected), "r"(desired)
      : "memory");
  return prev;
}

int main(void) {
  for (int i = 0; i < 1000; i++) __atomic_fetch_add(&counter, 3, __ATOMIC_SEQ_CST);
  u64 old = __atomic_exchange_n(&counter, 42, __ATOMIC_SEQ_CST);
  __atomic_fetch_or(&word, 0xf0u, __ATOMIC_RELAXED);
  __atomic_fetch_and(&word, 0x3cu, __ATOMIC_RELAXED);
  __atomic_fetch_xor(&word, 0x1u, __ATOMIC_RELAXED);
  u64 prev = cas64(&counter, 42, 7);
  u64 prev2 = cas64(&counter, 42, 9); /* fails: value is 7 */
  kputs("fetch_add total=");
  print_udec(old);
  kputs(" word=");
  print_hex(word);
  kputs(" cas1 prev=");
  print_udec(prev);
  kputs(" cas2 prev=");
  print_udec(prev2);
  kputs(" final=");
  print_udec(counter);
  kputc('\n');
  return (old == 3000 && word == 0x31 && counter == 7) ? 0 : 1;
}
