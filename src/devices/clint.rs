//! Core-Local Interruptor: machine software interrupt (msip) and the
//! machine timer (mtime / mtimecmp).
//!
//! `mtime` is *not* a free-running register here: the machine derives it
//! from the hart's cycle counter (`mtime = cycles / cycles_per_tick`), so
//! emulated time is fully deterministic and tied to the cycle model.

pub const MSIP: u64 = 0x0000;
pub const MTIMECMP: u64 = 0x4000;
pub const MTIME: u64 = 0xbff8;

pub struct Clint {
    pub msip: bool,
    pub mtimecmp: u64,
    pub mtime: u64,
    /// Offset applied when software writes mtime directly.
    pub mtime_offset: u64,
}

impl Default for Clint {
    fn default() -> Self {
        Self::new()
    }
}

impl Clint {
    pub fn new() -> Clint {
        Clint {
            msip: false,
            mtimecmp: u64::MAX,
            mtime: 0,
            mtime_offset: 0,
        }
    }

    pub fn timer_pending(&self) -> bool {
        self.mtime >= self.mtimecmp
    }

    fn read_reg(reg: u64, off: u64, len: u64) -> u64 {
        let shift = (off & 7) * 8;
        let v = reg >> shift;
        if len >= 8 {
            v
        } else {
            v & ((1u64 << (len * 8)) - 1)
        }
    }

    fn write_reg(reg: &mut u64, off: u64, len: u64, val: u64) {
        let shift = (off & 7) * 8;
        let mask = if len >= 8 { u64::MAX } else { ((1u64 << (len * 8)) - 1) << shift };
        *reg = (*reg & !mask) | ((val << shift) & mask);
    }

    pub fn read(&self, off: u64, len: u64) -> Option<u64> {
        match off {
            0..=3 => Some(Self::read_reg(self.msip as u64, off, len)),
            MTIMECMP..=0x4007 => Some(Self::read_reg(self.mtimecmp, off, len)),
            MTIME..=0xbfff => Some(Self::read_reg(self.mtime, off, len)),
            _ => None,
        }
    }

    pub fn write(&mut self, off: u64, len: u64, val: u64) -> bool {
        match off {
            0..=3 => {
                self.msip = (val & 1) != 0 && off == 0;
                true
            }
            MTIMECMP..=0x4007 => {
                Self::write_reg(&mut self.mtimecmp, off, len, val);
                true
            }
            MTIME..=0xbfff => {
                let mut t = self.mtime;
                Self::write_reg(&mut t, off, len, val);
                // Remember the delta so the cycle-derived clock continues from here.
                self.mtime_offset = self.mtime_offset.wrapping_add(t.wrapping_sub(self.mtime));
                self.mtime = t;
                true
            }
            _ => false,
        }
    }
}
