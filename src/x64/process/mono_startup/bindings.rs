//! Provider-owned registration table. Unverified entries remain deferred;
//! their Win64 targets never become native Mono function pointers.
use super::super::super::memory::{Memory, EXECUTE};
use std::{collections::BTreeMap, ffi::CString};
pub(super) const SCALAR_NAMES: [&str; 2] = [
    "UnityEngineInternal.GIDebugVisualisation::get_cycleMode",
    "UnityEngineInternal.GIDebugVisualisation::get_pauseCycleMode",
];
const SCALAR_RVAS: [u64; 2] = [0x5c2f0, 0x5c310];
#[derive(Default)]
pub(super) struct Registry {
    entries: BTreeMap<CString, u64>,
    pub(super) scalar_targets: [u64; 2],
    sealed: bool,
}
impl Registry {
    pub(super) fn register(
        &mut self,
        memory: &Memory,
        name: CString,
        target: u64,
    ) -> Result<Option<usize>, String> {
        if self.sealed {
            return Err("Mono binding registry is sealed".into());
        }
        let text = name
            .to_str()
            .map_err(|_| "Mono binding name is not UTF-8")?;
        if text.is_empty() || name.as_bytes().len() > 1024 {
            return Err("Mono binding name outside 1..1024 bytes".into());
        }
        if !self.entries.contains_key(&name) && self.entries.len() >= 32768 {
            return Err("Mono binding registry capacity exceeded".into());
        }
        memory.check(target, 1, EXECUTE)?;
        let scalar = SCALAR_NAMES.iter().position(|known| *known == text);
        if let Some(slot) = scalar {
            // Exact UnityPlayer fingerprint was checked before loading. These
            // are static bool() declarations, checked again
            // against native metadata before diagnostic managed invocation.
            if target != 0x180000000 + SCALAR_RVAS[slot] {
                return Err(format!(
                    "Mono scalar target differs from exact Unity profile: {text}"
                ));
            }
            self.scalar_targets[slot] = target;
        }
        self.entries.insert(name, target);
        Ok(scalar)
    }
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }
    pub(super) fn seal(&mut self) -> Result<(), String> {
        if self.scalar_targets.contains(&0) {
            return Err("Unity scalar registrations incomplete".into());
        }
        self.sealed = true;
        Ok(())
    }
    pub(super) fn tsv(&self) -> String {
        let mut text =
            String::from("name\tguest_target\tnative_scalar_adapter\tfull_game_abi_admitted\n");
        for (name, target) in &self.entries {
            let name = name.to_string_lossy();
            let scalar = SCALAR_NAMES.contains(&name.as_ref());
            text.push_str(&format!(
                "{}\t{target:#x}\t{}\t0\n",
                name.replace('\t', "\\t")
                    .replace('\r', "\\r")
                    .replace('\n', "\\n"),
                u8::from(scalar)
            ));
        }
        text
    }
}
#[cfg(test)]
mod tests {
    use super::super::super::super::memory::{READ, WRITE};
    use super::*;
    #[test]
    fn registry_updates_pending_entries_and_rejects_unmapped_targets_and_sealed_mutations() {
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE | EXECUTE).unwrap();
        let mut registry = Registry::default();
        registry
            .register(&memory, CString::new("Sample::Pending").unwrap(), 0x1000)
            .unwrap();
        registry
            .register(&memory, CString::new("Sample::Pending").unwrap(), 0x1001)
            .unwrap();
        assert_eq!(registry.len(), 1);
        assert!(registry
            .register(&memory, CString::new("Sample::Bad").unwrap(), 0)
            .is_err());
        assert_eq!(registry.len(), 1);
        assert!(registry.seal().is_err());
        registry.scalar_targets = [0x1000; 2];
        registry.seal().unwrap();
        assert!(registry
            .register(&memory, CString::new("Sample::Pending").unwrap(), 0x1002)
            .is_err());
        assert!(registry.tsv().contains("Sample::Pending\t0x1001\t0\t0"));
    }
}
