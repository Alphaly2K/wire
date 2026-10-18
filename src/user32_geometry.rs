/*
 * Adapted 2026-10-06 from Hangover Wine win32u/defwnd.c, fixed commit
 * 3ea3a14941b6a122b892c84141ce9662c008081e.
 * Copyright 1993, 1996 Alexandre Julliard
 * Copyright 1995 Alex Korobka
 * Copyright 2022 Jacek Caban for CodeWeavers
 * GNU Lesser General Public License, version 2.1 or later. No warranty.
 * Original excerpt and license: third_party/hangover/.
 */
//! Shared nonclient geometry for the initial 96-DPI desktop profile.
use crate::api::ApiState;
use std::collections::BTreeSet;

pub(crate) use crate::win32::user::geometry::frame;
pub(crate) fn client_origin(state: &ApiState, mut h: u32) -> Result<[i32; 2], String> {
    let mut seen = BTreeSet::new();
    let mut result = [0i32; 2];
    while h != 0 {
        if !seen.insert(h) {
            return Err("cyclic parent coordinates".into());
        }
        let w = state
            .user32
            .windows
            .get(&h)
            .ok_or("invalid window coordinates")?;
        let f = w.frame();
        for i in 0..2 {
            result[i] = result[i].wrapping_add(w.rect[i]).wrapping_add(f[i]);
        }
        if w.style & 0x40000000 == 0 {
            break;
        }
        h = w.parent;
    }
    Ok(result)
}
