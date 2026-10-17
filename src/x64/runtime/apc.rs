//! User APCs execute as guest callbacks only at alertable waits.
use super::{Control, Cpu64, Memory, Runtime64};
impl Runtime64 {
    pub(super) fn deliver_apcs(
        &mut self,
        mem: &Memory,
        dll: &str,
        name: &str,
        args: &[u64],
        returned: &Cpu64,
        cpu: &mut Cpu64,
    ) -> Result<Option<Control>, String> {
        let calls = self.threads.take_apcs(mem, &self.gateways)?;
        if calls.is_empty() {
            return Ok(None);
        }
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
