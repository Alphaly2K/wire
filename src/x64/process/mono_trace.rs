//! Opt-in observations of original Mono export entries, never ABI admission.
//! Calls continue through the original guest VM; samples are bounded and raw.
use super::{Cpu64, Exports64, Memory};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
};

const MAX_ENTRIES: usize = 8192;
const FIRST_SAMPLES: u64 = 1024;
const PER_ENTRY_SAMPLES: u64 = 8;
const MAX_SAMPLES: usize = 8192;

#[derive(Default)]
struct Entry {
    names: Vec<String>,
    hits: u64,
    startup_kind: u8,
}
#[derive(Default)]
pub(super) struct MonoTrace {
    entries: BTreeMap<u64, Entry>,
    first: u64,
    last: u64,
    total: u64,
    samples: Vec<String>,
    omitted: u64,
    unindexed_callbacks: u64,
    bindings: BTreeMap<String, BTreeSet<u64>>,
    unavailable_bindings: u64,
    binding_targets: usize,
}
impl MonoTrace {
    pub(super) fn register(&mut self, module: &str, exports: &Exports64) -> Result<(), String> {
        if !matches!(
            module,
            "mono-2.0-bdwgc.dll" | "mono-2.0-sgen.dll" | "mono.dll"
        ) {
            return Ok(());
        }
        // Reserve before modifying the index, including aliases at one PC.
        let count = exports
            .names
            .values()
            .filter(|pc| !self.entries.contains_key(pc))
            .collect::<BTreeSet<_>>()
            .len();
        if self.entries.len().saturating_add(count) > MAX_ENTRIES {
            return Err("Mono export observation capacity exceeded".into());
        }
        for (name, &pc) in &exports.names {
            let entry = self.entries.entry(pc).or_default();
            entry.names.push(format!("{module}!{}", clean(name)));
            entry.startup_kind |= match name.as_str() {
                "mono_unity_install_memory_callbacks" => 1,
                "mono_set_find_plugin_callback" => 2,
                "mono_trace_set_log_handler" => 4,
                "mono_unity_set_vprintf_func" => 8,
                "mono_add_internal_call" => 16,
                _ => 0,
            };
        }
        self.first = self
            .entries
            .first_key_value()
            .map_or(u64::MAX, |(&pc, _)| pc);
        self.last = self.entries.last_key_value().map_or(0, |(&pc, _)| pc);
        Ok(())
    }
    pub(super) fn has_entry(&self, start: u64, end: u64) -> bool {
        end > start
            && end > self.first
            && start <= self.last
            && self.entries.range(start..end).next().is_some()
    }
    pub(super) fn contains(&self, pc: u64) -> bool {
        pc >= self.first && pc <= self.last && self.entries.contains_key(&pc)
    }
    pub(super) fn record(&mut self, cpu: &Cpu64, memory: &Memory, thread: u64) {
        if !self.contains(cpu.rip) {
            return;
        }
        self.observe_callback_entries(cpu, memory);
        let entry = self.entries.get_mut(&cpu.rip).unwrap();
        entry.hits += 1;
        self.total += 1;
        if self.samples.len() >= MAX_SAMPLES
            || (entry.hits > PER_ENTRY_SAMPLES && self.total > FIRST_SAMPLES)
        {
            self.omitted += 1;
            return;
        }
        let names = entry.names.join("|");
        let mut row = format!(
            "sample\t{:#x}\t{}\t{}\t{}\t{}\t{}",
            cpu.rip, names, entry.hits, thread, cpu.work, cpu.retired
        );
        for reg in [1, 2, 8, 9, 4] {
            write!(row, "\t{:#x}", cpu.regs[reg]).unwrap();
        }
        for xmm in &cpu.xmm[..4] {
            write!(row, "\t{}", hex(xmm)).unwrap();
        }
        // Slots are raw observations, not inferred typed parameters. Capture
        // each separately so one guarded slot cannot hide the return address.
        for offset in [0, 40, 48, 56, 64, 72, 80, 88, 96] {
            let value = cpu.regs[4]
                .checked_add(offset)
                .ok_or_else(|| "address overflow".to_owned())
                .and_then(|pc| observe(memory, pc, 8));
            write!(
                row,
                "\t{}",
                value.unwrap_or_else(|error| format!("unavailable:{}", clean(&error)))
            )
            .unwrap();
        }
        let mut observations = Vec::new();
        // Capture only raw bytes for known startup inputs. The legacy and new
        // allocator names remain distinct. Opaque UnityTLS bytes are not a
        // claim that its complete layout is understood or compatible.
        for name in &entry.names {
            let function = name.split_once('!').unwrap().1;
            let captures: &[(usize, usize)] = match function {
                "mono_unity_install_memory_callbacks" => &[(1, 40)],
                "unity_mono_install_memory_callbacks" => &[(1, 32)],
                "mono_unity_install_unitytls_interface" => &[(1, 128)],
                _ => &[],
            };
            for &(reg, size) in captures {
                let bytes = observe(memory, cpu.regs[reg], size)
                    .unwrap_or_else(|e| format!("unavailable:{}", clean(&e)));
                observations.push(format!("{function}:r{reg}@{:#x}:{bytes}", cpu.regs[reg]));
            }
            let string_regs: &[usize] = match function {
                "mono_set_dirs" | "mono_jit_init_version" => &[1, 2],
                "mono_jit_init"
                | "mono_set_assemblies_path"
                | "mono_config_parse"
                | "mono_add_internal_call"
                | "mono_domain_create_appdomain" => &[1],
                _ => &[],
            };
            for &reg in string_regs {
                let bytes = c_string(memory, cpu.regs[reg])
                    .unwrap_or_else(|e| format!("unavailable:{}", clean(&e)));
                observations.push(format!(
                    "{function}:string.r{reg}@{:#x}:{bytes}",
                    cpu.regs[reg]
                ));
            }
        }
        writeln!(row, "\t{}", observations.join(";")).unwrap();
        self.samples.push(row);
    }
    fn observe_callback_entries(&mut self, cpu: &Cpu64, memory: &Memory) {
        let kind = self.entries[&cpu.rip].startup_kind;
        if kind == 0 {
            return;
        }
        if kind & 16 != 0 {
            match c_string_bytes(memory, cpu.regs[1], 1024) {
                Ok(name) => {
                    let key = hex(&name);
                    let targets = self.bindings.get(&key);
                    if targets.is_some_and(|targets| targets.contains(&cpu.regs[2])) {
                        // Repeated registrations do not consume capacity.
                    } else if self.binding_targets >= 32768
                        || targets.is_some_and(|targets| targets.len() >= 8)
                    {
                        self.unavailable_bindings += 1;
                    } else {
                        self.bindings.entry(key).or_default().insert(cpu.regs[2]);
                        self.binding_targets += 1;
                    }
                }
                Err(_) => self.unavailable_bindings += 1,
            }
        }
        let mut callbacks = Vec::new();
        if kind & 1 != 0 {
            let mut table = [0; 40];
            if memory.observe(cpu.regs[1], &mut table).is_ok()
                && u32::from_le_bytes(table[..4].try_into().unwrap()) == 1
            {
                for (index, name) in ["malloc", "realloc", "free", "calloc"]
                    .into_iter()
                    .enumerate()
                {
                    let offset = 8 + index * 8;
                    let pc = u64::from_le_bytes(table[offset..offset + 8].try_into().unwrap());
                    callbacks.push((pc, format!("observed-callback!mono_allocator.{name}")));
                }
            }
        }
        for (flag, label) in [(2, "find_plugin"), (4, "log_handler"), (8, "vprintf")] {
            if kind & flag != 0 {
                callbacks.push((cpu.regs[1], format!("observed-callback!{label}")));
            }
        }
        for (pc, name) in callbacks {
            // This is an observation index, not permission to call native code.
            // Null/noncanonical values stay in raw samples, never become PCs.
            if pc == 0 || pc >= 1 << 47 {
                continue;
            }
            if !self.entries.contains_key(&pc) && self.entries.len() >= MAX_ENTRIES {
                self.unindexed_callbacks += 1;
                continue;
            }
            let entry = self.entries.entry(pc).or_default();
            if !entry.names.contains(&name) {
                entry.names.push(name);
            }
            self.first = self.first.min(pc);
            self.last = self.last.max(pc);
        }
    }
    pub(super) fn tsv(&self) -> String {
        let mut out = format!("# original guest Mono and observed callback entries; no ABI admission; timing perturbed\n# total={} samples={} omitted={} unindexed_callbacks={}\n", self.total, self.samples.len(), self.omitted, self.unindexed_callbacks);
        writeln!(out, "# binding_names={} unavailable_bindings={}; binding aliases column is UTF-8 name bytes in hex, pc is raw guest target", self.bindings.len(), self.unavailable_bindings).unwrap();
        out.push_str("kind\tpc\taliases\thits\tthread\twork\tretired\trcx\trdx\tr8\tr9\trsp\txmm0_bytes\txmm1_bytes\txmm2_bytes\txmm3_bytes\treturn_bytes\tstack40_bytes\tstack48_bytes\tstack56_bytes\tstack64_bytes\tstack72_bytes\tstack80_bytes\tstack88_bytes\tstack96_bytes\tobservations\n");
        for (&pc, entry) in &self.entries {
            writeln!(
                out,
                "count\t{pc:#x}\t{}\t{}{}",
                entry.names.join("|"),
                entry.hits,
                "\t".repeat(22)
            )
            .unwrap();
        }
        for (name, targets) in &self.bindings {
            for target in targets {
                writeln!(
                    out,
                    "binding\t{target:#x}\t{name}\t{}{}",
                    targets.len(),
                    "\t".repeat(22)
                )
                .unwrap();
            }
        }
        for row in &self.samples {
            out.push_str(row);
        }
        out
    }
}
fn clean(text: &str) -> String {
    text.replace(['\t', '\r', '\n'], " ")
}
fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(out, "{byte:02x}").unwrap();
    }
    out
}
fn observe(memory: &Memory, address: u64, len: usize) -> Result<String, String> {
    let mut bytes = vec![0; len];
    memory.observe(address, &mut bytes)?;
    Ok(hex(&bytes))
}
fn c_string(memory: &Memory, address: u64) -> Result<String, String> {
    c_string_bytes(memory, address, 256).map(|bytes| hex(&bytes))
}
fn c_string_bytes(memory: &Memory, address: u64, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    for offset in 0..limit {
        let mut byte = [0];
        memory.observe(
            address.checked_add(offset).ok_or("address overflow")?,
            &mut byte,
        )?;
        if byte[0] == 0 {
            return Ok(bytes);
        }
        bytes.push(byte[0]);
    }
    Err(format!("string exceeds observation limit {limit}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x64::memory::{READ, WRITE};
    #[test]
    fn samples_are_bounded_while_every_entry_hit_is_counted() {
        let mut exports = Exports64::default();
        exports.names.insert("mono_probe".into(), 0x1000);
        let mut trace = MonoTrace::default();
        trace.register("mono.dll", &exports).unwrap();
        let cpu = Cpu64::new(0x1000, 0x2000);
        let memory = Memory::default();
        for _ in 0..2050 {
            trace.record(&cpu, &memory, 7);
        }
        assert_eq!(trace.entries[&0x1000].hits, 2050);
        assert_eq!(trace.samples.len(), FIRST_SAMPLES as usize);
        assert_eq!(trace.omitted, 1026);
    }
    #[test]
    fn installer_indexes_raw_guest_callbacks_and_collects_actual_internal_call_targets() {
        let mut exports = Exports64::default();
        exports
            .names
            .insert("mono_unity_install_memory_callbacks".into(), 0x1000);
        exports
            .names
            .insert("mono_add_internal_call".into(), 0x1100);
        let mut trace = MonoTrace::default();
        trace.register("mono.dll", &exports).unwrap();
        let mut memory = Memory::default();
        memory.map(0x20000, 4096, READ | WRITE).unwrap();
        memory.write_int(0x20000, 4, 1).unwrap();
        for (i, pc) in [0x3000, 0x3010, 0x3020, 0x3030].into_iter().enumerate() {
            memory.write_int(0x20008 + i as u64 * 8, 8, pc).unwrap();
        }
        let mut cpu = Cpu64::new(0x1000, 0x20100);
        cpu.regs[1] = 0x20000;
        trace.record(&cpu, &memory, 1);
        assert!(trace.has_entry(0x3000, 0x3001));
        cpu.rip = 0x3030;
        trace.record(&cpu, &memory, 1);
        assert_eq!(
            trace.entries[&0x3030].names,
            ["observed-callback!mono_allocator.calloc"]
        );
        cpu.rip = 0x1100;
        cpu.regs[1] = 0x20200;
        cpu.regs[2] = 0x4000;
        memory.write(0x20200, b"Wire.Test::Method\0").unwrap();
        trace.record(&cpu, &memory, 1);
        cpu.regs[2] = 0x5000;
        trace.record(&cpu, &memory, 1);
        let name = hex(b"Wire.Test::Method");
        assert_eq!(trace.bindings[&name], BTreeSet::from([0x4000, 0x5000]));
        memory
            .virtual_protect(0x20000, 4096, 4 | crate::mem::PAGE_GUARD)
            .unwrap();
        trace.record(&cpu, &memory, 1);
        assert_eq!(trace.unavailable_bindings, 1);
        assert!(memory
            .read_int(0x20200, 8)
            .unwrap_err()
            .contains("guard-page"));
        assert!(trace.tsv().contains(&format!("binding\t0x4000\t{name}\t2")));
        for target in 0x6000..0x6009 {
            cpu.regs[2] = target;
            trace.record(&cpu, &memory, 1);
        }
        assert_eq!(trace.bindings[&name].len(), 8);
        assert_eq!(trace.binding_targets, 8);
        assert_eq!(trace.unavailable_bindings, 4); // One fault plus three excess targets.
    }
}
