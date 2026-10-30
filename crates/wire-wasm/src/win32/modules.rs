use super::{call::Call, catalog::ModuleApi, context, ServiceError, Services};
use crate::{
    machine::{Mode, State},
    memory::{Fault, FaultKind, Memory, READ, WRITE},
    pe::{linker, Symbol},
    process::Loaded,
};

pub(super) enum Failure {
    Fault(Fault),
    Windows(u32),
}
impl From<u32> for Failure {
    fn from(value: u32) -> Self {
        Self::Windows(value)
    }
}
impl From<Fault> for Failure {
    fn from(value: Fault) -> Self {
        Self::Fault(value)
    }
}

enum Update {
    Reference { base: u64, refs: u32, pin: bool },
    DisableThread(u64),
}

struct Answer {
    value: u64,
    error: Option<u32>,
    output: Option<(u64, Vec<u8>)>,
    update: Option<Update>,
}
impl Answer {
    fn value(value: u64) -> Self {
        Self {
            value,
            error: None,
            output: None,
            update: None,
        }
    }
}

pub(super) fn string(
    memory: &Memory,
    address: u64,
    wide: bool,
    limit: usize,
) -> Result<Vec<u8>, Failure> {
    let width = if wide { 2 } else { 1 };
    let mut result = Vec::new();
    for n in 0..limit {
        let at = if memory.mode() == Mode::X86 {
            memory
                .mode()
                .address(address.wrapping_add((n * width) as u64))
        } else {
            address.checked_add((n * width) as u64).ok_or(Fault {
                address,
                length: width,
                access: READ,
                kind: FaultKind::Address,
            })?
        };
        let value = memory.read_int(at, width)?;
        if value == 0 {
            return Ok(result);
        }
        result.extend_from_slice(&value.to_le_bytes()[..width]);
    }
    Err(Failure::Windows(206))
}

pub(super) fn name(memory: &Memory, address: u64, wide: bool) -> Result<String, Failure> {
    let bytes = string(memory, address, wide, 32768)?;
    if wide {
        let units: Vec<_> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        String::from_utf16(&units).map_err(|_| Failure::Windows(1113))
    } else {
        crate::codepage::decode_lossy(&bytes, 932).map_err(Failure::Windows)
    }
}

fn handle(
    services: &Services,
    image: &Loaded,
    address: u64,
    wide: bool,
    memory: &Memory,
    files: &linker::Files,
) -> Result<u64, Failure> {
    if address == 0 {
        return image
            .modules
            .modules
            .contains_key(&image.info[0])
            .then_some(image.info[0])
            .ok_or(Failure::Windows(126));
    }
    let requested = name(memory, address, wide)?;
    let guest = image.modules.base_at(&requested, files.cwd());
    let handle = if guest != 0 {
        guest
    } else {
        image
            .search
            .provider(&requested, files)
            .map_err(|_| Failure::Windows(126))?
            .map_or(0, |name| services.provider_handle(&name))
    };
    if handle == 0 {
        return Err(Failure::Windows(126));
    }
    Ok(handle)
}

