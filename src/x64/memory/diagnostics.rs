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
    count: u64,
}

#[derive(Default)]
pub(super) struct Diagnostics {
    v2: V2,

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
        let count = self.history.get(&pc).map_or(1, |h| h.count + 1);
        let mut saved = [0; 15];
        saved[..bytes.len()].copy_from_slice(bytes);
        self.history.insert(
            pc,
            History {
                generations,
                bytes: saved,
                len: bytes.len(),
                count,
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
        if super::BUILD_DIAGNOSTICS_V2 {
            self.v2.report(&mut out);
            let mut pcs: Vec<_> = self.history.iter().map(|(&pc, h)| (pc, h.count)).collect();
            pcs.sort_unstable_by_key(|&(pc, count)| (std::cmp::Reverse(count), pc));
            for (pc, count) in pcs.into_iter().take(64) {
                writeln!(out, "AMD64 top translation PC: {pc:#x}\t{count}").unwrap();
            }
            let mut pages = BTreeMap::<u64, u64>::new();
            for (&pc, h) in &self.history {
                *pages.entry(pc & !4095).or_default() += h.count;
            }
            let mut pages: Vec<_> = pages.into_iter().collect();
            pages.sort_unstable_by_key(|&(pc, count)| (std::cmp::Reverse(count), pc));
            for (pc, count) in pages.into_iter().take(64) {
                writeln!(out, "AMD64 top translation page: {pc:#x}\t{count}").unwrap();
            }
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

// Compile-time gated: the ordinary build eliminates callers and has no per-hit
// telemetry branch, allocation, formatting, or refcell access.
const WINDOW: u64 = 262144;
const STARTS: [u64; 4] = [0, 10_000_000, 100_000_000, 1_000_000_000];
#[derive(Default)]
struct V2 {
    requests: u64,
    events: Vec<(u64, u8, u64, u64, u64, u64)>,
    lookups: [u64; 7],
    hits: [u64; 7],
    segments: [u64; 17],
    rejects: [u64; 12],
    attempts: u64,
    builds: u64,
    segment_work: u64,
    links: [u64; 4],     // valid, changed target, negative, stale slot/version
    data: [[u64; 8]; 5], // width 1/2/4/8/16/other/cross/page-cache-hit
    data_misses: [u64; 5],
    operands: [u64; 2],
}
impl Diagnostics {
    pub fn recording(&self) -> bool {
        let n = self.v2.requests;
        STARTS.iter().any(|&s| n >= s && n - s < WINDOW)
    }
    pub fn crossing(&self, pc: u64) -> bool {
        self.history
            .get(&pc)
            .is_some_and(|h| h.len as u64 > 4096 - (pc & 4095))
    }
    pub fn request(&mut self, kind: u8, pc: u64, hit: bool, epoch: u64, generations: (u64, u64)) {
        let n = self.v2.requests;
        self.v2.requests += 1;
        self.v2.lookups[kind as usize] += 1;
        self.v2.hits[kind as usize] += hit as u64;
        if STARTS.iter().any(|&s| n >= s && n - s < WINDOW) {
            self.v2.events.push((
                n,
                kind | ((hit as u8) << 7),
                pc,
                epoch,
                generations.0,
                generations.1,
            ));
        }
    }
    pub fn segment_attempt(&mut self) {
        self.v2.attempts += 1;
    }
    pub fn segment_reject(&mut self, kind: usize) {
        self.v2.rejects[kind] += 1;
    }
    pub fn segment_run(&mut self, len: usize) {
        self.v2.segments[len] += 1;
        self.v2.segment_work += len as u64;
        self.v2.operands[0] += len as u64;
    }
    pub fn link(&mut self, kind: usize) {
        self.v2.links[kind] += 1;
    }
    pub fn operands(&mut self, memory: bool) {
        self.v2.operands[memory as usize] += 1;
    }
    pub fn segment_build(&mut self, bytes: &[u8], pc: u64) {
        use iced_x86::{Decoder, DecoderOptions, OpKind};
        self.v2.builds += 1;
        let mut decoder = Decoder::with_ip(64, bytes, pc, DecoderOptions::NONE);
        let mut n = 0;
        let mut reason = 3; // same-page end
        while n < 16 && decoder.can_decode() {
            let i = decoder.decode();
            if i.is_invalid() {
                reason = 10;
                break;
            }
            let ir = super::super::cpu::ir::ScalarIr::lower(&i);
            if !ir.is_some_and(|ir| ir.register_only()) {
                reason = if (0..i.op_count()).any(|op| i.op_kind(op) == OpKind::Memory) {
                    6
                } else if ir.is_some_and(|ir| !ir.is_scalar()) {
                    5
                } else {
                    4
                };
                break;
            }
            n += 1;
            if matches!(
                i.mnemonic(),
                iced_x86::Mnemonic::Jmp
                    | iced_x86::Mnemonic::Je
                    | iced_x86::Mnemonic::Jne
                    | iced_x86::Mnemonic::Ja
                    | iced_x86::Mnemonic::Jae
                    | iced_x86::Mnemonic::Jb
                    | iced_x86::Mnemonic::Jbe
                    | iced_x86::Mnemonic::Jg
                    | iced_x86::Mnemonic::Jge
                    | iced_x86::Mnemonic::Jl
                    | iced_x86::Mnemonic::Jle
                    | iced_x86::Mnemonic::Jo
                    | iced_x86::Mnemonic::Jno
                    | iced_x86::Mnemonic::Js
                    | iced_x86::Mnemonic::Jns
                    | iced_x86::Mnemonic::Jp
                    | iced_x86::Mnemonic::Jnp
            ) {
                reason = 8;
                break;
            }
        }
        if n == 16 {
            reason = 9;
        }
        self.v2.rejects[reason] += 1;
        if n < 4 {
            self.v2.rejects[0] += 1;
        }
    }
    pub fn data(&mut self, kind: usize, width: usize, cross: bool, hit: bool) {
        let index = match width {
            1 => 0,
            2 => 1,
            4 => 2,
            8 => 3,
            16 => 4,
            _ => 5,
        };
        self.v2.data[kind][index] += 1;
        self.v2.data[kind][6] += cross as u64;
        self.v2.data[kind][7] += hit as u64;
        self.v2.data_misses[kind] += !hit as u64;
    }
}
impl V2 {
    fn report(&self, out: &mut String) {
        use std::fmt::Write;
        writeln!(
            out,
            "AMD64 V2 segments: attempts={} builds={} work={} lengths={:?} stops={:?}",
            self.attempts, self.builds, self.segment_work, self.segments, self.rejects
        )
        .unwrap();
        writeln!(out,"AMD64 V2 stop names: short,budget,observed-PC,page,unsupported,vector,memory,string,terminal,full,invalid,reserved").unwrap();
        writeln!(
            out,
            "AMD64 V2 links: valid={} changed={} negative={} stale={}",
            self.links[0], self.links[1], self.links[2], self.links[3]
        )
        .unwrap();
        writeln!(
            out,
            "AMD64 V2 operands: register={} memory={}",
            self.operands[0], self.operands[1]
        )
        .unwrap();
        for kind in 0..7 {
            writeln!(
                out,
                "AMD64 V2 lookups: kind={kind} requests={} hits={}",
                self.lookups[kind], self.hits[kind]
            )
            .unwrap();
        }
        for kind in 0..5 {
            writeln!(
                out,
                "AMD64 V2 data: kind={kind} widths_cross_hit={:?} misses={}",
                self.data[kind], self.data_misses[kind]
            )
            .unwrap();
        }
        writeln!(out,"AMD64 V2 window columns: sequence,kind(+128=hit),PC,epoch,page-generation,second-generation; starts={STARTS:?}; length={WINDOW}").unwrap();
        for &(n, k, pc, e, g, g2) in &self.events {
            writeln!(out, "AMD64 V2 request: {n},{k},{pc:#x},{e},{g},{g2}").unwrap();
        }
    }
}

#[cfg(test)]
mod v2_tests {
    use super::*;
    #[test]
    fn request_windows_include_hits_and_are_bounded_and_ordered() {
        let mut d = Diagnostics::default();
        for n in 0..WINDOW + 20 {
            d.request(0, 0x1000 + n % 3, n % 4 != 0, 2, (7, 0));
        }
        assert_eq!(d.v2.events.len(), WINDOW as usize);
        assert_eq!(d.v2.hits[0], (WINDOW + 20) * 3 / 4);
        d.v2.requests = STARTS[1];
        d.request(3, 0x2000, true, 3, (9, 0));
        assert_eq!(d.v2.events.last().unwrap().0, STARTS[1]);
        assert_eq!(d.v2.events.last().unwrap().1, 131);
        assert_eq!(d.v2.events.last().unwrap().4, 9);
    }
}
