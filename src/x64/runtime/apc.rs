//! User APCs execute as guest callbacks only at alertable waits.
use super::{Control, Cpu64, Memory, Runtime64, READ};
use crate::x64::memory::EXECUTE;
impl Runtime64 {
    pub(super) fn queue_apc(&mut self, mem: &mut Memory, a: &[u64]) -> Result<u64, String> {
        let result = self.threads.id(a[1]).and_then(|id| {
            let t = &self.threads.entries[&id];
            if t.exit.is_some() {
                Err(31)
            } else if t.apcs.len() >= 256 {
                Err(8)
            } else {
                Ok(id)
            }
        });
        match result {
            Err(error) => {
                self.set_error(mem, error)?;
                Ok(0)
            }
            Ok(id) => {
                if !self.gateways.contains_key(&a[0]) {
                    mem.check(a[0], 1, READ | EXECUTE)?;
                }
                self.threads
                    .entries
                    .get_mut(&id)
                    .unwrap()
                    .apcs
                    .push_back((a[0], a[2]));
                Ok(1)
            }
        }
    }
    pub(super) fn deliver_apcs(
        &mut self,
        mem: &Memory,
        dll: &str,
        name: &str,
        args: &[u64],
        returned: &Cpu64,
        cpu: &mut Cpu64,
    ) -> Result<Option<Control>, String> {
        let queue = &self.threads.entries[&self.threads.active].apcs;
        if queue.is_empty() {
            return Ok(None);
        }
        for &(target, _) in queue {
            if !self.gateways.contains_key(&target) {
                mem.check(target, 1, READ | EXECUTE)?;
            }
        }
        let calls = self
            .threads
            .entries
            .get_mut(&self.threads.active)
            .unwrap()
            .apcs
            .drain(..)
            .map(|(target, data)| (target, [data, 0, 0, 0]))
            .collect();
        *cpu = returned.clone();
        cpu.regs[0] = 0xc0; // WAIT_IO_COMPLETION
        self.unblock();
        self.api_calls += 1;
        self.record(dll, name, args, 0xc0);
        Ok(Some(Control::Callbacks {
            calls,
            release: Vec::new(),
            stop_on_false: false,
            return_boolean: false,
            window_call: None,
        }))
    }
}
