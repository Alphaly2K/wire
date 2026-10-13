//! Render target identifiers with no borrowed buffer. Numeric IDs retain their
//! original engine meaning; a nonzero buffer needs its own owner/lifetime route.
use super::Object;
use std::{ffi::CStr, ptr};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Layout {
    class: usize,
    pub(super) width: usize,
    pub(super) alignment: usize,
    pointer: usize,
    cells: [(usize, usize); 7],
}
impl Layout {
    pub(super) fn describe(
        library: Object,
        core: Object,
        class: Object,
    ) -> Result<Option<Self>, String> {
        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing render target metadata API: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        let find = sym!(
            "mono_class_from_name",
            unsafe extern "C" fn(Object, *const libc::c_char, *const libc::c_char) -> Object
        );
        let expected = unsafe {
            find(
                core,
                c"UnityEngine.Rendering".as_ptr(),
                c"RenderTargetIdentifier".as_ptr(),
            )
        };
        if class.is_null() || class != expected {
            return Ok(None);
        }
        super::closed_types::Api::load(library)?.concrete(class)?;
        let fields = sym!(
            "mono_class_get_fields",
            unsafe extern "C" fn(Object, *mut Object) -> Object
        );
        let name = sym!(
            "mono_field_get_name",
            unsafe extern "C" fn(Object) -> *const libc::c_char
        );
        let flags = sym!("mono_field_get_flags", unsafe extern "C" fn(Object) -> u32);
        let parent = sym!(
            "mono_field_get_parent",
            unsafe extern "C" fn(Object) -> Object
        );
        let ty = sym!(
            "mono_field_get_type",
            unsafe extern "C" fn(Object) -> Object
        );
        let kind = sym!("mono_type_get_type", unsafe extern "C" fn(Object) -> u32);
        let byref = sym!("mono_type_is_byref", unsafe extern "C" fn(Object) -> u32);
        let type_class = sym!(
            "mono_class_from_mono_type",
            unsafe extern "C" fn(Object) -> Object
        );
        let enum_base = sym!(
            "mono_class_enum_basetype",
            unsafe extern "C" fn(Object) -> Object
        );
        let is_enum = sym!("mono_class_is_enum", unsafe extern "C" fn(Object) -> i32);
        let is_value = sym!(
            "mono_class_is_valuetype",
            unsafe extern "C" fn(Object) -> i32
        );
        let size = sym!(
            "mono_class_value_size",
            unsafe extern "C" fn(Object, *mut u32) -> i32
        );
        let offset = sym!("mono_field_get_offset", unsafe extern "C" fn(Object) -> i32);
        let intptr = sym!("mono_get_intptr_class", unsafe extern "C" fn() -> Object);
        let intptr = unsafe { intptr() };
        let builtin = unsafe {
            find(
                core,
                c"UnityEngine.Rendering".as_ptr(),
                c"BuiltinRenderTextureType".as_ptr(),
            )
        };
        let cube = unsafe { find(core, c"UnityEngine".as_ptr(), c"CubemapFace".as_ptr()) };
        if [intptr, builtin, cube].iter().any(|p| p.is_null()) || unsafe { is_value(class) } == 0 {
            return Err("render target actual value/field classes absent".into());
        }
        for enumeration in [builtin, cube] {
            let base = unsafe { enum_base(enumeration) };
            let mut alignment = 0;
            if unsafe { is_enum(enumeration) } == 0
                || base.is_null()
                || unsafe { kind(base) } != 8
                || unsafe { size(enumeration, &mut alignment) } != 4
                || alignment != 4
            {
                return Err("render target enum is not actual I4 storage".into());
            }
        }
        let mut alignment = 0;
        let width = unsafe { size(class, &mut alignment) };
        if !(32..=128).contains(&width) || alignment != 8 {
            return Err("render target value size/alignment outside bounded ABI".into());
        }
        let names = [
            c"m_Type",
            c"m_NameID",
            c"m_InstanceID",
            c"m_BufferPointer",
            c"m_MipLevel",
            c"m_CubeFace",
            c"m_DepthSlice",
        ];
        let mut cells = [(0, 0); 7];
        let mut seen = [false; 7];
        let mut covered = vec![false; width as usize];
        let mut iterator = ptr::null_mut();
        let mut count = 0;
        loop {
            let field = unsafe { fields(class, &mut iterator) };
            if field.is_null() {
                break;
            }
            count += 1;
            if count > 32 || unsafe { parent(field) } != class {
                return Err("render target field owner/count mismatch".into());
            }
            if unsafe { flags(field) } & (0x10 | 0x40) != 0 {
                continue;
            }
            let text = unsafe { name(field) };
            if text.is_null() {
                return Err("render target field name absent".into());
            }
            let index = names
                .iter()
                .position(|n| *n == unsafe { CStr::from_ptr(text) })
                .ok_or("render target has an unknown instance field")?;
            let ty = unsafe { ty(field) };
            let field_class = if ty.is_null() {
                ptr::null_mut()
            } else {
                unsafe { type_class(ty) }
            };
            let expected_class = match index {
                0 => Some(builtin),
                3 => Some(intptr),
                5 => Some(cube),
                _ => None,
            };
            let expected_kind = match index {
                0 | 5 => 0x11,
                3 => 0x18,
                _ => 8,
            };
            if seen[index]
                || ty.is_null()
                || unsafe { byref(ty) } != 0
                || unsafe { kind(ty) } != expected_kind
                || expected_class.is_some_and(|c| c != field_class)
            {
                return Err("render target exact field type mismatch".into());
            }
            let width = if index == 3 { 8 } else { 4 };
            let off = unsafe { offset(field) }
                .checked_sub(16)
                .filter(|off| *off >= 0)
                .ok_or("render target field points into object header")?
                as usize;
            if !off.is_multiple_of(width)
                || off + width > covered.len()
                || covered[off..off + width].iter().any(|b| *b)
            {
                return Err("render target field overlaps or leaves actual layout".into());
            }
            cells[index] = (off, width);
            covered[off..off + width].fill(true);
            seen[index] = true;
        }
        if seen.iter().any(|b| !b) {
            return Err("render target is missing a semantic field".into());
        }
        Ok(Some(Self {
            class: class as usize,
            width: width as usize,
            alignment: alignment as usize,
            pointer: cells[3].0,
            cells,
        }))
    }
    pub(super) fn canonicalize(self, bytes: &mut [u8]) -> Result<(), String> {
        if bytes.len() != self.width
            || bytes[self.pointer..self.pointer + 8]
                .iter()
                .any(|b| *b != 0)
        {
            return Err("render target buffer requires a retained typed owner/lifetime".into());
        }
        let mut covered = vec![false; self.width];
        for &(offset, width) in &self.cells {
            covered[offset..offset + width].fill(true);
        }
        for (byte, covered) in bytes.iter_mut().zip(covered) {
            if !covered {
                *byte = 0;
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn render_target_preserves_numeric_ids_zeroes_padding_and_rejects_buffer_pointers() {
        let layout = Layout {
            class: 7,
            width: 40,
            alignment: 8,
            pointer: 16,
            cells: [(0, 4), (4, 4), (8, 4), (16, 8), (24, 4), (28, 4), (32, 4)],
        };
        let mut bytes = vec![0xcc; 40];
        bytes[16..24].fill(0);
        layout.canonicalize(&mut bytes).unwrap();
        assert_eq!(&bytes[..12], &[0xcc; 12]);
        assert_eq!(&bytes[12..24], &[0; 12]);
        assert_eq!(&bytes[24..36], &[0xcc; 12]);
        assert_eq!(&bytes[36..], &[0; 4]);
        for value in [1u64, 0x6ffe00000000, 0x30100000, u64::MAX] {
            bytes[16..24].copy_from_slice(&value.to_le_bytes());
            let before = bytes.clone();
            assert!(layout.canonicalize(&mut bytes).is_err());
            assert_eq!(bytes, before);
        }
        assert!(layout.canonicalize(&mut bytes[..32]).is_err());
    }
}
