/* Tiny bare-metal runtime for hartwood demo programs. */
#ifndef HARTWOOD_RT_H
#define HARTWOOD_RT_H

typedef unsigned long u64;
typedef long i64;
typedef unsigned int u32;
typedef unsigned char u8;
typedef unsigned long size_t;

#define UART_BASE 0x10000000UL
#define CLINT_BASE 0x02000000UL
#define CLINT_MTIMECMP (*(volatile u64 *)(CLINT_BASE + 0x4000))
#define CLINT_MTIME (*(volatile u64 *)(CLINT_BASE + 0xbff8))
#define FINISHER (*(volatile u32 *)0x00100000UL)

#define csr_read(name) ({ u64 __v; __asm__ volatile("csrr %0, " #name : "=r"(__v)); __v; })
#define csr_write(name, val) __asm__ volatile("csrw " #name ", %0" ::"r"((u64)(val)))
#define csr_set(name, val) __asm__ volatile("csrs " #name ", %0" ::"r"((u64)(val)))
#define csr_clear(name, val) __asm__ volatile("csrc " #name ", %0" ::"r"((u64)(val)))

void kputc(char c);
void kputs(const char *s); /* no trailing newline added */
void print_dec(i64 v);
void print_udec(u64 v);
void print_hex(u64 v);
int getc_nonblock(void); /* -1 if nothing available */
void rt_exit(int code) __attribute__((noreturn));
void *memset(void *d, int c, size_t n);
void *memcpy(void *d, const void *s, size_t n);

#endif
