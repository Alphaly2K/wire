//! Win64 COM adapters borrow apartment/task owners; CPU commits stay in the gateway.
use super::{Memory, Runtime64, WRITE};
use crate::win32::{catalog::ComApi, com::apartment::InitializeError};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct TaskMemory {
    blocks: BTreeSet<u64>,
}

impl TaskMemory {
    pub fn track(&mut self, pointer: u64) {
        self.blocks.insert(pointer);
    }

    fn free(
        &mut self,
        mem: &mut Memory,
        allocations: &mut BTreeMap<u64, (usize, u64)>,
        pointer: u64,
    ) -> Result<(), String> {
        if pointer != 0 {
            if !self.blocks.contains(&pointer) {
                return Err("invalid Win64 task allocation".into());
            }
            let &(_, span) = allocations
                .get(&pointer)
                .ok_or("released Win64 task allocation")?;
            mem.unmap(pointer, span)?;
            allocations.remove(&pointer);
            self.blocks.remove(&pointer);
        }
        Ok(())
    }
}

impl Runtime64 {
    pub(super) fn com_call(
        &mut self,
        mem: &mut Memory,
        id: ComApi,
        args: &[u64],
    ) -> Result<u64, String> {
        use ComApi::*;
        Ok(match id {
            TaskAlloc => {
                let pointer = self.heap_alloc(mem, args[0])?;
                if pointer != 0 {
                    self.task_memory.track(pointer);
                }
                pointer
            }
            TaskFree => {
                self.task_memory.free(mem, &mut self.allocations, args[0])?;
                0
            }
            CreateInstance => {
                if args[0] == 0 || args[3] == 0 || args[4] == 0 {
                    0x80070057
                } else {
                    mem.check(args[4], 8, WRITE)?;
                    let mut clsid = [0; 16];
                    let mut iid = [0; 16];
                    mem.read(args[0], &mut clsid)?;
                    mem.read(args[3], &mut iid)?;
                    self.trace_note(format!("Win64 COM class={clsid:02x?} iid={iid:02x?}: no registered native class factory"));
                    mem.write_int(args[4], 8, 0)?;
                    if !self.threads.apartment().initialized() {
                        0x800401f0
                    } else if args[2] == 0 {
                        0x80070057
                    } else {
                        0x80040154
                    }
                }
            }
            Initialize | InitializeEx | WinrtInitialize => {
                let winrt = id == WinrtInitialize;
                let flags = if id == Initialize {
                    2
                } else if winrt {
                    0
                } else {
                    args[1]
                };
                let invalid = if winrt {
                    args[0] > 1
                } else {
                    args[0] != 0 || flags & !0xe != 0
                };
                if invalid {
                    0x80070057
                } else {
                    let mode = if winrt {
                        if args[0] == 0 {
                            2
                        } else {
                            0
                        }
                    } else {
                        flags as u32 & 2
                    };
                    match self.threads.apartment_mut().initialize(mode, winrt) {
                        Ok(value) => value as u64,
                        Err(InitializeError::ChangedMode) => 0x80010106,
                        Err(InitializeError::Overflow) => {
                            return Err("Win64 apartment reference overflow".into())
                        }
                    }
                }
            }
            Uninitialize | WinrtUninitialize => {
                self.threads
                    .apartment_mut()
                    .uninitialize(id == WinrtUninitialize);
                0
            }
            OleInitialize | OleUninitialize => {
                unreachable!("OLE initialization has no Win64 binding")
            }
        })
    }
}
