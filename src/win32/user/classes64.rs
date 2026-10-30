//! Window-class identity and atom ownership, independent of guest memory/windows.
use std::collections::BTreeMap;

#[derive(Clone)]
pub(crate) struct Class {
    pub(crate) instance: u64,
    pub(crate) name: String,
    pub(crate) procedure: u64,
    pub(crate) style: u32,
    pub(crate) window_extra: usize,
    pub(crate) wide: bool,
}

pub(crate) enum Key<'a> {
    Id(u32),
    Name(&'a str),
}

pub(crate) struct Classes {
    records: BTreeMap<u16, Class>,
    next_atom: u32,
    pe32: bool,
}

impl Default for Classes {
    fn default() -> Self {
        Self::new(false)
    }
}

impl Classes {
    pub(crate) fn new(pe32: bool) -> Self {
        Self {
            records: BTreeMap::new(),
            next_atom: 0xc000,
            pe32,
        }
    }
    fn same_name(&self, a: &str, b: &str) -> bool {
        if self.pe32 {
            a.to_lowercase() == b.to_lowercase()
        } else {
            a.eq_ignore_ascii_case(b)
        }
    }
    pub(crate) fn check_register(&self, class: &Class) -> Result<(), u32> {
        if self.records.values().any(|c| {
            (c.instance == class.instance || self.pe32 && c.style & 0x4000 != 0)
                && self.same_name(&c.name, &class.name)
        }) {
            return Err(1410);
        }
        if self.next_atom >= if self.pe32 { 0x10000 } else { 0xffff } {
            return Err(8);
        }
        Ok(())
    }
    pub(crate) fn register(&mut self, class: Class) -> Result<u16, u32> {
        self.check_register(&class)?;
        let atom = self.next_atom as u16;
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
                Key::Name(name) => self.same_name(&class.name, name),
            };
            (matches && (class.instance == instance || allow_global && class.style & 0x4000 != 0))
                .then_some((atom, class))
        })
    }

    pub(crate) fn remove(&mut self, atom: u16) {
        self.records.remove(&atom);
    }
    #[allow(dead_code)]
    pub(crate) fn get_mut(&mut self, atom: u16) -> Option<&mut Class> {
        self.records.get_mut(&atom)
    }
    #[allow(dead_code)]
    pub(crate) fn has_ascii_name(&self, name: &str) -> bool {
        self.records
            .values()
            .any(|c| c.name.eq_ignore_ascii_case(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn class(name: &str) -> Class {
        Class {
            instance: 1,
            name: name.into(),
            procedure: 2,
            style: 0,
            window_extra: 0,
            wide: true,
        }
    }
    #[test]
    fn preflight_preserves_identity_and_each_architectures_last_atom() {
        let mut pe32 = Classes::new(true);
        pe32.next_atom = 0xffff;
        assert!(pe32.check_register(&class("last")).is_ok());
        assert!(pe32.check_register(&class("last")).is_ok());
        assert_eq!(pe32.register(class("last")), Ok(0xffff));
        assert_eq!(pe32.check_register(&class("LAST")), Err(1410));
        pe32.remove(0xffff);
        assert_eq!(pe32.register(class("new")), Err(8));
        let mut amd64 = Classes::new(false);
        amd64.next_atom = 0xffff;
        assert_eq!(amd64.register(class("last")), Err(8));
    }
}
