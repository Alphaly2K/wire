//! Owns process error mode and startup attribute records; no child executor.
use super::{events::Events, pipes::Pipes, Memory, WRITE};
use crate::win32::catalog::ProcessOptionApi;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct ProcessOptions {
    error_mode: u32,
    attributes: BTreeMap<u64, (u32, BTreeMap<u64, Vec<u8>>)>,
}

impl ProcessOptions {
    pub(super) fn error_mode(&self) -> u32 {
        self.error_mode
    }

    pub(super) fn call(
        &mut self,
        mem: &mut Memory,
        id: ProcessOptionApi,
        args: &[u64],
        last_error: u64,
        pipes: &Pipes,
        events: &Events,
    ) -> Result<u64, String> {
        use ProcessOptionApi::*;
        Ok(match id {
            GetErrorMode => self.error_mode as u64,
            SetErrorMode => {
                if args[0] & !0x8007 != 0 {
                    return Err("unsupported Win64 process error-mode bits".into());
                }
                let previous = self.error_mode;
                self.error_mode = args[0] as u32;
                previous as u64
            }
            InitializeAttributes => {
                if args[1] == 0 || args[1] > 16 || args[2] != 0 {
                    mem.write_int(last_error, 4, 87)?;
                    0
                } else {
                    let needed = 32 + args[1] * 32;
                    mem.check(args[3], 8, WRITE)?;
                    let capacity = mem.read_int(args[3], 8)?;
                    if args[0] == 0 || capacity < needed {
                        mem.write_int(args[3], 8, needed)?;
                        mem.write_int(last_error, 4, 122)?;
                        0
                    } else {
                        mem.check(args[0], needed as usize, WRITE)?;
                        mem.write(args[0], &vec![0; needed as usize])?;
                        self.attributes
                            .insert(args[0], (args[1] as u32, BTreeMap::new()));
                        1
                    }
                }
            }
            UpdateAttribute => {
                if args[1] & !1 != 0 || args[5] != 0 || args[6] != 0 {
                    mem.write_int(last_error, 4, 87)?;
                    0
                } else if args[2] != 0x20002 {
                    return Err(format!(
                        "unsupported Win64 process attribute {:#x}",
                        args[2]
                    ));
                } else if args[4] == 0 || args[4] > 64 * 8 || !args[4].is_multiple_of(8) {
                    mem.write_int(last_error, 4, 87)?;
                    0
                } else {
                    let mut bytes = vec![0; args[4] as usize];
                    mem.read(args[3], &mut bytes)?;
                    let valid_handles = bytes.as_chunks::<8>().0.iter().all(|b| {
                        let handle = u64::from_le_bytes(*b);
                        pipes
                            .flags(handle)
                            .or_else(|| events.flags(handle))
                            .is_some_and(|flags| flags & 1 != 0)
                    });
                    if !valid_handles {
                        mem.write_int(last_error, 4, 6)?;
                        0
                    } else if let Some((capacity, attributes)) = self.attributes.get_mut(&args[0]) {
                        if attributes.contains_key(&args[2]) && args[1] == 0
                            || !attributes.contains_key(&args[2])
                                && attributes.len() >= *capacity as usize
                        {
                            mem.write_int(last_error, 4, 87)?;
                            0
                        } else {
                            attributes.insert(args[2], bytes);
                            1
                        }
                    } else {
                        mem.write_int(last_error, 4, 87)?;
                        0
                    }
                }
            }
            DeleteAttributes => {
                if self.attributes.remove(&args[0]).is_none() {
                    return Err("invalid Win64 process attribute list".into());
                }
                0
            }
        })
    }
}
