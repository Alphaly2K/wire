//! Character transfers share conversion tables and copy inputs before publishing.
use super::*;

pub(super) fn implemented(id: ApiId) -> bool {
    matches!(
        id,
        ApiId::Nls(
            N::GetStringTypeA
                | N::GetStringTypeW
                | N::GetStringTypeExA
                | N::GetStringTypeExW
                | N::LcMapStringA
                | N::LcMapStringW
                | N::LcMapStringEx
        )
    )
}
fn input(memory: &Memory, pointer: u64, count: u64, width: usize) -> Result<Vec<u8>, Failure> {
    read(
        memory,
        pointer,
        if (count as u32 as i32) < 0 {
            u64::MAX
        } else {
            count
        },
        width,
    )
}
fn classify(id: N, a: &[u64; 16], memory: &mut Memory) -> Result<u64, Failure> {
    let shift = usize::from(id != N::GetStringTypeW);
    if a[shift + 3] == 0 {
        return Err(Failure::Windows(87));
    }
    if a[shift] as u32 != 1 {
        return Err(Failure::Windows(if matches!(a[shift] as u32, 2 | 4) {
            50
        } else {
            1004
        }));
    }
    let narrow = matches!(id, N::GetStringTypeA | N::GetStringTypeExA);
    let cp = if narrow {
        ansi_codepage(lcid(a[0] as u32)?, false)
    } else {
        0
    };
    let bytes = input(
        memory,
        a[shift + 1],
        a[shift + 2],
        if narrow { 1 } else { 2 },
    )?;
    let bytes = if narrow {
        utf16_bytes(&crate::codepage::decode_lossy(&bytes, cp).map_err(Failure::Windows)?)
    } else {
        bytes
    };
    let output: Vec<_> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .flat_map(|b| locale::ctype1(u16::from_le_bytes(*b)).to_le_bytes())
        .collect();
    memory.write(a[shift + 3], &output)?;
    Ok(1)
}

fn case(bytes: &[u8], upper: bool) -> Vec<u8> {
    let mut output = Vec::new();
    let mut run = String::new();
    let flush = |run: &mut String, output: &mut Vec<u8>| {
        output.extend(utf16_bytes(&if upper {
            run.to_uppercase()
        } else {
            run.to_lowercase()
        }));
        run.clear();
    };
    for scalar in char::decode_utf16(
        bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes(*b)),
    ) {
        match scalar {
            Ok(ch) => run.push(ch),
            Err(error) => {
                flush(&mut run, &mut output);
                output.extend_from_slice(&error.unpaired_surrogate().to_le_bytes());
            }
        }
    }
    flush(&mut run, &mut output);
    output
}

fn mapping(id: N, a: &[u64; 16], memory: &mut Memory) -> Result<u64, Failure> {
    if (a[5] as u32 as i32) < 0 || a[5] != 0 && a[4] == 0 {
        return Err(Failure::Windows(87));
    }
    let locale = if id == N::LcMapStringEx {
        let id = locale_id(memory, a[0])?;
        if a[6..9].iter().any(|&p| p != 0) {
            return Err(Failure::Windows(50));
        }
        id
    } else {
        lcid(a[0] as u32)?
    };
    let flags = a[1] as u32;
    let operation = flags & 0x300;
    let known = 0x60000000 | 0x0ff00000 | 0x00071f3f;
    if flags & !known != 0 {
        return Err(Failure::Windows(1004));
    }
    if flags & !0x40000300 != 0 || operation == 0x300 {
        return Err(Failure::Windows(50));
    }
    if operation == 0 {
        return Err(Failure::Windows(1004));
    }
    let narrow = id == N::LcMapStringA;
    let cp = ansi_codepage(locale, flags & 0x40000000 != 0);
    let bytes = input(memory, a[2], a[3], if narrow { 1 } else { 2 })?;
    let bytes = if narrow {
        utf16_bytes(&crate::codepage::decode_lossy(&bytes, cp).map_err(Failure::Windows)?)
    } else {
        bytes
    };
    let mapped = case(&bytes, operation == 0x200);
    let mapped = if narrow {
        crate::codepage::encode(&decode_utf16(&mapped, false).map_err(Failure::Windows)?, cp)
            .map_err(Failure::Windows)?
            .0
    } else {
        mapped
    };
    write(memory, a[4], a[5], &mapped, if narrow { 1 } else { 2 })
}
pub(super) fn call(id: N, a: &[u64; 16], memory: &mut Memory) -> Result<u64, Failure> {
    match id {
        N::LcMapStringA | N::LcMapStringW | N::LcMapStringEx => mapping(id, a, memory),
        _ => classify(id, a, memory),
    }
}
