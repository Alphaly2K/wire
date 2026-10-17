//! Cooperative guest thread metadata. CPU contexts belong to Process64; no host threads run guest code.
use super::{Cpu64, Memory, Runtime64, PEB, READ, TEB, WRITE};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
mod address;
mod control;
mod properties;
pub(super) use control::ThreadControlResult;

pub(crate) struct Spawn {
    pub id: u64,
    pub cpu: Cpu64,
    pub parameter: u64,
}
#[derive(Clone)]
pub(super) enum Block {
    Address {
        address: u64,
        ticket: u64,
        notified: bool,
        deadline: Option<Instant>,
    },
    Wait {
        handles: Vec<u64>,
        all: bool,
        deadline: Option<Instant>,
        alertable: bool,
    },
    Sleep(Instant),
    SleepEx {
        deadline: Option<Instant>,
        alertable: bool,
    },
    Critical(u64),
    Srw(u64, bool),
    Once(u64),
    Messages {
        window: u64,
        min: u32,
        max: u32,
    },
}
pub(super) struct Thread {
    pub teb: u64,
    pub exit: Option<u32>,
    pub suspended: u32,
    pub block: Option<Block>,
    pub priority: i32,
    apartment: crate::win32::com::apartment::Apartment,
    pub resources: Option<(u64, u64, Vec<u64>)>,
    description: String,
    apcs: std::collections::VecDeque<(u64, u64)>,
    error_mode: u32,
}
pub(super) struct Threads {
    pub entries: BTreeMap<u64, Thread>,
    pub handles: BTreeMap<u64, u64>,
    pub handle_flags: BTreeMap<u64, u32>,
    pub next_alias: u64,
    pub pending: Vec<Spawn>,
    pub active: u64,
    pub next: u64,
    pub stack_size: u64,
    pub enabled: bool,
    next_address_wait: u64,
}
impl Threads {
    pub fn new(stack_size: u64) -> Self {
        Self {
            entries: BTreeMap::from([(
                1,
                Thread {
                    teb: TEB,
                    exit: None,
                    suspended: 0,
                    block: None,
                    priority: 0,
                    apartment: Default::default(),
                    resources: None,
                    description: String::new(),
                    apcs: Default::default(),
                    error_mode: 0,
                },
            )]),
            handles: BTreeMap::new(),
            handle_flags: BTreeMap::new(),
            next_alias: 0x7fff9000,
            pending: Vec::new(),
            active: 1,
            next: 2,
            stack_size,
            enabled: false,
            next_address_wait: 0,
        }
    }
    fn block(&mut self, reason: Block) -> Result<(), String> {
        if !self.enabled {
            return Err("Win64 blocking operation requires a thread scheduler".into());
        }
        let thread = self.entries.get_mut(&self.active).unwrap();
        // Preserve the original deadline and retry state.
        if thread.block.is_none() {
            thread.block = Some(reason);
        }
        Ok(())
    }
    pub fn id(&self, handle: u64) -> Result<u64, u32> {
        if handle == u64::MAX - 1 {
            return Ok(self.active);
        }
        self.handles.get(&handle).copied().ok_or(6)
    }
    pub fn apartment(&self) -> &crate::win32::com::apartment::Apartment {
        &self.entries[&self.active].apartment
    }
    pub fn apartment_mut(&mut self) -> &mut crate::win32::com::apartment::Apartment {
        &mut self.entries.get_mut(&self.active).unwrap().apartment
    }
    pub fn flags(&self, handle: u64) -> Option<u32> {
        self.handle_flags.get(&handle).copied()
    }
    pub fn close(&mut self, handle: u64) -> Result<(), u32> {
        self.handles.get(&handle).ok_or(6u32)?;
        if self.handle_flags[&handle] & 2 != 0 {
            return Err(5);
        }
        self.handles.remove(&handle);
        self.handle_flags.remove(&handle);
        Ok(())
    }
    pub fn duplicate(
        &mut self,
        source: u64,
        inherit: bool,
        close_source: bool,
    ) -> Result<u64, u32> {
        let id = self.id(source)?;
        if close_source && self.flags(source).is_some_and(|flags| flags & 2 != 0) {
            return Err(6);
        }
        if self.handles.len() >= 1024 || self.next_alias >= 0x7fffa000 {
            return Err(8);
        }
        let handle = self.next_alias;
        self.next_alias += 1;
        self.handles.insert(handle, id);
        self.handle_flags.insert(handle, inherit as u32);
        Ok(handle)
    }
    pub fn initialize_teb(
        mem: &mut Memory,
        teb: u64,
        id: u64,
        stack: u64,
        size: u64,
    ) -> Result<(), String> {
        mem.write_int(teb + 8, 8, stack + size)?;
        mem.write_int(teb + 16, 8, stack)?;
        mem.write_int(teb + 0x30, 8, teb)?;
        mem.write_int(teb + 0x40, 8, 1)?;
        mem.write_int(teb + 0x48, 8, id)?;
        mem.write_int(teb + 0x58, 8, teb + 2 * super::PAGE)?;
        mem.write_int(teb + 0x60, 8, PEB)?;
        Ok(())
    }
}

