/*
 * Copyright 1993 Robert J. Amstadt
 * Copyright 1995, 2009 Alexandre Julliard
 * Copyright 1993 Martin Ayotte
 * Copyright 1994 Alexandre Julliard
 * Copyright 1997 Morten Welinder
 * Copyright 2005 Maxime Bellengé
 * Copyright 2006 Phil Krylov
 *
 * This library is free software; you can redistribute it and/or
 * modify it under the terms of the GNU Lesser General Public
 * License as published by the Free Software Foundation; either
 * version 2.1 of the License, or (at your option) any later version.
 *
 * This library is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU
 * Lesser General Public License for more details.
 *
 * You should have received a copy of the GNU Lesser General Public
 * License along with this library; if not, write to the Free Software
 * Foundation, Inc., 51 Franklin St, Fifth Floor, Boston, MA 02110-1301, USA
 */
//! Wine 10.0 menu lookup, insertion and MENUITEMINFO normalization/update flow,
//! adapted to process-owned objects and checked x86 guest pointers. No menu
//! drawing, modal tracking, Wine server or native pointer is imported.
use crate::{api::ApiState, mem::GuestMem};
use std::collections::{BTreeMap, BTreeSet};

const TYPE: u32 = 0x6b64;
const STATE: u32 = !(TYPE | 0x10 | 0x2000 | 0x400 | 0x8000);
const INVALID: u32 = 87;
const BAD_MENU: u32 = 1401;
const NOT_FOUND: u32 = 1456;

#[derive(Clone, Default)]
struct Item {
    kind: u32,
    state: u32,
    id: u32,
    submenu: u32,
    checked: u32,
    unchecked: u32,
    text: Option<Vec<u16>>,
    data: u32,
    bitmap: u32,
}
struct Menu {
    popup: bool,
    items: Vec<Item>,
}
pub(crate) struct Menus {
    objects: BTreeMap<u32, Menu>,
    next: u32,
}
impl Default for Menus {
    fn default() -> Self {
        Self {
            objects: BTreeMap::new(),
            next: 0x41010000,
        }
    }
}
impl Menus {
    fn create(&mut self, popup: bool) -> Result<u32, u32> {
        if self.objects.len() >= 4096 {
            return Err(8);
        }
        let handle = self.next;
        self.next = handle.checked_add(4).ok_or(8u32)?;
        self.objects.insert(
            handle,
            Menu {
                popup,
                items: Vec::new(),
            },
        );
        Ok(handle)
    }
    fn get(&self, h: u32) -> Result<&Menu, u32> {
        self.objects.get(&h).ok_or(BAD_MENU)
    }
    fn find(&self, h: u32, id: u32, bypos: bool) -> Result<(u32, usize), u32> {
        let menu = self.get(h)?;
        if bypos {
            return (id < menu.items.len() as u32)
                .then_some((h, id as usize))
                .ok_or(NOT_FOUND);
        }
        self.find_command(h, id, &mut BTreeSet::new())
            .ok_or(NOT_FOUND)
    }
    fn find_command(&self, h: u32, id: u32, seen: &mut BTreeSet<u32>) -> Option<(u32, usize)> {
        if !seen.insert(h) {
            return None;
        }
        let menu = self.objects.get(&h)?;
        let mut fallback = None;
        for (i, item) in menu.items.iter().enumerate() {
            if item.submenu != 0 {
                if let Some(found) = self.find_command(item.submenu, id, seen) {
                    return Some(found);
                }
                if item.id == id {
                    fallback = Some((h, i));
                }
            } else if item.id == id {
                return Some((h, i));
            }
        }
        fallback
    }
    // Wine caps menu hierarchy at 30 levels and rejects loops. Memoization
    // also bounds work for shared submenus; validate the whole graph so adding
    // a link cannot silently make an existing ancestor exceed that limit.
    fn valid_graph(&self) -> bool {
        fn height(
            m: &Menus,
            h: u32,
            active: &mut BTreeSet<u32>,
            memo: &mut BTreeMap<u32, u32>,
        ) -> Option<u32> {
            if let Some(&n) = memo.get(&h) {
                return Some(n);
            }
            if active.len() >= 30 || !active.insert(h) {
                return None;
            }
            let mut n = 1;
            for item in &m.objects.get(&h)?.items {
                if item.submenu != 0 {
                    n = n.max(height(m, item.submenu, active, memo)? + 1);
                }
            }
            active.remove(&h);
            if n > 30 {
                return None;
            }
            memo.insert(h, n);
            Some(n)
        }
        let mut memo = BTreeMap::new();
        self.objects
            .keys()
            .all(|&h| height(self, h, &mut BTreeSet::new(), &mut memo).is_some())
    }
    pub(crate) fn destroy(&mut self, h: u32) -> bool {
        let Some(menu) = self.objects.remove(&h) else {
            return false;
        };
        for item in menu.items {
            if item.submenu != 0 {
                self.destroy(item.submenu);
            }
        }
        // Keep surviving objects usable if the guest destroys a shared child.
        for menu in self.objects.values_mut() {
            for item in &mut menu.items {
                if item.submenu == h {
                    item.submenu = 0;
                }
            }
        }
        true
    }
}

