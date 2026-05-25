//! Minimal ELF64 little-endian RISC-V loader (PT_LOAD segments + symbols).

use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElfError {
    TooShort,
    BadMagic,
    NotElf64Le,
    NotRiscv,
    Truncated(&'static str),
}

impl fmt::Display for ElfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ElfError::TooShort => write!(f, "file too short to be ELF"),
            ElfError::BadMagic => write!(f, "bad ELF magic"),
            ElfError::NotElf64Le => write!(f, "not a 64-bit little-endian ELF"),
            ElfError::NotRiscv => write!(f, "ELF machine is not RISC-V"),
            ElfError::Truncated(what) => write!(f, "truncated ELF ({what})"),
        }
    }
}

impl std::error::Error for ElfError {}

#[derive(Debug, Clone)]
pub struct Segment {
    pub paddr: u64,
    pub data: Vec<u8>,
    pub memsz: u64,
}

#[derive(Debug, Clone)]
pub struct Elf {
    pub entry: u64,
    pub segments: Vec<Segment>,
    pub symbols: HashMap<String, u64>,
}

fn u16_at(b: &[u8], o: usize) -> Result<u16, ElfError> {
    b.get(o..o + 2)
        .map(|s| u16::from_le_bytes(s.try_into().unwrap()))
        .ok_or(ElfError::Truncated("u16"))
}
fn u32_at(b: &[u8], o: usize) -> Result<u32, ElfError> {
    b.get(o..o + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        .ok_or(ElfError::Truncated("u32"))
}
fn u64_at(b: &[u8], o: usize) -> Result<u64, ElfError> {
    b.get(o..o + 8)
        .map(|s| u64::from_le_bytes(s.try_into().unwrap()))
        .ok_or(ElfError::Truncated("u64"))
}

const PT_LOAD: u32 = 1;
const SHT_SYMTAB: u32 = 2;
const EM_RISCV: u16 = 243;

impl Elf {
    pub fn parse(b: &[u8]) -> Result<Elf, ElfError> {
        if b.len() < 64 {
            return Err(ElfError::TooShort);
        }
        if &b[0..4] != b"\x7fELF" {
            return Err(ElfError::BadMagic);
        }
        if b[4] != 2 || b[5] != 1 {
            return Err(ElfError::NotElf64Le);
        }
        if u16_at(b, 18)? != EM_RISCV {
            return Err(ElfError::NotRiscv);
        }
        let entry = u64_at(b, 24)?;
        let phoff = u64_at(b, 32)? as usize;
        let shoff = u64_at(b, 40)? as usize;
        let phentsize = u16_at(b, 54)? as usize;
        let phnum = u16_at(b, 56)? as usize;
        let shentsize = u16_at(b, 58)? as usize;
        let shnum = u16_at(b, 60)? as usize;

        let mut segments = Vec::new();
        for i in 0..phnum {
            let p = phoff + i * phentsize;
            if u32_at(b, p)? != PT_LOAD {
                continue;
            }
            let offset = u64_at(b, p + 8)? as usize;
            let paddr = u64_at(b, p + 24)?;
            let filesz = u64_at(b, p + 32)? as usize;
            let memsz = u64_at(b, p + 40)?;
            let data = b.get(offset..offset + filesz).ok_or(ElfError::Truncated("segment"))?.to_vec();
            segments.push(Segment { paddr, data, memsz });
        }

        let mut symbols = HashMap::new();
        for i in 0..shnum {
            let s = shoff + i * shentsize;
            if u32_at(b, s + 4)? != SHT_SYMTAB {
                continue;
            }
            let off = u64_at(b, s + 24)? as usize;
            let size = u64_at(b, s + 32)? as usize;
            let link = u32_at(b, s + 40)? as usize;
            let entsize = (u64_at(b, s + 56)? as usize).max(24);
            let strtab_hdr = shoff + link * shentsize;
            let str_off = u64_at(b, strtab_hdr + 24)? as usize;
            for k in 0..size / entsize {
                let e = off + k * entsize;
                let name_off = u32_at(b, e)? as usize;
                let value = u64_at(b, e + 8)?;
                let start = str_off + name_off;
                if let Some(rest) = b.get(start..) {
                    let end = rest.iter().position(|&c| c == 0).unwrap_or(0);
                    if end > 0 {
                        let name = String::from_utf8_lossy(&rest[..end]).into_owned();
                        symbols.insert(name, value);
                    }
                }
            }
        }
        Ok(Elf { entry, segments, symbols })
    }
}
