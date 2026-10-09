//! Optional, deliberately untimed diagnostics. No instruction lookup is added
//! to the scalar hit path. History describes retranslation, not every lookup.
use iced_x86::{Code, Instruction};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TranslationDiagnosticsSummary {
    pub cold: u64,
    pub evicted: u64,
    pub epoch_reuse: u64,
    pub byte_changes: u64,
    pub mapping_invalidations: u64,
}

struct History {
    generations: (u64, u64),
    bytes: [u8; 15],
    len: usize,
}

#[derive(Default)]
pub(super) struct Diagnostics {
    pub summary: TranslationDiagnosticsSummary,
    history: BTreeMap<u64, History>,
    translations: BTreeMap<(u64, u32), u64>,
    fallback: BTreeMap<((u64, u32), Code), u64>,
    // Per executable page: writes, writes that actually changed bytes.
    writes: BTreeMap<u64, (u64, u64)>,
}

impl Diagnostics {
    pub fn translated(
        &mut self,
        pc: u64,
        generations: (u64, u64),
        bytes: &[u8],
        origin: (u64, u32),
    ) {
        match self.history.get(&pc) {
            None => self.summary.cold += 1,
            Some(old) if old.len != bytes.len() || old.bytes[..old.len] != *bytes => {
                self.summary.byte_changes += 1;
            }
            Some(old) if old.generations == generations => self.summary.evicted += 1,
            Some(_) => self.summary.epoch_reuse += 1,
        }
        let mut saved = [0; 15];
        saved[..bytes.len()].copy_from_slice(bytes);
        self.history.insert(
            pc,
            History {
                generations,
                bytes: saved,
                len: bytes.len(),
            },
        );
        *self.translations.entry(origin).or_default() += 1;
    }

    pub fn fallback(&mut self, i: &Instruction, origin: (u64, u32)) {
        *self.fallback.entry((origin, i.code())).or_default() += 1;
    }

    pub fn write(&mut self, page: u64, changed: bool) {
        let entry = self.writes.entry(page).or_default();
        entry.0 += 1;
        entry.1 += changed as u64;
    }

    pub fn report(&self, images: &BTreeMap<u64, String>) -> String {
        use std::fmt::Write;
        let s = self.summary;
        let mut out = format!(
            "AMD64 translation causes: cold={} evicted={} epoch_reuse={} byte_changes={} mapping_invalidations={}\n",
            s.cold, s.evicted, s.epoch_reuse, s.byte_changes, s.mapping_invalidations
        );
        let origin_name = |(base, kind)| {
            let category = if kind == crate::mem::MEM_IMAGE {
                "image"
            } else {
                "private-or-unknown"
            };
            if kind == crate::mem::MEM_IMAGE {
                if let Some(name) = images.get(&base) {
                    return format!("{category}:{name}@{base:#x}");
                }
            }
            format!("{category}@{base:#x}")
        };
        for (&origin, count) in &self.translations {
            writeln!(
                out,
                "AMD64 translations by origin: {}\t{count}",
                origin_name(origin)
            )
            .unwrap();
        }
        for (&(origin, code), count) in &self.fallback {
            writeln!(
                out,
                "AMD64 fallback work: {}\t{code:?}\t{count}",
                origin_name(origin)
            )
            .unwrap();
        }
        for (page, (writes, changes)) in &self.writes {
            writeln!(
                out,
                "AMD64 executable page writes: {page:#x}\twrites={writes}\tchanged={changes}"
            )
            .unwrap();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn eviction_cause_uses_only_both_instruction_page_dependencies() {
        let mut d = Diagnostics::default();
        let origin = (0x1000, 0);
        d.translated(0x1ffe, (7, 9), &[0x48, 0x89, 0xc0], origin);
        // An unrelated write changes the global epoch but not these versions.
        d.translated(0x1ffe, (7, 9), &[0x48, 0x89, 0xc0], origin);
        d.translated(0x1ffe, (7, 10), &[0x48, 0x89, 0xc0], origin);
        assert_eq!(d.summary.cold, 1);
        assert_eq!(d.summary.evicted, 1);
        assert_eq!(d.summary.epoch_reuse, 1);
    }
}
