#include "rt.h"

#define UART_THR (*(volatile u8 *)(UART_BASE + 0))
#define UART_LSR (*(volatile u8 *)(UART_BASE + 5))

void kputc(char c) {
  while (!(UART_LSR & 0x20)) {
  }
  UART_THR = (u8)c;
}

void kputs(const char *s) {
  while (*s) kputc(*s++);
}

void print_udec(u64 v) {
  char buf[24];
  int i = 0;
  do {
    buf[i++] = (char)('0' + v % 10);
    v /= 10;
  } while (v);
  while (i) kputc(buf[--i]);
}

void print_dec(i64 v) {
  if (v < 0) {
    kputc('-');
    print_udec((u64)0 - (u64)v);
  } else {
    print_udec((u64)v);
  }
}

void print_hex(u64 v) {
  kputs("0x");
  for (int s = 60; s >= 0; s -= 4) kputc("0123456789abcdef"[(v >> s) & 15]);
}

int getc_nonblock(void) {
  if (UART_LSR & 1) return *(volatile u8 *)UART_BASE;
  return -1;
}

void rt_exit(int code) {
  FINISHER = code == 0 ? 0x5555u : (((u32)code << 16) | 0x3333u);
  for (;;) {
  }
}

void *memset(void *d, int c, size_t n) {
  u8 *p = d;
  while (n--) *p++ = (u8)c;
  return d;
}

void *memcpy(void *d, const void *s, size_t n) {
  u8 *p = d;
  const u8 *q = s;
  while (n--) *p++ = *q++;
  return d;
}
