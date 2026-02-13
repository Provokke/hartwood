/* Sieve of Eratosthenes plus a trial-division cross-check (exercises M ext). */
#include "rt.h"

#define N 20000
static u8 composite[N + 1];

static int is_prime_trial(u64 n) {
  if (n < 2) return 0;
  for (u64 d = 2; d * d <= n; d++)
    if (n % d == 0) return 0;
  return 1;
}

int main(void) {
  int count = 0;
  u64 sum = 0;
  for (u64 i = 2; i <= N; i++) {
    if (composite[i]) continue;
    count++;
    sum += i;
    for (u64 j = i * i; j <= N; j += i) composite[j] = 1;
  }
  kputs("primes below ");
  print_udec(N);
  kputs(": ");
  print_dec(count);
  kputs(" (sum ");
  print_udec(sum);
  kputs(")\nfirst: ");
  int shown = 0;
  for (u64 i = 2; shown < 15; i++)
    if (!composite[i]) {
      print_udec(i);
      kputc(' ');
      shown++;
    }
  kputs("\nlast:  ");
  int last[5], k = 0;
  for (u64 i = N; k < 5; i--)
    if (!composite[i]) last[k++] = (int)i;
  for (k = 4; k >= 0; k--) {
    print_dec(last[k]);
    kputc(' ');
  }
  kputc('\n');
  for (u64 i = 0; i <= 2000; i++)
    if (is_prime_trial(i) != (i >= 2 && !composite[i])) {
      kputs("MISMATCH at ");
      print_udec(i);
      kputc('\n');
      return 1;
    }
  kputs("trial-division cross-check OK\n");
  return 0;
}
