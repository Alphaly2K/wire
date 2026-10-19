//! Owner operations do not run guest callbacks or commit CPU return frames.
use super::{Memory, Runtime64, Threads, READ, WRITE};
use crate::{
    win32::catalog::ThreadControlApi,
    x64::{
        memory::EXECUTE,
        runtime::{exception::CONTEXT_SIZE, gateways::Gateways},
    },
};

pub(crate) enum ThreadControlResult {
    Value(u64),
    Exit(u32),
    Context { thread: u64, flags: u32 },
}

type ApcCall = (u64, [u64; 4]);

impl Threads {
    fn context_target(&self, handle: u64) -> Result<u64, u32> {
        let id = self.id(handle)?;
        let t = &self.entries[&id];
        if id == self.active || t.suspended == 0 || t.exit.is_some() {
            Err(50)
        } else {
            Ok(id)
        }
    }

    fn set_description(&mut self, mem: &Memory, id: u64, pointer: u64) -> Result<(), String> {
        let description = crate::win32::x64::text::utf16(mem, pointer)?;
        self.entries.get_mut(&id).unwrap().description = description;
        Ok(())
    }

    fn description_bytes(&self, id: u64) -> Vec<u8> {
        self.entries[&id]
            .description
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    fn queue_apc(
        &mut self,
        mem: &mut Memory,
        gateways: &Gateways,
        a: &[u64],
        error_slot: u64,
    ) -> Result<u64, String> {
        let result = self.id(a[1]).and_then(|id| {
            let t = &self.entries[&id];
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
                mem.write_int(error_slot, 4, error as u64)?;
                Ok(0)
            }
            Ok(id) => {
                if !gateways.contains_key(&a[0]) {
                    mem.check(a[0], 1, READ | EXECUTE)?;
                }
                self.entries
                    .get_mut(&id)
                    .unwrap()
                    .apcs
                    .push_back((a[0], a[2]));
                Ok(1)
            }
        }
    }

    pub(in crate::x64::runtime) fn take_apcs(
        &mut self,
        mem: &Memory,
        gateways: &Gateways,
    ) -> Result<Vec<ApcCall>, String> {
        let queue = &mut self.entries.get_mut(&self.active).unwrap().apcs;
        // Validate every callback before draining any of the queue.
        for &(target, _) in queue.iter() {
            if !gateways.contains_key(&target) {
                mem.check(target, 1, READ | EXECUTE)?;
            }
        }
        Ok(queue
            .drain(..)
            .map(|(target, data)| (target, [data, 0, 0, 0]))
            .collect())
    }
}

impl Runtime64 {
    pub(in crate::x64::runtime) fn thread_control_call(
        &mut self,
        mem: &mut Memory,
        id: ThreadControlApi,
        args: &[u64],
    ) -> Result<ThreadControlResult, String> {
        use ThreadControlApi::*;
        let value = match id {
            Create => self.create_thread(mem, args)?,
            Exit => {
                if !self.threads.enabled {
                    return Err("Win64 ExitThread requires a thread scheduler".into());
                }
                return Ok(ThreadControlResult::Exit(args[0] as u32));
            }
            QueueApc => {
                let error_slot = self.teb() + 0x68;
                self.threads
                    .queue_apc(mem, &self.gateways, args, error_slot)?
            }
            Context => match self.threads.context_target(args[0]) {
                Err(error) => {
                    self.set_error(mem, error)?;
                    0
                }
                Ok(thread) => {
                    if !args[1].is_multiple_of(16) {
                        self.set_error(mem, 998)?;
                        0
                    } else {
                        mem.check(args[1], CONTEXT_SIZE, WRITE)?;
                        let flags = mem.read_int(args[1] + 48, 4)? as u32;
                        self.trace_note(format!(
                            "GetThreadContext thread={thread} flags={flags:#x}"
                        ));
                        if flags & 0x100000 == 0 || flags & !0x10001f != 0 {
                            self.set_error(mem, 87)?;
                            0
                        } else {
                            return Ok(ThreadControlResult::Context { thread, flags });
                        }
                    }
                }
            },
            SetDescription | GetDescription => match self.threads.id(args[0]) {
                Err(error) => 0x80070000 | error as u64,
                Ok(thread) if id == SetDescription => {
                    self.threads.set_description(mem, thread, args[1])?;
                    0
                }
                Ok(thread) => {
                    mem.check(args[1], 8, WRITE)?;
                    let data = self.threads.description_bytes(thread);
                    let pointer = self.heap_alloc(mem, data.len() as u64)?;
                    if pointer == 0 {
                        0x8007000e
                    } else {
                        mem.write(pointer, &data)?;
                        mem.write_int(args[1], 8, pointer)?;
                        self.local_memory.track(pointer);
                        0
                    }
                }
            },
        };
        Ok(ThreadControlResult::Value(value))
    }
}
