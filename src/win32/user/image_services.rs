//! Shared image loading policy and checked PE resource ownership; no renderer.
use super::images;
use crate::win32::catalog::ImageApi;

pub(crate) trait Backend {
    type Error;
    fn key(&self, pointer: u64, wide: bool) -> Result<images::Key, Self::Error>;
    fn valid_module(&self, base: u64) -> bool;
    fn resource(
        &self,
        base: u64,
        kind: u32,
        key: &images::Key,
    ) -> Result<Option<Vec<u8>>, Self::Error>;
    fn invalid(&self, message: String) -> Self::Error;
    fn error(&mut self, code: u32) -> Result<(), Self::Error>;
}
pub(crate) fn call<B: Backend>(
    owner: &mut images::Images,
    backend: &mut B,
    id: ImageApi,
    args: &[u64],
) -> Result<u64, B::Error> {
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
                backend.error(50)?;
                return Ok(0);
            }
            if !matches!(kind, 1 | 2) {
                backend.error(50)?;
                return Ok(0);
            }
            let key = backend.key(args[1], wide)?;
            if args[0] != 0 && !backend.valid_module(args[0]) {
                backend.error(6)?;
                return Ok(0);
            }
            if shared {
                if let Some(handle) = owner.shared(args[0], &key, kind) {
                    return Ok(handle);
                }
            }
            let payload = if args[0] == 0 {
                let images::Key::Id(id) = key else {
                    backend.error(1814)?;
                    return Ok(0);
                };
                if !images::stock(kind, id) {
                    backend.error(1814)?;
                    return Ok(0);
                }
                // Logical stock icon/cursor identity. Native drawing of these
                // stock objects is separate from the window/frame presenter.
                Vec::new()
            } else {
                let group = backend.resource(args[0], if kind == 1 { 14 } else { 12 }, &key)?;
                let Some(group) = group else {
                    backend.error(1814)?;
                    return Ok(0);
                };
                if kind != 1 {
                    backend.error(50)?;
                    return Ok(0);
                }
                let (id, length) =
                    images::icon_group(&group).map_err(|error| backend.invalid(error))?;
                let Some(payload) = backend.resource(args[0], 3, &images::Key::Id(id as u32))?
                else {
                    backend.error(1814)?;
                    return Ok(0);
                };
                if !images::icon_payload(&payload, length)
                    .map_err(|error| backend.invalid(error))?
                {
                    backend.error(50)?;
                    return Ok(0);
                }
                // Retain the actual resource; no substitute icon makes a
                // missing guest resource look successfully loaded.
                payload
            };
            match owner.insert(args[0], key, kind, shared, payload) {
                Ok(handle) => Ok(handle),
                Err(error) => {
                    backend.error(error)?;
                    Ok(0)
                }
            }
        }
        ImageApi::DestroyIcon | ImageApi::DestroyCursor => {
            let kind = if id == ImageApi::DestroyIcon { 1 } else { 2 };
            match owner.destroy(args[0], kind) {
                Ok(()) => Ok(1),
                Err(error) => {
                    backend.error(error)?;
                    Ok(0)
                }
            }
        }
    }
}
