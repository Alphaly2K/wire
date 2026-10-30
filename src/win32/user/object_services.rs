//! Shared registered-name and window-property rules over checked adapters.
use super::objects::{NameError, Properties, PropertyKey, RegisteredNames};
use crate::win32::catalog::UserObjectApi as U;

pub(crate) trait Backend {
    type Error;
    fn name(&self, pointer: u64, wide: bool) -> Result<String, Self::Error>;
    fn names(&mut self) -> &mut RegisteredNames;
    fn valid_window(&self, window: u64) -> bool;
    fn properties(&mut self, window: u64) -> &mut Properties;
    fn error(&mut self, code: u32) -> Result<(), Self::Error>;
}
pub(crate) fn call<B: Backend>(
    backend: &mut B,
    id: U,
    a: &[u64; 16],
) -> Result<(u64, &'static str), B::Error> {
    if id.clipboard_data() {
        if id == U::ClipboardAvailable {
            return Ok((0, "no clipboard formats available"));
        }
        let code = if id == U::OpenClipboard {
            if a[0] != 0 && !backend.valid_window(a[0]) {
                1400
            } else {
                50
            }
        } else {
            1418
        };
        backend.error(code)?;
        return Ok((0, "clipboard data backend unavailable"));
    }
    if id.registered() {
        let text = backend.name(a[0], id.wide())?;
        return Ok(match backend.names().register(&text) {
            Ok(atom) => (atom.into(), "registered process format/message"),
            Err(NameError::Invalid) => {
                backend.error(87)?;
                (0, "invalid registered name")
            }
            Err(NameError::Full) => {
                backend.error(8)?;
                (0, "format table exhausted")
            }
        });
    }
    assert!(!id.enumeration(), "property enumeration remains ABI-only");
    let key = if a[1] <= 0xffff {
        PropertyKey::Atom(a[1] as u32)
    } else {
        PropertyKey::Name(backend.name(a[1], id.wide())?.to_lowercase())
    };
    if !backend.valid_window(a[0]) {
        backend.error(1400)?;
        return Ok((0, "invalid property HWND"));
    }
    if a[1] == 0 {
        backend.error(87)?;
        return Ok((0, "invalid property key"));
    }
    let properties = backend.properties(a[0]);
    let value = match id {
        U::SetA | U::SetW => {
            properties.set(key, a[2]);
            1
        }
        U::GetA | U::GetW => properties.get(&key),
        U::RemoveA | U::RemoveW => properties.remove(&key),
        _ => unreachable!("only property calls remain"),
    };
    Ok((value, "window-owned property"))
}
