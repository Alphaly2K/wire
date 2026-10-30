//! Windows CONTEXT images at the guest exception/callback boundary.
use super::*;

const X86_REGS: [usize; 8] = [176, 172, 168, 164, 196, 180, 160, 156];
const USER_FLAGS: u64 = 0x0025_0dd5;
pub(super) fn size(mode: Mode) -> usize {
    if mode == Mode::X86 {
        716
    } else {
        1232
    }
}
fn put(data: &mut [u8], at: usize, width: usize, value: u64) {
    data[at..at + width].copy_from_slice(&value.to_le_bytes()[..width]);
}
fn get(data: &[u8], at: usize, width: usize) -> u64 {
    let mut bytes = [0; 8];
    bytes[..width].copy_from_slice(&data[at..at + width]);
    u64::from_le_bytes(bytes)
}
fn fx_capture(state: &State, mode: Mode, data: &mut [u8]) {
    put(data, 0, 2, u64::from(state.fpu.cw));
    put(data, 2, 2, u64::from(state.fpu.sw));
    for n in 0..8 {
        if state.fpu.tags >> (n * 2) & 3 != 3 {
            data[4] |= 1 << n;
        }
        data[32 + n * 16..42 + n * 16].copy_from_slice(&state.fpu.st_bits(n).to_le_bytes()[..10]);
    }
    put(data, 6, 2, u64::from(state.fpu.opcode));
    put(data, 8, if mode == Mode::X86 { 4 } else { 8 }, state.fpu.ip);
    put(
        data,
        16,
        if mode == Mode::X86 { 4 } else { 8 },
        state.fpu.dp,
    );
    put(data, 24, 4, u64::from(state.mxcsr));
    put(data, 28, 4, 0xffff);
    for n in 0..if mode == Mode::X86 { 8 } else { 16 } {
        data[160 + n * 16..176 + n * 16].copy_from_slice(&state.xmm[n]);
    }
}
pub(super) fn capture(state: &State, mode: Mode) -> Vec<u8> {
    let mut data = vec![0; size(mode)];
    if mode == Mode::X86 {
        put(&mut data, 0, 4, 0x1002f);
        for (n, at) in X86_REGS.into_iter().enumerate() {
            put(&mut data, at, 4, state.regs[n]);
        }
        for (at, value) in [
            (28, u64::from(state.fpu.cw)),
            (32, u64::from(state.fpu.sw)),
            (36, u64::from(state.fpu.tags)),
            (40, state.fpu.ip),
            (44, u64::from(state.fpu.opcode) << 16),
            (48, state.fpu.dp),
            (144, 0x53),
            (148, 0x23),
            (152, 0x23),
            (184, state.ip),
            (188, 0x1b),
            (192, state.flags),
            (200, 0x23),
        ] {
            put(&mut data, at, 4, value);
        }
        for n in 0..8 {
            data[56 + n * 10..66 + n * 10]
                .copy_from_slice(&state.fpu.st_bits(n).to_le_bytes()[..10]);
        }
        fx_capture(state, mode, &mut data[204..]);
    } else {
        put(&mut data, 48, 4, 0x10000f);
        put(&mut data, 52, 4, u64::from(state.mxcsr));
        for (n, value) in [0x33, 0x2b, 0x2b, 0x53, 0x2b, 0x2b].into_iter().enumerate() {
            put(&mut data, 56 + n * 2, 2, value);
        }
        put(&mut data, 68, 4, state.flags);
        for (n, value) in state.regs.into_iter().enumerate() {
            put(&mut data, 120 + n * 8, 8, value);
        }
        put(&mut data, 248, 8, state.ip);
        fx_capture(state, mode, &mut data[256..768]);
    }
    data
}

pub(super) fn components(mode: Mode, flags: u32) -> Result<(), u32> {
    let architecture = if mode == Mode::X86 { 0x10000 } else { 0x100000 };
    let supported = if mode == Mode::X86 { 0x2f } else { 0xf };
    if flags & architecture == 0 {
        return Err(87);
    }
    if flags & !(architecture | supported) != 0 {
        return Err(50);
    }
    Ok(())
}

