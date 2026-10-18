//! Image resource coordination; image membership/payloads have a private owner.
use super::super::{Memory, Runtime64};
use crate::win32::{catalog::ImageApi, user::images};

impl Runtime64 {
    pub(in crate::x64::runtime) fn image_call(
        &mut self,
        mem: &mut Memory,
        id: ImageApi,
        args: &[u64],
    ) -> Result<u64, String> {
        let wide = id.wide();
        match id {
            ImageApi::LoadIconA
            | ImageApi::LoadIconW
            | ImageApi::LoadCursorA
            | ImageApi::LoadCursorW
            | ImageApi::LoadImageA
            | ImageApi::LoadImageW => {
                let load_image = id.load_image();
                let kind = if load_image {
                    args[2] as u32
                } else {
                    if matches!(id, ImageApi::LoadCursorA | ImageApi::LoadCursorW) {
                        2
                    } else {
                        1
                    }
                };
                let shared = !load_image || args[5] & 0x8000 != 0;
                if load_image && (args[5] & !0x8040 != 0 || args[3] != 0 || args[4] != 0) {
                    self.set_error(mem, 50)?;
                    return Ok(0);
                }
                if !matches!(kind, 1 | 2) {
                    self.set_error(mem, 50)?;
                    return Ok(0);
                }
                let key = Self::resource_key(mem, args[1], wide)?;
                if shared {
                    if let Some(handle) = self.user.images.shared(args[0], &key, kind) {
                        return Ok(handle);
                    }
                }
                let payload = if args[0] == 0 {
                    let images::Key::Id(id) = key else {
                        self.set_error(mem, 1814)?;
                        return Ok(0);
                    };
                    if !images::stock(kind, id) {
                        self.set_error(mem, 1814)?;
                        return Ok(0);
                    }
                    // Logical stock icon/cursor identity. Native drawing of these
                    // stock objects is separate from the window/frame presenter.
                    Vec::new()
                } else {
                    if self.modules.image(args[0]).is_none() {
                        self.set_error(mem, 6)?;
                        return Ok(0);
                    }
                    let group =
                        self.resource(mem, args[0], if kind == 1 { 14 } else { 12 }, &key)?;
                    let Some(group) = group else {
                        self.set_error(mem, 1814)?;
                        return Ok(0);
                    };
                    if kind != 1 {
                        self.set_error(mem, 50)?;
                        return Ok(0);
                    }
                    let (id, length) = images::icon_group(&group)?;
                    let Some(payload) =
                        self.resource(mem, args[0], 3, &images::Key::Id(id as u32))?
                    else {
                        self.set_error(mem, 1814)?;
                        return Ok(0);
                    };
                    if !images::icon_payload(&payload, length)? {
                        self.set_error(mem, 50)?;
                        return Ok(0);
                    }
                    // Retain the actual resource; no substitute icon makes a
                    // missing guest resource look successfully loaded.
                    payload
                };
                match self.user.images.insert(args[0], key, kind, shared, payload) {
                    Ok(handle) => Ok(handle),
                    Err(error) => {
                        self.set_error(mem, error)?;
                        Ok(0)
                    }
                }
            }
            ImageApi::DestroyIcon | ImageApi::DestroyCursor => {
                let kind = if id == ImageApi::DestroyIcon { 1 } else { 2 };
                match self.user.images.destroy(args[0], kind) {
                    Ok(()) => Ok(1),
                    Err(error) => {
                        self.set_error(mem, error)?;
                        Ok(0)
                    }
                }
            }
        }
    }
}
