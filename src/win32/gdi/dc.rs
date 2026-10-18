//! Logical Win64 DC ownership. PE32 keeps its bitmap/font-backed Graphics owner.
use std::collections::BTreeMap;

struct Dc {
    window: u64,
    thread: u64,
    compatible: bool,
}

pub(crate) struct DeviceContexts {
    entries: BTreeMap<u64, Dc>,
    next: u64,
    end: u64,
}

impl DeviceContexts {
    pub(crate) fn new(start: u64, end: u64) -> Self {
        Self {
            entries: BTreeMap::new(),
            next: start,
            end,
        }
    }

    pub(crate) fn acquire(
        &mut self,
        source: u64,
        thread: u64,
        compatible: bool,
        window_exists: impl FnOnce(u64) -> bool,
    ) -> Result<u64, u32> {
        let valid = source == 0
            || if compatible {
                self.entries.contains_key(&source)
            } else {
                window_exists(source)
            };
        if !valid {
            return Err(if compatible { 6 } else { 1400 });
        }
        if self.entries.len() >= 4096 || self.next >= self.end {
            return Err(8);
        }
        let handle = self.next;
        self.next += 16;
        self.entries.insert(
            handle,
            Dc {
                window: if compatible { 0 } else { source },
                thread,
                compatible,
            },
        );
        Ok(handle)
    }

    pub(crate) fn validate(&self, handle: u64) -> Result<(), u32> {
        self.entries.get(&handle).ok_or(6u32).map(|_| ())
    }

    pub(crate) fn release(
        &mut self,
        handle: u64,
        window: u64,
        thread: u64,
        compatible: bool,
    ) -> Result<u64, u32> {
        let dc = self.entries.get(&handle).ok_or(6u32)?;
        if dc.compatible != compatible
            || dc.thread != thread
            || (!compatible && dc.window != window)
        {
            return Err(6);
        }
        self.entries.remove(&handle);
        Ok(1)
    }
}

pub(crate) fn capabilities64(index: u32) -> u64 {
    match index {
        0 => 0x400,
        2 => 1,
        4 => 271,
        6 => 203,
        8 | 118 => 1024,
        10 | 117 => 768,
        12 => 32,
        14 => 1,
        24 => u32::MAX as u64,
        40 | 42 => 36,
        44 => 51,
        88 | 90 => 96,
        108 => 24,
        116 => 60,
        _ => 0,
    }
}