pub(super) fn select(state: &State, mode: Mode, flags: u32, data: &mut [u8]) {
    let captured = capture(state, mode);
    let ranges: &[(u32, usize, usize)] = if mode == Mode::X86 {
        &[
            (1, 180, 204),
            (2, 156, 180),
            (4, 140, 156),
            (8, 28, 140),
            (0x20, 204, 716),
        ]
    } else {
        &[
            (1, 56, 58),
            (1, 66, 72),
            (1, 152, 160),
            (1, 248, 256),
            (2, 120, 152),
            (2, 160, 248),
            (4, 58, 66),
            (8, 52, 56),
            (8, 256, 768),
        ]
    };
    for &(bit, start, end) in ranges {
        if flags & bit != 0 {
            data[start..end].copy_from_slice(&captured[start..end]);
        }
    }
    put(
        data,
        if mode == Mode::X86 { 0 } else { 48 },
        4,
        u64::from(flags),
    );
}

pub(crate) fn capture_call(
    call: &crate::win32::call::Call,
    state: &State,
    memory: &mut Memory,
) -> Result<u64, Fault> {
    let mode = memory.mode();
    if mode == Mode::X64 && call.arguments[0] & 15 != 0 {
        return Err(Fault {
            address: call.arguments[0],
            length: size(mode),
            access: WRITE,
            kind: crate::memory::FaultKind::Width,
        });
    }
    let mut caller = state.clone();
    call.clone().complete(&mut caller, state.regs[0]);
    let mut data = vec![0; size(mode)];
    memory.read(call.arguments[0], &mut data)?;
    select(
        &caller,
        mode,
        if mode == Mode::X86 { 0x10007 } else { 0x10000f },
        &mut data,
    );
    memory.write(call.arguments[0], &data)?;
    Ok(state.regs[0])
}
fn normalize(state: &mut State) {
    state.fpu.cw = (state.fpu.cw & 0x1f3f) | 0x40;
    if state.fpu.sw & !state.fpu.cw & 0x3f != 0 {
        state.fpu.sw |= 0x8080;
    } else {
        state.fpu.sw &= !0x8080;
    }
}
fn fx_restore(state: &mut State, mode: Mode, data: &[u8]) -> Result<(), StartupError> {
    let mxcsr = get(data, 24, 4) as u32;
    if mxcsr & !0xffff != 0 {
        return Err(StartupError::Invalid(
            "invalid exception MXCSR state".into(),
        ));
    }
    state.mxcsr = mxcsr;
    state.fpu.cw = get(data, 0, 2) as u16;
    state.fpu.sw = get(data, 2, 2) as u16;
    state.fpu.opcode = get(data, 6, 2) as u16 & 0x7ff;
    state.fpu.ip = get(data, 8, if mode == Mode::X86 { 4 } else { 8 });
    state.fpu.dp = get(data, 16, if mode == Mode::X86 { 4 } else { 8 });
    for n in 0..8 {
        let mut bytes = [0; 16];
        bytes[..10].copy_from_slice(&data[32 + n * 16..42 + n * 16]);
        let bits = u128::from_le_bytes(bytes);
        let physical = (state.fpu.top() + n) & 7;
        state.fpu.regs[physical] = bits;
        let exp = (bits >> 64) as u16 & 0x7fff;
        let fraction = bits as u64;
        let tag = if data[4] & (1 << physical) == 0 {
            3
        } else if exp == 0 && fraction == 0 {
            1
        } else if exp == 0 || exp == 0x7fff || fraction >> 63 == 0 {
            2
        } else {
            0
        };
        state.fpu.tags = (state.fpu.tags & !(3 << (physical * 2))) | (tag << (physical * 2));
    }
    for n in 0..if mode == Mode::X86 { 8 } else { 16 } {
        state.xmm[n].copy_from_slice(&data[160 + n * 16..176 + n * 16]);
    }
    normalize(state);
    Ok(())
}
pub(super) fn restore(memory: &Memory, pointer: u64, saved: &State) -> Result<State, StartupError> {
    let mode = memory.mode();
    let mut data = vec![0; size(mode)];
    memory.read(pointer, &mut data)?;
    restore_data(mode, &data, saved)
}

pub(super) fn restore_thread(
    memory: &Memory,
    pointer: u64,
    saved: &State,
) -> Result<State, StartupError> {
    let mode = memory.mode();
    let mut data = vec![0; size(mode)];
    memory.read(pointer, &mut data)?;
    let flags = get(&data, if mode == Mode::X86 { 0 } else { 48 }, 4) as u32;
    if flags & 1 != 0 {
        let at = if mode == Mode::X86 { 192 } else { 68 };
        let user = get(&data, at, 4);
        put(
            &mut data,
            at,
            4,
            (saved.flags & !USER_FLAGS) | (user & USER_FLAGS),
        );
        if mode == Mode::X86 {
            put(&mut data, 188, 4, 0x1b);
            put(&mut data, 200, 4, 0x23);
        }
    }
    if flags & 4 != 0 {
        if mode == Mode::X86 {
            for (at, value) in [(140, 0), (144, 0x53), (148, 0x23), (152, 0x23)] {
                put(&mut data, at, 4, value);
            }
        } else {
            for (n, value) in [0x33, 0x2b, 0x2b, 0x53, 0x2b, 0x2b].into_iter().enumerate() {
                put(&mut data, 56 + n * 2, 2, value);
            }
        }
    }
    restore_data(mode, &data, saved)
}

