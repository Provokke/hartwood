# Bare-metal entry: set up the stack, clear .bss, call main, exit with its result.
  .section .text.start
  .globl _start
_start:
  la sp, __stack_top
  la t0, __bss_start
  la t1, __bss_end
1:
  bgeu t0, t1, 2f
  sd zero, 0(t0)
  addi t0, t0, 8
  j 1b
2:
  call main
  call rt_exit
3:
  j 3b
