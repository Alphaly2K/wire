//! PE32 logical provider identity. Guest PE images/TLS are owned by Modules.
use std::collections::HashMap;

pub(crate) struct Providers {
    by_name: HashMap<String, u32>,
    by_handle: HashMap<u32, String>,
    next: u32,
}
impl Default for Providers {
    fn default() -> Self {
        Self {
            by_name: HashMap::new(),
            by_handle: HashMap::new(),
            next: 0x7000_1000,
        }
    }
}
impl Providers {
    pub(crate) fn handle_for(&mut self, dll: &str) -> u32 {
        let dll = dll.to_ascii_lowercase();
        if let Some(&handle) = self.by_name.get(&dll) {
            return handle;
        }
        let handle = self.next;
        self.next += 0x1_0000;
        self.by_name.insert(dll.clone(), handle);
        self.by_handle.insert(handle, dll);
        handle
    }
    pub(crate) fn handle(&self, dll: &str) -> Option<u32> {
        self.by_name.get(dll).copied()
    }
    pub(crate) fn name(&self, handle: u32) -> Option<&String> {
        self.by_handle.get(&handle)
    }
}
