//! Guest apartment metadata; no CPU, memory, callback or host COM dependency.
use std::collections::HashMap;

#[derive(Default)]
pub(crate) struct Apartment {
    mode: Option<u32>,
    com_refs: u32,
    winrt_refs: u32,
}

pub(crate) enum InitializeError {
    ChangedMode,
    Overflow,
}

impl Apartment {
    pub(crate) fn initialized(&self) -> bool {
        self.mode.is_some()
    }

    pub(crate) fn initialize(&mut self, mode: u32, winrt: bool) -> Result<u32, InitializeError> {
        if self.mode.is_some_and(|current| current != mode) {
            return Err(InitializeError::ChangedMode);
        }
        let result = self.mode.is_some() as u32;
        self.mode = Some(mode);
        let refs = if winrt {
            &mut self.winrt_refs
        } else {
            &mut self.com_refs
        };
        *refs = refs.checked_add(1).ok_or(InitializeError::Overflow)?;
        Ok(result)
    }

    pub(crate) fn uninitialize(&mut self, winrt: bool) {
        let refs = if winrt {
            &mut self.winrt_refs
        } else {
            &mut self.com_refs
        };
        *refs = refs.saturating_sub(1);
        if self.com_refs == 0 && self.winrt_refs == 0 {
            self.mode = None;
        }
    }
}

#[derive(Default)]
pub(crate) struct Apartments32 {
    entries: HashMap<u32, Apartment>,
}

impl Apartments32 {
    pub(crate) fn initialize(&mut self, tid: u32, mode: u32) -> Result<u32, InitializeError> {
        self.entries.entry(tid).or_default().initialize(mode, false)
    }
    pub(crate) fn uninitialize(&mut self, tid: u32) {
        if let Some(apartment) = self.entries.get_mut(&tid) {
            apartment.uninitialize(false);
            if !apartment.initialized() {
                self.entries.remove(&tid);
            }
        }
    }
}
