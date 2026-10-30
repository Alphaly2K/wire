use super::{layout, pixels, Failure};
use crate::memory::{Memory, READ, WRITE};

pub(super) enum Storage {
    Owned(Vec<u32>),
    Dib { base: u64, layout: layout::Layout },
}
pub(super) struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub bpp: u32,
    pub storage: Storage,
    pub internal: bool,
}
impl Bitmap {
    pub fn size(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height) * 4
    }
    pub fn dib(&self) -> Option<(u64, layout::Layout)> {
        match self.storage {
            Storage::Dib { base, layout } => Some((base, layout)),
            _ => None,
        }
    }
    pub fn span(&self) -> u64 {
        self.size().div_ceil(4096) * 4096
    }
    pub fn owned(&self) -> &[u32] {
        match &self.storage {
            Storage::Owned(p) => p,
            _ => unreachable!("window backing is owned"),
        }
    }
    pub fn owned_mut(&mut self) -> &mut [u32] {
        match &mut self.storage {
            Storage::Owned(p) => p,
            _ => unreachable!("window backing is owned"),
        }
    }
    pub fn read(&self, memory: &Memory, x: u32, y: u32) -> Result<u32, Failure> {
        Ok(match self.storage {
            Storage::Owned(ref p) => p[(y * self.width + x) as usize],
            Storage::Dib { base, layout } => {
                layout::swap_rb(memory.read_int(base + layout.offset(x, y), 4)? as u32) & 0xffffff
            }
        })
    }
    pub fn write(&mut self, memory: &mut Memory, x: u32, y: u32, c: u32) -> Result<u32, Failure> {
        let value = pixels::color(c, self.bpp);
        match &mut self.storage {
            Storage::Owned(p) => p[(y * self.width + x) as usize] = value,
            Storage::Dib { base, layout } => {
                memory.write_int(
                    *base + layout.offset(x, y),
                    4,
                    u64::from(layout::swap_rb(value)),
                )?;
            }
        }
        Ok(value)
    }
    pub fn check_rect(&self, memory: &Memory, r: [u32; 4], write: bool) -> Result<(), Failure> {
        if let Storage::Dib { base, layout } = self.storage {
            for y in r[1]..r[3] {
                memory.check(
                    base + layout.offset(r[0], y),
                    (r[2] - r[0]) as usize * 4,
                    if write { WRITE } else { READ },
                )?;
            }
        }
        Ok(())
    }
    pub fn read_rect(&self, memory: &Memory, r: [u32; 4]) -> Result<Vec<u32>, Failure> {
        let width = (r[2] - r[0]) as usize;
        let count = width * (r[3] - r[1]) as usize;
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|_| Failure::Windows(8))?;
        match &self.storage {
            Storage::Owned(p) => {
                for y in r[1]..r[3] {
                    let at = (y * self.width + r[0]) as usize;
                    values.extend_from_slice(&p[at..at + width]);
                }
            }
            Storage::Dib { base, layout } => {
                let mut row = bytes(width * 4)?;
                for y in r[1]..r[3] {
                    memory.read(*base + layout.offset(r[0], y), &mut row)?;
                    values.extend(
                        row.as_chunks::<4>()
                            .0
                            .iter()
                            .map(|p| layout::swap_rb(u32::from_le_bytes(*p)) & 0xffffff),
                    );
                }
            }
        }
        Ok(values)
    }
    pub fn write_rect(
        &mut self,
        memory: &mut Memory,
        r: [u32; 4],
        values: &[u32],
    ) -> Result<(), Failure> {
        self.check_rect(memory, r, true)?;
        let width = (r[2] - r[0]) as usize;
        match &mut self.storage {
            Storage::Owned(p) => {
                for (y, row) in (r[1]..r[3]).zip(values.chunks_exact(width)) {
                    let at = (y * self.width + r[0]) as usize;
                    for (target, &source) in p[at..at + width].iter_mut().zip(row) {
                        *target = pixels::color(source, self.bpp);
                    }
                }
            }
            Storage::Dib { base, layout } => {
                let mut output = bytes(width * 4)?;
                for (y, row) in (r[1]..r[3]).zip(values.chunks_exact(width)) {
                    for (p, &c) in output.as_chunks_mut::<4>().0.iter_mut().zip(row) {
                        p.copy_from_slice(&layout::swap_rb(c).to_le_bytes());
                    }
                    memory.write(*base + layout.offset(r[0], y), &output)?;
                }
            }
        }
        Ok(())
    }
}
pub(super) fn bytes(count: usize) -> Result<Vec<u8>, Failure> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| Failure::Windows(8))?;
    result.resize(count, 0);
    Ok(result)
}
