/* ASCII Mandelbrot in 16.16-ish fixed point (no FPU in RV64IMAC). */
#include "rt.h"

#define FRAC 24
#define ONE ((i64)1 << FRAC)

int main(void) {
  const char *shade = " .:-=+*#%@";
  const int W = 64, H = 24, MAXIT = 64;
  for (int y = 0; y < H; y++) {
    i64 ci = -ONE + (2 * ONE / H) * y + ONE / H;
    for (int x = 0; x < W; x++) {
      i64 cr = -2 * ONE + (3 * ONE / W) * x;
      i64 zr = 0, zi = 0;
      int it = 0;
      while (it < MAXIT) {
        i64 zr2 = (zr * zr) >> FRAC, zi2 = (zi * zi) >> FRAC;
        if (zr2 + zi2 > 4 * ONE) break;
        i64 nzi = ((zr * zi) >> (FRAC - 1)) + ci;
        zr = zr2 - zi2 + cr;
        zi = nzi;
        it++;
      }
      kputc(it == MAXIT ? '@' : shade[it % 9]);
    }
    kputc('\n');
  }
  return 0;
}
