//! Guest MonoError status snapshots. Native private fields and resource ownership
//! stay in Mono; only checked status words are admitted in the live main stack.
use super::super::super::memory::{READ, WRITE};
use super::{Cpu64, Memory};
use std::collections::BTreeMap;
#[derive(Default)]
pub(super) struct Registry {
    entries: BTreeMap<u64, u16>,
    initialized: usize,
    completed: usize,
    observed: usize,
    retired: bool,
}
impl Registry {
    fn stack(
        memory: &Memory,
        cpu: &Cpu64,
        address: u64,
        access: u8,
        bounds: (u64, u64),
    ) -> Result<(), String> {
        // Exact locked MonoErrorExternal layout: 4B init, alignment padding,
        // 12 opaque pointer fields. None of the pointer fields cross the ABI.
        let first = cpu.regs[4].checked_add(40).ok_or("error stack overflow")?;
        let end = address.checked_add(104).ok_or("error object overflow")?;
        if !address.is_multiple_of(8)
            || cpu.regs[4] < bounds.0
            || cpu.regs[4] >= bounds.1
            || address < first
            || address < bounds.0
            || end > bounds.1
        {
            return Err("MonoError status requires an aligned live owner-stack object".into());
        }
        let mut at = address;
        while at < end {
            let bytes = (end - at).min(4096 - (at & 4095)) as usize;
            if !memory.owned_noncode_access(at, bytes) {
                return Err("MonoError status stack has shared storage or code history".into());
            }
            at += bytes as u64;
        }
        memory.check(address, 104, access)
    }
    fn live(
        &self,
        memory: &Memory,
        cpu: &Cpu64,
        address: u64,
        bounds: (u64, u64),
    ) -> Result<u16, String> {
        if self.retired {
            return Err("MonoError snapshots retired".into());
        }
        Self::stack(memory, cpu, address, READ, bounds)?;
        let code = *self
            .entries
            .get(&address)
            .ok_or("uninitialized MonoError snapshot")?;
        if code == 65535 || memory.read_int(address, 4)? != code as u64 {
            return Err("MonoError snapshot cleaned or changed outside checked provider".into());
        }
        Ok(code)
    }
    pub(super) fn init(
        &mut self,
        memory: &mut Memory,
        cpu: &Cpu64,
        address: u64,
    ) -> Result<u64, String> {
        self.init_in(
            memory,
            cpu,
            address,
            (super::super::STACK, super::super::STACK + 64 * 1024 * 1024),
        )
    }
    pub(super) fn init_in(
        &mut self,
        memory: &mut Memory,
        cpu: &Cpu64,
        address: u64,
        bounds: (u64, u64),
    ) -> Result<u64, String> {
        if self.retired || (!self.entries.contains_key(&address) && self.entries.len() >= 4096) {
            return Err("MonoError snapshot outside live capacity".into());
        }
        Self::stack(memory, cpu, address, WRITE, bounds)?;
        memory.write_int(address, 4, 0)?;
        self.entries.insert(address, 0);
        self.initialized += 1;
        Ok(0)
    }
    pub(super) fn prepare(&self, memory: &Memory, cpu: &Cpu64, address: u64) -> Result<(), String> {
        self.prepare_in(
            memory,
            cpu,
            address,
            (super::super::STACK, super::super::STACK + 64 * 1024 * 1024),
        )
    }
    pub(super) fn prepare_in(
        &self,
        memory: &Memory,
        cpu: &Cpu64,
        address: u64,
        bounds: (u64, u64),
    ) -> Result<(), String> {
        if self.live(memory, cpu, address, bounds)? != 0 {
            return Err("nonempty MonoError passed to checked operation".into());
        }
        Self::stack(memory, cpu, address, WRITE, bounds)
    }
    pub(super) fn complete(
        &mut self,
        memory: &mut Memory,
        cpu: &Cpu64,
        address: u64,
        status: u16,
    ) -> Result<(), String> {
        self.complete_in(
            memory,
            cpu,
            address,
            status,
            (super::super::STACK, super::super::STACK + 64 * 1024 * 1024),
        )
    }
    pub(super) fn complete_in(
        &mut self,
        memory: &mut Memory,
        cpu: &Cpu64,
        address: u64,
        status: u16,
        bounds: (u64, u64),
    ) -> Result<(), String> {
        self.prepare_in(memory, cpu, address, bounds)?;
        if status == 65535 {
            return Err("checked runtime did not produce an error status".into());
        }
        memory.write_int(address, 4, status as u64)?;
        self.entries.insert(address, status);
        self.completed += 1;
        Ok(())
    }
    pub(super) fn query(
        &mut self,
        memory: &mut Memory,
        cpu: &Cpu64,
        function: &str,
        address: u64,
    ) -> Result<u64, String> {
        self.query_in(
            memory,
            cpu,
            function,
            address,
            (super::super::STACK, super::super::STACK + 64 * 1024 * 1024),
        )
    }
    pub(super) fn query_in(
        &mut self,
        memory: &mut Memory,
        cpu: &Cpu64,
        function: &str,
        address: u64,
        bounds: (u64, u64),
    ) -> Result<u64, String> {
        let code = self.live(memory, cpu, address, bounds)?;
        self.observed += 1;
        match function {
            "mono_error_ok" => Ok(u64::from(code == 0)),
            "mono_error_get_error_code" => Ok(code as u64),
            "mono_error_cleanup" => {
                Self::stack(memory, cpu, address, WRITE, bounds)?;
                // The checked native wrapper has already cleaned its owned
                // private resources. This marks only the guest snapshot.
                memory.write_int(address, 4, 65535)?;
                self.entries.insert(address, 65535);
                Ok(0)
            }
            _ => Err("unadmitted MonoError operation".into()),
        }
    }
    pub(super) fn ready(&self) -> bool {
        self.completed > 0 && self.observed > 0
    }
    pub(super) fn retire(&mut self) {
        self.entries.clear();
        self.retired = true;
    }
    pub(super) fn retire_stack(&mut self, bounds: (u64, u64)) {
        self.entries
            .retain(|&address, _| address < bounds.0 || address >= bounds.1);
    }
    pub(super) fn report(&self) -> String {
        format!("native-error-inits={}\nnative-error-completions={}\nnative-error-observations={}\nnative-error-snapshots-retired={}\n", self.initialized, self.completed, self.observed, self.retired)
    }
}
pub(super) fn handles(function: &str) -> bool {
    matches!(
        function,
        "mono_error_init" | "mono_error_ok" | "mono_error_get_error_code" | "mono_error_cleanup"
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_status_requires_current_scope_and_keeps_private_fields_opaque() {
        let mut memory = Memory::default();
        let base = 0x6ffd00000000;
        memory.map(base, 8192, READ | WRITE).unwrap();
        let cpu = Cpu64::new(0, base + 4000);
        let address = base + 4080; // Header crosses an owned page boundary.
        memory.write(address, &[0x5a; 104]).unwrap();
        let bounds = (base, base + 8192);
        let mut errors = Registry::default();
        assert!(errors.init(&mut memory, &cpu, address).is_err());
        errors.init_in(&mut memory, &cpu, address, bounds).unwrap();
        assert_eq!(
            errors
                .query_in(&mut memory, &cpu, "mono_error_ok", address, bounds)
                .unwrap(),
            1
        );
        assert!(errors
            .query_in(
                &mut memory,
                &cpu,
                "mono_error_ok",
                address,
                (base + 4096, base + 8192)
            )
            .is_err());
        let mut hidden = [0; 100];
        assert!(errors.prepare(&memory, &cpu, address).is_err());
        errors.prepare_in(&memory, &cpu, address, bounds).unwrap();
        errors
            .complete_in(&mut memory, &cpu, address, 7, bounds)
            .unwrap();
        assert_eq!(
            errors
                .query_in(
                    &mut memory,
                    &cpu,
                    "mono_error_get_error_code",
                    address,
                    bounds
                )
                .unwrap(),
            7
        );
        assert!(errors.prepare_in(&memory, &cpu, address, bounds).is_err());
        memory.read(address + 4, &mut hidden).unwrap();
        assert_eq!(hidden, [0x5a; 100]);
        errors
            .query_in(&mut memory, &cpu, "mono_error_cleanup", address, bounds)
            .unwrap();
        assert!(errors
            .query_in(&mut memory, &cpu, "mono_error_ok", address, bounds)
            .is_err());
        assert!(errors
            .init_in(&mut memory, &cpu, address + 1, bounds)
            .is_err());
        assert!(errors
            .init_in(&mut memory, &cpu, base + 8192 - 8, bounds)
            .is_err());
        memory.protect(base + 4096, 4096, READ).unwrap();
        assert!(errors.init_in(&mut memory, &cpu, address, bounds).is_err());
        memory.protect(base + 4096, 4096, READ | WRITE).unwrap();
        errors.init_in(&mut memory, &cpu, address, bounds).unwrap();
        errors.retire_stack(bounds);
        assert!(errors
            .query_in(&mut memory, &cpu, "mono_error_ok", address, bounds)
            .is_err());
    }
    #[test]
    fn checked_status_preserves_hidden_fields_reports_failure_and_rejects_forgery() {
        let mut memory = Memory::default();
        let base = super::super::super::STACK;
        memory.map(base, 4096, READ | WRITE).unwrap();
        let cpu = Cpu64::new(0, base + 16);
        let address = base + 256;
        memory.write(address, &[0xaa; 104]).unwrap();
        let mut errors = Registry::default();
        assert!(errors
            .query(&mut memory, &cpu, "mono_error_ok", address)
            .is_err());
        errors.init(&mut memory, &cpu, address).unwrap();
        let mut hidden = [0u8; 100];
        memory.read(address + 4, &mut hidden).unwrap();
        assert_eq!(hidden, [0xaa; 100]);
        errors.complete(&mut memory, &cpu, address, 10).unwrap();
        assert_eq!(
            errors
                .query(&mut memory, &cpu, "mono_error_ok", address)
                .unwrap(),
            0
        );
        assert_eq!(
            errors
                .query(&mut memory, &cpu, "mono_error_get_error_code", address)
                .unwrap(),
            10
        );
        assert!(errors.prepare(&memory, &cpu, address).is_err());
        errors
            .query(&mut memory, &cpu, "mono_error_cleanup", address)
            .unwrap();
        assert_eq!(memory.read_int(address, 4).unwrap(), 65535);
        assert!(errors
            .query(&mut memory, &cpu, "mono_error_cleanup", address)
            .is_err());
        errors.init(&mut memory, &cpu, address).unwrap();
        memory.write_int(address, 4, 1).unwrap();
        assert!(errors
            .query(&mut memory, &cpu, "mono_error_ok", address)
            .is_err());
        assert!(errors.init(&mut memory, &cpu, address + 1).is_err());
        assert!(errors.init(&mut memory, &cpu, cpu.regs[4]).is_err());
        memory.protect(base, 4096, READ).unwrap();
        assert!(errors.init(&mut memory, &cpu, address).is_err());
        assert_eq!(memory.read_int(address, 4).unwrap(), 1);
        errors.retire();
        assert!(errors
            .query(&mut memory, &cpu, "mono_error_ok", address)
            .is_err());
    }
}
