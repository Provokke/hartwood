//! A complete single-hart system: CPU + bus + devices, plus the run loop
//! that keeps emulated time (CLINT mtime) tied to the CPU cycle counter.

use std::io::Write;
use std::sync::mpsc::Receiver;

use crate::bus::Bus;
use crate::cpu::Cpu;
use crate::csr;
use crate::devices::uart::Uart;
use crate::disasm::disassemble;
use crate::elf::{Elf, ElfError};

#[derive(Clone, Debug)]
pub struct Config {
    pub ram_size: usize,
    /// CPU cycles per CLINT mtime tick (e.g. 10 = 100 MHz core, 10 MHz timer).
    pub cycles_per_tick: u64,
    pub max_cycles: Option<u64>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            ram_size: 128 << 20,
            cycles_per_tick: 10,
            max_cycles: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// riscv-tests style `tohost` write (1 = pass, (n<<1)|1 = failed test n).
    ToHost(u64),
    /// Test-finisher device write: Ok(0) for pass, otherwise exit code.
    Finisher(u32),
    /// Cycle budget exhausted.
    MaxCycles,
    /// The hart is in WFI and nothing can ever wake it.
    Halted,
}

impl Exit {
    /// Process exit code for the CLI.
    pub fn code(&self) -> i32 {
        match *self {
            Exit::ToHost(1) => 0,
            Exit::ToHost(v) => ((v >> 1) as i32).max(1),
            Exit::Finisher(v) if v & 0xffff == 0x5555 => 0,
            Exit::Finisher(v) => ((v >> 16) as i32).max(1),
            Exit::MaxCycles => 124,
            Exit::Halted => 3,
        }
    }
}

pub struct Machine {
    pub cpu: Cpu,
    pub bus: Bus,
    pub cfg: Config,
    pub trace: Option<Box<dyn Write>>,
    pub input: Option<Receiver<u8>>,
}

impl Machine {
    pub fn new(cfg: Config, uart_out: Box<dyn Write + Send>) -> Machine {
        let bus = Bus::new(cfg.ram_size, Uart::new(uart_out));
        Machine {
            cpu: Cpu::new(),
            bus,
            cfg,
            trace: None,
            input: None,
        }
    }

    /// A machine whose UART output is captured in memory (for tests).
    pub fn new_captured(cfg: Config) -> Machine {
        let mut m = Machine::new(cfg, Box::new(std::io::sink()));
        m.bus.uart.capture = true;
        m
    }

    pub fn load_elf(&mut self, bytes: &[u8]) -> Result<(), ElfError> {
        let elf = Elf::parse(bytes)?;
        for seg in &elf.segments {
            if !self.bus.ram.load(seg.paddr, &seg.data) {
                return Err(ElfError::Truncated("segment outside RAM"));
            }
            let bss = seg.memsz.saturating_sub(seg.data.len() as u64);
            if bss > 0 {
                let zeros = vec![0u8; bss as usize];
                self.bus.ram.load(seg.paddr + seg.data.len() as u64, &zeros);
            }
        }
        self.bus.tohost_addr = elf.symbols.get("tohost").copied();
        self.cpu.pc = elf.entry;
        Ok(())
    }

    /// Load a raw binary image at `addr` and start executing there.
    pub fn load_raw(&mut self, addr: u64, bytes: &[u8]) -> bool {
        self.cpu.pc = addr;
        self.bus.ram.load(addr, bytes)
    }

    #[inline]
    fn sync_devices(&mut self) {
        let clint = &mut self.bus.clint;
        clint.mtime = (self.cpu.cycles / self.cfg.cycles_per_tick).wrapping_add(clint.mtime_offset);
        self.cpu.time = clint.mtime;
        let mut hw = 0;
        if clint.msip {
            hw |= csr::MIP_MSIP;
        }
        if clint.timer_pending() {
            hw |= csr::MIP_MTIP;
        }
        if self.bus.plic.irq_for(0) {
            hw |= csr::MIP_MEIP;
        }
        if self.bus.plic.irq_for(1) {
            hw |= csr::MIP_SEIP;
        }
        self.cpu.mip_hw = hw;
    }

    fn poll_input(&mut self) {
        if let Some(rx) = &self.input {
            let mut got = false;
            while let Ok(b) = rx.try_recv() {
                self.bus.uart.rx.push_back(b);
                got = true;
            }
            if got {
                self.bus.sync_uart_irq();
            }
        }
    }

    /// While in WFI with nothing pending, jump the cycle counter forward to
    /// the next timer deadline. Returns false if nothing can ever wake us.
    fn fast_forward_wfi(&mut self, limit: u64) -> bool {
        let clint = &self.bus.clint;
        if self.cpu.mie & csr::MIP_MTIP != 0 && clint.mtimecmp != u64::MAX {
            let target_ticks = clint.mtimecmp.wrapping_sub(clint.mtime_offset);
            let target = target_ticks.saturating_mul(self.cfg.cycles_per_tick);
            if target > self.cpu.cycles {
                self.cpu.cycles = target.min(limit);
            }
            return true;
        }
        if self.input.is_some() {
            std::thread::sleep(std::time::Duration::from_millis(1));
            return true;
        }
        false
    }

    /// Run until the program signals completion or the cycle budget runs out.
    pub fn run(&mut self) -> Exit {
        let limit = self.cfg.max_cycles.unwrap_or(u64::MAX);
        let mut poll = 0u32;
        loop {
            self.sync_devices();
            if self.cpu.wfi {
                if self.cpu.mip() & self.cpu.mie != 0 {
                    self.cpu.wfi = false;
                } else {
                    self.poll_input();
                    if !self.fast_forward_wfi(limit) {
                        return Exit::Halted;
                    }
                    if self.cpu.cycles >= limit {
                        return Exit::MaxCycles;
                    }
                    continue;
                }
            }
            let pc = self.cpu.pc;
            let prv = self.cpu.prv;
            let retired = self.cpu.step(&mut self.bus);
            if let Some(t) = self.trace.as_mut() {
                if let Some(inst) = retired {
                    let _ = writeln!(
                        t,
                        "{:>10} {:?} {:016x}: {:08x}  {}",
                        self.cpu.cycles,
                        prv as u8,
                        pc,
                        inst.raw,
                        disassemble(&inst, pc)
                    );
                } else {
                    let _ = writeln!(
                        t,
                        "{:>10} trap -> pc={:016x} mcause={:#x} scause={:#x}",
                        self.cpu.cycles, self.cpu.pc, self.cpu.mcause, self.cpu.scause
                    );
                }
            }
            if let Some(v) = self.bus.tohost.take() {
                self.bus.uart.flush();
                return Exit::ToHost(v);
            }
            if let Some(v) = self.bus.finisher.take() {
                self.bus.uart.flush();
                return Exit::Finisher(v);
            }
            if self.cpu.cycles >= limit {
                self.bus.uart.flush();
                return Exit::MaxCycles;
            }
            poll = poll.wrapping_add(1);
            if poll & 0xfff == 0 {
                self.poll_input();
            }
        }
    }

    pub fn uart_output(&self) -> String {
        String::from_utf8_lossy(&self.bus.uart.captured).into_owned()
    }
}
