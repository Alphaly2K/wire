use super::group::{Group, Record, Target};
use crate::machine::Mode;
use std::collections::BTreeMap;
use std::rc::Rc;

pub(crate) const ID: u32 = 1;
pub(crate) const ALL_ACCESS: u32 = 0x001f_ffff;
pub(crate) const DUP_HANDLE: u32 = 0x40;
pub(crate) const QUERY: u32 = 0x400;
pub(crate) const QUERY_LIMITED: u32 = 0x1000;
pub(crate) const SYNCHRONIZE: u32 = 0x10_0000;
pub(crate) const VM_OPERATION: u32 = 8;
pub(crate) const VM_READ: u32 = 0x10;
pub(crate) const VM_WRITE: u32 = 0x20;
pub(crate) const TERMINATE: u32 = 1;

#[derive(Clone)]
struct Handle {
    target: Target,
    access: u32,
    flags: u32,
}

pub(crate) struct Current {
    pub(crate) error_mode: u32,
    pub(crate) exit: Option<u32>,
    mode: Mode,
    pub(crate) record: Rc<Record>,
    group: Rc<Group>,
    handles: BTreeMap<u64, Handle>,
    next: u64,
}

impl Current {
    pub(crate) fn inherit_handles(&mut self, source: &Self, select: impl Fn(u64, u32) -> bool) {
        self.handles = source
            .handles
            .iter()
            .filter(|(h, v)| select(**h, v.flags))
            .map(|(&h, v)| (h, v.clone()))
            .collect();
        self.next = source.next;
    }
    pub(crate) fn new(mode: Mode) -> Self {
        Self::with_id(mode, ID).expect("root process identity")
    }

    pub(crate) fn with_id(mode: Mode, id: u32) -> Result<Self, u32> {
        if !(1..=65535).contains(&id) {
            return Err(87);
        }
        let (group, record) = Group::root(id);
        Ok(Self {
            error_mode: 0,
            exit: None,
            mode,
            record,
            group,
            handles: BTreeMap::new(),
            next: 0x7fff_0000,
        })
    }

    pub(crate) fn id(&self) -> u32 {
        self.record.id
    }
    pub(crate) fn process_ids(&self) -> Vec<u32> {
        self.group.process_ids()
    }

    pub(super) fn mode(&self) -> Mode {
        self.mode
    }

    pub(crate) fn first_thread(&self) -> u32 {
        self.record.first_thread()
    }

    pub(crate) fn pseudo(&self, handle: u64) -> bool {
        handle == self.mode.address(u64::MAX)
    }

    pub(crate) fn access(&self, handle: u64) -> Result<u32, u32> {
        if self.pseudo(handle) {
            Ok(ALL_ACCESS)
        } else {
            self.handles
                .get(&handle)
                .filter(|h| matches!(h.target, Target::Process(_)))
                .map(|h| h.access)
                .ok_or(6)
        }
    }

    pub(crate) fn target(&self, handle: u64) -> Result<Rc<Record>, u32> {
        if self.pseudo(handle) {
            return Ok(self.record.clone());
        }
        match &self.handles.get(&handle).ok_or(6u32)?.target {
            Target::Process(record) => Ok(record.clone()),
            _ => Err(6),
        }
    }

    pub(crate) fn check_current(&self, handle: u64, required: u32) -> Result<(), u32> {
        self.check(handle, required)?;
        if self.target(handle)?.id != self.id() {
            Err(50)
        } else {
            Ok(())
        }
    }

    pub(crate) fn child(&self, mode: Mode) -> Result<Self, u32> {
        let record = self.group.reserve()?;
        if matches!(self.record.priority_class.get(), 0x40 | 0x4000) {
            record.priority_class.set(self.record.priority_class.get());
        }
        Ok(Self {
            error_mode: 0,
            exit: None,
            mode,
            record,
            group: self.group.clone(),
            handles: BTreeMap::new(),
            next: 0x7fff0000,
        })
    }

    pub(crate) fn capacity(&self, count: usize) -> Result<(), u32> {
        if self.handles.len() + count > 1024
            || self.next + count as u64 > crate::win32::standard_handles::ALIAS_START
        {
            Err(8)
        } else {
            Ok(())
        }
    }

    pub(crate) fn publish_child(&mut self, child: &Self) -> Result<(u64, u64), u32> {
        self.capacity(2)?;
        if !self.group.contains(&child.group) || child.record.published.get() {
            return Err(87);
        }
        let process = self.alias(Target::Process(child.record.clone()), ALL_ACCESS, false)?;
        let thread = self.alias(
            Target::Primary(child.record.clone()),
            super::threads::ACCESS,
            false,
        )?;
        child.record.published.set(true);
        Ok((process, thread))
    }

    pub(crate) fn primary(&self, handle: u64, access: u32) -> Result<Option<Rc<Record>>, u32> {
        let Some(h) = self.handles.get(&handle) else {
            return Ok(None);
        };
        let Target::Primary(record) = &h.target else {
            return Ok(None);
        };
        if access != 0 && h.access & access == 0 {
            return Err(5);
        }
        Ok(Some(record.clone()))
    }

    pub(crate) fn primary_open(&mut self, id: u32, access: u32, inherit: bool) -> Result<u64, u32> {
        let pid = id.checked_sub(1).ok_or(87u32)? / 65536 + 1;
        let record = self
            .group
            .get(pid)
            .filter(|r| r.id != self.id() && r.first_thread() == id)
            .ok_or(87u32)?;
        let access = if access == ALL_ACCESS {
            super::threads::ACCESS
        } else {
            access
        };
        if access & !super::threads::ACCESS != 0 {
            return Err(50);
        }
        self.alias(Target::Primary(record), access, inherit)
    }

