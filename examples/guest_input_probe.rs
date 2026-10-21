//! Original guest WNDPROC exercised by a real native window and mouse events.
#[allow(dead_code)]
#[path = "../tests/support/x64.rs"]
mod support;
use std::{
    io::Write,
    time::{Duration, Instant},
};
use support::{fixture, startup_dll_call, BASE};
use wire::x64::{
    memory::{EXECUTE, READ, WRITE},
    process::{Outcome, Process64},
};

const CODE: u64 = 0x6d0000000;
const DATA: u64 = CODE + 4096;
fn mov(code: &mut Vec<u8>, r: usize, value: u64) {
    let (rex, op) = [(0x48, 0xb9), (0x48, 0xba), (0x49, 0xb8), (0x49, 0xb9)][r];
    code.extend([rex, op]);
    code.extend(value.to_le_bytes());
}
fn call(code: &mut Vec<u8>, address: u64, args: &[u64]) {
    for (n, &arg) in args.iter().enumerate() {
        if n < 4 {
            mov(code, n, arg);
        } else {
            code.extend([0x48, 0xb8]);
            code.extend(arg.to_le_bytes());
            code.extend([0x48, 0x89, 0x44, 0x24, 32 + (n as u8 - 4) * 8]);
        }
    }
    code.extend([0x48, 0xb8]);
    code.extend(address.to_le_bytes());
    code.extend([0xff, 0xd0]);
}
#[cfg(target_os = "macos")]
extern "C" {
    fn wire_ui_frame(hwnd: u32, rgba: *const u8, width: u32, height: u32) -> i32;
}

