//! Platform-Level Interrupt Controller with two contexts:
//! context 0 = hart 0 M-mode, context 1 = hart 0 S-mode.

pub const NUM_SOURCES: usize = 32;

pub struct Plic {
    pub priority: [u32; NUM_SOURCES],
    pub pending: u32,
    pub enable: [u32; 2],
    pub threshold: [u32; 2],
    /// Sources currently claimed and not yet completed.
    pub in_service: u32,
}

impl Default for Plic {
    fn default() -> Self {
        Self::new()
    }
}

impl Plic {
    pub fn new() -> Plic {
        Plic {
            priority: [0; NUM_SOURCES],
            pending: 0,
            enable: [0; 2],
            threshold: [0; 2],
            in_service: 0,
        }
    }

    /// Level-triggered input line from a device.
    pub fn set_irq(&mut self, src: usize, level: bool) {
        if src == 0 || src >= NUM_SOURCES {
            return;
        }
        if level {
            self.pending |= 1 << src;
        } else {
            self.pending &= !(1 << src);
        }
    }

    fn best(&self, ctx: usize) -> u32 {
        let mut best = 0u32;
        let mut best_prio = self.threshold[ctx];
        let cand = self.pending & self.enable[ctx] & !self.in_service;
        if cand == 0 {
            return 0;
        }
        for src in 1..NUM_SOURCES {
            if cand & (1 << src) != 0 && self.priority[src] > best_prio {
                best_prio = self.priority[src];
                best = src as u32;
            }
        }
        best
    }

    /// Whether context `ctx` has an interrupt to deliver.
    pub fn irq_for(&self, ctx: usize) -> bool {
        self.best(ctx) != 0
    }

    pub fn read(&mut self, off: u64) -> Option<u64> {
        let v = match off {
            0..=0x7f => self.priority[(off / 4) as usize],
            0x1000 => self.pending,
            0x2000 => self.enable[0],
            0x2080 => self.enable[1],
            0x20_0000 => self.threshold[0],
            0x20_1000 => self.threshold[1],
            0x20_0004 | 0x20_1004 => {
                let ctx = ((off >> 12) & 1) as usize;
                let src = self.best(ctx);
                if src != 0 {
                    self.in_service |= 1 << src;
                }
                src
            }
            _ => 0,
        };
        Some(v as u64)
    }

    pub fn write(&mut self, off: u64, val: u64) -> bool {
        let val = val as u32;
        match off {
            0..=0x7f => self.priority[(off / 4) as usize] = val & 7,
            0x2000 => self.enable[0] = val & !1,
            0x2080 => self.enable[1] = val & !1,
            0x20_0000 => self.threshold[0] = val & 7,
            0x20_1000 => self.threshold[1] = val & 7,
            0x20_0004 | 0x20_1004 if (val as usize) < NUM_SOURCES => self.in_service &= !(1 << val),
            _ => {}
        }
        true
    }
}
