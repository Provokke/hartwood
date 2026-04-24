//! Main memory: a flat little-endian byte array.

pub struct Ram {
    pub base: u64,
    pub data: Vec<u8>,
}

impl Ram {
    pub fn new(base: u64, size: usize) -> Ram {
        Ram { base, data: vec![0; size] }
    }

    #[inline]
    pub fn contains(&self, addr: u64, len: u64) -> bool {
        addr >= self.base && addr.wrapping_sub(self.base).wrapping_add(len) <= self.data.len() as u64
    }

    #[inline]
    pub fn read(&self, addr: u64, len: u64) -> u64 {
        let off = (addr - self.base) as usize;
        let d = &self.data;
        match len {
            1 => d[off] as u64,
            2 => u16::from_le_bytes([d[off], d[off + 1]]) as u64,
            4 => u32::from_le_bytes(d[off..off + 4].try_into().unwrap()) as u64,
            _ => u64::from_le_bytes(d[off..off + 8].try_into().unwrap()),
        }
    }

    #[inline]
    pub fn write(&mut self, addr: u64, len: u64, val: u64) {
        let off = (addr - self.base) as usize;
        let bytes = val.to_le_bytes();
        self.data[off..off + len as usize].copy_from_slice(&bytes[..len as usize]);
    }

    /// Copy a byte slice into memory (used by the loader).
    pub fn load(&mut self, addr: u64, bytes: &[u8]) -> bool {
        if !self.contains(addr, bytes.len() as u64) {
            return false;
        }
        let off = (addr - self.base) as usize;
        self.data[off..off + bytes.len()].copy_from_slice(bytes);
        true
    }
}
