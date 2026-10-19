//! PE32's existing mixed kernel-object namespace and named-object lifetime.
use std::{collections::HashMap, fs::File};

#[derive(Debug)]
pub enum HandleObj {
    File {
        file: File,
        readable: bool,
        writable: bool,
    },
    Search {
        entries: Vec<std::path::PathBuf>,
        next: usize,
    },
    Event {
        manual: bool,
        signaled: bool,
    },
    Mutex {
        owner: Option<u32>,
        count: u32,
    },
    Semaphore {
        count: i32,
        max: i32,
    },
    Thread {
        tid: u32,
    },
    Process {
        access: u32,
    },
}

#[derive(Debug)]
pub struct KernelObjects32 {
    objects: HashMap<u32, HandleObj>,
    names: HashMap<String, u32>,
    next: u32,
}
impl Default for KernelObjects32 {
    fn default() -> Self {
        Self {
            objects: HashMap::new(),
            names: HashMap::new(),
            next: 0x100,
        }
    }
}
impl KernelObjects32 {
    pub fn len(&self) -> usize {
        self.objects.len()
    }
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }
    pub fn contains_key(&self, handle: &u32) -> bool {
        self.objects.contains_key(handle)
    }
    pub fn get(&self, handle: &u32) -> Option<&HandleObj> {
        self.objects.get(handle)
    }
    pub fn get_mut(&mut self, handle: &u32) -> Option<&mut HandleObj> {
        self.objects.get_mut(handle)
    }

    pub(crate) fn alloc(&mut self, obj: HandleObj) -> u32 {
        let h = self.next;
        self.next += 4;
        self.objects.insert(h, obj);
        h
    }
    pub(crate) fn create_named(
        &mut self,
        name: &str,
        make: impl FnOnce() -> HandleObj,
    ) -> (u32, bool) {
        if !name.is_empty() {
            if let Some(&h) = self.names.get(name) {
                return (h, true);
            }
        }
        let h = self.alloc(make());
        if !name.is_empty() {
            self.names.insert(name.to_string(), h);
        }
        (h, false)
    }
    pub(crate) fn named(&self, name: &str) -> Option<u32> {
        self.names.get(name).copied()
    }
    pub(crate) fn close(&mut self, handle: u32) -> bool {
        if self.objects.remove(&handle).is_some() {
            self.names.retain(|_, h| *h != handle);
            true
        } else {
            false
        }
    }
    // FindClose already checks the search type; preserve its distinct removal.
    pub(crate) fn remove(&mut self, handle: &u32) -> Option<HandleObj> {
        self.objects.remove(handle)
    }
}