fn query(
    services: &mut Services,
    id: ModuleApi,
    arguments: &[u64; 16],
    memory: &Memory,
    image: &Loaded,
    files: &linker::Files,
) -> Result<Answer, Failure> {
    let [first, second, third, ..] = *arguments;
    match id {
        ModuleApi::GetModuleHandleA | ModuleApi::GetModuleHandleW => Ok(Answer::value(handle(
            services,
            image,
            first,
            id.wide(),
            memory,
            files,
        )?)),
        ModuleApi::GetModuleHandleExA | ModuleApi::GetModuleHandleExW => {
            memory.check(third, memory.mode() as usize / 8, WRITE)?;
            let outcome = (|| {
                if first & !7 != 0 || first & 3 == 3 {
                    return Err(Failure::Windows(87));
                }
                let base = if first & 4 != 0 {
                    image
                        .modules
                        .modules
                        .values()
                        .find(|m| second >= m.base && second - m.base < u64::from(m.image.size))
                        .map(|m| m.base)
                        .or_else(|| services.containing_provider(second))
                        .ok_or(Failure::Windows(126))?
                } else {
                    handle(services, image, second, id.wide(), memory, files)?
                };
                let update = if let Some(module) = image.modules.modules.get(&base) {
                    if module.phase == linker::Phase::Failed
                        || (module.phase == linker::Phase::Detaching && first & 2 == 0)
                    {
                        return Err(Failure::Windows(1114));
                    }
                    let refs = if first & 2 == 0 {
                        module
                            .references
                            .checked_add(1)
                            .ok_or(Failure::Windows(8))?
                    } else {
                        module.references
                    };
                    Some(Update::Reference {
                        base,
                        refs,
                        pin: first & 1 != 0,
                    })
                } else {
                    None
                };
                Ok((base, update))
            })();
            let mut answer = Answer::value(0);
            match outcome {
                Ok((base, update)) => {
                    answer.value = 1;
                    answer.output = Some((
                        third,
                        base.to_le_bytes()[..memory.mode() as usize / 8].to_vec(),
                    ));
                    answer.update = update;
                }
                Err(Failure::Windows(code)) => {
                    answer.error = Some(code);
                    if code == 126 {
                        answer.output = Some((third, vec![0; memory.mode() as usize / 8]));
                    }
                }
                Err(error) => return Err(error),
            }
            Ok(answer)
        }
        ModuleApi::DisableThreadLibraryCalls => {
            if services.provider_name(first).is_some() {
                return Ok(Answer::value(1));
            }
            let module = image
                .modules
                .modules
                .get(&first)
                .ok_or(Failure::Windows(126))?;
            if module.phase == linker::Phase::Failed {
                return Err(Failure::Windows(1114));
            }
            if module.tls.is_some() {
                return Err(Failure::Windows(126));
            }
            let mut answer = Answer::value(1);
            answer.update = Some(Update::DisableThread(first));
            Ok(answer)
        }
        ModuleApi::GetModuleFileNameA | ModuleApi::GetModuleFileNameW => {
            let base = if first == 0 { image.info[0] } else { first };
            let path = if let Some(module) = image.modules.modules.values().find(|m| m.base == base)
            {
                module.path.clone()
            } else if let Some(dll) = services.provider_name(base) {
                format!("C:\\Windows\\System32\\{dll}")
            } else {
                return Err(Failure::Windows(6));
            };
            let wide = id.wide();
            let width = if wide { 2 } else { 1 };
            let mut bytes = if wide {
                path.encode_utf16().flat_map(u16::to_le_bytes).collect()
            } else {
                crate::codepage::encode(&path, 932)
                    .map_err(Failure::Windows)?
                    .0
            };
            let length = bytes.len() / width;
            let capacity = third as u32 as usize;
            let copied = length.min(capacity.saturating_sub(1));
            let truncated = capacity <= length;
            let mut answer = Answer::value(if truncated {
                capacity as u64
            } else {
                length as u64
            });
            answer.error = truncated.then_some(122);
            if capacity != 0 {
                bytes.truncate(copied * width);
                bytes.resize((copied + 1) * width, 0);
                answer.output = Some((second, bytes));
            }
            Ok(answer)
        }
        _ => unreachable!(),
    }
}

