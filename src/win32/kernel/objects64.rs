//! Shared waitable object state. Clock adapters own time; readiness inspection
//! does not acquire signals. Handle identities are separate from object lifetime.
use std::{collections::BTreeMap, time::Duration};
pub(crate) trait Clock {
    type Instant: Copy + Ord;
    fn now(&self) -> Self::Instant;
    fn next_period(
        &self,
        deadline: Self::Instant,
        now: Self::Instant,
        period: Duration,
    ) -> Option<Self::Instant>;
}

const FULL_ACCESS: u32 = 0x1f0003;
enum State<D> {
    Mutex {
        owner: Option<u32>,
        depth: u32,
        abandoned: bool,
    },
    Event {
        manual: bool,
        signaled: bool,
    },
    Semaphore {
        count: i32,
        maximum: i32,
    },
    Timer {
        manual: bool,
        signaled: bool,
        due: Option<D>,
        period: Duration,
    },
}
impl<D: Copy + Ord> State<D> {
    fn ready(&self) -> bool {
        match self {
            Self::Mutex { owner, .. } => owner.is_none(),
            Self::Event { signaled, .. } | Self::Timer { signaled, .. } => *signaled,
            Self::Semaphore { count, .. } => *count > 0,
        }
    }
    fn ready_for(&self, thread: u32) -> bool {
        match self {
            Self::Mutex { owner, .. } => owner.is_none() || *owner == Some(thread),
            _ => self.ready(),
        }
    }
    fn acquire(&mut self) {
        self.acquire_for(0);
    }
    fn acquire_for(&mut self, thread: u32) -> bool {
        let mut was_abandoned = false;
        match self {
            Self::Mutex {
                owner,
                depth,
                abandoned,
            } => {
                *owner = Some(thread);
                *depth = depth.checked_add(1).expect("validated mutex depth");
                was_abandoned = std::mem::take(abandoned);
            }
            Self::Event {
                manual: false,
                signaled,
            }
            | Self::Timer {
                manual: false,
                signaled,
                ..
            } => *signaled = false,
            Self::Semaphore { count, .. } => *count -= 1,
            _ => {}
        }
        was_abandoned
    }
    fn refresh<C: Clock<Instant = D>>(&mut self, clock: &C, now: D) {
        if let Self::Timer {
            due,
            period,
            signaled,
            ..
        } = self
        {
            if let Some(deadline) = *due {
                if now >= deadline {
                    *signaled = true;
                    *due = if period.is_zero() {
                        None
                    } else {
                        // Skip elapsed periods; signals are a state, not accumulated credits.
                        clock.next_period(deadline, now, *period)
                    };
                }
            }
        }
    }
}
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(crate) struct WaitState {
    pub id: u64,
    pub ready: bool,
    pub abandoned: bool,
}
struct Object<D> {
    waiters: u32,
    state: State<D>,
    name: Option<String>,
}
pub(crate) struct Objects<C: Clock> {
    pub clock: C,
    end: u64,
    flags: BTreeMap<u64, u32>,
    next: u64,
    objects: BTreeMap<u64, Object<C::Instant>>,
    handles: BTreeMap<u64, (u64, u32)>,
}
impl<C: Clock> Objects<C> {
    pub fn new(clock: C, start: u64, end: u64) -> Self {
        assert!(start < end);
        Self {
            clock,
            end,
            next: start,
            flags: BTreeMap::new(),
            objects: BTreeMap::new(),
            handles: BTreeMap::new(),
        }
    }
    // Resident adapter / resource diagnostics; optional in the native profile.
    #[allow(dead_code)]
    pub fn handle_count(&self) -> usize {
        self.handles.len()
    }
    // Resident adapter / resource diagnostics; optional in the native profile.
    #[allow(dead_code)]
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }
    fn handle(&mut self, object: u64, access: u32) -> Result<u64, u32> {
        if self.handles.len() >= 1024 || self.next >= self.end {
            return Err(8);
        }
        let handle = self.next;
        self.next += 1;
        self.handles.insert(handle, (object, access));
        self.flags.insert(handle, 0);
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
                waiters: 0,
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
        if !matches!(
            (&self.objects[&object].state, semaphore),
            (State::Semaphore { .. }, true) | (State::Event { .. }, false)
        ) {
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
                waiters: 0,
                state: State::Semaphore {
                    count: initial,
                    maximum,
                },
                name,
            },
        );
        Ok((handle, false))
    }
    pub fn timer(
        &mut self,
        manual: bool,
        name: Option<String>,
        access: u32,
    ) -> Result<(u64, bool), u32> {
        if access & !FULL_ACCESS != 0 {
            return Err(5);
        }
        let name = Self::name(name)?;
        if name.is_some() {
            if let Some((&object, _)) = self.objects.iter().find(|(_, obj)| obj.name == name) {
                if !matches!(self.objects[&object].state, State::Timer { .. }) {
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
                waiters: 0,
                state: State::Timer {
                    manual,
                    signaled: false,
                    due: None,
                    period: Duration::ZERO,
                },
                name,
            },
        );
        Ok((handle, false))
    }
    #[allow(dead_code)]
    pub fn mutex(
        &mut self,
        owner: Option<u32>,
        name: Option<String>,
        access: u32,
    ) -> Result<(u64, bool), u32> {
        if access & !0x1f0001 != 0 {
            return Err(5);
        }
        let name = Self::name(name)?;
        if name.is_some() {
            if let Some((&object, _)) = self.objects.iter().find(|(_, obj)| obj.name == name) {
                if !matches!(self.objects[&object].state, State::Mutex { .. }) {
                    return Err(6);
                }
                return self.handle(object, access).map(|h| (h, true));
            }
        }
        let object = self.next;
        let handle = self.handle(object, access)?;
        self.objects.insert(
            object,
            Object {
                waiters: 0,
                name,
                state: State::Mutex {
                    owner,
                    depth: u32::from(owner.is_some()),
                    abandoned: false,
                },
            },
        );
        Ok((handle, false))
    }
    #[allow(dead_code)]
    pub fn open_mutex(&mut self, name: String, access: u32) -> Result<u64, u32> {
        if access & !0x1f0001 != 0 {
            return Err(5);
        }
        let name = Self::name(Some(name))?;
        let (&object, _) = self
            .objects
            .iter()
            .find(|(_, obj)| obj.name == name)
            .ok_or(2u32)?;
        if !matches!(self.objects[&object].state, State::Mutex { .. }) {
            return Err(6);
        }
        self.handle(object, access)
    }
    #[allow(dead_code)]
    pub fn release_mutex(&mut self, handle: u64, thread: u32) -> Result<(), u32> {
        let &(id, access) = self.handles.get(&handle).ok_or(6u32)?;
        if access & 1 == 0 {
            return Err(5);
        }
        let State::Mutex { owner, depth, .. } = &mut self.objects.get_mut(&id).unwrap().state
        else {
            return Err(6);
        };
        if *owner != Some(thread) {
            return Err(288);
        }
        *depth -= 1;
        if *depth == 0 {
            *owner = None;
        }
        Ok(())
    }
    #[allow(dead_code)]
    pub fn abandon_thread(&mut self, thread: u32) {
        for object in self.objects.values_mut() {
            if let State::Mutex {
                owner,
                depth,
                abandoned,
            } = &mut object.state
            {
                if *owner == Some(thread) {
                    *owner = None;
                    *depth = 0;
                    *abandoned = true;
                }
            }
        }
    }
    #[allow(dead_code)]
    pub fn retain_wait(&mut self, id: u64) {
        let object = self.objects.get_mut(&id).expect("validated wait identity");
        object.waiters = object
            .waiters
            .checked_add(1)
            .expect("bounded guest wait references");
    }
    #[allow(dead_code)]
    pub fn release_wait(&mut self, id: u64) {
        let object = self.objects.get_mut(&id).expect("retained wait identity");
        object.waiters -= 1;
        self.collect(id);
    }
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.objects.clear();
        self.handles.clear();
        self.flags.clear();
    }
    #[allow(dead_code)]
    pub fn needs_time(&self, id: u64) -> bool {
        self.objects
            .get(&id)
            .is_some_and(|o| matches!(o.state, State::Timer { due: Some(_), .. }))
    }
    #[allow(dead_code)]
    pub fn handle_needs_time(&self, handle: u64) -> bool {
        self.handles
            .get(&handle)
            .is_some_and(|&(id, _)| self.needs_time(id))
    }
    #[allow(dead_code)]
    pub fn peek_id(&self, id: u64, thread: u32) -> Result<WaitState, u32> {
        let state = &self.objects.get(&id).ok_or(6u32)?.state;
        if matches!(state, State::Mutex { owner: Some(owner), depth: u32::MAX, .. } if *owner == thread)
        {
            return Err(8);
        }
        let expired = matches!(state, State::Timer { due: Some(d), .. } if self.clock.now() >= *d);
        Ok(WaitState {
            id,
            ready: state.ready_for(thread) || expired,
            abandoned: matches!(
                state,
                State::Mutex {
                    abandoned: true,
                    ..
                }
            ),
        })
    }
    #[allow(dead_code)]
    pub fn peek_for(&self, handle: u64, thread: u32) -> Result<WaitState, u32> {
        let &(id, access) = self.handles.get(&handle).ok_or(6u32)?;
        if access & 0x100000 == 0 {
            return Err(5);
        }
        self.peek_id(id, thread)
    }
    #[allow(dead_code)]
    pub fn acquire_id(&mut self, id: u64, thread: u32) {
        let state = &mut self
            .objects
            .get_mut(&id)
            .expect("validated wait identity")
            .state;
        state.refresh(&self.clock, self.clock.now());
        assert!(state.ready_for(thread), "validated wait readiness");
        state.acquire_for(thread);
    }
    pub fn open_timer(&mut self, name: String, access: u32) -> Result<u64, u32> {
        if access & !FULL_ACCESS != 0 {
            return Err(5);
        }
        let name = Self::name(Some(name))?;
        let object = self
            .objects
            .iter()
            .find(|(_, obj)| obj.name == name)
            .map(|(&id, _)| id)
            .ok_or(2u32)?;
        if !matches!(self.objects[&object].state, State::Timer { .. }) {
            return Err(6);
        }
        self.handle(object, access)
    }
    pub fn set_timer(
        &mut self,
        handle: u64,
        deadline: Option<C::Instant>,
        period: Duration,
    ) -> Result<(), u32> {
        let &(object, access) = self.handles.get(&handle).ok_or(6u32)?;
        if access & 2 == 0 {
            return Err(5);
        }
        let state = &mut self.objects.get_mut(&object).unwrap().state;
        if deadline.is_none() {
            // A timer may have expired since the last observation. Cancel
            // preserves that signal even when nobody has waited on it yet.
            state.refresh(&self.clock, self.clock.now());
        }
        let State::Timer {
            signaled,
            due,
            period: p,
            ..
        } = state
        else {
            return Err(6);
        };
        if let Some(deadline) = deadline {
            *signaled = false;
            *due = Some(deadline);
            *p = period;
        } else {
            // CancelWaitableTimer preserves an already signaled state.
            *due = None;
            *p = Duration::ZERO;
        }
        Ok(())
    }
    pub fn next_timer(&self) -> Option<C::Instant> {
        self.objects
            .values()
            .filter_map(|obj| match obj.state {
                State::Timer { due, .. } => due,
                _ => None,
            })
            .min()
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
    #[allow(dead_code)]
    pub fn pulse_identity(&self, handle: u64) -> Result<(u64, bool), u32> {
        let &(id, access) = self.handles.get(&handle).ok_or(6u32)?;
        if access & 2 == 0 {
            return Err(5);
        }
        match self.objects[&id].state {
            State::Event { manual, .. } => Ok((id, manual)),
            _ => Err(6),
        }
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
    pub fn signal(&mut self, handle: u64, thread: u32) -> Result<(), u32> {
        let &(id, access) = self.handles.get(&handle).ok_or(6u32)?;
        let state = &mut self.objects.get_mut(&id).unwrap().state;
        let required = if matches!(state, State::Mutex { .. }) {
            1
        } else {
            2
        };
        if access & required == 0 {
            return Err(5);
        }
        match state {
            State::Event { signaled, .. } => *signaled = true,
            State::Semaphore { count, maximum } => {
                if *count == *maximum {
                    return Err(298);
                }
                *count += 1;
            }
            State::Mutex { owner, depth, .. } => {
                if *owner != Some(thread) {
                    return Err(288);
                }
                *depth -= 1;
                if *depth == 0 {
                    *owner = None;
                }
            }
            State::Timer { .. } => return Err(6),
        }
        Ok(())
    }
    pub fn flags(&self, handle: u64) -> Option<u32> {
        self.flags.get(&handle).copied()
    }
    #[allow(dead_code)]
    pub(crate) fn selects_handles(&self, select: impl Fn(u64, u32) -> bool) -> bool {
        self.flags.iter().any(|(&h, &flags)| select(h, flags))
    }
    pub fn waitable(&mut self, handle: u64) -> Result<(u64, bool), u32> {
        let &(object, access) = self.handles.get(&handle).ok_or(6u32)?;
        if access & 0x100000 == 0 {
            return Err(5);
        }
        let state = &mut self.objects.get_mut(&object).unwrap().state;
        state.refresh(&self.clock, self.clock.now());
        Ok((object, state.ready()))
    }
    // Resident adapter / resource diagnostics; optional in the native profile.
    #[allow(dead_code)]
    pub fn peek_waitable(&self, handle: u64) -> Result<(u64, bool), u32> {
        self.peek_for(handle, 0).map(|s| (s.id, s.ready))
    }

    pub fn set_flags(&mut self, handle: u64, mask: u32, flags: u32) -> Result<(), u32> {
        let current = self.flags.get_mut(&handle).ok_or(6u32)?;
        if mask & !3 != 0 {
            return Err(87);
        }
        *current = (*current & !mask) | (flags & mask);
        Ok(())
    }
    // Resident adapter / resource diagnostics; optional in the native profile.
    #[allow(dead_code)]
    pub fn duplicate(
        &mut self,
        source: u64,
        desired: Option<u32>,
        inherit: bool,
        close_source: bool,
    ) -> Result<u64, u32> {
        let &(object, access) = self.handles.get(&source).ok_or(6u32)?;
        if close_source && self.flags[&source] & 2 != 0 {
            return Err(6);
        }
        let requested = desired.unwrap_or(access);
        if requested & !FULL_ACCESS != 0 || requested & !access != 0 {
            return Err(5);
        }
        let handle = self.handle(object, requested)?;
        self.flags.insert(handle, inherit as u32);
        Ok(handle)
    }
    // Resident adapter / resource diagnostics; optional in the native profile.
    #[allow(dead_code)]
    pub fn close_checked(&mut self, handle: u64) -> Result<(), u32> {
        let flags = self.flags.get(&handle).ok_or(6u32)?;
        if flags & 2 != 0 {
            return Err(5);
        }
        assert!(self.close(handle));
        Ok(())
    }
    pub fn close(&mut self, handle: u64) -> bool {
        let Some((object, _)) = self.handles.remove(&handle) else {
            return false;
        };
        self.flags.remove(&handle);
        self.collect(object);
        true
    }
    fn collect(&mut self, object: u64) {
        if self.objects[&object].waiters == 0 && !self.handles.values().any(|&(id, _)| id == object)
        {
            self.objects.remove(&object);
        }
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
            if matches!(
                self.objects[&object].state,
                State::Mutex {
                    owner: Some(0),
                    depth: u32::MAX,
                    ..
                }
            ) {
                return Err(8);
            }
            if all && objects.contains(&object) {
                return Err(87);
            }
            objects.push(object);
        }
        let now = self.clock.now();
        for object in &objects {
            self.objects
                .get_mut(object)
                .unwrap()
                .state
                .refresh(&self.clock, now);
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

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct ManualClock(u64);
    impl Clock for ManualClock {
        type Instant = u64;
        fn now(&self) -> u64 {
            self.0
        }
        fn next_period(&self, deadline: u64, now: u64, period: Duration) -> Option<u64> {
            let ticks = period.as_millis() as u64;
            now.checked_add(ticks - (now - deadline) % ticks)
        }
    }
    fn objects() -> Objects<ManualClock> {
        Objects::new(ManualClock::default(), 0x7ffe30000000, 0x7ffe40000000)
    }
    #[test]
    fn wait_all_is_atomic_and_readiness_does_not_consume_auto_reset_or_semaphore() {
        let mut o = objects();
        let event = o.create(false, true, None, FULL_ACCESS).unwrap().0;
        let sem = o.semaphore(0, 2, None, FULL_ACCESS).unwrap().0;
        assert_eq!(o.wait(&[event, sem], true), Ok(None));
        assert!(o.peek_waitable(event).unwrap().1);
        assert!(o.peek_waitable(event).unwrap().1);
        assert_eq!(o.release_count(sem, 1), Ok(0));
        o.release(sem, 1);
        assert_eq!(o.wait(&[event, sem], true), Ok(Some(0)));
        assert!(!o.peek_waitable(event).unwrap().1);
        assert!(!o.peek_waitable(sem).unwrap().1);
        o.change(event, true).unwrap();
        assert_eq!(o.wait(&[event, u64::MAX], false), Err(6));
        assert!(o.peek_waitable(event).unwrap().1);
        let alias = o.duplicate(event, None, false, false).unwrap();
        assert_eq!(o.wait(&[event, alias], true), Err(87));
        assert!(o.peek_waitable(event).unwrap().1);
        assert_eq!(o.wait(&[sem, event], false), Ok(Some(1)));
        assert_eq!(o.wait(&[event], false), Ok(None));
    }
    #[test]
    fn periodic_timer_skips_missed_periods_without_accumulating_signals() {
        let mut o = objects();
        let timer = o.timer(false, None, FULL_ACCESS).unwrap().0;
        o.set_timer(timer, Some(10), Duration::from_millis(10))
            .unwrap();
        o.clock.0 = 45;
        assert!(o.peek_waitable(timer).unwrap().1);
        assert_eq!(o.next_timer(), Some(10)); // Pure readiness, not a state transition.
        assert_eq!(o.wait(&[timer], false), Ok(Some(0)));
        assert_eq!(o.next_timer(), Some(50));
        assert_eq!(o.wait(&[timer], false), Ok(None));
        o.clock.0 = 49;
        assert_eq!(o.wait(&[timer], false), Ok(None));
        o.clock.0 = 50;
        assert_eq!(o.wait(&[timer], false), Ok(Some(0)));
        let manual = o.timer(true, None, FULL_ACCESS).unwrap().0;
        o.set_timer(manual, Some(50), Duration::ZERO).unwrap();
        assert!(o.waitable(manual).unwrap().1);
        o.set_timer(manual, None, Duration::ZERO).unwrap();
        assert_eq!(o.wait(&[manual], false), Ok(Some(0)));
        assert_eq!(o.wait(&[manual], false), Ok(Some(0)));
        o.set_timer(manual, Some(60), Duration::ZERO).unwrap();
        o.clock.0 = 61;
        o.set_timer(manual, None, Duration::ZERO).unwrap();
        assert_eq!(o.wait(&[manual], false), Ok(Some(0)));
        assert_eq!(o.wait(&[manual], false), Ok(Some(0)));
    }
    #[test]
    fn aliases_share_state_but_keep_independent_rights_flags_and_name_lifetime() {
        let mut o = objects();
        let (event, existed) = o
            .create(true, false, Some("Local\\shared".into()), FULL_ACCESS)
            .unwrap();
        assert!(!existed);
        assert_eq!(
            o.semaphore(0, 2, Some("shared".into()), FULL_ACCESS),
            Err(6)
        );
        let opened = o.open("shared".into(), 0x100000, false).unwrap();
        let modified = o.duplicate(event, Some(2), true, false).unwrap();
        assert_eq!(o.flags(modified), Some(1));
        assert_eq!(o.change(opened, true), Err(5));
        assert_eq!(o.peek_waitable(modified), Err(5));
        assert_eq!(o.duplicate(opened, Some(FULL_ACCESS), false, false), Err(5));
        o.change(modified, true).unwrap();
        assert_eq!(o.wait(&[opened], false), Ok(Some(0)));
        o.set_flags(event, 2, 2).unwrap();
        assert_eq!(o.close_checked(event), Err(5));
        assert_eq!(o.duplicate(event, None, false, true), Err(6));
        o.set_flags(event, 2, 0).unwrap();
        o.close_checked(event).unwrap();
        assert_eq!(o.object_count(), 1);
        o.close_checked(opened).unwrap();
        o.close_checked(modified).unwrap();
        assert_eq!((o.handle_count(), o.object_count()), (0, 0));
        assert_eq!(o.open("shared".into(), FULL_ACCESS, false), Err(2));
        assert!(o
            .semaphore(0, 2, Some("shared".into()), FULL_ACCESS)
            .is_ok());
    }
    #[test]
    fn lifetime_creation_exceeds_native_small_range_and_live_capacity_remains_bounded() {
        let mut o = objects();
        let mut first = None;
        let mut previous = 0;
        for _ in 0..70000 {
            let h = o
                .create(false, false, Some("one".into()), FULL_ACCESS)
                .unwrap()
                .0;
            first.get_or_insert(h);
            assert!(h > previous);
            previous = h;
            o.close_checked(h).unwrap();
            assert_eq!(
                (o.handle_count(), o.object_count(), o.flags.len()),
                (0, 0, 0)
            );
        }
        assert!(previous - first.unwrap() > 65536);
        let mut handles = Vec::new();
        for _ in 0..1024 {
            handles.push(o.create(false, false, None, FULL_ACCESS).unwrap().0);
        }
        let cursor = o.next;
        assert_eq!(o.create(false, false, None, FULL_ACCESS), Err(8));
        assert_eq!(o.next, cursor);
        assert_eq!((o.handle_count(), o.object_count()), (1024, 1024));
        for h in handles {
            o.close_checked(h).unwrap();
        }
        assert_eq!(
            (o.handle_count(), o.object_count(), o.flags.len()),
            (0, 0, 0)
        );
    }
}
