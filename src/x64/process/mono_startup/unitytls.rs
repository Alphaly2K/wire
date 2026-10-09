//! Owned native UnityTLS interface. Only the two error-state leaf callbacks
//! are implemented; all other typed compiled entries fail-stop on activation.
use super::{callbacks, Memory};
use std::ffi::c_void;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ErrorState {
    magic: u32,
    code: u32,
    reserved: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Handle {
    value: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct ProtocolRange {
    min: u32,
    max: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct IoCallbacks {
    read: *const c_void,
    write: *const c_void,
    data: *mut c_void,
}
#[repr(C)]
struct NativeTable {
    invalid_handle: u64,
    protocols: ProtocolRange,
    entries: [*const c_void; 33],
}
pub(super) struct Table {
    native: Box<NativeTable>,
    targets: [u64; 33],
}
const NAMES: [&str; 33] = [
    "unitytls_errorstate_create",
    "unitytls_errorstate_raise_error",
    "unitytls_key_get_ref",
    "unitytls_key_parse_der",
    "unitytls_key_parse_pem",
    "unitytls_key_free",
    "unitytls_x509_export_der",
    "unitytls_x509list_get_ref",
    "unitytls_x509list_get_x509",
    "unitytls_x509list_create",
    "unitytls_x509list_append",
    "unitytls_x509list_append_der",
    "unitytls_x509list_append_pem",
    "unitytls_x509list_free",
    "unitytls_x509verify_default_ca",
    "unitytls_x509verify_explicit_ca",
    "unitytls_tlsctx_create_server",
    "unitytls_tlsctx_create_client",
    "unitytls_tlsctx_server_require_client_authentication",
    "unitytls_tlsctx_set_certificate_callback",
    "unitytls_tlsctx_set_trace_callback",
    "unitytls_tlsctx_set_x509verify_callback",
    "unitytls_tlsctx_set_supported_ciphersuites",
    "unitytls_tlsctx_get_ciphersuite",
    "unitytls_tlsctx_get_protocol",
    "unitytls_tlsctx_process_handshake",
    "unitytls_tlsctx_read",
    "unitytls_tlsctx_write",
    "unitytls_tlsctx_notify_close",
    "unitytls_tlsctx_free",
    "unitytls_random_generate_bytes",
    "unitytls_x509verify_result_to_string",
    "unitytls_tlsctx_set_trace_level",
];
impl Table {
    pub(super) fn from_guest(memory: &Memory, address: u64) -> Result<Self, String> {
        if address != 0x1821fa690
            || memory.read_int(address, 8)? != 1
            || memory.read_int(address + 8, 8)? != 0x200000002
        {
            return Err("UnityTLS exact-sample header/address mismatch".into());
        }
        let mut targets = [0; 33];
        for (slot, target) in targets.iter_mut().enumerate() {
            *target = memory.read_int(address + 16 + slot as u64 * 8, 8)?;
            memory.check(*target, 1, super::super::super::memory::EXECUTE)?;
        }
        if targets[..2] != [0x180ca7ce0, 0x180ca7cc0] {
            return Err("UnityTLS leaf targets differ from exact sample".into());
        }
        Ok(Self {
            native: Box::new(NativeTable {
                invalid_handle: 1,
                protocols: ProtocolRange { min: 2, max: 2 },
                entries: [
                    unitytls_errorstate_create as *const () as *const c_void,
                    unitytls_errorstate_raise_error as *const () as *const c_void,
                    unitytls_key_get_ref as *const () as *const c_void,
                    unitytls_key_parse_der as *const () as *const c_void,
                    unitytls_key_parse_pem as *const () as *const c_void,
                    unitytls_key_free as *const () as *const c_void,
                    unitytls_x509_export_der as *const () as *const c_void,
                    unitytls_x509list_get_ref as *const () as *const c_void,
                    unitytls_x509list_get_x509 as *const () as *const c_void,
                    unitytls_x509list_create as *const () as *const c_void,
                    unitytls_x509list_append as *const () as *const c_void,
                    unitytls_x509list_append_der as *const () as *const c_void,
                    unitytls_x509list_append_pem as *const () as *const c_void,
                    unitytls_x509list_free as *const () as *const c_void,
                    unitytls_x509verify_default_ca as *const () as *const c_void,
                    unitytls_x509verify_explicit_ca as *const () as *const c_void,
                    unitytls_tlsctx_create_server as *const () as *const c_void,
                    unitytls_tlsctx_create_client as *const () as *const c_void,
                    unitytls_tlsctx_server_require_client_authentication as *const ()
                        as *const c_void,
                    unitytls_tlsctx_set_certificate_callback as *const () as *const c_void,
                    unitytls_tlsctx_set_trace_callback as *const () as *const c_void,
                    unitytls_tlsctx_set_x509verify_callback as *const () as *const c_void,
                    unitytls_tlsctx_set_supported_ciphersuites as *const () as *const c_void,
                    unitytls_tlsctx_get_ciphersuite as *const () as *const c_void,
                    unitytls_tlsctx_get_protocol as *const () as *const c_void,
                    unitytls_tlsctx_process_handshake as *const () as *const c_void,
                    unitytls_tlsctx_read as *const () as *const c_void,
                    unitytls_tlsctx_write as *const () as *const c_void,
                    unitytls_tlsctx_notify_close as *const () as *const c_void,
                    unitytls_tlsctx_free as *const () as *const c_void,
                    unitytls_random_generate_bytes as *const () as *const c_void,
                    unitytls_x509verify_result_to_string as *const () as *const c_void,
                    unitytls_tlsctx_set_trace_level as *const () as *const c_void,
                ],
            }),
            targets,
        })
    }
    pub(super) fn pointer(&self) -> *const c_void {
        (&*self.native as *const NativeTable).cast()
    }
    pub(super) fn leaf_targets(&self) -> [u64; 2] {
        [self.targets[0], self.targets[1]]
    }
    pub(super) fn tsv(&self) -> String {
        let mut result = String::from("name\tguest_target\ttyped_leaf_admitted\n");
        for (slot, name) in NAMES.iter().enumerate() {
            result.push_str(&format!(
                "{name}\t{:#x}\t{}\n",
                self.targets[slot],
                u8::from(slot < 2)
            ));
        }
        result
    }
    pub(super) unsafe fn validate_leaf(&self) -> Result<(), String> {
        let expected = callbacks::payload(5, &[0; 16], &[], true)?;
        let create = unsafe {
            std::mem::transmute::<*const c_void, unsafe extern "C" fn() -> ErrorState>(
                self.native.entries[0],
            )
        };
        let raise = unsafe {
            std::mem::transmute::<*const c_void, unsafe extern "C" fn(*mut ErrorState, u32)>(
                self.native.entries[1],
            )
        };
        let state = unsafe { create() };
        if state.bytes() != expected {
            return Err("UnityTLS hidden-sret create differs from original Win64".into());
        }
        if state.magic != 0x06cbfac7 || state.code != 0 || state.reserved != 0 {
            return Err("UnityTLS initial error-state layout differs".into());
        }
        let mut state = state;
        for code in [8u32, 0x100004, 0] {
            let expected = callbacks::payload(6, &state.bytes(), &[code as u64], false)?;
            unsafe {
                raise(&mut state, code);
            }
            if state.bytes() != expected {
                return Err("UnityTLS error-state ref differs from original Win64".into());
            }
        }
        if state.code != 8 {
            return Err("UnityTLS sticky first-error behavior changed".into());
        }
        Ok(())
    }
}
impl ErrorState {
    fn bytes(self) -> [u8; 16] {
        let mut bytes = [0; 16];
        bytes[..4].copy_from_slice(&self.magic.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.code.to_le_bytes());
        bytes[8..].copy_from_slice(&self.reserved.to_le_bytes());
        bytes
    }
    fn from_bytes(bytes: [u8; 16]) -> Self {
        Self {
            magic: u32::from_le_bytes(bytes[..4].try_into().unwrap()),
            code: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            reserved: u64::from_le_bytes(bytes[8..].try_into().unwrap()),
        }
    }
}
fn reject(name: &str) -> ! {
    eprintln!("NATIVE_MONO_UNITYTLS_REJECT: {name}");
    std::process::abort()
}
unsafe extern "C" fn unitytls_errorstate_create() -> ErrorState {
    let bytes = callbacks::payload(5, &[0; 16], &[], true).unwrap_or_else(|error| reject(&error));
    ErrorState::from_bytes(bytes)
}
unsafe extern "C" fn unitytls_errorstate_raise_error(state: *mut ErrorState, code: u32) {
    if state.is_null() {
        reject("NULL native error-state reference");
    }
    let initial = unsafe { state.read() };
    let bytes = callbacks::payload(6, &initial.bytes(), &[code as u64], false)
        .unwrap_or_else(|error| reject(&error));
    unsafe {
        state.write(ErrorState::from_bytes(bytes));
    }
}
macro_rules! rejected {
    ($name:ident($($argument:ident : $ty:ty),*) -> $result:ty) => {
        unsafe extern "C" fn $name($($argument:$ty),*) -> $result { reject(stringify!($name)) }
    };
}
rejected!(unitytls_key_get_ref(_a0: *mut c_void, _a1: *mut ErrorState) -> Handle);
rejected!(unitytls_key_parse_der(_a0: *mut c_void, _a1: usize, _a2: *mut c_void, _a3: usize, _a4: *mut ErrorState) -> *mut c_void);
rejected!(unitytls_key_parse_pem(_a0: *mut c_void, _a1: usize, _a2: *mut c_void, _a3: usize, _a4: *mut ErrorState) -> *mut c_void);
rejected!(unitytls_key_free(_a0: *mut c_void) -> ());
rejected!(unitytls_x509_export_der(_a0: Handle, _a1: *mut c_void, _a2: usize, _a3: *mut ErrorState) -> usize);
rejected!(unitytls_x509list_get_ref(_a0: *mut c_void, _a1: *mut ErrorState) -> Handle);
rejected!(unitytls_x509list_get_x509(_a0: Handle, _a1: usize, _a2: *mut ErrorState) -> Handle);
rejected!(unitytls_x509list_create(_a0: *mut ErrorState) -> *mut c_void);
rejected!(unitytls_x509list_append(_a0: *mut c_void, _a1: Handle, _a2: *mut ErrorState) -> ());
rejected!(unitytls_x509list_append_der(_a0: *mut c_void, _a1: *mut c_void, _a2: usize, _a3: *mut ErrorState) -> ());
rejected!(unitytls_x509list_append_pem(_a0: *mut c_void, _a1: *mut c_void, _a2: usize, _a3: *mut ErrorState) -> ());
rejected!(unitytls_x509list_free(_a0: *mut c_void) -> ());
rejected!(unitytls_x509verify_default_ca(_a0: Handle, _a1: *mut c_void, _a2: usize, _a3: *const c_void, _a4: *mut c_void, _a5: *mut ErrorState) -> u32);
rejected!(unitytls_x509verify_explicit_ca(_a0: Handle, _a1: Handle, _a2: *mut c_void, _a3: usize, _a4: *const c_void, _a5: *mut c_void, _a6: *mut ErrorState) -> u32);
rejected!(unitytls_tlsctx_create_server(_a0: ProtocolRange, _a1: IoCallbacks, _a2: u64, _a3: u64, _a4: *mut ErrorState) -> *mut c_void);
rejected!(unitytls_tlsctx_create_client(_a0: ProtocolRange, _a1: IoCallbacks, _a2: *mut c_void, _a3: usize, _a4: *mut ErrorState) -> *mut c_void);
rejected!(unitytls_tlsctx_server_require_client_authentication(_a0: *mut c_void, _a1: Handle, _a2: *mut ErrorState) -> ());
rejected!(unitytls_tlsctx_set_certificate_callback(_a0: *mut c_void, _a1: *const c_void, _a2: *mut c_void, _a3: *mut ErrorState) -> ());
rejected!(unitytls_tlsctx_set_trace_callback(_a0: *mut c_void, _a1: *const c_void, _a2: *mut c_void, _a3: *mut ErrorState) -> ());
rejected!(unitytls_tlsctx_set_x509verify_callback(_a0: *mut c_void, _a1: *const c_void, _a2: *mut c_void, _a3: *mut ErrorState) -> ());
rejected!(unitytls_tlsctx_set_supported_ciphersuites(_a0: *mut c_void, _a1: *mut c_void, _a2: usize, _a3: *mut ErrorState) -> ());
rejected!(unitytls_tlsctx_get_ciphersuite(_a0: *mut c_void, _a1: *mut ErrorState) -> u32);
rejected!(unitytls_tlsctx_get_protocol(_a0: *mut c_void, _a1: *mut ErrorState) -> u32);
rejected!(unitytls_tlsctx_process_handshake(_a0: *mut c_void, _a1: *mut ErrorState) -> u32);
rejected!(unitytls_tlsctx_read(_a0: *mut c_void, _a1: *mut c_void, _a2: usize, _a3: *mut ErrorState) -> usize);
rejected!(unitytls_tlsctx_write(_a0: *mut c_void, _a1: *mut c_void, _a2: usize, _a3: *mut ErrorState) -> usize);
rejected!(unitytls_tlsctx_notify_close(_a0: *mut c_void, _a1: *mut ErrorState) -> ());
rejected!(unitytls_tlsctx_free(_a0: *mut c_void) -> ());
rejected!(unitytls_random_generate_bytes(_a0: *mut c_void, _a1: usize, _a2: *mut ErrorState) -> ());
rejected!(unitytls_x509verify_result_to_string(_a0: u32) -> *mut c_void);
rejected!(unitytls_tlsctx_set_trace_level(_a0: *mut c_void, _a1: u32) -> ());
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn table_and_error_state_follow_the_locked_native_classlib_layout() {
        assert_eq!(std::mem::size_of::<ErrorState>(), 16);
        assert_eq!(std::mem::offset_of!(ErrorState, code), 4);
        assert_eq!(std::mem::offset_of!(ErrorState, reserved), 8);
        assert_eq!(std::mem::size_of::<NativeTable>(), 280);
        assert_eq!(std::mem::offset_of!(NativeTable, entries), 16);
        let bytes = [0xa5; 16];
        assert_eq!(ErrorState::from_bytes(bytes).bytes(), bytes);
        let memory = Memory::default();
        assert!(Table::from_guest(&memory, 0).is_err());
    }
}
