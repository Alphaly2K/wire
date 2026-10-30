/*
 * Copyright 1993, 1994, 1996 Alexandre Julliard
 * Copyright 1995 Alex Korobka; 2022 Jacek Caban for CodeWeavers
 * LGPL-2.1-or-later; LGPL 2.1 selected for wire's combined work.
 * Retains the Wine/Hangover USER coordinate and enumeration derivation.
 */
//! Parent traversal and window snapshots without a guest executor or memory owner.
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn message_parent(handle: u64) -> bool {
    matches!(handle, 0xfffffffd | 0xfffffffffffffffd)
}

#[derive(Clone, Copy)]
pub(crate) struct Link {
    pub parent: u64,
    pub style: u32,
    pub id: u64,
    pub tid: u64,
    pub offset: [i32; 2],
}
pub(crate) fn origin(
    mut window: u64,
    lookup: impl Fn(u64) -> Option<Link>,
) -> Result<[i32; 2], &'static str> {
    if window == 0 {
        return Ok([0; 2]);
    }
    let first = lookup(window).ok_or("invalid window coordinates")?;
    if first.style & 0x40000000 == 0 || first.parent == 0 || message_parent(first.parent) {
        return Ok(first.offset);
    }
    let mut seen = BTreeSet::new();
    let mut result = [0i32; 2];
    while window != 0 {
        if !seen.insert(window) {
            return Err("cyclic parent coordinates");
        }
        let w = lookup(window).ok_or("invalid window coordinates")?;
        for (i, value) in result.iter_mut().enumerate() {
            *value = value.wrapping_add(w.offset[i]);
        }
        if w.style & 0x40000000 == 0 || message_parent(w.parent) {
            break;
        }
        window = w.parent;
    }
    Ok(result)
}
pub(crate) fn validate_parent(
    window: u64,
    mut parent: u64,
    lookup: impl Fn(u64) -> Option<Link>,
) -> Result<(), u32> {
    if parent != 0 && lookup(parent).is_none() {
        return Err(1400);
    }
    let mut seen = BTreeSet::new();
    while parent != 0 {
        if parent == window || !seen.insert(parent) {
            return Err(87);
        }
        parent = lookup(parent).map_or(0, |w| w.parent);
    }
    Ok(())
}
pub(crate) fn reparent_position(screen: [i32; 2], frame: [i32; 2], parent: [i32; 2]) -> [i32; 2] {
    std::array::from_fn(|i| screen[i].wrapping_sub(frame[i]).wrapping_sub(parent[i]))
}
pub(crate) fn enabled(style: u32, enable: bool) -> (u32, bool, bool) {
    let disabled = style & 0x08000000 != 0;
    (
        if enable {
            style & !0x08000000
        } else {
            style | 0x08000000
        },
        disabled,
        disabled == enable,
    )
}
pub(crate) fn child(
    windows: impl Iterator<Item = (u64, Link)>,
    parent: u64,
    id: u64,
) -> Option<u64> {
    windows
        .filter(|(_, w)| {
            w.parent == parent && w.style & 0x40000000 != 0 && w.id as u32 == id as u32
        })
        .map(|(h, _)| h)
        .next()
}
pub(crate) fn children(windows: impl Iterator<Item = (u64, Link)>, parent: u64) -> Vec<u64> {
    let windows: BTreeMap<_, _> = windows.collect();
    let mut seen = BTreeSet::new();
    let mut pending = vec![parent];
    let mut children = Vec::new();
    while let Some(parent) = pending.pop() {
        if !seen.insert(parent) {
            continue;
        }
        for (&h, w) in &windows {
            if w.parent == parent && w.style & 0x40000000 != 0 {
                children.push(h);
                pending.push(h);
            }
        }
    }
    children
}
pub(crate) fn top(windows: impl DoubleEndedIterator<Item = (u64, Link)>, tid: u64) -> Vec<u64> {
    windows
        .rev()
        .filter(|(_, w)| {
            w.tid != 0
                && w.style & 0x40000000 == 0
                && !message_parent(w.parent)
                && (tid == 0 || tid == w.tid)
        })
        .map(|(h, _)| h)
        .collect()
}
