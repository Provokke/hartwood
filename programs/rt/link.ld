OUTPUT_ARCH("riscv")
ENTRY(_start)
SECTIONS
{
  . = 0x80000000;
  .text : { *(.text.start) *(.text .text.*) }
  .rodata : ALIGN(16) { *(.rodata .rodata.* .srodata .srodata.*) }
  .data : ALIGN(16) { *(.data .data.* .sdata .sdata.*) }
  .bss : ALIGN(16) { __bss_start = .; *(.bss .bss.* .sbss .sbss.* COMMON) . = ALIGN(16); __bss_end = .; }
  . = ALIGN(4096);
  __stack_bottom = .;
  . += 0x10000;
  __stack_top = .;
  _end = .;
}
