# Top-level convenience targets.
export CARGO_BUILD_JOBS ?= 2

.PHONY: all build programs riscv-tests test lint bench clean

all: build programs

build:
	cargo build --release

programs:
	$(MAKE) -C programs

riscv-tests:
	./scripts/build-riscv-tests.sh

# Full suite: unit + property + system + golden programs + official riscv-tests.
test: programs riscv-tests
	HARTWOOD_REQUIRE_RISCV_TESTS=1 cargo test --workspace

lint:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings

bench: programs
	cargo bench --bench mips

clean:
	cargo clean
	$(MAKE) -C programs clean
