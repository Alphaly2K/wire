//! Gateway lookup rejects guest code outside the registered address envelope.
//! Bounds follow every insertion, including arbitrary addresses installed by
//! the loader. The ordered map remains authoritative for hits and holes.
use std::{collections::BTreeMap, ops::Index, sync::Arc};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HotImport {
    General,
    ThreadId,
    ProcessId,
    ThreadHandle,
    ProcessHandle,
    Counter,
    Frequency,
    Tick64,
    Tick32,
    TlsGet,
    TlsSet,
    TlsFree,
    ErrorGet,
    ErrorSet,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControlImport {
    None,
    LocaleEnumeration,
    Module(crate::win32::catalog::ModuleApi),
    Wait(crate::win32::catalog::WaitApi),
}
pub(crate) struct BoundImport {
    pub export: Arc<str>,
    pub dll: Arc<str>,
    pub original_function: Arc<str>,
    pub name: Arc<str>,
    pub trace_export: Arc<str>,
    pub arity: Option<usize>,
    pub supported: bool,
    pub dwords: &'static [usize],
    pub groups: u32,
    pub hot: HotImport,
    pub api: Option<crate::win32::catalog::ApiId>,
    pub control: ControlImport,
    pub valid: bool,
}
impl BoundImport {
    pub fn new(export: &str) -> Self {
        let parts = export.split_once('!');
        let (dll, original) = parts.unwrap_or(("", ""));
        Self::build(dll, original, export, parts.is_some())
    }
    pub fn from_parts(dll: &str, original: &str) -> Self {
        Self::build(dll, original, &format!("{dll}!{original}"), true)
    }
    fn build(dll: &str, original: &str, export: &str, valid: bool) -> Self {
        use crate::win32::catalog::{
            resolve, ApiId, Architecture, ClockApi, ContextApi, LocalApi, Resolution,
        };
        let name = super::normalize_symbol(dll, original);
        let contract = match resolve(Architecture::X64, dll, name) {
            Resolution::Bound(contract) => Some(contract),
            Resolution::WrongDll(_) | Resolution::Legacy => None,
        };
        let hot = match contract.map(|contract| contract.id) {
            Some(ApiId::Clock(id)) => match id {
                ClockApi::TickCount | ClockApi::MultimediaTick => HotImport::Tick32,
                ClockApi::TickCount64 => HotImport::Tick64,
                ClockApi::PerformanceCounter => HotImport::Counter,
                ClockApi::PerformanceFrequency => HotImport::Frequency,
                ClockApi::SystemFileTime => HotImport::General,
            },
            Some(ApiId::Context(id)) => match id {
                ContextApi::ThreadId => HotImport::ThreadId,
                ContextApi::ProcessId => HotImport::ProcessId,
                ContextApi::ThreadHandle => HotImport::ThreadHandle,
                ContextApi::ProcessHandle => HotImport::ProcessHandle,
                ContextApi::GetLastError => HotImport::ErrorGet,
                ContextApi::SetLastError => HotImport::ErrorSet,
                ContextApi::DebuggerPresent => HotImport::General,
            },
            Some(ApiId::Local(id)) => match id {
                LocalApi::TlsGet => HotImport::TlsGet,
                LocalApi::TlsSet => HotImport::TlsSet,
                LocalApi::TlsFree => HotImport::TlsFree,
                _ => HotImport::General,
            },
            _ => HotImport::General,
        };
        Self {
            export: export.into(),
            dll: dll.into(),
            original_function: original.into(),
            name: name.into(),
            trace_export: format!("{dll}!{name}").into(),
            arity: contract.map_or_else(|| super::import_arity(name), |c| Some(c.parameters.len())),
            supported: valid && super::import_supported(dll, name),
            dwords: contract.map_or_else(|| super::import_dwords(name), |c| c.win64_dwords()),
            groups: super::import_groups(name)
                | if contract.is_some_and(|contract| {
                    matches!(contract.id, crate::win32::catalog::ApiId::File(id) if id.is_search())
                }) {
                    1 << 8
                } else {
                    0
                },
            hot,
            api: contract.and_then(|c| match c.id {
                crate::win32::catalog::ApiId::Nls(
                    crate::win32::catalog::NlsApi::EnumSystemLocalesEx,
                ) => None,
                crate::win32::catalog::ApiId::Module(id) if id.control() => None,
                crate::win32::catalog::ApiId::Wait(_) => None,
                _ => Some(c.id),
            }),
            control: match contract.map(|c| c.id) {
                Some(crate::win32::catalog::ApiId::Nls(
                    crate::win32::catalog::NlsApi::EnumSystemLocalesEx,
                )) => ControlImport::LocaleEnumeration,
                Some(crate::win32::catalog::ApiId::Module(id)) if id.control() => {
                    ControlImport::Module(id)
                }
                Some(crate::win32::catalog::ApiId::Wait(id)) => ControlImport::Wait(id),
                _ => ControlImport::None,
            },
            valid,
        }
    }
}

pub struct Gateways {
    entries: BTreeMap<u64, String>,
    bindings: BTreeMap<u64, Arc<BoundImport>>,
    first: u64,
    last: u64,
}

impl From<BTreeMap<u64, String>> for Gateways {
    fn from(entries: BTreeMap<u64, String>) -> Self {
        Self {
            bindings: entries
                .iter()
                .map(|(&pc, name)| (pc, Arc::new(BoundImport::new(name))))
                .collect(),
            first: entries.first_key_value().map_or(u64::MAX, |(&at, _)| at),
            last: entries.last_key_value().map_or(0, |(&at, _)| at),
            entries,
        }
    }
}

impl Gateways {
    pub(crate) fn has_entry(&self, start: u64, end: u64) -> bool {
        if end <= start || end <= self.first || start > self.last {
            return false;
        }
        self.entries
            .range((
                std::ops::Bound::Included(start),
                std::ops::Bound::Excluded(end),
            ))
            .next()
            .is_some()
    }
    pub(crate) fn binding(&self, address: &u64) -> Option<Arc<BoundImport>> {
        if *address < self.first || *address > self.last {
            return None;
        }
        self.bindings.get(address).cloned()
    }
    pub fn get(&self, address: &u64) -> Option<&String> {
        if *address < self.first || *address > self.last {
            return None;
        }
        self.entries.get(address)
    }

    pub fn contains_key(&self, address: &u64) -> bool {
        self.get(address).is_some()
    }

    pub fn insert(&mut self, address: u64, name: String) -> Option<String> {
        self.first = self.first.min(address);
        self.last = self.last.max(address);
        self.bindings
            .insert(address, Arc::new(BoundImport::new(&name)));
        self.entries.insert(address, name)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> std::collections::btree_map::Iter<'_, u64, String> {
        self.entries.iter()
    }

    pub fn values(&self) -> std::collections::btree_map::Values<'_, u64, String> {
        self.entries.values()
    }
}

