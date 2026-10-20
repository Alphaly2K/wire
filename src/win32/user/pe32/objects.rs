//! PE32 checked name/property adapter; state owners do not inspect guest memory.
use crate::{
    api::Args,
    mem::GuestMem,
    win32::{
        catalog::UserObjectApi,
        user::objects::{NameError, PropertyKey},
    },
};

impl super::User32 {
    pub(crate) fn object_call(
        &mut self,
        id: UserObjectApi,
        a: &Args,
        mem: &GuestMem,
        last_error: &mut u32,
    ) -> Result<(u32, &'static str), String> {
        if id.registered() {
            let text = crate::file_api::read_name(mem, a[0], id.wide())
                .map_err(|e| format!("registered format: {e}"))?;
            return Ok(match self.registered.register(&text) {
                Ok(atom) => (atom, "registered process format/message"),
                Err(NameError::Invalid) => {
                    *last_error = 87;
                    (0, "invalid registered name")
                }
                Err(NameError::Full) => {
                    *last_error = 8;
                    (0, "format table exhausted")
                }
            });
        }
        assert!(!id.enumeration(), "property enumeration remains ABI-only");
        let key = if a[1] <= 0xffff {
            PropertyKey::Atom(a[1])
        } else {
            PropertyKey::Name(
                crate::file_api::read_name(mem, a[1], id.wide())
                    .map_err(|e| format!("window property: {e}"))?
                    .to_lowercase(),
            )
        };
        let Some(window) = self.windows.get_mut(&a[0]) else {
            *last_error = 1400;
            return Ok((0, "invalid property HWND"));
        };
        if a[1] == 0 {
            *last_error = 87;
            return Ok((0, "invalid property key"));
        }
        let value = match id {
            UserObjectApi::SetA | UserObjectApi::SetW => {
                window.props.set(key, a[2]);
                1
            }
            UserObjectApi::RemoveA | UserObjectApi::RemoveW => window.props.remove(&key),
            UserObjectApi::GetA | UserObjectApi::GetW => window.props.get(&key),
            _ => unreachable!("only property calls remain"),
        };
        Ok((value, "window-owned property"))
    }
}
