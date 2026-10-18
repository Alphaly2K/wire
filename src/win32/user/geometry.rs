/*
 * Rectangle helpers retain the Wine 10.0 user32_common.rs attribution:
 * Copyright 1993, 1994, 1996 Alexandre Julliard
 *
 * Frame calculation adapted 2026-10-06 from Hangover Wine win32u/defwnd.c,
 * fixed commit 3ea3a14941b6a122b892c84141ce9662c008081e.
 * Copyright 1993, 1996 Alexandre Julliard
 * Copyright 1995 Alex Korobka
 * Copyright 2022 Jacek Caban for CodeWeavers
 *
 * This library is free software; you can redistribute it and/or modify it
 * under the GNU Lesser General Public License, version 2.1 or later.
 * This library is distributed without any warranty; see COPYING.LIB.
 * Original frame excerpt and license: third_party/hangover/.
 */
//! Signed rectangle values and nonclient dimensions, without guest state/memory.
pub(crate) type Rect = [i32; 4];

pub(crate) fn empty(r: Rect) -> bool {
    r[0] >= r[2] || r[1] >= r[3]
}

pub(crate) fn point(r: Rect, x: i32, y: i32) -> bool {
    !empty(r) && x >= r[0] && x < r[2] && y >= r[1] && y < r[3]
}

pub(crate) fn offset(mut r: Rect, x: i32, y: i32) -> Rect {
    let delta = [x, y];
    for i in 0..4 {
        r[i] = r[i].wrapping_add(delta[i % 2]);
    }
    r
}

pub(crate) fn inflate(mut r: Rect, x: i32, y: i32) -> Rect {
    let delta = [x, y];
    for i in 0..4 {
        r[i] = if i < 2 {
            r[i].wrapping_sub(delta[i % 2])
        } else {
            r[i].wrapping_add(delta[i % 2])
        };
    }
    r
}

pub(crate) fn intersection(r: Rect, s: Rect) -> (Rect, bool) {
    nonempty([
        r[0].max(s[0]),
        r[1].max(s[1]),
        r[2].min(s[2]),
        r[3].min(s[3]),
    ])
}

pub(crate) fn union(r: Rect, s: Rect) -> (Rect, bool) {
    nonempty(if empty(r) {
        s
    } else if empty(s) {
        r
    } else {
        [
            r[0].min(s[0]),
            r[1].min(s[1]),
            r[2].max(s[2]),
            r[3].max(s[3]),
        ]
    })
}

fn nonempty(r: Rect) -> (Rect, bool) {
    if empty(r) {
        ([0; 4], false)
    } else {
        (r, true)
    }
}

pub(crate) fn adjusted(mut r: Rect, style: u32, ex: u32, menu: bool) -> Rect {
    let f = frame(style, ex, menu, false);
    for i in 0..4 {
        r[i] = if i < 2 {
            r[i].wrapping_sub(f[i])
        } else {
            r[i].wrapping_add(f[i])
        };
    }
    r
}

pub(crate) fn frame(style: u32, ex: u32, menu: bool, scroll: bool) -> [i32; 4] {
    let mut border = if ex & (0x20000 | 1) == 0x20000 {
        1
    } else if ex & 1 != 0 || style & (0x40000 | 0x400000) != 0 {
        2
    } else {
        0
    };
    if style & 0x40000 != 0 {
        border += 1;
    } // border width 1, padded border 0
    if style & (0x800000 | 0x400000) != 0 || ex & 1 != 0 {
        border += 1;
    }
    if ex & 0x200 != 0 {
        border += 2;
    }
    let caption = if style & 0xc00000 == 0xc00000 {
        if ex & 0x80 != 0 {
            18
        } else {
            23
        }
    } else {
        0
    };
    let mut result = [
        border,
        border + caption + if menu { 19 } else { 0 },
        border,
        border,
    ];
    if scroll {
        if style & 0x200000 != 0 {
            result[if ex & 0x4000 != 0 { 0 } else { 2 }] += 17;
        }
        if style & 0x100000 != 0 {
            result[3] += 17;
        }
    }
    result
}