impl Runtime64 {
    pub fn enable_scheduler(&mut self) {
        self.threads.enabled = true;
    }
    pub(crate) fn active_thread(&self) -> u64 {
        self.threads.active
    }
    pub(crate) fn active_suspended(&self) -> bool {
        self.threads.entries[&self.threads.active].suspended != 0
    }
    pub(crate) fn teb(&self) -> u64 {
        self.threads.entries[&self.threads.active].teb
    }
    pub(crate) fn take_threads(&mut self) -> Vec<Spawn> {
        std::mem::take(&mut self.threads.pending)
    }
    pub(crate) fn release_callback_buffers(
        &mut self,
        mem: &mut Memory,
        pointers: &[u64],
    ) -> Result<(), String> {
        for pointer in pointers {
            let &(_, span) = self
                .allocations
                .get(pointer)
                .ok_or("native callback buffer was released by guest")?;
            mem.unmap(*pointer, span)?;
            self.allocations.remove(pointer);
        }
        Ok(())
    }
    pub(crate) fn activate_thread(&mut self, id: u64) {
        // Apartment state resides in its thread; there is no active-thread mirror.
        let _ = &self.threads.entries[&id];
        self.threads.active = id;
    }
    pub(crate) fn finish_thread(&mut self, mem: &mut Memory, code: u32) -> Result<(), String> {
        self.user.keyboard.exit64(self.threads.active);
        self.user.events.retain(|m| m.tid != self.threads.active);
        let t = self.threads.entries.get_mut(&self.threads.active).unwrap();
        t.exit = Some(code);
        t.block = None;
        if let Some((stack, size, tls_blocks)) = t.resources.take() {
            mem.unmap(stack, size)?;
            mem.unmap(t.teb, 3 * super::PAGE)?;
            for ptr in tls_blocks {
                let (_, span) = self
                    .allocations
                    .remove(&ptr)
                    .ok_or("missing thread TLS allocation")?;
                mem.unmap(ptr, span)?;
            }
        }
        Ok(())
    }
    pub(crate) fn fls_destructors(&self, mem: &mut Memory) -> Result<Vec<(u64, u64)>, String> {
        let mut calls = Vec::new();
        for (n, slot) in self.fls.iter().enumerate() {
            if let Some((_, callback)) = slot {
                let ptr = self.teb() + super::PAGE + n as u64 * 8;
                let value = mem.read_int(ptr, 8)?;
                if *callback != 0 {
                    mem.check(ptr, 8, WRITE)?;
                    if value != 0 {
                        calls.push((*callback, value));
                    }
                }
            }
        }
        // Validate all slots before publishing any callback's cleared value.
        for (n, slot) in self.fls.iter().enumerate() {
            if slot.is_some_and(|(_, cb)| cb != 0) {
                mem.write_int(self.teb() + super::PAGE + n as u64 * 8, 8, 0)?;
            }
        }
        Ok(calls)
    }
    pub(crate) fn thread_runnable(&mut self, id: u64, mem: &Memory) -> Result<bool, String> {
        let t = &self.threads.entries[&id];
        if t.exit.is_some() || t.suspended != 0 {
            return Ok(false);
        }
        let block = t.block.clone();
        Ok(match &block {
            None => true,
            Some(Block::Sleep(deadline)) => Instant::now() >= *deadline,
            Some(Block::SleepEx {
                deadline,
                alertable,
            }) => {
                (*alertable && !self.threads.entries[&id].apcs.is_empty())
                    || deadline.is_some_and(|d| Instant::now() >= d)
            }
            Some(Block::Address {
                notified, deadline, ..
            }) => *notified || deadline.is_some_and(|d| Instant::now() >= d),
            Some(Block::Wait {
                handles,
                all,
                deadline,
                alertable,
            }) => {
                (*alertable && !self.threads.entries[&id].apcs.is_empty())
                    || deadline.is_some_and(|d| Instant::now() >= d)
                    || !matches!(self.wait_objects(handles, *all, false), Ok(None))
            }
            Some(Block::Critical(ptr)) => {
                let owner = mem.read_int(*ptr + 16, 8)?;
                owner == 0 || owner == id
            }
            Some(Block::Srw(ptr, shared)) => self.srw.available(*ptr, *shared),
            Some(Block::Once(ptr)) => mem.read_int(*ptr, 8)? != 1,
            Some(Block::Messages { window, min, max }) => {
                self.message_ready(id, *window, *min, *max)
            }
        })
    }
    pub(crate) fn idle_delay(&self) -> Option<Duration> {
        self.threads
            .entries
            .values()
            .filter(|t| t.suspended == 0 && t.exit.is_none())
            .filter_map(|t| match &t.block {
                Some(Block::Address {
                    deadline: Some(d), ..
                })
                | Some(Block::Sleep(d))
                | Some(Block::SleepEx {
                    deadline: Some(d), ..
                })
                | Some(Block::Wait {
                    deadline: Some(d), ..
                }) => Some(*d),
                _ => None,
            })
            .chain(self.events.next_timer())
            .min()
            .map(|d| {
                d.saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(10))
            })
    }
    pub(super) fn block(&mut self, reason: Block) -> Result<(), String> {
        self.threads.block(reason)
    }
    pub(super) fn unblock(&mut self) {
        self.threads
            .entries
            .get_mut(&self.threads.active)
            .unwrap()
            .block = None;
    }
    pub(super) fn wait_deadline(&self, timeout: u64) -> Option<Instant> {
        if let Some(Block::Wait { deadline, .. }) =
            &self.threads.entries[&self.threads.active].block
        {
            *deadline
        } else if timeout == u32::MAX as u64 {
            None
        } else {
            Some(Instant::now() + Duration::from_millis(timeout))
        }
    }
    pub(super) fn wait_objects(
        &mut self,
        handles: &[u64],
        all: bool,
        consume: bool,
    ) -> Result<Option<u64>, u32> {
        let mut identities = Vec::new();
        let mut ready = Vec::new();
        for &handle in handles {
            let (identity, signaled) = if let Some(&id) = self.threads.handles.get(&handle) {
                (0x10000 + id, self.threads.entries[&id].exit.is_some())
            } else {
                self.events.waitable(handle)?
            };
            if all && identities.contains(&identity) {
                return Err(87);
            }
            identities.push(identity);
            ready.push(signaled);
        }
        let selected = if all {
            if !ready.iter().all(|r| *r) {
                return Ok(None);
            }
            0
        } else {
            let Some(n) = ready.iter().position(|r| *r) else {
                return Ok(None);
            };
            n
        };
        if consume {
            let events: Vec<_> = (if all {
                handles
            } else {
                &handles[selected..selected + 1]
            })
            .iter()
            .copied()
            .filter(|h| !self.threads.handles.contains_key(h))
            .collect();
            if !events.is_empty() {
                self.events.wait(&events, all)?;
            }
        }
        Ok(Some(selected as u64))
    }
    pub(super) fn create_thread(&mut self, mem: &mut Memory, args: &[u64]) -> Result<u64, String> {
        if !self.threads.enabled {
            return Err("Win64 CreateThread requires a thread scheduler".into());
        }
        if args[4] & !0x10004 != 0 {
            self.set_error(mem, 87)?;
            return Ok(0);
        }
        let inherit = if args[0] == 0 {
            false
        } else {
            mem.check(args[0], 24, READ)?;
            if mem.read_int(args[0], 4)? != 24 || mem.read_int(args[0] + 8, 8)? != 0 {
                return Err("Win64 thread security descriptor not implemented".into());
            }
            mem.read_int(args[0] + 16, 4)? != 0
        };
        if args[5] != 0 {
            mem.check(args[5], 4, WRITE)?;
        }
        let size = args[1].max(self.threads.stack_size).max(65536);
        let Some(size) = size
            .checked_add(65535)
            .map(|s| s / 65536 * 65536)
            .filter(|s| *s <= 16 * 1024 * 1024)
        else {
            self.set_error(mem, 8)?;
            return Ok(0);
        };
        let id = self.threads.next;
        if id > 64 || self.threads.handles.len() >= 1024 {
            self.set_error(mem, 8)?;
            return Ok(0);
        }
        let stack = 0x7fe000000000 + id * 64 * 1024 * 1024;
        let teb = 0x7ffc00000000 + id * 65536;
        let mut tls_blocks = Vec::new();
        let mut mapped_stack = false;
        let mut mapped_teb = false;
        let prepared = (|| -> Result<Cpu64, String> {
            mem.map(stack, size, READ | WRITE)?;
            mapped_stack = true;
            mem.map(teb, 3 * super::PAGE, READ | WRITE)?;
            mapped_teb = true;
            Threads::initialize_teb(mem, teb, id, stack, size)?;
            // Copy the image template, never the spawning thread's mutated TLS.
            for n in 0..self.static_tls.len() {
                let (template, size, alignment) = self.static_tls[n].clone();
                self.next_alloc = self.next_alloc.div_ceil(alignment) * alignment;
                let ptr = self.heap_alloc(mem, size)?;
                if ptr == 0 {
                    return Err("thread static TLS allocation failed".into());
                }
                tls_blocks.push(ptr);
                mem.write(ptr, &template)?;
                mem.write_int(teb + 2 * super::PAGE + n as u64 * 8, 8, ptr)?;
            }
            let mut cpu = Cpu64::new(args[2], stack + size - 160 + 8);
            cpu.gs_base = teb;
            cpu.regs[1] = args[3];
            Ok(cpu)
        })();
        let cpu = match prepared {
            Ok(cpu) => cpu,
            Err(_) => {
                for ptr in tls_blocks {
                    let (_, span) = self.allocations.remove(&ptr).unwrap();
                    mem.unmap(ptr, span)?;
                }
                // Only unmap allocations created by this attempt, never an existing guest mapping.
                if mapped_stack {
                    mem.unmap(stack, size)?;
                }
                if mapped_teb {
                    mem.unmap(teb, 3 * super::PAGE)?;
                }
                self.set_error(mem, 8)?;
                return Ok(0);
            }
        };
        if args[5] != 0 {
            mem.write_int(args[5], 4, id)?;
        }
        let handle = 0x7fff0000 + id;
        self.threads.next += 1;
        self.threads.entries.insert(
            id,
            Thread {
                teb,
                exit: None,
                suspended: (args[4] & 4 != 0) as u32,
                block: None,
                priority: 0,
                apartment: Default::default(),
                resources: Some((stack, size, tls_blocks)),
                description: String::new(),
                apcs: Default::default(),
                error_mode: self.error_mode,
            },
        );
        self.threads.handles.insert(handle, id);
        self.threads.handle_flags.insert(handle, inherit as u32);
        self.threads.pending.push(Spawn {
            id,
            cpu,
            parameter: args[3],
        });
        Ok(handle)
    }
}
