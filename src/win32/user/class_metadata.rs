/*
 * Copyright 1993, 1994, 1996 Alexandre Julliard
 * LGPL-2.1-or-later; LGPL 2.1 selected for wire's combined work.
 * Text copying retains the Wine 10.0 USER derivation.
 */
//! PE32 class records and field/text rules, independent of guest memory.
use crate::win32::catalog::ClassApi;

pub(crate) const BASIC_CLASSES: &[&str] = &[
    "Button",
    "Edit",
    "Static",
    "ListBox",
    "ComboBox",
    "ScrollBar",
];
pub(crate) const COMMON_CLASSES: &[&str] = &[
    "Button",
    "Edit",
    "Static",
    "ListBox",
    "ComboBox",
    "ScrollBar",
    "SysListView32",
    "SysTreeView32",
    "SysTabControl32",
    "msctls_progress32",
];

#[derive(Clone)]
pub struct Record {
    pub name: String,
    pub fields: [u32; 11],
    pub(crate) wide: bool,
    pub(crate) name_ptrs: [u32; 2],
    pub(crate) menu_ptrs: [u32; 2],
}
impl Record {
    pub(crate) fn query(&self, wide: bool) -> [u32; 11] {
        let mut fields = self.fields;
        fields[8] = self.menu_ptrs[usize::from(wide)];
        fields[9] = self.name_ptrs[usize::from(wide)];
        fields
    }
    pub(crate) fn field(&mut self, id: ClassApi, index: i32, value: u32) -> Result<u32, u32> {
        let index = match index {
            -26 => 0,
            -24 => 1,
            -20 => 2,
            -18 => 3,
            -16 => 4,
            -14 => 5,
            -12 => 6,
            -10 => 7,
            -8 => 8,
            -34 => 10,
            _ => return Err(1413),
        };
        let old = self.fields[index];
        if id.set() {
            if matches!(index, 2 | 3) && value > 4096 {
                return Err(87);
            }
            if index == 8 {
                return Err(120);
            }
            self.fields[index] = value;
        }
        Ok(old)
    }
}
pub(crate) fn strings(text: &str) -> Result<(Vec<u8>, Vec<u16>), u32> {
    Ok((
        crate::codepage::encode(text, 932)?.0,
        text.encode_utf16().collect(),
    ))
}
pub(crate) fn copy_text(text: &[u16], capacity: u32, wide: bool) -> Result<(Vec<u8>, u32), u32> {
    let mut bytes = if wide {
        text.iter()
            .flat_map(|c| c.to_le_bytes())
            .collect::<Vec<_>>()
    } else {
        crate::codepage::encode(&String::from_utf16_lossy(text), 932)?.0
    };
    let unit = if wide { 2 } else { 1 };
    let count = (bytes.len() / unit).min(capacity as usize - 1);
    bytes.truncate(count * unit);
    bytes.resize((count + 1) * unit, 0);
    Ok((bytes, count as u32))
}
