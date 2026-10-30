use super::*;

pub(crate) struct Reservation {
    path: String,
    rights: u32,
    share: u32,
}

#[derive(Default)]
pub(crate) struct Sharing {
    entries: RefCell<Vec<Weak<Reservation>>>,
}

impl Sharing {
    pub fn check(&self, path: &str, rights: u32, share: u32) -> Result<(), u32> {
        let key = path.to_lowercase();
        if self
            .entries
            .borrow()
            .iter()
            .filter_map(Weak::upgrade)
            .any(|r| r.path == key && (rights & !r.share != 0 || r.rights & !share != 0))
        {
            Err(32)
        } else {
            Ok(())
        }
    }

    pub fn reserve(
        self: &Rc<Self>,
        path: &str,
        rights: u32,
        share: u32,
    ) -> Result<Rc<Reservation>, u32> {
        self.check(path, rights, share)?;
        let reservation = Rc::new(Reservation {
            path: path.to_lowercase(),
            rights,
            share,
        });
        let mut entries = self.entries.borrow_mut();
        entries.retain(|entry| entry.strong_count() != 0);
        entries.push(Rc::downgrade(&reservation));
        Ok(reservation)
    }

    pub fn in_use(&self, path: &str) -> bool {
        let key = path.to_lowercase();
        self.entries
            .borrow()
            .iter()
            .filter_map(Weak::upgrade)
            .any(|r| {
                r.path == key
                    || r.path
                        .strip_prefix(&key)
                        .is_some_and(|tail| tail.starts_with('\\'))
            })
    }
}
