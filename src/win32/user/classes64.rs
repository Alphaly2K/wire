//! Win64 class identity and atom ownership, independent of guest memory/windows.
use std::collections::BTreeMap;

#[derive(Clone)]
pub(crate) struct Class {
    pub(crate) instance: u64,
    pub(crate) name: String,
    pub(crate) procedure: u64,
    pub(crate) style: u32,
    pub(crate) window_extra: usize,
}

pub(crate) enum Key<'a> {
    Id(u32),
    Name(&'a str),
}

pub(crate) struct Classes {
    records: BTreeMap<u16, Class>,
    next_atom: u16,
}

impl Default for Classes {
    fn default() -> Self {
        Self {
            records: BTreeMap::new(),
            next_atom: 0xc000,
        }
    }
}

impl Classes {
    pub(crate) fn register(&mut self, class: Class) -> Result<u16, u32> {
        if self
            .records
            .values()
            .any(|c| c.instance == class.instance && c.name.eq_ignore_ascii_case(&class.name))
        {
            return Err(1410);
        }
        // Win64's existing boundary excludes 0xffff; PE32 includes it.
        if self.next_atom == 0xffff {
            return Err(8);
        }
        let atom = self.next_atom;
        self.next_atom += 1;
        self.records.insert(atom, class);
        Ok(atom)
    }

    pub(crate) fn find(
        &self,
        instance: u64,
        key: Key<'_>,
        allow_global: bool,
    ) -> Option<(u16, &Class)> {
        self.records.iter().find_map(|(&atom, class)| {
            let matches = match key {
                Key::Id(id) => atom as u32 == id,
                Key::Name(name) => class.name.eq_ignore_ascii_case(name),
            };
            (matches && (class.instance == instance || allow_global && class.style & 0x4000 != 0))
                .then_some((atom, class))
        })
    }

    pub(crate) fn remove(&mut self, atom: u16) {
        self.records.remove(&atom);
    }
}