    pub(crate) fn record(&self, id: u32) -> Option<Rc<Record>> {
        self.group.get(id)
    }

    pub(crate) fn image_path(&self, path: &str, descendants: bool) -> bool {
        self.group.image_path(path, descendants)
    }

    pub(crate) fn check(&self, handle: u64, required: u32) -> Result<(), u32> {
        if self.access(handle)? & required == required {
            Ok(())
        } else {
            Err(5)
        }
    }

    pub(crate) fn query(&self, handle: u64) -> Result<(), u32> {
        if self.access(handle)? & (QUERY | QUERY_LIMITED) != 0 {
            Ok(())
        } else {
            Err(5)
        }
    }

    fn alias(&mut self, target: Target, access: u32, inherit: bool) -> Result<u64, u32> {
        self.capacity(1)?;
        let handle = self.next;
        self.handles.insert(
            handle,
            Handle {
                target,
                access,
                flags: u32::from(inherit),
            },
        );
        self.next += 1;
        Ok(handle)
    }

    pub(crate) fn open(&mut self, id: u32, access: u32, inherit: bool) -> Result<u64, u32> {
        let record = self.group.get(id).ok_or(87u32)?;
        if access & !ALL_ACCESS != 0 {
            return Err(50);
        }
        self.alias(Target::Process(record), access, inherit)
    }

    pub(crate) fn duplicate(
        &mut self,
        handle: u64,
        desired: Option<u32>,
        inherit: bool,
    ) -> Result<u64, u32> {
        let (target, access) = if self.pseudo(handle) {
            (Target::Process(self.record.clone()), ALL_ACCESS)
        } else {
            let h = self.handles.get(&handle).ok_or(6u32)?;
            (h.target.clone(), h.access)
        };
        let desired = desired.unwrap_or(access);
        if desired & !access != 0 {
            return Err(5);
        }
        if desired & !target.access() != 0 {
            return Err(5);
        }
        self.alias(target, desired, inherit)
    }

    pub(crate) fn flags(&self, handle: u64) -> Option<u32> {
        self.handles.get(&handle).map(|h| h.flags)
    }

    pub(crate) fn set_flags(&mut self, handle: u64, mask: u32, value: u32) -> Result<(), u32> {
        let h = self.handles.get_mut(&handle).ok_or(6u32)?;
        if mask & !3 != 0 {
            return Err(87);
        }
        h.flags = (h.flags & !mask) | (value & mask);
        Ok(())
    }

    pub(crate) fn close(&mut self, handle: u64) -> Result<(), u32> {
        if self.flags(handle).ok_or(6u32)? & 2 != 0 {
            return Err(6);
        }
        self.handles.remove(&handle);
        Ok(())
    }

    pub(super) fn clear(&mut self) {
        self.error_mode = 0;
        self.exit = None;
        self.handles.clear();
    }
    pub(super) fn finish(&mut self, code: u32) {
        self.exit = Some(code);
        self.record.exit.set(Some(code));
        self.record.images.borrow_mut().clear();
        self.record
            .primary_exit
            .set(self.record.primary_exit.get().or(Some(code)));
        self.handles.clear();
    }
}

impl Drop for Current {
    fn drop(&mut self) {
        self.group.abandon(&self.record);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::win32::standard_handles::{StandardHandles, ALIAS_START};

    #[test]
    fn unpublished_processes_rollback_and_published_exit_outlives_handles() {
        let mut parent = Current::new(Mode::X64);
        let rejected = parent.child(Mode::X86).unwrap();
        assert_eq!(parent.open(rejected.id(), QUERY, false), Err(87));
        let reserved = rejected.id();
        drop(rejected);
        let mut child = parent.child(Mode::X86).unwrap();
        assert_eq!(child.id(), reserved);
        let (process, thread) = parent.publish_child(&child).unwrap();
        let wait_record = parent.target(process).unwrap();
        let alias = parent.duplicate(process, Some(QUERY), false).unwrap();
        parent.close(process).unwrap();
        parent.close(thread).unwrap();
        assert_eq!(wait_record.exit.get(), None);
        child.record.primary_exit.set(Some(19));
        child.finish(73);
        drop(child);
        assert_eq!(wait_record.exit.get(), Some(73));
        assert_eq!(wait_record.primary_exit.get(), Some(19));
        assert_eq!(parent.target(alias).unwrap().exit.get(), Some(73));
        assert_eq!(parent.check_current(alias, QUERY), Err(50));
        parent.close(alias).unwrap();
        assert_eq!(parent.record(reserved).unwrap().exit.get(), Some(73));
    }

    #[test]
    fn process_identity_exhaustion_never_enters_standard_aliases() {
        for mode in [Mode::X86, Mode::X64] {
            let mut process = Current::new(mode);
            process.next = ALIAS_START - 1;
            let last = process.open(ID, QUERY, false).unwrap();
            let mut standard = StandardHandles::default();
            let alias = standard
                .duplicate(0x101, None, false, false)
                .unwrap()
                .unwrap();
            assert!(last < alias);
            process.close(last).unwrap();
            assert_eq!(process.open(ID, QUERY, false), Err(8));
            assert_eq!(
                process.duplicate(mode.address(u64::MAX), None, false),
                Err(8)
            );
            assert_eq!(standard.flags(alias), Some(0));
        }
    }
}