fn read_info(mem: &GuestMem, p: u32) -> Result<[u32; 12], u32> {
    let size = mem.read_u32(p).map_err(|_| INVALID)?;
    if !matches!(size, 44 | 48) {
        return Err(INVALID);
    }
    let mut bytes = [0; 48];
    mem.read_bytes(p, &mut bytes[..size as usize])
        .map_err(|_| INVALID)?;
    Ok(std::array::from_fn(|i| {
        u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}
fn normalize(mut f: [u32; 12]) -> Result<[u32; 12], u32> {
    if f[1] & !0x1ff != 0
        || (f[1] & 0x10 != 0 && f[1] & (0x40 | 0x100 | 0x80) != 0)
        || (f[1] & 0x100 != 0 && f[2] & 4 != 0)
    {
        return Err(INVALID);
    }
    if f[1] & 0x10 != 0 {
        f[1] |= 0x100;
        if f[2] & (4 | 0x100 | 0x800) == 0 {
            f[1] |= 0x40;
        } else if f[2] & 4 != 0 {
            f[1] |= 0x80;
            f[11] = f[9] & 0xffff;
        }
    }
    Ok(f)
}
pub(crate) fn read_text(mem: &GuestMem, p: u32, wide: bool) -> Result<Option<Vec<u16>>, u32> {
    if p == 0 {
        return Ok(None);
    }
    let mut units = Vec::new();
    for i in 0..32768u32 {
        let addr = p.checked_add(i * if wide { 2 } else { 1 }).ok_or(INVALID)?;
        let c = if wide {
            mem.read_u16(addr).map_err(|_| INVALID)?
        } else {
            mem.read_u8(addr).map_err(|_| INVALID)? as u16
        };
        if c == 0 {
            return if wide {
                Ok(Some(units))
            } else {
                let bytes: Vec<u8> = units.into_iter().map(|u| u as u8).collect();
                let text = crate::codepage::decode_lossy(&bytes, 932).map_err(|_| INVALID)?;
                Ok(Some(text.encode_utf16().collect()))
            };
        }
        units.push(c);
    }
    Err(INVALID)
}
fn updated(mem: &GuestMem, mut item: Item, f: [u32; 12], wide: bool) -> Result<Item, u32> {
    let mask = f[1];
    if mask & 0x100 != 0 {
        item.kind = f[2] & TYPE;
    }
    if mask & 0x40 != 0 {
        item.text = read_text(mem, f[9], wide)?;
    }
    if mask & 1 != 0 {
        item.state = f[3] & STATE;
    }
    if mask & 2 != 0 {
        item.id = f[4];
    }
    if mask & 4 != 0 {
        item.submenu = f[5];
    }
    if mask & 8 != 0 {
        item.checked = f[6];
        item.unchecked = f[7];
    }
    if mask & 0x20 != 0 {
        item.data = f[8];
    }
    if mask & 0x80 != 0 {
        item.bitmap = f[11];
    }
    if item.text.is_none() && item.kind & 0x100 == 0 && item.bitmap == 0 {
        item.kind |= 0x800;
    }
    Ok(item)
}
fn set_item(
    name: &str,
    a: &crate::api::Args,
    mem: &GuestMem,
    menus: &mut Menus,
) -> Result<u32, u32> {
    let f = normalize(read_info(mem, a[3])?)?;
    menus.get(a[0])?;
    let inserting = name.starts_with("Insert");
    let (h, mut pos) = match menus.find(a[0], a[1], a[2] != 0) {
        Ok(found) => found,
        Err(NOT_FOUND) if inserting => (a[0], menus.get(a[0])?.items.len()),
        Err(e) => return Err(e),
    };
    if inserting {
        let menu = menus.objects.get_mut(&h).unwrap();
        if menu.items.len() >= 4096 {
            return Err(8);
        }
        while pos > 0 && (1..=6).contains(&menu.items[pos - 1].bitmap) {
            pos -= 1;
        }
        menu.items.try_reserve(1).map_err(|_| 8u32)?;
    }
    let old = if inserting {
        Item::default()
    } else {
        menus.get(h)?.items[pos].clone()
    };
    let item = updated(mem, old.clone(), f, name.ends_with('W'))?;
    if item.submenu != 0 {
        menus.get(item.submenu).map_err(|_| INVALID)?;
    }
    let submenu = item.submenu;
    let menu = menus.objects.get_mut(&h).unwrap();
    if inserting {
        menu.items.insert(pos, item);
    } else {
        menu.items[pos] = item;
    }
    if !menus.valid_graph() {
        let menu = menus.objects.get_mut(&h).unwrap();
        if inserting {
            menu.items.remove(pos);
        } else {
            menu.items[pos] = old;
        }
        return Err(INVALID);
    }
    if submenu != 0 {
        menus.objects.get_mut(&submenu).unwrap().popup = true;
    }
    Ok(1)
}
fn get_item(name: &str, a: &crate::api::Args, mem: &GuestMem, menus: &Menus) -> Result<u32, u32> {
    let mut f = read_info(mem, a[3])?;
    let mask = f[1];
    if mask & !0x1ff != 0 || (mask & 0x10 != 0 && mask & (0x40 | 0x100 | 0x80) != 0) {
        return Err(INVALID);
    }
    let (h, pos) = menus.find(a[0], a[1], a[2] != 0)?;
    let item = &menus.get(h)?.items[pos];
    let wide = name.ends_with('W');
    let mut text = None;
    if mask & 0x10 != 0 {
        f[2] = item.kind;
        f[11] = item.bitmap;
        if item.bitmap != 0 && !(1..=11).contains(&item.bitmap) && item.bitmap != u32::MAX {
            f[2] |= 4;
        }
        if f[2] & 4 != 0 {
            f[9] = item.bitmap;
            f[10] = 0;
        } else if f[2] & (0x100 | 0x800) != 0 {
            f[9] = 0;
            f[10] = 0;
        }
    }
    if mask & (0x10 | 0x40) != 0 {
        let units = item.text.as_deref().unwrap_or(&[]);
        let mut bytes: Vec<u8> = if wide {
            units.iter().flat_map(|u| u.to_le_bytes()).collect()
        } else {
            crate::codepage::encode(&String::from_utf16_lossy(units), 932)
                .map_err(|_| INVALID)?
                .0
        };
        let unit_size = if wide { 2 } else { 1 };
        let len = bytes.len() / unit_size;
        if f[9] != 0 && f[10] != 0 {
            let count = len.min(f[10] as usize - 1);
            bytes.truncate(count * unit_size);
            bytes.resize((count + 1) * unit_size, 0);
            mem.ptr(f[9], bytes.len() as u32, true)
                .map_err(|_| INVALID)?;
            text = Some((f[9], bytes));
            f[10] = count as u32;
        } else {
            f[10] = len as u32;
        }
    }
    if mask & 0x100 != 0 {
        f[2] = item.kind;
    }
    if mask & 0x80 != 0 {
        f[11] = item.bitmap;
    }
    if mask & 1 != 0 {
        f[3] = item.state;
    }
    if mask & 2 != 0 {
        f[4] = item.id;
    }
    if mask & 0x20 != 0 {
        f[8] = item.data;
    }
    f[5] = if mask & 4 != 0 { item.submenu } else { 0 };
    if mask & 8 != 0 {
        f[6] = item.checked;
        f[7] = item.unchecked;
    }
    let size = f[0] as usize;
    let bytes: Vec<u8> = f
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .take(size)
        .collect();
    mem.ptr(a[3], size as u32, true).map_err(|_| INVALID)?;
    if let Some((p, bytes)) = text {
        mem.write_bytes(p, &bytes).map_err(|_| INVALID)?;
    }
    mem.write_bytes(a[3], &bytes).map_err(|_| INVALID)?;
    Ok(1)
}

pub(crate) fn dispatch(
    dll: &str,
    name: &str,
    a: &crate::api::Args,
    mem: &GuestMem,
    state: &mut ApiState,
) -> Option<(u32, String, bool)> {
    if dll != "user32.dll"
        || !matches!(
            name,
            "CreateMenu"
                | "CreatePopupMenu"
                | "DestroyMenu"
                | "IsMenu"
                | "GetMenu"
                | "SetMenu"
                | "GetMenuItemCount"
                | "InsertMenuItemA"
                | "InsertMenuItemW"
                | "SetMenuItemInfoA"
                | "SetMenuItemInfoW"
                | "GetMenuItemInfoA"
                | "GetMenuItemInfoW"
                | "RemoveMenu"
                | "DeleteMenu"
                | "GetSubMenu"
                | "GetMenuItemID"
                | "GetMenuState"
                | "EnableMenuItem"
                | "CheckMenuItem"
                | "ClientToScreen"
                | "ScreenToClient"
                | "DrawMenuBar"
                | "TrackPopupMenu"
                | "TrackPopupMenuEx"
                | "RedrawWindow"
                | "MapVirtualKeyA"
                | "MapVirtualKeyW"
                | "GetKeyNameTextA"
                | "GetKeyNameTextW"
        )
    {
        return None;
    }
    let supported = !matches!(
        name,
        "DrawMenuBar"
            | "TrackPopupMenu"
            | "TrackPopupMenuEx"
            | "RedrawWindow"
            | "MapVirtualKeyA"
            | "MapVirtualKeyW"
            | "GetKeyNameTextA"
            | "GetKeyNameTextW"
    );
    let result = (|| -> Result<u32, u32> {
        match name {
            "CreateMenu" | "CreatePopupMenu" => state.menus.create(name == "CreatePopupMenu"),
            "IsMenu" => Ok(state.menus.objects.contains_key(&a[0]) as u32),
            "DestroyMenu" => {
                state.menus.get(a[0])?;
                state.menus.destroy(a[0]);
                for w in state.user32.windows.values_mut() {
                    if w.style & 0x40000000 == 0 && !state.menus.objects.contains_key(&w.menu) {
                        w.menu = 0;
                    }
                }
                Ok(1)
            }
            "GetMenu" => {
                let w = state.user32.windows.get(&a[0]).ok_or(1400u32)?;
                Ok(if w.style & 0x40000000 != 0 { 0 } else { w.menu })
            }
            "SetMenu" => {
                let w = state.user32.windows.get_mut(&a[0]).ok_or(1400u32)?;
                if w.style & (0x40000000 | 0x80000000) == 0x40000000 {
                    return Err(1436);
                }
                if a[1] != 0 {
                    state.menus.get(a[1])?;
                }
                w.menu = a[1];
                Ok(1)
            }
            "GetMenuItemCount" => Ok(state.menus.get(a[0])?.items.len() as u32),
            "InsertMenuItemA" | "InsertMenuItemW" | "SetMenuItemInfoA" | "SetMenuItemInfoW" => {
                set_item(name, a, mem, &mut state.menus)
            }
            "GetMenuItemInfoA" | "GetMenuItemInfoW" => get_item(name, a, mem, &state.menus),
            "RemoveMenu" | "DeleteMenu" => {
                let (h, pos) = state.menus.find(a[0], a[1], a[2] & 0x400 != 0)?;
                let item = state.menus.objects.get_mut(&h).unwrap().items.remove(pos);
                if name == "DeleteMenu" && item.submenu != 0 {
                    state.menus.destroy(item.submenu);
                }
                Ok(1)
            }
            "GetSubMenu" | "GetMenuItemID" | "GetMenuState" | "EnableMenuItem"
            | "CheckMenuItem" => {
                let bypos = matches!(name, "GetSubMenu" | "GetMenuItemID") || a[2] & 0x400 != 0;
                let (h, pos) = state.menus.find(a[0], a[1], bypos)?;
                let item = &state.menus.get(h)?.items[pos];
                let old = item.state;
                let submenu = item.submenu;
                match name {
                    "GetSubMenu" => Ok(submenu),
                    "GetMenuItemID" => Ok(if submenu != 0 { u32::MAX } else { item.id }),
                    "GetMenuState" => Ok(if submenu == 0 {
                        item.kind | old
                    } else {
                        ((state.menus.get(submenu)?.items.len() as u32) << 8)
                            | ((item.kind | old | 0x10) & 0xff)
                    }),
                    _ => {
                        let mask = if name == "EnableMenuItem" { 3 } else { 8 };
                        state.menus.objects.get_mut(&h).unwrap().items[pos].state =
                            (old & !mask) | (a[2] & mask);
                        Ok(old & mask)
                    }
                }
            }
            "ClientToScreen" | "ScreenToClient" => {
                let origin =
                    crate::user32_geometry::client_origin(state, a[0]).map_err(|_| 1400u32)?;
                let mut bytes = [0; 8];
                mem.read_bytes(a[1], &mut bytes).map_err(|_| INVALID)?;
                for (i, offset) in origin.into_iter().enumerate() {
                    let n = i32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
                    let n = if name == "ClientToScreen" {
                        n.wrapping_add(offset)
                    } else {
                        n.wrapping_sub(offset)
                    };
                    bytes[i * 4..i * 4 + 4].copy_from_slice(&n.to_le_bytes());
                }
                mem.write_bytes(a[1], &bytes).map_err(|_| INVALID)?;
                Ok(1)
            }
            _ => Err(120),
        }
    })();
    Some(match result {
        Ok(value) => (
            value,
            "process-owned menu / logical client coordinates".into(),
            supported,
        ),
        Err(error) => {
            state.last_error = error;
            let value = if matches!(
                name,
                "GetMenuItemCount"
                    | "GetMenuItemID"
                    | "GetMenuState"
                    | "EnableMenuItem"
                    | "CheckMenuItem"
            ) {
                u32::MAX
            } else {
                0
            };
            (value, format!("menu/USER32 error {error}"), supported)
        }
    })
}
