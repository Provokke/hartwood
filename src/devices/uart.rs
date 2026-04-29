//! A 16550-compatible UART. Transmitted bytes go to a pluggable sink;
//! received bytes come from an input queue (fed from stdin by the CLI, or
//! pre-loaded by tests for determinism).

use std::collections::VecDeque;
use std::io::Write;

const IER_RDA: u8 = 0x01;
const IER_THRE: u8 = 0x02;
const LSR_DR: u8 = 0x01;
const LSR_THRE: u8 = 0x20;
const LSR_TEMT: u8 = 0x40;

pub struct Uart {
    pub rx: VecDeque<u8>,
    pub out: Box<dyn Write + Send>,
    /// Copy of everything written (handy for tests / golden outputs).
    pub captured: Vec<u8>,
    pub capture: bool,
    ier: u8,
    lcr: u8,
    mcr: u8,
    scr: u8,
    dll: u8,
    dlm: u8,
    /// THR-empty interrupt is pending until IIR is read or THR written.
    thre_ip: bool,
}

impl Uart {
    pub fn new(out: Box<dyn Write + Send>) -> Uart {
        Uart {
            rx: VecDeque::new(),
            out,
            captured: Vec::new(),
            capture: false,
            ier: 0,
            lcr: 0,
            mcr: 0,
            scr: 0,
            dll: 1,
            dlm: 0,
            thre_ip: false,
        }
    }

    /// Level of the interrupt line to the PLIC.
    pub fn irq(&self) -> bool {
        (self.ier & IER_RDA != 0 && !self.rx.is_empty()) || (self.ier & IER_THRE != 0 && self.thre_ip)
    }

    fn dlab(&self) -> bool {
        self.lcr & 0x80 != 0
    }

    pub fn read(&mut self, off: u64) -> u8 {
        match off {
            0 if self.dlab() => self.dll,
            0 => self.rx.pop_front().unwrap_or(0),
            1 if self.dlab() => self.dlm,
            1 => self.ier,
            2 => {
                // IIR: FIFOs enabled (0xc0) + interrupt id.
                if self.ier & IER_RDA != 0 && !self.rx.is_empty() {
                    0xc4
                } else if self.ier & IER_THRE != 0 && self.thre_ip {
                    self.thre_ip = false;
                    0xc2
                } else {
                    0xc1
                }
            }
            3 => self.lcr,
            4 => self.mcr,
            5 => LSR_THRE | LSR_TEMT | if self.rx.is_empty() { 0 } else { LSR_DR },
            6 => 0xb0,
            7 => self.scr,
            _ => 0,
        }
    }

    pub fn write(&mut self, off: u64, val: u8) {
        match off {
            0 if self.dlab() => self.dll = val,
            0 => {
                if self.capture {
                    self.captured.push(val);
                }
                let _ = self.out.write_all(&[val]);
                if val == b'\n' {
                    let _ = self.out.flush();
                }
                self.thre_ip = true;
            }
            1 if self.dlab() => self.dlm = val,
            1 => {
                self.ier = val & 0x0f;
                if self.ier & IER_THRE != 0 {
                    self.thre_ip = true;
                }
            }
            2 => {} // FCR: FIFOs always on
            3 => self.lcr = val,
            4 => self.mcr = val,
            7 => self.scr = val,
            _ => {}
        }
    }

    pub fn flush(&mut self) {
        let _ = self.out.flush();
    }
}
