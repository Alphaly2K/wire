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

pub(crate) use crate::win32::user::geometry::frame;
pub(crate) fn client_origin(state: &ApiState, h: u32) -> Result<[i32; 2], String> {
    state.user32.client_origin(h)
}
