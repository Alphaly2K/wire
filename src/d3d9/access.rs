//! Explicit PE32 allocation and device dependencies; resource lifetimes only borrow the heap.
use crate::{graphics::Graphics, heap::GuestHeap, user32::Window};
use std::collections::BTreeMap;

pub(super) struct HeapResources<'a> {
    pub heap: &'a mut GuestHeap,
    pub process_heap: u32,
}

pub(super) struct DeviceResources<'a> {
    pub allocations: HeapResources<'a>,
    pub windows: &'a BTreeMap<u32, Window>,
    pub graphics: &'a mut Graphics,
}
