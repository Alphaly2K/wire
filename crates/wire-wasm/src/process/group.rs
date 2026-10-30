use super::current;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

pub(crate) struct Record {
    pub images: RefCell<Vec<String>>,
    pub id: u32,
    pub published: Cell<bool>,
    pub exit: Cell<Option<u32>>,
    pub primary_exit: Cell<Option<u32>>,
    pub suspended: Cell<u32>,
    pub priority: Cell<i32>,
    pub priority_class: Cell<u32>,
    pub terminate: Cell<Option<u32>>,
}

impl Record {
    fn new(id: u32, published: bool) -> Self {
        Self {
            images: RefCell::new(Vec::new()),
            id,
            published: Cell::new(published),
            exit: Cell::new(None),
            primary_exit: Cell::new(None),
            suspended: Cell::new(0),
            priority: Cell::new(0),
            priority_class: Cell::new(0x20),
            terminate: Cell::new(None),
        }
    }

    pub fn first_thread(&self) -> u32 {
        (self.id - 1) * 65536 + 1
    }
}

pub(crate) struct Group {
    next: Cell<u32>,
    records: RefCell<BTreeMap<u32, Rc<Record>>>,
}

impl Group {
    pub fn process_ids(&self) -> Vec<u32> {
        self.records
            .borrow()
            .values()
            .filter(|r| r.published.get() && r.exit.get().is_none())
            .map(|r| r.id)
            .collect()
    }
    pub fn root(id: u32) -> (Rc<Self>, Rc<Record>) {
        let record = Rc::new(Record::new(id, true));
        let group = Rc::new(Self {
            next: Cell::new(id + 1),
            records: RefCell::new(BTreeMap::from([(id, record.clone())])),
        });
        (group, record)
    }

    pub fn reserve(&self) -> Result<Rc<Record>, u32> {
        let mut records = self.records.borrow_mut();
        let id = self.next.get();
        if id > 65535
            || records.len() >= 1024
            || records.values().filter(|r| r.exit.get().is_none()).count() >= 16
        {
            return Err(8);
        }
        let record = Rc::new(Record::new(id, false));
        records.insert(id, record.clone());
        self.next.set(id + 1);
        Ok(record)
    }

    pub fn get(&self, id: u32) -> Option<Rc<Record>> {
        self.records
            .borrow()
            .get(&id)
            .filter(|r| r.published.get())
            .cloned()
    }

    pub fn abandon(&self, record: &Record) {
        if !record.published.get() {
            self.records.borrow_mut().remove(&record.id);
            if self.next.get() == record.id + 1 {
                self.next.set(record.id);
            }
        } else if record.exit.get().is_none() {
            record.exit.set(Some(0xc000013a));
            record.primary_exit.set(Some(0xc000013a));
        }
    }

    pub fn image_path(&self, path: &str, descendants: bool) -> bool {
        let path = path.to_lowercase();
        self.records
            .borrow()
            .values()
            .filter(|r| r.published.get() && r.exit.get().is_none())
            .any(|record| {
                record.images.borrow().iter().any(|image| {
                    image == &path
                        || descendants
                            && image
                                .strip_prefix(&path)
                                .is_some_and(|tail| tail.starts_with('\\'))
                })
            })
    }

    pub fn contains(&self, other: &Rc<Self>) -> bool {
        std::ptr::eq(self, &**other)
    }
}

#[derive(Clone)]
pub(crate) enum Target {
    Process(Rc<Record>),
    Primary(Rc<Record>),
}

impl Target {
    pub fn access(&self) -> u32 {
        match self {
            Self::Process(_) => current::ALL_ACCESS,
            Self::Primary(_) => super::threads::ACCESS,
        }
    }
}
