//! Portable window metadata. Titles and guest procedures remain owned data.
pub(crate) struct Window {
    pub(crate) class: u16,
    pub(crate) procedure: u64,
    pub(crate) instance: u64,
    pub(crate) parent: u64,
    pub(crate) menu: u64,
    pub(crate) style: u32,
    pub(crate) ex_style: u32,
    pub(crate) rect: [i32; 4],
    pub(crate) client: [i32; 4],
    pub(crate) data: u64,
    pub(crate) extra: Vec<u8>,
    pub(crate) tid: u64,
    pub(crate) wide: bool,
    pub(crate) title: String,
    pub(crate) ready: bool,
}
impl Window {
    pub(crate) fn link(&self) -> super::hierarchy::Link {
        super::hierarchy::Link {
            parent: self.parent,
            style: self.style,
            id: self.menu,
            tid: self.tid,
            offset: [self.client[0], self.client[1]],
        }
    }
}

#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub handle: u64,
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub visible: bool,
    pub topmost: bool,
}
#[derive(Debug, Clone, Copy)]
/// Native input with full guest HWNDs and signed client mouse coordinates.
pub struct WindowEvent {
    pub window: u64,
    pub message: u32,
    pub wparam: u64,
    pub lparam: u64,
}
