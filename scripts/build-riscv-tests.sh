#!/usr/bin/env bash
# Fetch (if needed) and build the official riscv-tests "-p" ISA tests for
# rv64ui/um/ua/uc/mi/si with clang + ld.lld (no GNU cross toolchain needed).
# Output: third_party/riscv-tests-build/<suite>-p-<test>
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$ROOT/third_party/riscv-tests"
OUT="$ROOT/third_party/riscv-tests-build"
if [ ! -d "$SRC/isa" ]; then
  mkdir -p "$ROOT/third_party"
  git clone -q --depth 1 https://github.com/riscv-software-src/riscv-tests "$SRC"
  git -C "$SRC" submodule update -q --init --depth 1 env
fi
mkdir -p "$OUT"
CC=${CC:-clang}
built=0; failed=0
for suite in rv64ui rv64um rv64ua rv64uc rv64mi rv64si; do
  tests=$(awk -v v="${suite}_sc_tests" '$1==v {on=1; sub(/^[^=]*=/,"")} on {if ($0 ~ /^[ \t]*$/) exit; gsub(/\\/,""); print}' "$SRC/isa/$suite/Makefrag")
  for t in $tests; do
    out="$OUT/$suite-p-$t"
    [ -f "$out" ] && [ "$out" -nt "$SRC/isa/$suite/$t.S" ] && { built=$((built+1)); continue; }
    # clang's assembler rejects `.weak sym` followed by `.global sym` (which
    # the env + mi/si tests do), so preprocess first and drop the .global.
    pre="$OUT/$suite-p-$t.s"
    if $CC --target=riscv64-unknown-elf -E -P -I"$SRC/env/p" -I"$SRC/isa/macros/scalar" -DXLEN=64 \
          "$SRC/isa/$suite/$t.S" 2>"$out.log" | sed -E -e 's/\.glob[a-z]* +(mtvec_handler|stvec_handler)//' -e 's/\btcontrol\b/0x7a5/g' > "$pre" \
       && $CC --target=riscv64-unknown-elf -march=rv64ima_zicsr_zifencei -mabi=lp64 -mno-relax \
          -nostdlib -static -fuse-ld=lld -T"$SRC/env/p/link.ld" -x assembler "$pre" -o "$out" 2>>"$out.log"; then
      rm -f "$pre"
      rm -f "$out.log"; built=$((built+1))
    else
      echo "BUILD FAILED: $suite-p-$t (see $out.log)" >&2; failed=$((failed+1))
    fi
  done
done
echo "riscv-tests: built $built, build failures $failed -> $OUT"
