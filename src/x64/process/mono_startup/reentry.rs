//! Same-owner metadata reentry for the locked subsystem map callback. Native
//! handles remain in the existing registries; managed invocation, image loading
//! and object operations are excluded from this nested route.
use super::{c_char, images, metadata, string, Cpu64, Memory, Object};
use std::ffi::CString;
#[cfg(test)]
use std::ptr;
macro_rules! sym {
    ($lib:expr, $name:expr, $ty:ty) => {{
        let name = CString::new($name).map_err(|e| e.to_string())?;
        let p = unsafe { libc::dlsym($lib, name.as_ptr()) };
        if p.is_null() {
            return Err(format!("missing nested metadata symbol: {name:?}"));
        }
        unsafe { std::mem::transmute::<Object, $ty>(p) }
    }};
}
pub(super) struct Metadata<'a> {
    pub(super) library: Object,
    pub(super) domain: Object,
    pub(super) metadata: &'a mut metadata::Registry,
    pub(super) images: &'a mut images::Registry,
}
impl Metadata<'_> {
    pub(super) fn dispatch(
        &mut self,
        function: &str,
        cpu: &mut Cpu64,
        memory: &mut Memory,
        stack: (u64, u64),
    ) -> Result<(), String> {
        if function == "mono_domain_get" {
            let get = sym!(
                self.library,
                "mono_domain_get",
                unsafe extern "C" fn() -> Object
            );
            if self.domain.is_null() || unsafe { get() } != self.domain {
                return Err("nested domain identity mismatch".into());
            }
            return cpu.return_integer(memory, super::ROOT_TOKEN);
        }
        if function == "mono_gc_wbarrier_set_field" {
            memory.read_int(cpu.regs[4], 8)?;
            let a = [cpu.regs[1], cpu.regs[2], cpu.regs[8]];
            let first = cpu.regs[4].checked_add(40).ok_or("nested stack overflow")?;
            let end = a[1]
                .checked_add(8)
                .ok_or("nested NULL root slot overflow")?;
            if cpu.regs[4] < stack.0
                || cpu.regs[4] >= stack.1
                || a[0] != 0
                || a[2] != 0
                || !a[1].is_multiple_of(8)
                || a[1] < first
                || a[1] < stack.0
                || end > stack.1
            {
                return Err(
                    "nested barrier permits only NULL in a live callback-stack slot".into(),
                );
            }
            memory.write_int(a[1], 8, 0)?;
            return cpu.return_integer(memory, 0);
        }
        if !metadata::handles(function) {
            return Err(format!(
                "nested Mono operation outside metadata route: {function}"
            ));
        }
        let a = [cpu.regs[1], cpu.regs[2], cpu.regs[8]];
        memory.read_int(cpu.regs[4], 8)?;
        cpu.regs[4]
            .checked_add(8)
            .ok_or("nested metadata return overflow")?;
        let mut iterator_output = None;
        let value = match function {
            "mono_class_get_userdata" | "mono_class_set_userdata" => {
                metadata::Value::Scalar(self.metadata.userdata(
                    memory,
                    a[0],
                    (function == "mono_class_set_userdata").then_some(a[1]),
                )?)
            }
            "mono_get_corlib" => {
                let get = sym!(self.library, function, unsafe extern "C" fn() -> Object);
                self.metadata.note_query();
                metadata::Value::Scalar(self.images.borrow_registered(unsafe { get() })?)
            }
            "mono_class_get_image" => {
                let class = self.metadata.native(a[0], metadata::Kind::Class)?;
                let get = sym!(
                    self.library,
                    function,
                    unsafe extern "C" fn(Object) -> Object
                );
                self.metadata.note_query();
                metadata::Value::Scalar(self.images.borrow_registered(unsafe { get(class) })?)
            }
            "mono_class_from_name" => {
                let image = self.images.native_image(a[0])?;
                self.metadata.class_from_name(
                    self.library,
                    image,
                    string(memory, a[1])?,
                    string(memory, a[2])?,
                )?
            }
            "mono_class_get_field_from_name" | "mono_class_get_method_from_name" => {
                let class = self.metadata.native(a[0], metadata::Kind::Class)?;
                let name = string(memory, a[1])?;
                self.metadata.note_query();
                if function == "mono_class_get_field_from_name" {
                    let get = sym!(
                        self.library,
                        function,
                        unsafe extern "C" fn(Object, *const c_char) -> Object
                    );
                    metadata::Value::Handle(
                        unsafe { get(class, name.as_ptr()) },
                        metadata::Kind::Field,
                    )
                } else {
                    let argc = a[2] as u32 as i32;
                    if !(-1..=16).contains(&argc) {
                        return Err("nested method lookup arity outside contract".into());
                    }
                    let get = sym!(
                        self.library,
                        function,
                        unsafe extern "C" fn(Object, *const c_char, i32) -> Object
                    );
                    metadata::Value::Handle(
                        unsafe { get(class, name.as_ptr(), argc) },
                        metadata::Kind::Method,
                    )
                }
            }
            "mono_class_get_methods"
            | "mono_class_get_fields"
            | "mono_class_get_nested_types"
            | "mono_class_get_interfaces"
            | "mono_signature_get_params" => {
                memory.check(
                    a[1],
                    8,
                    crate::x64::memory::READ | crate::x64::memory::WRITE,
                )?;
                let iterator = memory.read_int(a[1], 8)?;
                let (value, next) =
                    self.metadata
                        .iterate(self.library, function, a[0], iterator)?;
                iterator_output = Some((a[1], next));
                value
            }
            // Names/table data are not currently used by this exact callback.
            "mono_image_get_name" | "mono_image_get_table_rows" => {
                return Err(format!("nested image metadata not admitted: {function}"))
            }
            _ => self
                .metadata
                .query(self.library, function, a[0], a[1], a[2])?,
        };
        let result = self.metadata.wrap(memory, value)?;
        if let Some((address, iterator)) = iterator_output {
            memory.write_int(address, 8, iterator)?;
        }
        cpu.return_integer(memory, result)
    }
}
/// Preparation borrows the registries and checked guest memory. Execution owns
/// only stable native metadata and no-argument ABI data, so GC and guest reentry
/// can run without an outstanding Memory/registry borrow.
pub(super) struct PreparedInvoke {
    library: Object,
    method: Object,
    image: Object,
    step: usize,
    pub(super) exception_slot: u64,
}
impl Metadata<'_> {
    pub(super) fn prepare_invoke(
        &mut self,
        cpu: &Cpu64,
        memory: &Memory,
    ) -> Result<PreparedInvoke, String> {
        memory.read_int(cpu.regs[4], 8)?;
        cpu.regs[4]
            .checked_add(8)
            .ok_or("nested invoke return overflow")?;
        if cpu.regs[2] != 0 {
            return Err("nested subsystem invoke must be static".into());
        }
        let exception_slot = cpu.regs[9];
        if exception_slot != 0 {
            memory.check(exception_slot, 8, crate::x64::memory::WRITE)?;
        }
        let method = self.metadata.native(cpu.regs[1], metadata::Kind::Method)?;
        let image = self.images.unique_native_image(super::SUBSYSTEMS_MODULE)?;
        let step = if super::invoke::validate_classlib_init(self.library, method, image, 5).is_ok()
        {
            5
        } else if super::invoke::validate_classlib_init(self.library, method, image, 6).is_ok() {
            6
        } else {
            eprintln!(
                "NATIVE_MONO_NESTED_INVOKE_REJECTED: {}",
                self.metadata.describe_method(self.library, cpu.regs[1])?
            );
            for (label, _, listing) in super::invoke::owned_bodies(self.library, method)? {
                eprintln!("NATIVE_MONO_NESTED_REJECTED_IL {label}:\n{listing}");
            }
            return Err("nested invoke outside exact SubsystemDescriptorStore/SubsystemManager clear methods".into());
        };
        self.metadata.note_query();
        Ok(PreparedInvoke {
            library: self.library,
            method,
            image,
            step,
            exception_slot,
        })
    }
}
impl PreparedInvoke {
    pub(super) fn execute(&self) -> Result<(), String> {
        let invoke = sym!(
            self.library,
            "mono_runtime_invoke",
            unsafe extern "C" fn(Object, Object, *mut Object, *mut Object) -> Object
        );
        let mut exception = std::ptr::null_mut();
        let result = unsafe {
            invoke(
                self.method,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut exception,
            )
        };
        if !exception.is_null() || !result.is_null() {
            return Err("nested subsystem clear did not return void without exception".into());
        }
        super::invoke::verify_subsystems(self.library, self.image, self.step == 6)?;
        eprintln!(
            "NATIVE_MONO_NESTED_INVOKE_VERIFIED: {} static-void=true",
            if self.step == 5 {
                "SubsystemDescriptorStore.ClearManagedDescriptors"
            } else {
                "SubsystemManager.ClearSubsystems"
            }
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn null_root_clear_checks_active_stack_and_keeps_outputs_on_failure() {
        let mut memory = Memory::default();
        memory
            .map(
                0x1000,
                4096,
                crate::x64::memory::READ | crate::x64::memory::WRITE,
            )
            .unwrap();
        memory.write_int(0x1800, 8, 0x1234).unwrap();
        let mut registry = metadata::Registry::default();
        let mut images = images::Registry::default();
        let mut route = Metadata {
            library: ptr::null_mut(),
            domain: ptr::null_mut(),
            metadata: &mut registry,
            images: &mut images,
        };
        let mut cpu = Cpu64::new(0, 0x1800);
        cpu.regs[2] = 0x1840;
        memory.write_int(0x1840, 8, 0xaaaaaaaa).unwrap();
        route
            .dispatch(
                "mono_gc_wbarrier_set_field",
                &mut cpu,
                &mut memory,
                (0x1000, 0x2000),
            )
            .unwrap();
        assert_eq!(memory.read_int(0x1840, 8).unwrap(), 0);
        assert_eq!(cpu.rip, 0x1234);
        for (object, value, destination, stack) in [
            (1, 0, 0x1840, (0x1000, 0x2000)),
            (0, 1, 0x1840, (0x1000, 0x2000)),
            (0, 0, 0x1400, (0x1000, 0x2000)),
            (0, 0, 0x1840, (0x1900, 0x2000)),
            (0, 0, 0x1841, (0x1000, 0x2000)),
        ] {
            cpu.regs[4] = 0x1800;
            cpu.regs[1] = object;
            cpu.regs[8] = value;
            cpu.regs[2] = destination;
            memory.write_int(0x1840, 8, 0xaaaaaaaa).unwrap();
            memory.write_int(0x1400, 8, 0xbbbbbbbb).unwrap();
            assert!(route
                .dispatch("mono_gc_wbarrier_set_field", &mut cpu, &mut memory, stack)
                .is_err());
            assert_eq!(memory.read_int(0x1840, 8).unwrap(), 0xaaaaaaaa);
            assert_eq!(memory.read_int(0x1400, 8).unwrap(), 0xbbbbbbbb);
            assert_eq!(cpu.regs[4], 0x1800);
        }
    }
    #[test]
    fn nested_userdata_uses_the_existing_identity_and_rejects_managed_invokes() {
        let mut memory = Memory::default();
        memory
            .map(
                0x1000,
                4096,
                crate::x64::memory::READ | crate::x64::memory::WRITE,
            )
            .unwrap();
        memory.write_int(0x1800, 8, 0x1234).unwrap();
        let mut registry = metadata::Registry::default();
        let mut native = [0u64; 32];
        let token = registry
            .wrap(
                &mut memory,
                metadata::Value::Handle(native.as_mut_ptr().cast(), metadata::Kind::Class),
            )
            .unwrap();
        let mut images = images::Registry::default();
        let mut route = Metadata {
            library: ptr::null_mut(),
            domain: ptr::null_mut(),
            metadata: &mut registry,
            images: &mut images,
        };
        let mut cpu = Cpu64::new(0, 0x1800);
        cpu.regs[1] = token;
        cpu.regs[2] = 0x334455;
        route
            .dispatch(
                "mono_class_set_userdata",
                &mut cpu,
                &mut memory,
                (0x1000, 0x2000),
            )
            .unwrap();
        assert_eq!(memory.read_int(token + 232, 8).unwrap(), 0x334455);
        assert_eq!(native[29], 0);
        cpu.regs[4] = 0x1800;
        route
            .dispatch(
                "mono_class_get_userdata",
                &mut cpu,
                &mut memory,
                (0x1000, 0x2000),
            )
            .unwrap();
        assert_eq!(cpu.regs[0], 0x334455);
        cpu.regs[4] = 0x1800;
        assert!(route
            .dispatch(
                "mono_runtime_invoke",
                &mut cpu,
                &mut memory,
                (0x1000, 0x2000)
            )
            .is_err());
        assert_eq!(cpu.regs[4], 0x1800);
        cpu.regs[1] = token + 1;
        assert!(route
            .dispatch(
                "mono_class_set_userdata",
                &mut cpu,
                &mut memory,
                (0x1000, 0x2000)
            )
            .is_err());
        assert_eq!(memory.read_int(token + 232, 8).unwrap(), 0x334455);
    }
}
