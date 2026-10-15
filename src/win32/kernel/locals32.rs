//! PE32 local-slot storage owns its indices and thread-keyed values.
//! LastError, ABI frames and scheduler identity remain in the caller.
use std::collections::HashMap;

#[derive(Clone, Copy)]
pub(crate) enum LocalKind {
    Tls,
    Fls,
}

pub(crate) struct LocalSlots32 {
    tls: HashMap<(u32, u32), u32>,
    fls: HashMap<(u32, u32), u32>,
    next_tls: u32,
    next_fls: u32,
}

impl Default for LocalSlots32 {
    fn default() -> Self {
        Self {
            tls: HashMap::new(),
            fls: HashMap::new(),
            next_tls: 1,
            next_fls: 1,
        }
    }
}

impl LocalSlots32 {
    pub(crate) fn allocate(&mut self, kind: LocalKind) -> u32 {
        let next = match kind {
            LocalKind::Tls => &mut self.next_tls,
            LocalKind::Fls => &mut self.next_fls,
        };
        let slot = *next;
        *next += 1;
        slot
    }

    pub(crate) fn get(&self, kind: LocalKind, tid: u32, slot: u32) -> u32 {
        let values = match kind {
            LocalKind::Tls => &self.tls,
            LocalKind::Fls => &self.fls,
        };
        values.get(&(tid, slot)).copied().unwrap_or(0)
    }

    pub(crate) fn set(&mut self, kind: LocalKind, tid: u32, slot: u32, value: u32) {
        let values = match kind {
            LocalKind::Tls => &mut self.tls,
            LocalKind::Fls => &mut self.fls,
        };
        values.insert((tid, slot), value);
    }

    pub(crate) fn free(&mut self, kind: LocalKind, slot: u32) {
        let values = match kind {
            LocalKind::Tls => &mut self.tls,
            LocalKind::Fls => &mut self.fls,
        };
        values.retain(|&(_, stored_slot), _| stored_slot != slot);
    }
}
