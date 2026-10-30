//! Virtual security identities share token/SID owners; no host credentials are used.
use super::{
    call::Call,
    catalog::SecurityApi as S,
    heaps,
    modules::{self, Failure},
    sid_values, user, ServiceError,
};
use crate::{
    machine::State,
    memory::{Memory, WRITE},
    process::Loaded,
};

fn heap_error(
    result: Result<u64, heaps::Error>,
    state: &State,
    memory: &mut Memory,
) -> Result<u64, ServiceError> {
    match result {
        Ok(v) => Ok(v),
        Err(heaps::Error::Fault(f)) => Err(f.into()),
        Err(heaps::Error::Windows(code)) => user::error(memory, state, code),
        Err(heaps::Error::Backing) => Err(ServiceError::Unavailable(
            "security allocation backing unavailable",
        )),
    }
}
fn lookup(a: &[u64; 16], memory: &mut Memory, image: &Loaded) -> Result<u64, Failure> {
    if a[0] != 0 {
        let system = modules::name(memory, a[0], true)?;
        if !system.is_empty()
            && system != "."
            && !system.eq_ignore_ascii_case(image.environment.profile.computer_name())
        {
            return Err(Failure::Windows(50));
        }
    }
    if a[1] == 0 {
        return Err(Failure::Windows(1337));
    }
    let mut header = [0; 8];
    memory.read(a[1], &mut header)?;
    if !sid_values::valid(&header) {
        return Err(Failure::Windows(1337));
    }
    let mut sid = vec![0; sid_values::length(header[1])];
    memory.read(a[1], &mut sid)?;
    let mut expected = sid_values::header(5, [0, 0, 0, 0, 0, 5]).to_vec();
    for n in [21u32, 1, 1, 1, 1000] {
        expected.extend(n.to_le_bytes());
    }
    if sid != expected {
        return Err(Failure::Windows(1332));
    }
    let texts = [
        image.environment.profile.user_name(),
        image.environment.profile.computer_name(),
    ];
    let encoded: Vec<Vec<u8>> = texts
        .iter()
        .map(|s| {
            s.encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect()
        })
        .collect();
    let sizes = [a[3], a[5]];
    let outputs = [a[2], a[4]];
    let capacities = [memory.read_int(sizes[0], 4)?, memory.read_int(sizes[1], 4)?];
    for p in sizes {
        memory.check(p, 4, WRITE)?;
    }
    if capacities
        .iter()
        .zip(&encoded)
        .any(|(&capacity, bytes)| capacity < (bytes.len() / 2) as u64)
    {
        return Err(Failure::Windows(122));
    }
    memory.check(a[6], 4, WRITE)?;
    for (p, bytes) in outputs.iter().zip(&encoded) {
        memory.check(*p, bytes.len(), WRITE)?;
    }
    for (p, bytes) in outputs.iter().zip(&encoded) {
        memory.write(*p, bytes)?;
    }
    for (p, bytes) in sizes.iter().zip(&encoded) {
        memory.write_int(*p, 4, (bytes.len() / 2 - 1) as u64)?;
    }
    memory.write_int(a[6], 4, 1)?;
    Ok(1)
}
pub(super) fn dispatch(
    id: S,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
) -> Result<u64, ServiceError> {
    let a = &call.arguments;
    let width = memory.mode() as usize / 8;
    if matches!(
        id,
        S::EffectiveRights | S::GetNamed | S::SetNamed | S::SetEntries
    ) {
        return Ok(50);
    }
    if id == S::LogonProcess {
        return user::error(memory, state, 50);
    }
    if id == S::Trustee {
        let prefix = if width == 4 { 16 } else { 20 };
        memory.check(a[0], if width == 4 { 20 } else { 32 }, WRITE)?;
        let tail = a[0] + if width == 4 { 16 } else { 24 };
        memory.write(a[0], &[0; 20][..prefix])?;
        memory.write_int(tail, width, a[1])?;
        return Ok(0);
    }
    let image = image.ok_or(ServiceError::Unavailable(
        "security services require a loaded image",
    ))?;
    if id == S::AllocateSid {
        let count = a[1] as u8;
        if count > 8 {
            return user::error(memory, state, 1337);
        }
        memory.check(a[10], width, WRITE)?;
        let mut authority = [0; 6];
        memory.read(a[0], &mut authority)?;
        let mut bytes = sid_values::header(count, authority).to_vec();
        for n in 0..count as usize {
            bytes.extend((a[2 + n] as u32).to_le_bytes());
        }
        let pointer = heap_error(
            image
                .heaps
                .allocate_local(memory, bytes.len() as u64, &bytes),
            state,
            memory,
        )?;
        if pointer == 0 {
            return Ok(0);
        }
        memory.write_int(a[10], width, pointer)?;
        return Ok(1);
    }
    if id == S::FreeSid {
        if a[0] != 0 {
            super::com::allocation(image.heaps.free_process(memory, a[0]).map(|()| 0))?;
        }
        return Ok(0);
    }
    let result = (|| -> Result<u64, Failure> {
        match id {
            S::DuplicateToken => {
                image.tokens.check(a[0], 2).map_err(Failure::Windows)?;
                if a[1] > 3 {
                    return Err(Failure::Windows(87));
                }
                let h = image.tokens.prepare(12).map_err(Failure::Windows)?;
                memory.write_int(a[2], width, h)?;
                image.tokens.commit_level(h, 12, false, Some(a[1] as u32));
                Ok(1)
            }
            S::Impersonate => {
                let level = image.tokens.level(a[0]).map_err(Failure::Windows)?;
                image
                    .tokens
                    .check(a[0], if level.is_some() { 12 } else { 10 })
                    .map_err(Failure::Windows)?;
                if level.is_some_and(|v| v < 2) {
                    return Err(Failure::Windows(1346));
                }
                image.threads.current_mut().impersonation = Some(level.unwrap_or(2));
                Ok(1)
            }
            S::Revert => {
                image.threads.current_mut().impersonation = None;
                Ok(1)
            }
            S::LookupSid => lookup(a, memory, image),
            _ => unreachable!(),
        }
    })();
    match result {
        Ok(v) => Ok(v),
        Err(Failure::Fault(f)) => Err(f.into()),
        Err(Failure::Windows(code)) => {
            if id == S::LookupSid && code == 122 {
                memory.check(a[3], 4, WRITE)?;
                memory.check(a[5], 4, WRITE)?;
                memory.check(
                    super::context::error_address(state, memory.mode()),
                    4,
                    WRITE,
                )?;
                memory.write_int(
                    a[3],
                    4,
                    (image.environment.profile.user_name().encode_utf16().count() + 1) as u64,
                )?;
                memory.write_int(
                    a[5],
                    4,
                    (image
                        .environment
                        .profile
                        .computer_name()
                        .encode_utf16()
                        .count()
                        + 1) as u64,
                )?;
            }
            user::error(memory, state, code)
        }
    }
}
