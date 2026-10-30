//! Completed IOSB results use the OVERLAPPED record, independently of hFile.
// Derived from Wine 10.0 dlls/kernelbase/file.c under LGPL-2.1-or-later.
// Copyright 1993 John Burton; 1996, 2004 Alexandre Julliard; 2008 Jeff Zaroyko.
use super::modules::Failure;
use crate::memory::{Memory, WRITE};

pub(super) fn query(a: &[u64; 16], memory: &mut Memory, error: u64) -> Result<u64, Failure> {
    let width = memory.mode() as usize / 8;
    let status = memory.read_int(a[1], 4)? as u32;
    if status == 0x103 {
        return Err(Failure::Windows(if a[3] == 0 { 996 } else { 50 }));
    }
    let code = match status {
        0 => 0,
        0x80000005 => 234,
        0xc0000001 => 31,
        0xc0000008 => 6,
        0xc000000d => 87,
        0xc000000f | 0xc0000034 => 2,
        0xc0000011 => 38,
        0xc0000017 => 8,
        0xc0000022 => 5,
        0xc000003a => 3,
        0xc000007f => 112,
        0xc00000b0 => 233,
        0xc000014b => 109,
        0xc00000bb => 50,
        0xc0000120 => 995,
        s if s & 0xffff0000 == 0xc0070000 => s & 0xffff,
        _ => return Err(Failure::Windows(50)),
    };
    let count = memory.read_int(
        a[1].checked_add(width as u64).ok_or(Failure::Windows(87))?,
        width,
    )?;
    memory.check(a[2], 4, WRITE)?;
    if status != 0 {
        memory.check(error, 4, WRITE)?;
    }
    memory.write_int(a[2], 4, count as u32 as u64)?;
    if status == 0 {
        Ok(1)
    } else {
        Err(Failure::Windows(code))
    }
}
