//! hartwood: a from-scratch RV64IMAC system emulator.
//!
//! See `README.md` and `docs/ARCHITECTURE.md` for an overview.

pub mod alu;
pub mod bus;
pub mod cpu;
pub mod csr;
pub mod decode;
pub mod devices;
pub mod disasm;
pub mod elf;
pub mod machine;

pub use machine::{Config, Exit, Machine};