pub(super) fn dispatch(
    services: &mut Services,
    id: ModuleApi,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
    files: &linker::Files,
) -> Result<u64, ServiceError> {
    let image = image.ok_or(ServiceError::Unavailable(
        "module queries require a loaded image",
    ))?;
    let answer = match query(services, id, &call.arguments, memory, image, files) {
        Ok(answer) => answer,
        Err(Failure::Fault(fault)) => return Err(ServiceError::Fault(fault)),
        Err(Failure::Windows(code)) => Answer {
            value: 0,
            error: Some(code),
            output: None,
            update: None,
        },
    };
    let error_at = context::error_address(state, memory.mode());
    if let Some((address, bytes)) = &answer.output {
        memory.check(*address, bytes.len(), WRITE)?;
    }
    if answer.error.is_some() {
        memory.check(error_at, 4, WRITE)?;
    }
    if let Some((address, bytes)) = answer.output {
        memory.write(address, &bytes)?;
    }
    if let Some(code) = answer.error {
        memory.write_int(error_at, 4, u64::from(code))?;
    }
    if let Some(update) = answer.update {
        match update {
            Update::Reference { base, refs, pin } => {
                let module = image.modules.modules.get_mut(&base).unwrap();
                module.references = refs;
                module.pinned |= pin;
            }
            Update::DisableThread(base) => {
                image
                    .modules
                    .modules
                    .get_mut(&base)
                    .unwrap()
                    .thread_calls_disabled = true
            }
        }
    }
    Ok(answer.value)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn control(
    services: &mut Services,
    id: ModuleApi,
    call: Call,
    state: &mut State,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
    files: &linker::Files,
) -> Result<(), ServiceError> {
    use crate::process::LoadError;
    let image = image.ok_or(ServiceError::Unavailable(
        "module operations require a loaded image",
    ))?;
    let result = if id == ModuleApi::GetProcAddress {
        (|| {
            let base = call.arguments[0];
            if services.provider_name(base).is_none() && !image.modules.modules.contains_key(&base)
            {
                return Err(LoadError::Windows(6));
            }
            let symbol = if call.arguments[1] <= 65535 {
                Symbol::Ordinal(call.arguments[1] as u16)
            } else {
                let bytes =
                    string(memory, call.arguments[1], false, 4096).map_err(|e| match e {
                        Failure::Fault(f) => LoadError::Fault(f),
                        Failure::Windows(c) => LoadError::Windows(c),
                    })?;
                if bytes.is_empty() || !bytes.is_ascii() {
                    return Err(LoadError::Windows(127));
                }
                Symbol::Name(String::from_utf8(bytes).unwrap())
            };
            match image
                .lookup_export(services, memory, base, symbol.clone(), false)
                .map_err(LoadError::Windows)?
            {
                crate::process::Lookup::Address { target, edges } => {
                    image.remember_exports(edges);
                    call.clone().complete(state, target);
                    Ok(())
                }
                crate::process::Lookup::Missing { .. } => {
                    image.begin_resolution(call.clone(), base, symbol, state, memory.mode())
                }
            }
        })()
    } else if id == ModuleApi::FreeLibrary {
        image.free_library(call.arguments[0], call.clone(), state, memory, services)
    } else {
        (|| {
            if call.arguments[0] == 0 {
                return Err(LoadError::Windows(87));
            }
            if id.extended() {
                if call.arguments[1] != 0 {
                    return Err(LoadError::Windows(87));
                }
                if call.arguments[2] & !0x1f08 != 0 {
                    return Err(LoadError::Windows(50));
                }
            }
            let requested =
                name(memory, call.arguments[0], id.wide()).map_err(|error| match error {
                    Failure::Fault(fault) => LoadError::Fault(fault),
                    Failure::Windows(code) => LoadError::Windows(code),
                })?;
            let flags = if id.extended() {
                call.arguments[2] as u32
            } else {
                0
            };
            if flags & 8 != 0 && flags != 8
                || flags & (8 | 0x100) != 0 && !crate::pe::search::absolute(&requested)
            {
                return Err(LoadError::Windows(87));
            }
            image.load_library(
                &requested,
                flags,
                call.clone(),
                state,
                memory,
                services,
                files,
            )
        })()
    };
    match result {
        Ok(()) => Ok(()),
        Err(LoadError::Fault(fault)) => Err(ServiceError::Fault(fault)),
        Err(LoadError::Windows(code)) => {
            if let Some(error) = super::file_input::suspend(files, Some(image), &call, state.ip) {
                return Err(error);
            }
            memory.write_int(
                context::error_address(state, memory.mode()),
                4,
                u64::from(code),
            )?;
            call.complete(state, 0);
            Ok(())
        }
    }
}
