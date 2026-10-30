//! Shared AMD64 graphics owners run against the current VMM and USER records.
use super::{call::Call, catalog::GraphicsExportApi, ServiceError, Services};
use crate::machine::State;
use crate::memory::{Memory as GuestMemory, PAGE, READ, WRITE};

struct Memory<'a>(&'a mut GuestMemory);
impl Memory<'_> {
    fn fault<T>(&self, result: Result<T, crate::memory::Fault>) -> Result<T, String> {
        result.map_err(|fault| {
            self.0.record_fault(fault);
            fault.to_string()
        })
    }
    fn check(&self, p: u64, n: usize, access: u8) -> Result<(), String> {
        self.fault(self.0.check(p, n, access))
    }
    fn read(&self, p: u64, bytes: &mut [u8]) -> Result<(), String> {
        self.fault(self.0.read(p, bytes))
    }
    fn read_int(&self, p: u64, n: usize) -> Result<u64, String> {
        self.fault(self.0.read_int(p, n))
    }
    fn write(&mut self, p: u64, bytes: &[u8]) -> Result<(), String> {
        let result = self.0.write(p, bytes);
        self.fault(result)
    }
    fn write_int(&mut self, p: u64, n: usize, value: u64) -> Result<(), String> {
        let result = self.0.write_int(p, n, value);
        self.fault(result)
    }
    fn map(&mut self, p: u64, n: u64, access: u8) -> Result<(), String> {
        self.0.map(p, n, access)
    }
    fn unmap(&mut self, p: u64, n: u64) -> Result<(), String> {
        self.0.unmap(p, n)
    }
    fn seal_runtime_readonly(&mut self, p: u64) -> Result<(), String> {
        self.0.seal_runtime_readonly(p)
    }
}
pub(crate) use dxgi::BoundMethod;
#[allow(dead_code)]
#[path = "../../../../src/graphics/dxgi.rs"]
pub(crate) mod dxgi;

pub(crate) fn bind_method(name: &str) -> Option<BoundMethod> {
    dxgi::bind_method(name)
}

pub(crate) struct Owner {
    shared: dxgi::Graphics,
    next: u64,
    pending: Option<(u64, Call)>,
}
impl Default for Owner {
    fn default() -> Self {
        Self {
            shared: Default::default(),
            next: 0x7ffb00000000,
            pending: None,
        }
    }
}
impl Owner {
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn attach(&mut self, backend: crate::gpu::Backend) -> Result<(), String> {
        self.shared.attach(backend)
    }
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn attached(&self) -> bool {
        self.shared.attached()
    }
    pub(crate) fn saved_call(&self, pc: u64) -> Option<&Call> {
        self.pending.as_ref().filter(|p| p.0 == pc).map(|p| &p.1)
    }
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn pending(&self) -> bool {
        self.pending.is_some()
    }
    pub(crate) fn dispatch(
        &mut self,
        id: Result<GraphicsExportApi, BoundMethod>,
        call: Call,
        state: &mut State,
        memory: &mut GuestMemory,
        services: &mut Services,
        user: &super::user::User,
    ) -> Result<(), ServiceError> {
        self.shared.clear_pending();
        let mut adapter = Memory(memory);
        let memory = &mut adapter;
        let mut notes = ();
        let result = match id {
            Err(method) if matches!(method.name(), "SwapChain.Present" | "SwapChain.Present1") => {
                (|| {
                    let value =
                        self.shared
                            .present_status(memory, user, method.name(), &call.arguments)?;
                    if value == 0 && call.arguments[2] & 1 == 0 {
                        let (window, texture, backend) = self.shared.frame(call.arguments[0])?;
                        backend.present(&texture, window, call.arguments[1] as u32)?;
                        self.shared.present_completed(call.arguments[0]);
                    }
                    Ok::<_, String>(value)
                })()
            }
            id => {
                let mut calls = dxgi::Calls {
                    graphics: &mut self.shared,
                    gateways: services,
                    next_alloc: &mut self.next,
                    trace: &mut notes,
                    user,
                };
                match id {
                    Ok(id) => calls.export(memory, id, &call.arguments),
                    Err(method) => calls.method(memory, method, &call.arguments),
                }
            }
        };
        match result {
            Ok(value) => {
                self.pending = None;
                call.complete(state, value);
                Ok(())
            }
            Err(_) if self.shared.pending() => {
                self.pending = Some((state.ip, call));
                Err(ServiceError::Graphics)
            }
            Err(message) => {
                if let Some(fault) = memory.0.fault() {
                    Err(ServiceError::Fault(fault))
                } else {
                    Err(ServiceError::GraphicsFailed(message))
                }
            }
        }
    }
}

impl dxgi::Windows for super::user::User {
    fn has_window(&self, handle: u64) -> bool {
        self.windows.contains_key(&handle)
    }
    fn window(&self, handle: u64) -> Option<dxgi::Window> {
        self.windows
            .get(&handle)
            .filter(|w| w.ready && w.parent == 0)
            .map(|w| dxgi::Window {
                width: (w.client[2] - w.client[0]).max(0) as u32,
                height: (w.client[3] - w.client[1]).max(0) as u32,
                visible: w.style & 0x10000000 != 0,
            })
    }
    fn dimensions(&self) -> [u32; 2] {
        [
            super::display::profile::WIDTH,
            super::display::profile::HEIGHT,
        ]
    }
    fn monitor(&self) -> u64 {
        super::display::monitor(crate::machine::Mode::X64)
    }
    fn device_name(&self) -> &str {
        super::display::profile::DEVICE
    }
    fn refresh_hz(&self) -> u32 {
        super::display::profile::REFRESH_HZ
    }
}