fn restore_data(mode: Mode, data: &[u8], saved: &State) -> Result<State, StartupError> {
    let flags = get(data, if mode == Mode::X86 { 0 } else { 48 }, 4) as u32;
    let architecture = if mode == Mode::X86 { 0x10000 } else { 0x100000 };
    let supported = if mode == Mode::X86 { 0x2f } else { 0xf };
    if flags & architecture == 0 || flags & !(architecture | supported) != 0 {
        return Err(StartupError::Invalid(
            "unsupported exception CONTEXT components".into(),
        ));
    }
    let mut state = saved.clone();
    if flags & 1 != 0 {
        if mode == Mode::X86 && (get(data, 188, 4) != 0x1b || get(data, 200, 4) != 0x23) {
            return Err(StartupError::Invalid(
                "unsupported exception control selectors".into(),
            ));
        }
        let (ip, sp, bp, user) = if mode == Mode::X86 {
            (
                get(data, 184, 4),
                get(data, 196, 4),
                get(data, 180, 4),
                get(data, 192, 4),
            )
        } else {
            (
                get(data, 248, 8),
                get(data, 152, 8),
                state.regs[5],
                get(data, 68, 4),
            )
        };
        if mode == Mode::X64 && (ip >= 1 << 47 || sp >= 1 << 47) {
            return Err(StartupError::Invalid(
                "noncanonical exception continuation".into(),
            ));
        }
        if (state.flags ^ user) & !USER_FLAGS != 0 {
            return Err(StartupError::Invalid(
                "unsupported privileged exception FLAGS change".into(),
            ));
        }
        state.ip = ip;
        state.regs[4] = sp;
        state.regs[5] = bp;
        state.flags = (state.flags & !USER_FLAGS) | (user & USER_FLAGS);
    }
    if flags & 2 != 0 {
        for (n, register) in state
            .regs
            .iter_mut()
            .enumerate()
            .take(if mode == Mode::X86 { 8 } else { 16 })
        {
            if n != 4 && (mode == Mode::X64 || n != 5) {
                *register = get(
                    data,
                    if mode == Mode::X86 {
                        X86_REGS[n]
                    } else {
                        120 + n * 8
                    },
                    if mode == Mode::X86 { 4 } else { 8 },
                );
            }
        }
    }
    if flags & 4 != 0 {
        let valid = if mode == Mode::X86 {
            [(140, 0), (144, 0x53), (148, 0x23), (152, 0x23)]
                .iter()
                .all(|&(at, val)| get(data, at, 4) == val)
        } else {
            [0x33, 0x2b, 0x2b, 0x53, 0x2b, 0x2b]
                .iter()
                .enumerate()
                .all(|(n, &val)| get(data, 56 + n * 2, 2) == val)
        };
        if !valid {
            return Err(StartupError::Invalid(
                "unsupported exception segment selectors".into(),
            ));
        }
    }
    if mode == Mode::X86 && flags & 0x20 != 0 {
        fx_restore(&mut state, mode, &data[204..])?;
    } else if mode == Mode::X86 && flags & 8 != 0 {
        state.fpu.cw = get(data, 28, 2) as u16;
        state.fpu.sw = get(data, 32, 2) as u16;
        state.fpu.tags = get(data, 36, 2) as u16;
        state.fpu.ip = get(data, 40, 4);
        state.fpu.dp = get(data, 48, 4);
        state.fpu.opcode = get(data, 46, 2) as u16 & 0x7ff;
        for n in 0..8 {
            let mut bytes = [0; 16];
            bytes[..10].copy_from_slice(&data[56 + n * 10..66 + n * 10]);
            state.fpu.regs[(state.fpu.top() + n) & 7] = u128::from_le_bytes(bytes);
        }
        normalize(&mut state);
    } else if mode == Mode::X64 && flags & 8 != 0 {
        if get(data, 52, 4) != get(data, 280, 4) {
            return Err(StartupError::Invalid(
                "inconsistent exception MXCSR copies".into(),
            ));
        }
        fx_restore(&mut state, mode, &data[256..768])?;
    }
    state.exception = 0;
    state.cancel_repeat();
    Ok(state)
}
