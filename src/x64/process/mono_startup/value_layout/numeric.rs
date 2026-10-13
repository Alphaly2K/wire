//! Numeric cells, canonical Boolean bytes and owned zero padding. Fixed-buffer
//! elements must have been expanded by their actual declaring metadata first.
use super::{Layout, Shape};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::x64::process::mono_startup) struct NumericLayout {
    pub(in crate::x64::process::mono_startup) class: usize,
    pub(in crate::x64::process::mono_startup) width: usize,
    pub(in crate::x64::process::mono_startup) alignment: usize,
    cells: Vec<(usize, usize)>,
    booleans: Vec<usize>,
}
impl Layout {
    pub(in crate::x64::process::mono_startup) fn numeric(&self) -> Result<NumericLayout, String> {
        self.check()?;
        if !self.value {
            return Err("numeric layout must be a value".into());
        }
        let mut cells = Vec::new();
        let mut booleans = Vec::new();
        for slot in &self.slots {
            match slot.shape {
                Shape::Scalar { boolean, .. } => {
                    if boolean {
                        booleans.push(slot.offset);
                    }
                }
                Shape::Inline { .. } => {}
                _ => return Err("numeric layout contains references or engine capabilities".into()),
            }
            cells.push((slot.offset, slot.shape.width()));
        }
        cells.sort_unstable();
        booleans.sort_unstable();
        Ok(NumericLayout {
            class: self.class as usize,
            width: self.width,
            alignment: self.alignment,
            cells,
            booleans,
        })
    }
}
impl NumericLayout {
    /// Validate all declared bytes; native padding is never exported. Engine
    /// padding is likewise discarded, since it carries no managed field value.
    pub(in crate::x64::process::mono_startup) fn validate(
        &self,
        source: &[u8],
    ) -> Result<(), String> {
        if source.len() != self.width || self.booleans.iter().any(|&offset| source[offset] > 1) {
            return Err("numeric payload width/Boolean mismatch".into());
        }
        Ok(())
    }
    pub(in crate::x64::process::mono_startup) fn canonicalize(
        &self,
        bytes: &mut [u8],
    ) -> Result<(), String> {
        self.validate(bytes)?;
        let mut prior = 0;
        for &(offset, width) in &self.cells {
            bytes[prior..offset].fill(0);
            prior = offset + width;
        }
        bytes[prior..].fill(0);
        Ok(())
    }
    #[cfg(test)]
    pub(in crate::x64::process::mono_startup) fn snapshot(
        &self,
        source: &[u8],
    ) -> Result<Vec<u8>, String> {
        let mut bytes = source.to_vec();
        self.canonicalize(&mut bytes)?;
        Ok(bytes)
    }
}
