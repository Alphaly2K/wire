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
//! adapted to process-owned objects and checked memory/window adapters.
use crate::win32::catalog::MenuApi as M;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) trait Memory {
    type Error: From<u32>;
    fn read(&self, address: u32, bytes: &mut [u8]) -> Result<(), Self::Error>;
    fn check_write(&self, address: u32, length: usize) -> Result<(), Self::Error>;
    fn write(&mut self, address: u32, bytes: &[u8]) -> Result<(), Self::Error>;
    fn read_int(&self, address: u32, length: usize) -> Result<u32, Self::Error> {
        let mut bytes = [0; 4];
        self.read(address, &mut bytes[..length])?;
        Ok(u32::from_le_bytes(bytes))
    }
}
pub(crate) trait Windows {
    fn menu(&self, window: u32) -> Result<(u32, u32), u32>;
    fn set_menu(&mut self, window: u32, menu: u32) -> Result<(), u32>;
    fn retain_menus(&mut self, menus: &Menus);
}
const MENU_BYTES: usize = 128;
const ITEM_BYTES: usize = 64;

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
    end: u32,
    bytes: usize,
    limit: usize,
}
impl Default for Menus {
    fn default() -> Self {
        Self {
            objects: BTreeMap::new(),
            next: 0x41010000,
            end: u32::MAX,
            bytes: 0,
            limit: usize::MAX,
        }
    }
}
impl Item {
    fn bytes(&self) -> usize {
        ITEM_BYTES + self.text.as_ref().map_or(0, |text| text.len() * 2)
    }
}
impl Menus {
    #[allow(dead_code)]
    pub(crate) fn with_limits(end: u32, bytes: usize) -> Self {
        Self {
            end,
            limit: bytes,
            ..Self::default()
        }
    }
    pub(crate) fn contains(&self, handle: u32) -> bool {
        self.objects.contains_key(&handle)
    }
    #[allow(dead_code)]
    pub(crate) fn retirements(&self, id: M, args: &[u32; 16]) -> Result<BTreeSet<u32>, u32> {
        let root = if id == M::Destroy {
            args[0]
        } else {
            let (h, p) = self.find(args[0], args[1], args[2] & 0x400 != 0)?;
            self.get(h)?.items[p].submenu
        };
        let mut retired = BTreeSet::new();
        let mut pending = if root == 0 { Vec::new() } else { vec![root] };
        while let Some(h) = pending.pop() {
            if !retired.insert(h) {
                continue;
            }
            for item in &self.get(h)?.items {
                if item.submenu != 0 {
                    pending.push(item.submenu);
                }
            }
        }
        if id == M::Destroy && root == 0 {
            return Err(BAD_MENU);
        }
        Ok(retired)
    }
    fn create(&mut self, popup: bool) -> Result<u32, u32> {
        if self.objects.len() >= 4096
            || self.next >= self.end
            || self
                .bytes
                .checked_add(MENU_BYTES)
                .is_none_or(|bytes| bytes > self.limit)
        {
            return Err(8);
        }
        let handle = self.next;
        self.next = handle.checked_add(4).ok_or(8u32)?;
        self.bytes += MENU_BYTES;
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
        self.bytes -= MENU_BYTES + menu.items.iter().map(Item::bytes).sum::<usize>();
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

fn read_info<T: Memory>(mem: &T, p: u32) -> Result<[u32; 12], T::Error> {
    let size = mem.read_int(p, 4)?;
    if !matches!(size, 44 | 48) {
        return Err(INVALID.into());
    }
    let mut bytes = [0; 48];
    mem.read(p, &mut bytes[..size as usize])?;
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
pub(crate) fn read_text<T: Memory>(
    mem: &T,
    p: u32,
    wide: bool,
) -> Result<Option<Vec<u16>>, T::Error> {
    if p == 0 {
        return Ok(None);
    }
    let mut units = Vec::new();
    for i in 0..32768u32 {
        let addr = p.checked_add(i * if wide { 2 } else { 1 }).ok_or(INVALID)?;
        let c = mem.read_int(addr, if wide { 2 } else { 1 })? as u16;
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
    Err(INVALID.into())
}
fn updated<T: Memory>(mem: &T, mut item: Item, f: [u32; 12], wide: bool) -> Result<Item, T::Error> {
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
fn set_item<T: Memory>(id: M, a: &[u32; 16], mem: &T, menus: &mut Menus) -> Result<u32, T::Error> {
    let f = normalize(read_info(mem, a[3])?)?;
    menus.get(a[0])?;
    let inserting = id.inserting();
    let (h, mut pos) = match menus.find(a[0], a[1], a[2] != 0) {
        Ok(found) => found,
        Err(NOT_FOUND) if inserting => (a[0], menus.get(a[0])?.items.len()),
        Err(e) => return Err(e.into()),
    };
    if inserting {
        let menu = menus.objects.get_mut(&h).unwrap();
        if menu.items.len() >= 4096 {
            return Err(8u32.into());
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
    let item = updated(mem, old.clone(), f, id.wide())?;
    if item.submenu != 0 {
        menus.get(item.submenu).map_err(|_| INVALID)?;
    }
    let bytes = menus.bytes - if inserting { 0 } else { old.bytes() };
    let bytes = bytes
        .checked_add(item.bytes())
        .filter(|&bytes| bytes <= menus.limit)
        .ok_or(8u32)?;
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
        return Err(INVALID.into());
    }
    menus.bytes = bytes;
    if submenu != 0 {
        menus.objects.get_mut(&submenu).unwrap().popup = true;
    }
    Ok(1)
}
fn get_item<T: Memory>(id: M, a: &[u32; 16], mem: &mut T, menus: &Menus) -> Result<u32, T::Error> {
    let mut f = read_info(mem, a[3])?;
    let mask = f[1];
    if mask & !0x1ff != 0 || (mask & 0x10 != 0 && mask & (0x40 | 0x100 | 0x80) != 0) {
        return Err(INVALID.into());
    }
    let (h, pos) = menus.find(a[0], a[1], a[2] != 0)?;
    let item = &menus.get(h)?.items[pos];
    let wide = id.wide();
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
            mem.check_write(f[9], bytes.len())?;
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
    mem.check_write(a[3], size)?;
    if let Some((p, bytes)) = text {
        mem.write(p, &bytes)?;
    }
    mem.write(a[3], &bytes)?;
    Ok(1)
}

impl Menus {
    pub(crate) fn apply<T: Memory, W: Windows>(
        &mut self,
        id: M,
        a: &[u32; 16],
        mem: &mut T,
        windows: &mut W,
    ) -> Result<u32, T::Error> {
        match id {
            M::Create | M::CreatePopup => Ok(self.create(id == M::CreatePopup)?),
            M::IsMenu => Ok(self.objects.contains_key(&a[0]) as u32),
            M::Destroy => {
                self.get(a[0])?;
                self.destroy(a[0]);
                windows.retain_menus(self);
                Ok(1)
            }
            M::Get => {
                let (style, menu) = windows.menu(a[0])?;
                Ok(if style & 0x40000000 != 0 { 0 } else { menu })
            }
            M::Set => {
                let (style, _) = windows.menu(a[0])?;
                if style & (0x40000000 | 0x80000000) == 0x40000000 {
                    return Err(1436u32.into());
                }
                if a[1] != 0 {
                    self.get(a[1])?;
                }
                windows.set_menu(a[0], a[1])?;
                Ok(1)
            }
            M::ItemCount => Ok(self.get(a[0])?.items.len() as u32),
            M::InsertA | M::InsertW | M::SetItemA | M::SetItemW => set_item(id, a, mem, self),
            M::GetItemA | M::GetItemW => get_item(id, a, mem, self),
            M::Remove | M::Delete => {
                let (h, pos) = self.find(a[0], a[1], a[2] & 0x400 != 0)?;
                let item = self.objects.get_mut(&h).unwrap().items.remove(pos);
                self.bytes -= item.bytes();
                if id == M::Delete && item.submenu != 0 {
                    self.destroy(item.submenu);
                    windows.retain_menus(self);
                }
                Ok(1)
            }
            M::Submenu | M::ItemId | M::State | M::Enable | M::Check => {
                let bypos = matches!(id, M::Submenu | M::ItemId) || a[2] & 0x400 != 0;
                let (h, pos) = self.find(a[0], a[1], bypos)?;
                let item = &self.get(h)?.items[pos];
                let old = item.state;
                let submenu = item.submenu;
                match id {
                    M::Submenu => Ok(submenu),
                    M::ItemId => Ok(if submenu != 0 { u32::MAX } else { item.id }),
                    M::State => Ok(if submenu == 0 {
                        item.kind | old
                    } else {
                        ((self.get(submenu)?.items.len() as u32) << 8)
                            | ((item.kind | old | 0x10) & 0xff)
                    }),
                    _ => {
                        let mask = if id == M::Enable { 3 } else { 8 };
                        self.objects.get_mut(&h).unwrap().items[pos].state =
                            (old & !mask) | (a[2] & mask);
                        Ok(old & mask)
                    }
                }
            }
            _ => Err(120u32.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Input([u8; 64]);
    impl Memory for Input {
        type Error = u32;
        fn read(&self, p: u32, bytes: &mut [u8]) -> Result<(), u32> {
            let source = self
                .0
                .get(p as usize..p as usize + bytes.len())
                .ok_or(87u32)?;
            bytes.copy_from_slice(source);
            Ok(())
        }
        fn check_write(&self, _: u32, _: usize) -> Result<(), u32> {
            Err(87)
        }
        fn write(&mut self, _: u32, _: &[u8]) -> Result<(), u32> {
            Err(87)
        }
    }
    fn input(mask: u32, submenu: u32, text: &[u16]) -> Input {
        let mut input = Input([0; 64]);
        for (i, value) in [48, mask, 0, 0, 7, submenu, 0, 0, 0, 48, 0, 0]
            .into_iter()
            .enumerate()
        {
            input.0[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        for (i, unit) in text.iter().enumerate() {
            input.0[48 + i * 2..50 + i * 2].copy_from_slice(&unit.to_le_bytes());
        }
        input
    }
    fn item_args(menu: u32) -> [u32; 16] {
        let mut a = [0; 16];
        a[0] = menu;
        a[2] = 1;
        a
    }

    #[test]
    fn menu_quota_replacement_and_cycle_rollback_release_owned_bytes() {
        let mut menus = Menus::with_limits(u32::MAX, 2 * MENU_BYTES + ITEM_BYTES + 4);
        let root = menus.create(false).unwrap();
        let shared = menus.create(true).unwrap();
        let a = item_args(root);
        assert_eq!(
            set_item(M::InsertW, &a, &input(0x46, shared, &[65, 66]), &mut menus),
            Ok(1)
        );
        assert_eq!(menus.create(false), Err(8));
        assert_eq!(
            set_item(M::SetItemW, &a, &input(0x40, 0, &[65, 66, 67]), &mut menus),
            Err(8)
        );
        assert_eq!(
            menus.get(root).unwrap().items[0].text.as_deref(),
            Some(&[65, 66][..])
        );
        assert_eq!(
            set_item(M::SetItemW, &a, &input(4, root, &[]), &mut menus),
            Err(87)
        );
        assert_eq!(menus.get(root).unwrap().items[0].submenu, shared);
        // Destroying a shared child clears links without double charging the parent.
        assert!(menus.destroy(shared));
        assert_eq!(menus.get(root).unwrap().items[0].submenu, 0);
        let next = menus.create(true).unwrap();
        assert!(next > shared);
        assert!(menus.destroy(root));
        assert!(menus.destroy(next));
        assert_eq!(menus.bytes, 0);
        assert!(menus.create(false).is_ok());
    }

    #[test]
    fn menu_retired_handles_do_not_wrap_or_reuse_identity() {
        let start = Menus::default().next;
        let mut menus = Menus::with_limits(start + 8, MENU_BYTES);
        for expected in [start, start + 4] {
            assert_eq!(menus.create(false), Ok(expected));
            assert_eq!(menus.create(true), Err(8));
            assert!(menus.destroy(expected));
        }
        assert_eq!(menus.create(false), Err(8));
        assert_eq!(menus.bytes, 0);
    }
}
