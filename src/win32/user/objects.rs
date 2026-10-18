//! Process registered names and per-window property values, without guest memory.
use std::collections::BTreeMap;

pub(crate) struct RegisteredNames {
    entries: BTreeMap<String, u32>,
    next: u32,
}

pub(crate) enum NameError {
    Invalid,
    Full,
}

impl Default for RegisteredNames {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
            next: 0xc000,
        }
    }
}

impl RegisteredNames {
    pub(crate) fn register(&mut self, text: &str) -> Result<u32, NameError> {
        if text.is_empty() || text.encode_utf16().count() > 255 {
            return Err(NameError::Invalid);
        }
        let key = text.to_lowercase();
        if let Some(&atom) = self.entries.get(&key) {
            return Ok(atom);
        }
        if self.next > 0xffff {
            return Err(NameError::Full);
        }
        let atom = self.next;
        self.next += 1;
        self.entries.insert(key, atom);
        Ok(atom)
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PropertyKey {
    Atom(u32),
    Name(String),
}

#[derive(Default)]
pub(crate) struct Properties {
    values: BTreeMap<PropertyKey, u32>,
}

impl Properties {
    pub(crate) fn set(&mut self, key: PropertyKey, value: u32) {
        self.values.insert(key, value);
    }
    pub(crate) fn get(&self, key: &PropertyKey) -> u32 {
        self.values.get(key).copied().unwrap_or(0)
    }
    pub(crate) fn remove(&mut self, key: &PropertyKey) -> u32 {
        self.values.remove(key).unwrap_or(0)
    }
}
