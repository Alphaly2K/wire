//! ETW provider registration with no tracing sessions. Every provider remains disabled.
//! No enable callback is sent and no event payload is consumed or claimed to be recorded.
use super::{Memory, Runtime64, READ, WRITE};
pub(super) fn is_call(name: &str) -> bool {
    matches!(
        name,
        "EventRegister"
            | "EventUnregister"
            | "EventEnabled"
            | "EventProviderEnabled"
            | "EventWrite"
            | "EventWriteString"
            | "EventWriteTransfer"
            | "EventSetInformation"
    )
}
impl Runtime64 {
    pub(super) fn etw_call(
        &mut self,
        mem: &mut Memory,
        name: &str,
        args: &[u64],
    ) -> Result<u64, String> {
        if name == "EventRegister" {
            mem.check(args[0], 16, READ)?;
            mem.check(args[3], 8, WRITE)?;
            if self.etw.len() >= 64 || self.next_etw >= 0x7fff7000 {
                return Ok(8);
            }
            let mut guid = [0; 16];
            mem.read(args[0], &mut guid)?;
            let handle = self.next_etw;
            mem.write_int(args[3], 8, handle)?;
            self.etw.insert(handle, (guid, args[1], args[2]));
            self.next_etw += 1;
            return Ok(0);
        }
        if !self.etw.contains_key(&args[0]) {
            return Ok(if matches!(name, "EventEnabled" | "EventProviderEnabled") {
                0
            } else {
                6
            });
        }
        Ok(match name {
            "EventUnregister" => {
                self.etw.remove(&args[0]);
                0
            }
            "EventEnabled" => {
                mem.check(args[1], 16, READ)?;
                0
            }
            "EventProviderEnabled" => 0,
            "EventSetInformation" => 50, // Provider traits/controller information are not implemented.
            "EventWriteString" => 0,
            "EventWrite" | "EventWriteTransfer" => {
                mem.check(args[1], 16, READ)?;
                let (count, data) = if name == "EventWrite" {
                    (args[2], args[3])
                } else {
                    (args[4], args[5])
                };
                if count > 128 {
                    return Ok(87);
                }
                // Disabled events do not read caller payloads, matching the no-listener fast path.
                if count != 0 {
                    mem.check(data, count as usize * 16, READ)?;
                }
                0
            }
            _ => unreachable!(),
        })
    }
}
