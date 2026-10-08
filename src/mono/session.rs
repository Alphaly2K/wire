//! A process chooses one VM owner before initialization. Failure closes calls;
//! cleanup waits for retained roots/metadata/callbacks/shared storage and calls.
use std::sync::{Arc, Mutex};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    OriginalX64,
    NativeArm64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Planned,
    Initializing,
    Active,
    Failed,
    Stopping,
    Closed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceKind {
    ManagedRoot,
    Metadata,
    Callback,
    SharedBacking,
}
impl ResourceKind {
    fn index(self) -> usize {
        match self {
            Self::ManagedRoot => 0,
            Self::Metadata => 1,
            Self::Callback => 2,
            Self::SharedBacking => 3,
        }
    }
}
/// Explicit admission input from the future in-process profile verifier. A
/// metadata declaration or successful fixture alone cannot set these proofs.
#[derive(Clone, Debug, Default)]
pub struct Admission {
    pub requested: bool,
    pub known_binary_fingerprint: bool,
    pub full_native_abi: bool,
    pub checked_memory_and_roots: bool,
    pub windows_services_and_scheduler: bool,
    pub unity_initialization_callbacks: bool,
    pub required_exports_covered: bool,
    pub no_dynamic_code: bool,
}
impl Admission {
    fn issues(&self) -> Vec<&'static str> {
        let requirements = [
            (self.requested, "native backend disabled"),
            (
                self.known_binary_fingerprint,
                "unknown original Mono fingerprint",
            ),
            (self.full_native_abi, "full native ABI not verified"),
            (
                self.checked_memory_and_roots,
                "checked memory/roots incomplete",
            ),
            (
                self.windows_services_and_scheduler,
                "Windows services/scheduler incomplete",
            ),
            (
                self.unity_initialization_callbacks,
                "Unity initialization callbacks incomplete",
            ),
            (
                self.required_exports_covered,
                "required typed exports incomplete",
            ),
            (
                self.no_dynamic_code,
                "no-dynamic-code configuration not verified",
            ),
        ];
        requirements
            .into_iter()
            .filter_map(|(ready, issue)| (!ready).then_some(issue))
            .collect()
    }
}
struct Core {
    backend: Backend,
    state: State,
    issues: Vec<&'static str>,
    failure: Option<String>,
    calls: usize,
    resources: [usize; 4],
}
#[derive(Clone)]
pub struct BackendSession(Arc<Mutex<Core>>);
impl BackendSession {
    /// Selection is made once before either VM initializes. A failed preflight
    /// chooses the original owner without entering the native VM.
    pub fn select(admission: &Admission) -> Self {
        let issues = admission.issues();
        let backend = if issues.is_empty() {
            Backend::NativeArm64
        } else {
            Backend::OriginalX64
        };
        Self(Arc::new(Mutex::new(Core {
            backend,
            state: State::Planned,
            issues,
            failure: None,
            calls: 0,
            resources: [0; 4],
        })))
    }
    pub fn backend(&self) -> Backend {
        self.0.lock().unwrap().backend
    }
    pub fn state(&self) -> State {
        self.0.lock().unwrap().state
    }
    pub fn admission_issues(&self) -> Vec<&'static str> {
        self.0.lock().unwrap().issues.clone()
    }
    pub fn failure(&self) -> Option<String> {
        self.0.lock().unwrap().failure.clone()
    }
    pub fn begin_initialization(&self) -> Result<Backend, &'static str> {
        let mut core = self.0.lock().unwrap();
        if core.state != State::Planned {
            return Err("VM owner already selected/started; use isolated process restart");
        }
        core.state = State::Initializing;
        Ok(core.backend)
    }
    pub fn initialized(&self) -> Result<(), &'static str> {
        let mut core = self.0.lock().unwrap();
        if core.state != State::Initializing {
            return Err("VM initialization is not active");
        }
        core.state = State::Active;
        Ok(())
    }
    /// Neither initialization failure nor a late unsupported call changes VM.
    pub fn fail(&self, reason: impl Into<String>) -> Result<(), &'static str> {
        let mut core = self.0.lock().unwrap();
        if !matches!(
            core.state,
            State::Initializing | State::Active | State::Failed
        ) {
            return Err("VM is not running/initializing");
        }
        if core.failure.is_none() {
            core.failure = Some(reason.into());
        }
        core.state = State::Failed;
        Ok(())
    }
    pub fn enter_call(&self, backend: Backend) -> Result<CallLease, &'static str> {
        let mut core = self.0.lock().unwrap();
        if core.backend != backend {
            return Err("cross-VM call rejected");
        }
        if core.state != State::Active {
            return Err("VM does not accept calls");
        }
        core.calls = core
            .calls
            .checked_add(1)
            .ok_or("VM call lease capacity exceeded")?;
        Ok(CallLease(self.0.clone()))
    }
    pub fn retain(
        &self,
        backend: Backend,
        kind: ResourceKind,
    ) -> Result<ResourceLease, &'static str> {
        let mut core = self.0.lock().unwrap();
        if core.backend != backend {
            return Err("cross-VM resource rejected");
        }
        if core.state != State::Active {
            return Err("VM does not accept retained resources");
        }
        core.resources[kind.index()] = core.resources[kind.index()]
            .checked_add(1)
            .ok_or("VM resource lease capacity exceeded")?;
        Ok(ResourceLease {
            core: self.0.clone(),
            kind,
        })
    }
    /// First close admission. Owners then release handles/revoke mappings and
    /// callbacks, and complete existing invocations before runtime cleanup.
    pub fn begin_shutdown(&self) -> Result<(), &'static str> {
        let mut core = self.0.lock().unwrap();
        if matches!(core.state, State::Closed | State::Stopping) {
            return Err("VM shutdown already started");
        }
        core.state = State::Stopping;
        Ok(())
    }
    pub fn cleanup_ready(&self) -> bool {
        let core = self.0.lock().unwrap();
        core.state == State::Stopping && core.calls == 0 && core.resources.iter().all(|&n| n == 0)
    }
    /// Call only after the single runtime's cleanup succeeds. This state
    /// machine performs no native cleanup and cannot recover a failed VM.
    pub fn cleaned_up(&self) -> Result<(), &'static str> {
        let mut core = self.0.lock().unwrap();
        if core.state != State::Stopping {
            return Err("VM shutdown has not started");
        }
        if core.calls != 0 || core.resources.iter().any(|&n| n != 0) {
            return Err("VM cleanup blocked by live calls/resources");
        }
        core.state = State::Closed;
        Ok(())
    }
}
/// Its owner must keep the lease until the native/guest invocation actually
/// returns, including nested calls and managed exception handling.
pub struct CallLease(Arc<Mutex<Core>>);
impl Drop for CallLease {
    fn drop(&mut self) {
        let mut core = self.0.lock().unwrap();
        assert!(core.calls > 0);
        core.calls -= 1;
    }
}
/// Retain alongside the real handle/metadata/entry/backing owner. Release the
/// real resource first, then drop this lease; it does not free native storage.
pub struct ResourceLease {
    core: Arc<Mutex<Core>>,
    kind: ResourceKind,
}
impl Drop for ResourceLease {
    fn drop(&mut self) {
        let mut core = self.core.lock().unwrap();
        let count = &mut core.resources[self.kind.index()];
        assert!(*count > 0);
        *count -= 1;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn ready() -> Admission {
        Admission {
            requested: true,
            known_binary_fingerprint: true,
            full_native_abi: true,
            checked_memory_and_roots: true,
            windows_services_and_scheduler: true,
            unity_initialization_callbacks: true,
            required_exports_covered: true,
            no_dynamic_code: true,
        }
    }
    #[test]
    fn preflight_requires_every_proof_before_choosing_native() {
        for missing in 0..8 {
            let mut proof = ready();
            match missing {
                0 => proof.requested = false,
                1 => proof.known_binary_fingerprint = false,
                2 => proof.full_native_abi = false,
                3 => proof.checked_memory_and_roots = false,
                4 => proof.windows_services_and_scheduler = false,
                5 => proof.unity_initialization_callbacks = false,
                6 => proof.required_exports_covered = false,
                _ => proof.no_dynamic_code = false,
            }
            let session = BackendSession::select(&proof);
            assert_eq!(session.backend(), Backend::OriginalX64);
            assert_eq!(session.admission_issues().len(), 1);
        }
        assert_eq!(
            BackendSession::select(&ready()).backend(),
            Backend::NativeArm64
        );
        assert_eq!(
            BackendSession::select(&Admission::default()).backend(),
            Backend::OriginalX64
        );
    }
    #[test]
    fn failed_initialization_never_switches_vm_owners() {
        let session = BackendSession::select(&ready());
        assert_eq!(session.begin_initialization(), Ok(Backend::NativeArm64));
        session.fail("allocator callback unsupported").unwrap();
        assert_eq!(session.backend(), Backend::NativeArm64);
        assert!(session.begin_initialization().is_err());
        assert!(session.initialized().is_err());
        assert!(session.enter_call(Backend::OriginalX64).is_err());
        session.fail("secondary failure").unwrap();
        assert_eq!(
            session.failure().as_deref(),
            Some("allocator callback unsupported")
        );
    }
    #[test]
    fn late_failure_closes_new_calls_but_retains_active_roots() {
        let session = BackendSession::select(&ready());
        session.begin_initialization().unwrap();
        session.initialized().unwrap();
        let call = session.enter_call(Backend::NativeArm64).unwrap();
        let root = session
            .retain(Backend::NativeArm64, ResourceKind::ManagedRoot)
            .unwrap();
        session.fail("late PInvoke failure").unwrap();
        assert!(session.enter_call(Backend::NativeArm64).is_err());
        session.begin_shutdown().unwrap();
        assert!(!session.cleanup_ready());
        drop(call);
        assert!(!session.cleanup_ready());
        drop(root);
        assert!(session.cleanup_ready());
        session.cleaned_up().unwrap();
        assert_eq!(session.state(), State::Closed);
        assert!(session.begin_initialization().is_err());
    }
    #[test]
    fn callbacks_metadata_and_shared_backing_prevent_cleanup() {
        let session = BackendSession::select(&ready());
        session.begin_initialization().unwrap();
        session.initialized().unwrap();
        assert!(session
            .retain(Backend::OriginalX64, ResourceKind::ManagedRoot)
            .is_err());
        let resources = [
            ResourceKind::ManagedRoot,
            ResourceKind::Metadata,
            ResourceKind::Callback,
            ResourceKind::SharedBacking,
        ]
        .map(|kind| session.retain(Backend::NativeArm64, kind).unwrap());
        session.begin_shutdown().unwrap();
        assert!(session.cleaned_up().is_err());
        assert!(session
            .retain(Backend::NativeArm64, ResourceKind::ManagedRoot)
            .is_err());
        for resource in resources {
            drop(resource);
        }
        assert!(session.cleanup_ready());
        session.cleaned_up().unwrap();
        assert!(!session.cleanup_ready());
    }
    #[test]
    fn shutdown_waits_for_a_remote_in_flight_call_and_callback_owner() {
        use std::sync::mpsc;
        let session = BackendSession::select(&ready());
        session.begin_initialization().unwrap();
        session.initialized().unwrap();
        let callback = session
            .retain(Backend::NativeArm64, ResourceKind::Callback)
            .unwrap();
        let remote = session.clone();
        let (started, receive) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let call = remote.enter_call(Backend::NativeArm64).unwrap();
            started.send(()).unwrap();
            released.recv().unwrap();
            drop(call);
        });
        receive.recv().unwrap();
        session
            .fail("owner revoked while remote call active")
            .unwrap();
        session.begin_shutdown().unwrap();
        assert!(session.enter_call(Backend::NativeArm64).is_err());
        assert!(!session.cleanup_ready());
        release.send(()).unwrap();
        worker.join().unwrap();
        assert!(!session.cleanup_ready());
        drop(callback);
        assert!(session.cleanup_ready());
        session.cleaned_up().unwrap();
    }
}