fn main() -> Result<(), String> {
    std::env::set_var("WIRE_INPUT_TRACE", "1");
    let raw_mode = std::env::args().any(|a| a == "--raw");
    let pointer_mode = std::env::args().any(|a| a == "--pointer");
    let path = std::env::temp_dir().join(format!("wire-guest-input-{}.exe", std::process::id()));
    std::fs::write(&path, fixture()).map_err(|e| e.to_string())?;
    let mut p = Process64::load(&path, false)?;
    std::fs::remove_file(path).map_err(|e| e.to_string())?;
    p.memory.map(CODE, 16384, READ | WRITE | EXECUTE)?;
    let entry = p.cpu.clone();
    for (offset, text) in [
        (0, "InputProbe"),
        (64, "Wire guest input probe — click the blue box"),
    ] {
        p.memory.write(
            DATA + offset,
            &text
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )?;
    }
    // Store actual guest callback arguments in a bounded guest-memory log.
    let mut callback = Vec::new();
    if raw_mode {
        let raw = p.runtime.register("user32.dll", "GetRawInputData");
        let default = p.runtime.register("user32.dll", "DefWindowProcW");
        callback.extend([0x81, 0xfa, 0xff, 0, 0, 0, 0x0f, 0x85]);
        let branch = callback.len();
        callback.extend([0; 4]);
        callback.extend([
            0x48, 0x83, 0xec, 0x58, 0x48, 0x89, 0x4c, 0x24, 0x30, 0x4c, 0x89, 0x44, 0x24, 0x38,
            0x4c, 0x89, 0x4c, 0x24, 0x40, 0x4c, 0x89, 0xc9,
        ]);
        // The guest reads its HRAWINPUT, records mouse flags and calls cleanup.
        mov(&mut callback, 1, 0x10000003);
        mov(&mut callback, 2, DATA + 0x1800);
        mov(&mut callback, 3, DATA + 0x17f0);
        callback.extend([0x41, 0xc7, 0x01, 48, 0, 0, 0]);
        call(&mut callback, raw, &[]);
        // The fifth Win64 argument lives above the 32-byte shadow space.
        let call_at = callback.len() - 12;
        callback.splice(
            call_at..call_at,
            [0x48, 0xc7, 0x44, 0x24, 0x20, 24, 0, 0, 0],
        );
        callback.extend([0x83, 0xf8, 48, 0x75, 28, 0x48, 0xb8]);
        callback.extend((DATA + 0x1800 + 28).to_le_bytes());
        callback.extend([0x0f, 0xb7, 0x00, 0x48, 0xb9]);
        callback.extend((DATA + 0x17e0).to_le_bytes());
        callback.extend([0x09, 0x01, 0xff, 0x41, 4]);
        callback.extend([
            0x48, 0x8b, 0x4c, 0x24, 0x30, 0x4c, 0x8b, 0x44, 0x24, 0x38, 0x4c, 0x8b, 0x4c, 0x24,
            0x40,
        ]);
        mov(&mut callback, 1, 0xff);
        call(&mut callback, default, &[]);
        callback.extend([0x48, 0x83, 0xc4, 0x58, 0x31, 0xc0, 0xc3]);
        let delta = (callback.len() - branch - 4) as i32;
        callback[branch..branch + 4].copy_from_slice(&delta.to_le_bytes());
    }
    if pointer_mode {
        let info = p.runtime.register("user32.dll", "GetPointerInfo");
        callback.extend([0x81, 0xfa, 0x45, 2, 0, 0, 0x0f, 0x82]);
        let below = callback.len();
        callback.extend([0; 4]);
        callback.extend([0x81, 0xfa, 0x4f, 2, 0, 0, 0x0f, 0x87]);
        let above = callback.len();
        callback.extend([0; 4]);
        callback.extend([
            0x48, 0x83, 0xec, 0x58, 0x48, 0x89, 0x4c, 0x24, 0x30, 0x48, 0x89, 0x54, 0x24, 0x38,
            0x4c, 0x89, 0x44, 0x24, 0x40, 0x4c, 0x89, 0x4c, 0x24, 0x48, 0x4c, 0x89, 0xc1, 0x81,
            0xe1, 0xff, 0xff, 0, 0,
        ]);
        mov(&mut callback, 1, DATA + 0x1900);
        call(&mut callback, info, &[]);
        callback.extend([0x48, 0xb8]);
        callback.extend((DATA + 0x190c).to_le_bytes());
        callback.extend([0x8b, 0x00, 0x48, 0xb9]);
        callback.extend((DATA + 0x17d0).to_le_bytes());
        callback.extend([
            0x09, 0x01, 0x48, 0x8b, 0x4c, 0x24, 0x30, 0x48, 0x8b, 0x54, 0x24, 0x38, 0x4c, 0x8b,
            0x44, 0x24, 0x40, 0x4c, 0x8b, 0x4c, 0x24, 0x48, 0x48, 0x83, 0xc4, 0x58,
        ]);
        for branch in [below, above] {
            let delta = (callback.len() - branch - 4) as i32;
            callback[branch..branch + 4].copy_from_slice(&delta.to_le_bytes());
        }
        assert_eq!(
            startup_dll_call(
                &mut p.memory,
                &mut p.cpu,
                &mut p.runtime,
                "user32.dll",
                "EnableMouseInPointer",
                &[1]
            ),
            1
        );
    }
    callback.extend([0x48, 0xb8]);
    callback.extend((DATA + 0x500).to_le_bytes());
    callback.extend([
        0x8b, 0x08, 0x81, 0xf9, 0, 1, 0, 0, 0x73, 24, 0xff, 0x00, 0x48, 0xc1, 0xe1, 5, 0x48, 0x8d,
        0x84, 0x08, 0, 2, 0, 0, 0x89, 0x10, 0x4c, 0x89, 0x40, 8, 0x4c, 0x89, 0x48, 16, 0xb8, 1, 0,
        0, 0, 0xc3,
    ]);
    p.memory.write(CODE, &callback)?;
    if raw_mode {
        for (n, usage) in [2, 6].into_iter().enumerate() {
            let at = DATA + 0x300 + n as u64 * 16;
            p.memory.write_int(at, 2, 1)?;
            p.memory.write_int(at + 2, 2, usage)?;
        }
        assert_eq!(
            startup_dll_call(
                &mut p.memory,
                &mut p.cpu,
                &mut p.runtime,
                "user32.dll",
                "RegisterRawInputDevices",
                &[DATA + 0x300, 2, 16]
            ),
            1
        );
    }
    p.memory.write_int(DATA + 0x100, 4, 80)?;
    p.memory.write_int(DATA + 0x108, 8, CODE)?;
    p.memory.write_int(DATA + 0x118, 8, BASE)?;
    p.memory.write_int(DATA + 0x140, 8, DATA)?;
    let atom = startup_dll_call(
        &mut p.memory,
        &mut p.cpu,
        &mut p.runtime,
        "user32.dll",
        "RegisterClassExW",
        &[DATA + 0x100],
    );
    assert_ne!(atom, 0);
    let create = p.runtime.register("user32.dll", "CreateWindowExW");
    let peek = p.runtime.register("user32.dll", "PeekMessageW");
    let dispatch = p.runtime.register("user32.dll", "DispatchMessageW");
    let mut main = vec![0x48, 0x83, 0xec, 0x68];
    call(
        &mut main,
        create,
        &[
            0,
            atom,
            DATA + 64,
            0x10000000,
            40,
            40,
            640,
            360,
            0,
            0,
            BASE,
            0,
        ],
    );
    let loop_start = main.len();
    call(&mut main, peek, &[DATA + 0x600, 0, 0, 0, 1]);
    main.extend([0x85, 0xc0, 0x74, 22]);
    call(&mut main, dispatch, &[DATA + 0x600]);
    main.push(0xe9);
    let next = main.len() + 4;
    main.extend(((loop_start as isize - next as isize) as i32).to_le_bytes());
    p.memory.write(BASE + 0x1000, &main)?;
    p.cpu = entry;
    p.enable_window()?;
    let mut rgba = vec![245; 640 * 360 * 4];
    for y in 0..360 {
        for x in 0..640 {
            let i = (y * 640 + x) * 4;
            rgba[i..i + 4].copy_from_slice(if (100..300).contains(&x) && (80..180).contains(&y) {
                &[25, 110, 220, 255]
            } else {
                &[245, 245, 245, 255]
            });
        }
    }
    let start = Instant::now();
    let mut drawn = false;
    let mut seen = 0;
    while start.elapsed() < Duration::from_secs(180) {
        if !matches!(p.run(p.cpu.work + 65536)?, Outcome::Budget) {
            return Err("probe guest returned".into());
        }
        if !drawn && !p.runtime.window_info().is_empty() {
            #[cfg(target_os = "macos")]
            {
                drawn = unsafe { wire_ui_frame(0x60000000, rgba.as_ptr(), 640, 360) } != 0;
            }
            if drawn {
                println!("READY: blue box is guest client [100,80..300,180]");
            }
        }
        let count = p.memory.read_int(DATA + 0x500, 4)?;
        for n in seen..count {
            let record = DATA + 0x700 + n * 32;
            let message = p.memory.read_int(record, 4)?;
            let lp = p.memory.read_int(record + 16, 8)?;
            if (0x200..=0x20e).contains(&message) || (0x245..=0x24f).contains(&message) {
                let x = lp as u16 as i16 - if pointer_mode { 40 } else { 0 };
                let y = (lp >> 16) as u16 as i16 - if pointer_mode { 40 } else { 0 };
                println!("GUEST WNDPROC: message={message:#x} x={x} y={y}");
                if message == if pointer_mode { 0x247 } else { 0x202 }
                    && (100..300).contains(&x)
                    && (80..180).contains(&y)
                {
                    if pointer_mode {
                        let flags = p.memory.read_int(DATA + 0x17d0, 4)?;
                        if flags & 0x50000 != 0x50000 {
                            return Err(format!(
                                "guest pointer down/up snapshots missing: {flags:#x}"
                            ));
                        }
                        assert_eq!(p.memory.read_int(DATA + 0x1900, 4)?, 4);
                        assert_eq!(p.memory.read_int(DATA + 0x1958, 4)?, 2);
                        println!(
                            "PASS: guest read pointer down/up snapshots through GetPointerInfo"
                        );
                    }
                    if raw_mode {
                        let flags = p.memory.read_int(DATA + 0x17e0, 4)?;
                        if flags & 3 != 3 {
                            return Err(format!(
                                "guest raw mouse down/up flags missing: {flags:#x}"
                            ));
                        }
                        println!("PASS: guest read raw mouse down/up packets and cleaned them up");
                    }
                    println!("PASS: real mouse click reached the guest button");
                    std::io::stdout().flush().map_err(|e| e.to_string())?;
                    return Ok(());
                }
            }
        }
        seen = count;
        std::io::stdout().flush().map_err(|e| e.to_string())?;
    }
    Err("no guest button click before probe deadline".into())
}
