//! Waitable event/semaphore objects with shared names and per-handle access.
use std::collections::BTreeMap;
const FULL_ACCESS: u32 = 0x1f0003;
enum State {
    Event { manual: bool, signaled: bool },
    Semaphore { count: i32, maximum: i32 },
}
impl State {
    fn ready(&self) -> bool {
        match self {
            Self::Event { signaled, .. } => *signaled,
            Self::Semaphore { count, .. } => *count > 0,
        }
    }
    fn acquire(&mut self) {
        match self {
            Self::Event {
                manual: false,
                signaled,
            } => *signaled = false,
            Self::Semaphore { count, .. } => *count -= 1,
            _ => {}
        }
    }
}
struct Object {
    state: State,
    name: Option<String>,
}
pub(super) struct Events {
    next: u64,
    objects: BTreeMap<u64, Object>,
    handles: BTreeMap<u64, (u64, u32)>,
}
impl Default for Events {
    fn default() -> Self {
        Self {
            next: 0x200,
            objects: BTreeMap::new(),
            handles: BTreeMap::new(),
        }
    }
}
impl Events {
    fn handle(&mut self, object: u64, access: u32) -> Result<u64, u32> {
        if self.handles.len() >= 1024 || self.next >= 0x10000 {
            return Err(8);
        }
        let handle = self.next;
        self.next += 1;
        self.handles.insert(handle, (object, access));
        Ok(handle)
    }
    fn name(name: Option<String>) -> Result<Option<String>, u32> {
        if name
            .as_ref()
            .is_some_and(|n| n.encode_utf16().count() > 260)
        {
            return Err(206);
        }
        Ok(name.map(|n| n.strip_prefix("Local\\").unwrap_or(&n).to_owned()))
    }
    pub fn create(
        &mut self,
        manual: bool,
        signaled: bool,
        name: Option<String>,
        access: u32,
    ) -> Result<(u64, bool), u32> {
        if access & !FULL_ACCESS != 0 {
            return Err(5);
        }
        let name = Self::name(name)?;
        if name.is_some() {
            if let Some((&object, _)) = self.objects.iter().find(|(_, event)| event.name == name) {
                if !matches!(self.objects[&object].state, State::Event { .. }) {
                    return Err(6);
                }
                return self.handle(object, access).map(|handle| (handle, true));
            }
        }
        let object = self.next;
        let handle = self.handle(object, access)?;
        self.objects.insert(
            object,
            Object {
                state: State::Event { manual, signaled },
                name,
            },
        );
        Ok((handle, false))
    }
    pub fn open(&mut self, name: String, access: u32, semaphore: bool) -> Result<u64, u32> {
        if access & !FULL_ACCESS != 0 {
            return Err(5);
        }
        let name = Self::name(Some(name))?;
        let object = self
            .objects
            .iter()
            .find(|(_, event)| event.name == name)
            .map(|(&id, _)| id)
            .ok_or(2u32)?;
        if matches!(self.objects[&object].state, State::Semaphore { .. }) != semaphore {
            return Err(6);
        }
        self.handle(object, access)
    }
    pub fn semaphore(
        &mut self,
        initial: i32,
        maximum: i32,
        name: Option<String>,
        access: u32,
    ) -> Result<(u64, bool), u32> {
        if access & !FULL_ACCESS != 0 {
            return Err(5);
        }
        let name = Self::name(name)?;
        if name.is_some() {
            if let Some((&object, _)) = self.objects.iter().find(|(_, obj)| obj.name == name) {
                if !matches!(self.objects[&object].state, State::Semaphore { .. }) {
                    return Err(6);
                }
                return self.handle(object, access).map(|handle| (handle, true));
            }
        }
        if initial < 0 || maximum <= 0 || initial > maximum {
            return Err(87);
        }
        let object = self.next;
        let handle = self.handle(object, access)?;
        self.objects.insert(
            object,
            Object {
                state: State::Semaphore {
                    count: initial,
                    maximum,
                },
                name,
            },
        );
        Ok((handle, false))
    }
    pub fn release_count(&self, handle: u64, increment: i32) -> Result<i32, u32> {
        let &(object, access) = self.handles.get(&handle).ok_or(6u32)?;
        if access & 2 == 0 {
            return Err(5);
        }
        let State::Semaphore { count, maximum } = self.objects[&object].state else {
            return Err(6);
        };
        if increment <= 0 {
            return Err(87);
        }
        if increment > maximum - count {
            return Err(298);
        }
        Ok(count)
    }
    pub fn release(&mut self, handle: u64, increment: i32) {
        let object = self.handles[&handle].0;
        let State::Semaphore { count, .. } = &mut self.objects.get_mut(&object).unwrap().state
        else {
            unreachable!("release must be validated before mutation")
        };
        *count += increment;
    }
    pub fn change(&mut self, handle: u64, signaled: bool) -> Result<u64, u32> {
        let &(object, access) = self.handles.get(&handle).ok_or(6u32)?;
        if access & 2 == 0 {
            return Err(5);
        }
        let State::Event {
            signaled: state, ..
        } = &mut self.objects.get_mut(&object).unwrap().state
        else {
            return Err(6);
        };
        *state = signaled;
        Ok(1)
    }
    pub fn flags(&self, handle: u64) -> Option<u32> {
        self.handles.contains_key(&handle).then_some(0)
    }
    pub fn waitable(&self, handle: u64) -> Result<(u64, bool), u32> {
        let &(object, access) = self.handles.get(&handle).ok_or(6u32)?;
        if access & 0x100000 == 0 {
            return Err(5);
        }
        Ok((object, self.objects[&object].state.ready()))
    }
    pub fn close(&mut self, handle: u64) -> bool {
        let Some((object, _)) = self.handles.remove(&handle) else {
            return false;
        };
        if !self.handles.values().any(|&(id, _)| id == object) {
            self.objects.remove(&object);
        }
        true
    }
    /// None means the caller must block; no fictional signal or timeout is produced.
    pub fn wait(&mut self, handles: &[u64], all: bool) -> Result<Option<u64>, u32> {
        if handles.is_empty() || handles.len() > 64 {
            return Err(87);
        }
        let mut objects = Vec::new();
        for handle in handles {
            let &(object, access) = self.handles.get(handle).ok_or(6u32)?;
            if access & 0x100000 == 0 {
                return Err(5);
            }
            if all && objects.contains(&object) {
                return Err(87);
            }
            objects.push(object);
        }
        let selected = if all {
            if !objects.iter().all(|id| self.objects[id].state.ready()) {
                return Ok(None);
            }
            0
        } else {
            let Some(index) = objects.iter().position(|id| self.objects[id].state.ready()) else {
                return Ok(None);
            };
            index
        };
        for object in if all {
            &objects[..]
        } else {
            &objects[selected..selected + 1]
        } {
            self.objects.get_mut(object).unwrap().state.acquire();
        }
        Ok(Some(selected as u64))
    }
}
