# Builds the bare-metal demo programs with clang + ld.lld (no GNU toolchain).
RVCC ?= clang
CFLAGS := --target=riscv64-unknown-elf -march=rv64imac_zicsr_zifencei -mabi=lp64 -mcmodel=medany \
          -O2 -ffreestanding -fno-builtin -nostdlib -Wall -Wextra -Werror -Irt
LDFLAGS := -fuse-ld=lld -nostdlib -static -Wl,-T,rt/link.ld
PROGS := hello primes mandelbrot timer paging atomics
OUT := build
RT := rt/start.S rt/rt.c

all: $(addprefix $(OUT)/,$(addsuffix .elf,$(PROGS)))

$(OUT)/%.elf: src/%.c $(RT) rt/rt.h rt/link.ld
	@mkdir -p $(OUT)
	$(RVCC) $(CFLAGS) $(LDFLAGS) $(RT) $< -o $@

clean:
	rm -rf $(OUT)

.PHONY: all clean
