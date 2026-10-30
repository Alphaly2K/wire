//! DC identity and lifetime, with optional raster state in the same owner table.
use std::collections::BTreeMap;

struct Dc<T> {
    window: u64,
    thread: u64,
    compatible: bool,
    state: T,
}

pub(crate) struct DeviceContexts<T = ()> {
    entries: BTreeMap<u64, Dc<T>>,
    next: u64,
    end: u64,
}

impl<T: Default> DeviceContexts<T> {
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
        self.preview(source, compatible, window_exists)?;
        let handle = self.next;
        self.next += 16;
        self.entries.insert(
            handle,
            Dc {
                window: if compatible { 0 } else { source },
                thread,
                compatible,
                state: T::default(),
            },
        );
        Ok(handle)
    }

    pub(crate) fn preview(
        &self,
        source: u64,
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
        Ok(self.next)
    }
    pub(crate) fn state(&self, handle: u64) -> Result<&T, u32> {
        self.entries.get(&handle).map(|dc| &dc.state).ok_or(6)
    }
    pub(crate) fn state_mut(&mut self, handle: u64) -> Result<&mut T, u32> {
        self.entries
            .get_mut(&handle)
            .map(|dc| &mut dc.state)
            .ok_or(6)
    }
    pub(crate) fn states(&self) -> impl Iterator<Item = (u64, &T)> {
        self.entries.iter().map(|(&h, dc)| (h, &dc.state))
    }
    pub(crate) fn count(&self) -> usize {
        self.entries.len()
    }
    pub(crate) fn information(&self, handle: u64) -> Result<(u64, u64, bool), u32> {
        let dc = self.entries.get(&handle).ok_or(6u32)?;
        Ok((dc.window, dc.thread, dc.compatible))
    }
    pub(crate) fn remove_window(&mut self, window: u64) {
        self.entries
            .retain(|_, dc| dc.compatible || dc.window != window);
    }
    pub(crate) fn exit_thread(&mut self, thread: u64) {
        self.entries.retain(|_, dc| dc.thread != thread);
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
