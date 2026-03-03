//! The physical address space. Dispatches to RAM and MMIO devices.
//!
//! ```text
//! 0x0010_0000  test finisher (write 0x5555 = pass, (code<<16)|0x3333 = fail)
//! 0x0200_0000  CLINT   (64 KiB)
//! 0x0c00_0000  PLIC    (64 MiB window)
//! 0x1000_0000  UART0   (256 B, PLIC source 10)
//! 0x8000_0000  RAM
//! ```

use crate::devices::{clint::Clint, plic::Plic, ram::Ram, uart::Uart};

pub const FINISHER_BASE: u64 = 0x0010_0000;
pub const CLINT_BASE: u64 = 0x0200_0000;
pub const CLINT_SIZE: u64 = 0x1_0000;
pub const PLIC_BASE: u64 = 0x0c00_0000;
pub const PLIC_SIZE: u64 = 0x400_0000;
pub const UART_BASE: u64 = 0x1000_0000;
pub const UART_SIZE: u64 = 0x100;
pub const UART_IRQ: usize = 10;
pub const RAM_BASE: u64 = 0x8000_0000;

/// Physical memory access failed (no device / bad width).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BusError;

pub struct Bus {
    pub ram: Ram,
    pub uart: Uart,
    pub clint: Clint,
    pub plic: Plic,
    /// Address of the riscv-tests `tohost` symbol, if the program has one.
    pub tohost_addr: Option<u64>,
    /// Last non-zero value written to `tohost`.
    pub tohost: Option<u64>,
    /// Value written to the test finisher device.
    pub finisher: Option<u32>,
}

impl Bus {
    pub fn new(ram_size: usize, uart: Uart) -> Bus {
        Bus {
            ram: Ram::new(RAM_BASE, ram_size),
            uart,
            clint: Clint::new(),
            plic: Plic::new(),
            tohost_addr: None,
            tohost: None,
            finisher: None,
        }
    }

    #[inline]
    pub fn read(&mut self, addr: u64, len: u64) -> Result<u64, BusError> {
        if self.ram.contains(addr, len) {
            return Ok(self.ram.read(addr, len));
        }
        self.read_mmio(addr, len)
    }

    #[cold]
    fn read_mmio(&mut self, addr: u64, len: u64) -> Result<u64, BusError> {
        if (CLINT_BASE..CLINT_BASE + CLINT_SIZE).contains(&addr) {
            return self.clint.read(addr - CLINT_BASE, len).ok_or(BusError);
        }
        if (PLIC_BASE..PLIC_BASE + PLIC_SIZE).contains(&addr) {
            if len != 4 {
                return Err(BusError);
            }
            return self.plic.read(addr - PLIC_BASE).ok_or(BusError);
        }
        if (UART_BASE..UART_BASE + UART_SIZE).contains(&addr) {
            let v = self.uart.read(addr - UART_BASE) as u64;
            self.sync_uart_irq();
            return Ok(v);
        }
        if (FINISHER_BASE..FINISHER_BASE + 0x1000).contains(&addr) {
            return Ok(0);
        }
        Err(BusError)
    }

    #[inline]
    pub fn write(&mut self, addr: u64, len: u64, val: u64) -> Result<(), BusError> {
        if self.ram.contains(addr, len) {
            self.ram.write(addr, len, val);
            if let Some(t) = self.tohost_addr {
                if addr == t && val != 0 {
                    self.tohost = Some(self.ram.read(t, 8));
                }
            }
            return Ok(());
        }
        self.write_mmio(addr, len, val)
    }

    #[cold]
    fn write_mmio(&mut self, addr: u64, len: u64, val: u64) -> Result<(), BusError> {
        if (CLINT_BASE..CLINT_BASE + CLINT_SIZE).contains(&addr) {
            return if self.clint.write(addr - CLINT_BASE, len, val) {
                Ok(())
            } else {
                Err(BusError)
            };
        }
        if (PLIC_BASE..PLIC_BASE + PLIC_SIZE).contains(&addr) {
            if len != 4 {
                return Err(BusError);
            }
            self.plic.write(addr - PLIC_BASE, val);
            return Ok(());
        }
        if (UART_BASE..UART_BASE + UART_SIZE).contains(&addr) {
            self.uart.write(addr - UART_BASE, val as u8);
            self.sync_uart_irq();
            return Ok(());
        }
        if addr == FINISHER_BASE {
            let v = val as u32;
            if v & 0xffff == 0x5555 || v & 0xffff == 0x3333 {
                self.finisher = Some(v);
            }
            return Ok(());
        }
        Err(BusError)
    }

    pub fn sync_uart_irq(&mut self) {
        let level = self.uart.irq();
        self.plic.set_irq(UART_IRQ, level);
    }
}
