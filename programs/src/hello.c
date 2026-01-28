#include "rt.h"

int main(void) {
  kputs("Hello from hartwood! (RV64IMAC, hart ");
  print_udec(csr_read(mhartid));
  kputs(")\n");
  return 0;
}
