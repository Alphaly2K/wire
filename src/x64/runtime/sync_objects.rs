//! Typed event/semaphore adapters; Events retains object and alias ownership.
use super::{events::Events, threads::Threads, Memory, Runtime64};
use crate::win32::{catalog::SyncObjectApi, x64::text};

impl Runtime64 {
    pub(super) fn sync_object_call(
        &mut self,
        id: SyncObjectApi,
        args: &[u64],
        mem: &mut Memory,
    ) -> Result<u64, String> {
        self.events.sync_object_call(id, args, mem, &self.threads)
    }
}

impl Events {
    pub(super) fn sync_object_call(
        &mut self,
        id: SyncObjectApi,
        args: &[u64],
        mem: &mut Memory,
        threads: &Threads,
    ) -> Result<u64, String> {
        use SyncObjectApi::*;
        Ok(match id {
            CreateEventW | CreateEventA | CreateEventExW | CreateEventExA | OpenEventW
            | OpenEventA => {
                let extended = id.extended();
                let opening = id.opening();
                if !opening && args[0] != 0 {
                    return Err("Win64 event SECURITY_ATTRIBUTES not implemented".into());
                }
                let pointer = args[if opening {
                    2
                } else if extended {
                    1
                } else {
                    3
                }];
                let event_name = if pointer == 0 {
                    None
                } else if id.wide() {
                    Some(text::utf16(mem, pointer)?)
                } else {
                    let bytes = text::string_bytes(mem, pointer, u32::MAX as u64, 1)?;
                    Some(
                        crate::codepage::decode(&bytes[..bytes.len() - 1], 932)
                            .map_err(|e| format!("event codepage error {e}"))?,
                    )
                };
                let result = if opening {
                    if let Some(event_name) = event_name {
                        self.open(event_name, args[0] as u32, false)
                            .map(|h| (h, false))
                    } else {
                        Err(87)
                    }
                } else if extended && args[2] & !3 != 0 {
                    Err(87)
                } else {
                    let (manual, initial, access) = if extended {
                        (args[2] & 1 != 0, args[2] & 2 != 0, args[3] as u32)
                    } else {
                        (args[1] != 0, args[2] != 0, 0x1f0003)
                    };
                    self.create(manual, initial, event_name, access)
                };
                match result {
                    Ok((handle, existed)) => {
                        threads.set_error(mem, if existed { 183 } else { 0 })?;
                        handle
                    }
                    Err(error) => {
                        threads.set_error(mem, error)?;
                        0
                    }
                }
            }
            CreateSemaphoreW | CreateSemaphoreA | CreateSemaphoreExW | CreateSemaphoreExA
            | OpenSemaphoreW | OpenSemaphoreA => {
                let opening = id.opening();
                if !opening && args[0] != 0 {
                    return Err("Win64 semaphore SECURITY_ATTRIBUTES not implemented".into());
                }
                let pointer = args[if opening { 2 } else { 3 }];
                let object_name = if pointer == 0 {
                    None
                } else if id.wide() {
                    Some(text::utf16(mem, pointer)?)
                } else {
                    let bytes = text::string_bytes(mem, pointer, u32::MAX as u64, 1)?;
                    Some(
                        crate::codepage::decode(&bytes[..bytes.len() - 1], 932)
                            .map_err(|e| format!("semaphore codepage error {e}"))?,
                    )
                };
                let result = if opening {
                    object_name
                        .ok_or(87u32)
                        .and_then(|n| self.open(n, args[0] as u32, true).map(|h| (h, false)))
                } else if id.extended() && args[4] != 0 {
                    Err(87)
                } else {
                    let access = if id.extended() {
                        args[5] as u32
                    } else {
                        0x1f0003
                    };
                    self.semaphore(args[1] as i32, args[2] as i32, object_name, access)
                };
                match result {
                    Ok((handle, existed)) => {
                        threads.set_error(mem, if existed { 183 } else { 0 })?;
                        handle
                    }
                    Err(error) => {
                        threads.set_error(mem, error)?;
                        0
                    }
                }
            }
            ReleaseSemaphore => match self.release_count(args[0], args[1] as i32) {
                Ok(previous) => {
                    if args[2] != 0 {
                        mem.write_int(args[2], 4, previous as u64)?;
                    }
                    self.release(args[0], args[1] as i32);
                    1
                }
                Err(error) => {
                    threads.set_error(mem, error)?;
                    0
                }
            },
            SetEvent | ResetEvent => match self.change(args[0], id == SetEvent) {
                Ok(result) => result,
                Err(error) => {
                    threads.set_error(mem, error)?;
                    0
                }
            },
            PulseEvent => unreachable!("PulseEvent has no Win64 binding"),
        })
    }
}
