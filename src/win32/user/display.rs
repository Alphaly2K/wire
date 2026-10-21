/*
 * Copyright 1993, 1994, 1996 Alexandre Julliard
 *
 * This library is free software; you can redistribute it and/or
 * modify it under the terms of the GNU Lesser General Public License
 * as published by the Free Software Foundation; either version 2.1
 * of the License, or (at your option) any later version.
 * This library is distributed without any warranty; see COPYING.LIB.
 */
//! Fixed desktop values and byte encoding; adapters retain input/error policy.
pub(crate) const DEVICE: &str = "\\\\.\\DISPLAY1";
pub(crate) const WIDTH: u32 = 1024;
pub(crate) const HEIGHT: u32 = 768;
pub(crate) const REFRESH_HZ: u32 = 60;

fn text(bytes: &mut [u8], at: usize, value: &str, wide: bool) {
    if !wide {
        bytes[at..at + value.len()].copy_from_slice(value.as_bytes());
        return;
    }
    let data = value
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    bytes[at..at + data.len()].copy_from_slice(&data);
}

pub(crate) fn device(wide: bool, adapter: bool, label: &str) -> Vec<u8> {
    let size = if wide { 840u32 } else { 424 };
    let mut bytes = vec![0; size as usize];
    bytes[..4].copy_from_slice(&size.to_le_bytes());
    if adapter {
        text(&mut bytes, 4, DEVICE, wide);
    } else {
        text(&mut bytes, 4, &format!("{DEVICE}\\Monitor0"), wide);
    }
    text(&mut bytes, if wide { 68 } else { 36 }, label, wide);
    let flags = if wide { 324 } else { 164 };
    bytes[flags..flags + 4].copy_from_slice(&(if adapter { 5u32 } else { 3 }).to_le_bytes());
    bytes
}

pub(crate) fn mode(wide: bool, size: usize) -> Vec<u8> {
    mode_for_size(wide, size, [WIDTH, HEIGHT])
}
pub(crate) fn mode_for_size(wide: bool, size: usize, dimensions: [u32; 2]) -> Vec<u8> {
    let mut bytes = vec![0; size];
    text(&mut bytes, 0, DEVICE, wide);
    let offset = if wide { 68 } else { 36 };
    bytes[offset - 4..offset - 2].copy_from_slice(&0x401u16.to_le_bytes());
    bytes[offset..offset + 2].copy_from_slice(&(size as u16).to_le_bytes());
    bytes[offset + 4..offset + 8].copy_from_slice(&0x005c0020u32.to_le_bytes());
    let bits = if wide { 168 } else { 104 };
    for (i, value) in [32u32, dimensions[0], dimensions[1], 0, REFRESH_HZ]
        .into_iter()
        .enumerate()
    {
        bytes[bits + 4 * i..bits + 4 * i + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

pub(crate) fn monitor(wide: bool, size: usize) -> Vec<u8> {
    monitor_for_size(wide, size, [WIDTH, HEIGHT])
}
pub(crate) fn monitor_for_size(wide: bool, size: usize, dimensions: [u32; 2]) -> Vec<u8> {
    let mut bytes = vec![0; size];
    for (i, value) in [
        size as u32,
        0,
        0,
        dimensions[0],
        dimensions[1],
        0,
        0,
        dimensions[0],
        dimensions[1],
        1,
    ]
    .into_iter()
    .enumerate()
    {
        bytes[4 * i..4 * i + 4].copy_from_slice(&value.to_le_bytes());
    }
    if size != 40 {
        text(&mut bytes, 40, DEVICE, wide);
    }
    bytes
}

pub(crate) fn metric(index: u32) -> u32 {
    metric_for_size(index, [WIDTH, HEIGHT])
}
pub(crate) fn metric_for_size(index: u32, dimensions: [u32; 2]) -> u32 {
    match index {
        0 | 16 | 61 | 78 => dimensions[0],
        1 | 17 | 62 | 79 => dimensions[1],
        2 | 3 | 9 | 10 | 20 | 21 => 17,
        4 => 23,
        5..=8 => 1,
        11..=14 => 32,
        15 => 19,
        28..=31 => 112,
        32 | 33 => 4,
        36 | 37 | 48 | 49 => 16,
        43 => 3,
        45 | 46 | 68 | 80 => 1,
        83 => 2,
        _ => 0,
    }
}
