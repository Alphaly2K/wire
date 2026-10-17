use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyboardApi {
    State,
    AsyncState,
    StateArray,
    Layout,
    LayoutList,
    LayoutNameA,
    LayoutNameW,
    Type,
    MapA,
    MapW,
    MapExA,
    MapExW,
    KeyNameA,
    KeyNameW,
    Unicode,
    UnicodeEx,
}

const fn declaration(
    id: KeyboardApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    let state = matches!(
        id,
        KeyboardApi::State | KeyboardApi::AsyncState | KeyboardApi::StateArray
    );
    Contract::new(
        ApiId::Keyboard(id),
        name,
        parameters,
        returns,
        "existing asynchronous/queue keyboard owner and per-thread state; PE32 layout helpers remain ABI-only; Win64 explicit US layout, caller-supplied Unicode state, no IME/dead-key/composition state, full HKL pointers and original validation/output/saved-frame order; key-state kernel providers retain static-import-only policy",
        [
            ArchitecturePlan {
                dlls: &["user32.dll"],
                execution: if state { ExecutionPolicy::Typed } else { ExecutionPolicy::LegacyUnimplemented },
            },
            ArchitecturePlan {
                dlls: if state { &["kernel32.dll", "kernelbase.dll", "user32.dll"] } else { &["user32.dll"] },
                execution: ExecutionPolicy::Typed,
            },
        ],
    )
    .with_lookup_dlls([&["user32.dll"], &["user32.dll"]])
}

use Parameter::{Int32 as I, Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, CharacterCount as C, Pointer as R, Uint32 as U};
pub static KEYBOARD_CONTRACTS: [Contract; 16] = [
    declaration(KeyboardApi::State, "GetKeyState", &[I], U),
    declaration(KeyboardApi::AsyncState, "GetAsyncKeyState", &[I], U),
    declaration(KeyboardApi::StateArray, "GetKeyboardState", &[P], B),
    declaration(KeyboardApi::Layout, "GetKeyboardLayout", &[D], R),
    declaration(KeyboardApi::LayoutList, "GetKeyboardLayoutList", &[I, P], U),
    declaration(KeyboardApi::LayoutNameA, "GetKeyboardLayoutNameA", &[P], B),
    declaration(KeyboardApi::LayoutNameW, "GetKeyboardLayoutNameW", &[P], B),
    declaration(KeyboardApi::Type, "GetKeyboardType", &[I], U),
    declaration(KeyboardApi::MapA, "MapVirtualKeyA", &[D, D], U),
    declaration(KeyboardApi::MapW, "MapVirtualKeyW", &[D, D], U),
    declaration(KeyboardApi::MapExA, "MapVirtualKeyExA", &[D, D, P], U),
    declaration(KeyboardApi::MapExW, "MapVirtualKeyExW", &[D, D, P], U),
    declaration(KeyboardApi::KeyNameA, "GetKeyNameTextA", &[I, P, I], C),
    declaration(KeyboardApi::KeyNameW, "GetKeyNameTextW", &[I, P, I], C),
    declaration(KeyboardApi::Unicode, "ToUnicode", &[D, D, P, P, I, D], C),
    declaration(
        KeyboardApi::UnicodeEx,
        "ToUnicodeEx",
        &[D, D, P, P, I, D, P],
        C,
    ),
];
