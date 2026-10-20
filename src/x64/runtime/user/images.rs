//! Image resource coordination; image membership/payloads have a private owner.
use super::super::resources;
use super::{Memory, User};
use crate::win32::x64::modules::Modules;
use crate::win32::{catalog::ImageApi, user::images};

impl User {
    pub(in crate::x64::runtime) fn image_call(
        &mut self,
        mem: &mut Memory,
        id: ImageApi,
        args: &[u64],
        modules: &Modules,
        last_error: u64,
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
                    mem.write_int(last_error, 4, 50)?;
                    return Ok(0);
                }
                if !matches!(kind, 1 | 2) {
                    mem.write_int(last_error, 4, 50)?;
                    return Ok(0);
                }
                let key = super::super::resources::resource_key(mem, args[1], wide)?;
                if shared {
                    if let Some(handle) = self.images.shared(args[0], &key, kind) {
                        return Ok(handle);
                    }
                }
                let payload = if args[0] == 0 {
                    let images::Key::Id(id) = key else {
                        mem.write_int(last_error, 4, 1814)?;
                        return Ok(0);
                    };
                    if !images::stock(kind, id) {
                        mem.write_int(last_error, 4, 1814)?;
                        return Ok(0);
                    }
                    // Logical stock icon/cursor identity. Native drawing of these
                    // stock objects is separate from the window/frame presenter.
                    Vec::new()
                } else {
                    if modules.image(args[0]).is_none() {
                        mem.write_int(last_error, 4, 6)?;
                        return Ok(0);
                    }
                    let group = resources::resource(
                        mem,
                        modules,
                        args[0],
                        if kind == 1 { 14 } else { 12 },
                        &key,
                    )?;
                    let Some(group) = group else {
                        mem.write_int(last_error, 4, 1814)?;
                        return Ok(0);
                    };
                    if kind != 1 {
                        mem.write_int(last_error, 4, 50)?;
                        return Ok(0);
                    }
                    let (id, length) = images::icon_group(&group)?;
                    let Some(payload) =
                        resources::resource(mem, modules, args[0], 3, &images::Key::Id(id as u32))?
                    else {
                        mem.write_int(last_error, 4, 1814)?;
                        return Ok(0);
                    };
                    if !images::icon_payload(&payload, length)? {
                        mem.write_int(last_error, 4, 50)?;
                        return Ok(0);
                    }
                    // Retain the actual resource; no substitute icon makes a
                    // missing guest resource look successfully loaded.
                    payload
                };
                match self.images.insert(args[0], key, kind, shared, payload) {
                    Ok(handle) => Ok(handle),
                    Err(error) => {
                        mem.write_int(last_error, 4, error as u64)?;
                        Ok(0)
                    }
                }
            }
            ImageApi::DestroyIcon | ImageApi::DestroyCursor => {
                let kind = if id == ImageApi::DestroyIcon { 1 } else { 2 };
                match self.images.destroy(args[0], kind) {
                    Ok(()) => Ok(1),
                    Err(error) => {
                        mem.write_int(last_error, 4, error as u64)?;
                        Ok(0)
                    }
                }
            }
        }
    }
}