impl Index<&u64> for Gateways {
    type Output = String;

    fn index(&self, address: &u64) -> &Self::Output {
        &self.entries[address]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x64::{
        cpu::Cpu64,
        memory::{Memory, READ, WRITE},
        runtime::{Control, Runtime64},
    };
    const STACK: u64 = 0x7ff000000000;
    const RETURN: u64 = 0x140000100;
    #[test]
    fn segment_boundaries_include_first_and_last_gateway_but_exclude_end() {
        let mut gateways = Gateways::from(BTreeMap::new());
        assert!(!gateways.has_entry(100, 101));
        gateways.insert(100, "kernel32.dll!GetCurrentThreadId".into());
        assert!(gateways.has_entry(100, 101));
        assert!(!gateways.has_entry(99, 100));
        assert!(!gateways.has_entry(100, 100));
        assert!(!gateways.has_entry(101, 100));
        gateways.insert(200, "kernel32.dll!GetCurrentProcessId".into());
        assert!(gateways.has_entry(100, 101));
        assert!(gateways.has_entry(200, 201));
        assert!(!gateways.has_entry(101, 200));
        assert!(gateways.has_entry(101, 201));
    }
    fn setup() -> (Memory, Cpu64, Runtime64) {
        let mut mem = Memory::default();
        mem.map(STACK, 65536, READ | WRITE).unwrap();
        let runtime =
            Runtime64::new(&mut mem, 0x140000000, STACK, 65536, "\"C:\\test.exe\"").unwrap();
        (mem, Cpu64::new(0, STACK + 0x1008), runtime)
    }
    fn prepare(cpu: &mut Cpu64, mem: &mut Memory, pc: u64, args: &[u64]) {
        cpu.rip = pc;
        cpu.regs[4] = STACK + 0x1008;
        mem.write_int(cpu.regs[4], 8, RETURN).unwrap();
        for (n, value) in args.iter().enumerate() {
            cpu.regs[[1, 2, 8, 9][n]] = *value;
        }
    }
    #[test]
    fn bound_hot_routes_match_general_abi_tls_last_error_and_stack_aliasing() {
        let (mut fast_mem, mut fast, mut fast_runtime) = setup();
        let (mut ref_mem, mut reference, mut ref_runtime) = setup();
        for (name, args) in [
            ("TlsAlloc", vec![]),
            ("SetLastError", vec![77]),
            ("TlsSetValue", vec![0xaaaa00000000, 0x123456789abcdef0]),
            ("TlsGetValue", vec![0xbbbb00000000]),
            ("GetLastError", vec![]),
            ("GetCurrentThreadId", vec![]),
            ("GetCurrentProcessId", vec![]),
            ("GetCurrentThread", vec![]),
            ("GetCurrentProcess", vec![]),
            ("QueryPerformanceFrequency", vec![STACK + 0x1008]),
            ("TlsFree", vec![0xcccc00000000]),
            ("TlsGetValue", vec![999]),
        ] {
            let gateway = fast_runtime.register("kernel32.dll", name);
            let ref_gateway = ref_runtime.register("kernel32.dll", name);
            prepare(&mut fast, &mut fast_mem, gateway, &args);
            prepare(&mut reference, &mut ref_mem, ref_gateway, &args);
            assert!(matches!(
                fast_runtime
                    .dispatch_gateway(&mut fast, &mut fast_mem)
                    .unwrap(),
                Control::Returned
            ));
            let mut descriptor = BoundImport::from_parts("kernel32.dll", name);
            descriptor.hot = HotImport::General;
            assert!(matches!(
                ref_runtime
                    .dispatch_bound(&descriptor, &mut reference, &mut ref_mem)
                    .unwrap(),
                Control::Returned
            ));
            assert_eq!(fast, reference, "{name}");
            assert_eq!(fast_runtime.api_calls, ref_runtime.api_calls);
            assert_eq!(
                fast_runtime.trace.back().unwrap().to_string(),
                ref_runtime.trace.back().unwrap().to_string()
            );
            assert_eq!(
                fast_mem.read_int(STACK + 0x1008, 8).unwrap(),
                ref_mem.read_int(STACK + 0x1008, 8).unwrap()
            );
        }
    }
    #[test]
    fn replacing_gateway_binding_and_faulting_return_frames_preserve_call_state() {
        let (mut mem, mut cpu, mut runtime) = setup();
        let gateway = runtime.register("kernel32.dll", "GetCurrentThreadId");
        runtime
            .gateways
            .insert(gateway, "kernel32.dll!GetCurrentProcessId".into());
        prepare(&mut cpu, &mut mem, gateway, &[]);
        runtime.dispatch_gateway(&mut cpu, &mut mem).unwrap();
        assert_eq!(cpu.regs[0], super::super::PID);
        for replacement in ["kernel32.dll!NoSuchApi", "invalid"] {
            runtime.gateways.insert(gateway, replacement.into());
            prepare(&mut cpu, &mut mem, gateway, &[]);
            let before = cpu.clone();
            let calls = runtime.api_calls;
            let trace = runtime.trace.len();
            assert!(runtime.dispatch_gateway(&mut cpu, &mut mem).is_err());
            assert_eq!(cpu, before);
            assert_eq!(runtime.api_calls, calls);
            assert_eq!(runtime.trace.len(), trace);
        }
        runtime
            .gateways
            .insert(gateway, "kernel32.dll!SetLastError".into());
        prepare(&mut cpu, &mut mem, gateway, &[123]);
        cpu.regs[4] = 0x12345;
        let before = cpu.clone();
        let calls = runtime.api_calls;
        assert!(runtime.dispatch_gateway(&mut cpu, &mut mem).is_err());
        assert_eq!(cpu, before);
        assert_eq!(runtime.api_calls, calls);
    }
}
