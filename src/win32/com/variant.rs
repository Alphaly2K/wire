/*
 * BSTR/VARIANT operations adapted from Hangover's Wine oleaut32/oleaut.c,
 * commit 3ea3a14941b6a122b892c84141ce9662c008081e.
 * Copyright 1999, 2000 Marcus Meissner
 * LGPL-2.1-or-later; see third_party/hangover/README.md.
 */
use crate::win32::catalog::VariantApi;

pub(crate) const INVALID: u32 = 0x80070057;
pub(crate) trait Backend {
    type Error;
    const SIZE: usize;
    const POINTER: usize;
    fn check(&self, p: u64, n: usize) -> Result<(), Self::Error>;
    fn read(&self, p: u64, bytes: &mut [u8]) -> Result<(), Self::Error>;
    fn write(&mut self, p: u64, bytes: &[u8]) -> Result<(), Self::Error>;
    fn string(&mut self, source: Option<u64>) -> Result<u64, Self::Error>;
    fn release(&mut self, pointer: u64, output: u64, size: usize) -> Result<(), Self::Error>;
    fn scalar(&self, vt: u16) -> bool;
    fn windows(code: u32) -> Self::Error;
}
fn pointer<B: Backend>(bytes: &[u8]) -> u64 {
    let mut value = [0; 8];
    value[..B::POINTER].copy_from_slice(&bytes[8..8 + B::POINTER]);
    u64::from_le_bytes(value)
}
fn clear<B: Backend>(mem: &mut B, p: u64) -> Result<(), B::Error> {
    let mut tag = [0; 2];
    mem.read(p, &mut tag)?;
    let vt = u16::from_le_bytes(tag);
    if vt == 8 {
        let mut bytes = [0; 24];
        mem.read(p + 8, &mut bytes[8..8 + B::POINTER])?;
        mem.release(pointer::<B>(&bytes), p, B::SIZE)?;
    } else if !mem.scalar(vt) && vt & 0x4000 == 0 {
        return Err(B::windows(0x80020008));
    }
    Ok(())
}
pub(crate) fn call<B: Backend>(mem: &mut B, id: VariantApi, a: &[u64]) -> Result<u64, B::Error> {
    use VariantApi::*;
    match id {
        Copy | ChangeType => {
            mem.check(a[0], B::SIZE)?;
            let mut bytes = [0; 24];
            mem.read(a[1], &mut bytes[..B::SIZE])?;
            let vt = u16::from_le_bytes(bytes[..2].try_into().unwrap());
            let target = if id == Copy { vt } else { a[3] as u16 };
            let allocation = if vt == 8 && target == 8 {
                Some(mem.string(Some(pointer::<B>(&bytes)))?)
            } else if vt == 0 && target == 8 {
                bytes.fill(0);
                bytes[0] = 8;
                Some(mem.string(None)?)
            } else if vt != target || !mem.scalar(vt) {
                return Err(B::windows(0x80020005));
            } else {
                None
            };
            if let Some(p) = allocation {
                bytes[8..8 + B::POINTER].copy_from_slice(&p.to_le_bytes()[..B::POINTER]);
            }
            if let Err(error) = clear(mem, a[0]) {
                if let Some(p) = allocation {
                    let _ = mem.release(p, 0, 0);
                }
                return Err(error);
            }
            mem.write(a[0], &bytes[..B::SIZE])?;
        }
        Initialize => mem.write(a[0], &[0; 24][..B::SIZE])?,
        Clear => {
            mem.check(a[0], B::SIZE)?;
            clear(mem, a[0])?;
            mem.write(a[0], &[0; 2])?;
        }
    }
    Ok(0)
}
