//! Bounded directory snapshots and shared cursor ownership.
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
pub(crate) struct Budget {
    pub(crate) used: Cell<usize>,
    pub(crate) limit: usize,
}
impl Budget {
    #[allow(dead_code)] // Configured by the resident file owner.
    pub(crate) fn new(limit: usize) -> Rc<Self> {
        Rc::new(Self {
            used: Cell::new(0),
            limit,
        })
    }
    #[cfg(test)]
    pub(crate) fn used(&self) -> usize {
        self.used.get()
    }
}
pub(crate) struct Entries {
    pub(crate) records: Vec<Vec<u8>>,
    pub(crate) budget: Option<Rc<Budget>>,
    pub(crate) charge: usize,
}
impl Drop for Entries {
    fn drop(&mut self) {
        if let Some(budget) = &self.budget {
            budget.used.set(budget.used.get() - self.charge);
        }
    }
}
impl std::ops::Deref for Entries {
    type Target = [Vec<u8>];
    fn deref(&self) -> &Self::Target {
        &self.records
    }
}
#[derive(Default)]
pub(crate) struct Cursor {
    pub(crate) entries: Option<Rc<Entries>>,
    pub(crate) next: usize,
    pub(crate) pattern: Option<String>,
    pub(crate) budget: Option<Rc<Budget>>,
}
impl Cursor {
    #[allow(dead_code)] // Native storage retains its existing unbounded profile.
    pub(crate) fn with_budget(budget: Rc<Budget>) -> Self {
        Self {
            budget: Some(budget),
            ..Default::default()
        }
    }
}
pub(crate) struct Batch {
    pub(crate) cursor: Rc<RefCell<Cursor>>,
    pub(crate) entries: Rc<Entries>,
    pub(crate) next: usize,
    pub(crate) pattern: String,
    pub(crate) bytes: Vec<u8>,
    pub(crate) status: u32,
}
impl Batch {
    pub(crate) fn commit(self) {
        let mut cursor = self.cursor.borrow_mut();
        cursor.entries = Some(self.entries);
        cursor.next = self.next;
        cursor.pattern = Some(self.pattern);
    }
}

pub(crate) const SUCCESS: u32 = 0;
pub(crate) const NO_MORE_FILES: u32 = 0x80000006;
pub(crate) const BUFFER_TOO_SMALL: u32 = 0xc0000023;
pub(crate) fn nt_error(error: u32) -> u32 {
    match error {
        2 => 0xc000000f,
        3 => 0xc000003a,
        5 => 0xc0000022,
        6 => 0xc0000008,
        8 => 0xc0000017,
        50 => 0xc00000bb,
        87 => 0xc000000d,
        124 => 0xc0000003,
        267 => 0xc0000103,
        _ => 0xc0000001,
    }
}

impl Batch {
    pub(crate) fn prepare(
        cursor: Rc<RefCell<Cursor>>,
        capacity: usize,
        single: bool,
        restart: bool,
        pattern: Option<String>,
        load: impl FnOnce(&str) -> Result<Vec<Vec<u8>>, u32>,
    ) -> Result<Self, u32> {
        let saved = cursor.borrow();
        // A committed query keeps its expression across later calls and restarts.
        let pattern = saved
            .pattern
            .clone()
            .unwrap_or_else(|| pattern.unwrap_or_else(|| "*".into()));
        let entries = if let Some(entries) = saved.entries.as_ref().filter(|_| !restart) {
            entries.clone()
        } else {
            let entries = load(&pattern)?;
            let charge = entries.capacity() * std::mem::size_of::<Vec<u8>>()
                + entries.iter().map(Vec::capacity).sum::<usize>();
            if let Some(budget) = &saved.budget {
                let used = budget
                    .used
                    .get()
                    .checked_add(charge)
                    .filter(|&used| used <= budget.limit)
                    .ok_or(8u32)?;
                budget.used.set(used);
            }
            Rc::new(Entries {
                records: entries,
                budget: saved.budget.clone(),
                charge,
            })
        };
        let mut next = if restart { 0 } else { saved.next };
        drop(saved);
        let mut bytes = Vec::<u8>::new();
        let mut last = None;
        while let Some(record) = entries.get(next) {
            if record.len() > capacity.saturating_sub(bytes.len()) {
                break;
            }
            if let Some(at) = last {
                let offset = (bytes.len() - at) as u32;
                bytes[at..at + 4].copy_from_slice(&offset.to_le_bytes());
            }
            last = Some(bytes.len());
            bytes.extend_from_slice(record);
            next += 1;
            if single {
                break;
            }
        }
        let status = if !bytes.is_empty() {
            SUCCESS
        } else if next == entries.len() {
            NO_MORE_FILES
        } else {
            BUFFER_TOO_SMALL
        };
        Ok(Batch {
            cursor,
            entries,
            next,
            pattern,
            bytes,
            status,
        })
    }
}
