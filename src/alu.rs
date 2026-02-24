//! Pure integer ALU and M-extension semantics.
//!
//! Kept separate from the executor so it can be property-tested against an
//! independent wide-integer reference model.

#[inline]
pub fn sext32(v: u64) -> u64 {
    v as u32 as i32 as i64 as u64
}

#[inline]
pub fn mulh(a: u64, b: u64) -> u64 {
    (((a as i64 as i128) * (b as i64 as i128)) >> 64) as u64
}
#[inline]
pub fn mulhu(a: u64, b: u64) -> u64 {
    (((a as u128) * (b as u128)) >> 64) as u64
}
#[inline]
pub fn mulhsu(a: u64, b: u64) -> u64 {
    (((a as i64 as i128).wrapping_mul(b as u128 as i128)) >> 64) as u64
}
#[inline]
pub fn div(a: u64, b: u64) -> u64 {
    let (a, b) = (a as i64, b as i64);
    if b == 0 {
        u64::MAX
    } else {
        a.wrapping_div(b) as u64
    }
}
#[inline]
pub fn divu(a: u64, b: u64) -> u64 {
    a.checked_div(b).unwrap_or(u64::MAX)
}
#[inline]
pub fn rem(a: u64, b: u64) -> u64 {
    let (a, b) = (a as i64, b as i64);
    if b == 0 {
        a as u64
    } else {
        a.wrapping_rem(b) as u64
    }
}
#[inline]
pub fn remu(a: u64, b: u64) -> u64 {
    if b == 0 {
        a
    } else {
        a % b
    }
}
#[inline]
pub fn divw(a: u64, b: u64) -> u64 {
    let (a, b) = (a as i32, b as i32);
    if b == 0 {
        u64::MAX
    } else {
        a.wrapping_div(b) as i64 as u64
    }
}
#[inline]
pub fn divuw(a: u64, b: u64) -> u64 {
    let (a, b) = (a as u32, b as u32);
    a.checked_div(b).map_or(u64::MAX, |q| q as i32 as i64 as u64)
}
#[inline]
pub fn remw(a: u64, b: u64) -> u64 {
    let (a, b) = (a as i32, b as i32);
    if b == 0 {
        a as i64 as u64
    } else {
        a.wrapping_rem(b) as i64 as u64
    }
}
#[inline]
pub fn remuw(a: u64, b: u64) -> u64 {
    let (a, b) = (a as u32, b as u32);
    if b == 0 {
        a as i32 as i64 as u64
    } else {
        (a % b) as i32 as i64 as u64
    }
}
#[inline]
pub fn mulw(a: u64, b: u64) -> u64 {
    sext32((a as u32).wrapping_mul(b as u32) as u64)
}

#[inline]
pub fn sll(a: u64, b: u64) -> u64 {
    a << (b & 63)
}
#[inline]
pub fn srl(a: u64, b: u64) -> u64 {
    a >> (b & 63)
}
#[inline]
pub fn sra(a: u64, b: u64) -> u64 {
    ((a as i64) >> (b & 63)) as u64
}
#[inline]
pub fn sllw(a: u64, b: u64) -> u64 {
    sext32(((a as u32) << (b & 31)) as u64)
}
#[inline]
pub fn srlw(a: u64, b: u64) -> u64 {
    sext32(((a as u32) >> (b & 31)) as u64)
}
#[inline]
pub fn sraw(a: u64, b: u64) -> u64 {
    ((a as i32) >> (b & 31)) as i64 as u64
}
#[inline]
pub fn slt(a: u64, b: u64) -> u64 {
    ((a as i64) < (b as i64)) as u64
}
#[inline]
pub fn sltu(a: u64, b: u64) -> u64 {
    (a < b) as u64
}
