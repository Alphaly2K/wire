//! One variable model owns the PEB mirror and separately issued block snapshots.
use super::{
    call::Call,
    catalog::EnvironmentApi as E,
    context,
    modules::{name, Failure},
    ServiceError,
};
use crate::{
    environment::{EnvironmentVariables, GuestEnvironment},
    machine::{Mode, State},
    memory::{Memory, PAGE, WRITE},
};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
struct Block {
    base: u64,
    span: u64,
}
pub(crate) struct Environment {
    variables: EnvironmentVariables,
    current: Option<Block>,
    snapshots: BTreeMap<u64, (u64, bool)>,
    snapshot_bytes: u64,
    parameter: u64,
    pub(crate) profile: GuestEnvironment,
}

fn allocate(memory: &mut Memory, bytes: &[u8]) -> Result<Block, u32> {
    let span = (bytes.len() as u64).div_ceil(PAGE) * PAGE;
    let base = memory.map_heap(span, false)?;
    memory.write(base, bytes).expect("fresh environment block");
    Ok(Block { base, span })
}
fn mirror(memory: &Memory, parameter: u64) -> (u64, u64, usize) {
    if memory.mode() == Mode::X86 {
        (parameter + 0x48, parameter + 0x290, 4)
    } else {
        (parameter + 0x80, parameter + 0x3f0, 8)
    }
}
impl Environment {
    pub(crate) fn variables(&self) -> EnvironmentVariables {
        self.variables.clone()
    }

    pub(super) fn inherit(
        &mut self,
        memory: &mut Memory,
        variables: EnvironmentVariables,
    ) -> Result<(), Failure> {
        self.replace(memory, variables)
    }
    pub(crate) fn new(
        memory: &mut Memory,
        existing: &Memory,
        parameter: u64,
    ) -> Result<Self, String> {
        let profile = if memory.mode() == Mode::X86 {
            GuestEnvironment::pe32()
        } else {
            GuestEnvironment::amd64()
        };
        let mut variables = EnvironmentVariables::default();
        for (name, value) in [
            ("SystemRoot", profile.windows_directory()),
            ("windir", profile.windows_directory()),
            ("SystemDrive", "C:"),
            ("TEMP", profile.temporary_directory()),
            ("TMP", profile.temporary_directory()),
            ("USERPROFILE", profile.home()),
            ("LOCALAPPDATA", profile.local()),
            ("APPDATA", profile.roaming()),
            ("USERNAME", profile.user_name()),
            ("COMPUTERNAME", profile.computer_name()),
            ("OS", "Windows_NT"),
            ("NUMBER_OF_PROCESSORS", "1"),
            (
                "PROCESSOR_ARCHITECTURE",
                if memory.mode() == Mode::X86 {
                    "x86"
                } else {
                    "AMD64"
                },
            ),
        ] {
            variables.set(name.into(), value.into());
        }
        variables.set(
            "PATH".into(),
            format!(
                "{};{}",
                profile.system_directory(),
                profile.windows_directory()
            ),
        );
        let bytes = variables.wide_block();
        let span = (bytes.len() as u64).div_ceil(PAGE) * PAGE;
        let base = crate::process::allocate_process_data(memory, existing, span, &bytes)?;
        let block = Block { base, span };
        let (pointer, size, width) = mirror(memory, parameter);
        memory.write_int(pointer, width, block.base)?;
        memory.write_int(size, width, bytes.len() as u64)?;
        Ok(Self {
            variables,
            current: Some(block),
            snapshots: BTreeMap::new(),
            snapshot_bytes: 0,
            parameter,
            profile,
        })
    }
    pub(crate) fn get(&self, name: &str) -> Option<&str> {
        self.variables.get(name)
    }
    pub(crate) fn check_clear(&self, memory: &Memory) -> Result<(), String> {
        if let Some(b) = self.current {
            memory.check_unmap(b.base, b.span)?;
        }
        for (&p, &(span, _)) in &self.snapshots {
            memory.check_unmap(p, span)?;
        }
        Ok(())
    }
    pub(crate) fn clear(&mut self, memory: &mut Memory) {
        for (p, (span, _)) in std::mem::take(&mut self.snapshots) {
            memory
                .unmap(p, span)
                .expect("preflighted environment snapshot cleanup");
        }
        if let Some(b) = self.current.take() {
            memory
                .unmap(b.base, b.span)
                .expect("preflighted environment cleanup");
        }
        self.variables = EnvironmentVariables::default();
        self.snapshot_bytes = 0;
    }
    fn replace(
        &mut self,
        memory: &mut Memory,
        variables: EnvironmentVariables,
    ) -> Result<(), Failure> {
        let bytes = variables.wide_block();
        if variables.len() > 1024 || bytes.len() > 2 * 1024 * 1024 {
            return Err(Failure::Windows(8));
        }
        let old = self.current.ok_or(Failure::Windows(87))?;
        let (pointer, size, width) = mirror(memory, self.parameter);
        memory.check(pointer, width, WRITE)?;
        memory.check(size, width, WRITE)?;
        memory
            .check_unmap(old.base, old.span)
            .map_err(|_| Failure::Windows(87))?;
        let block = allocate(memory, &bytes).map_err(Failure::Windows)?;
        // All remaining writes and retirement were preflighted before allocation.
        memory
            .write_int(pointer, width, block.base)
            .expect("preflighted environment pointer");
        memory
            .write_int(size, width, bytes.len() as u64)
            .expect("preflighted environment size");
        memory
            .unmap(old.base, old.span)
            .expect("preflighted environment replacement");
        self.current = Some(block);
        self.variables = variables;
        Ok(())
    }
    fn call(&mut self, id: E, call: &Call, memory: &mut Memory) -> Result<u64, Failure> {
        let a = &call.arguments;
        match id {
            E::ExpandA | E::ExpandW => {
                if a[0] == 0 || a[0] == a[1] && a[2] != 0 {
                    return Err(Failure::Windows(87));
                }
                let input = name(memory, a[0], id.wide())?;
                let mut output = String::new();
                let mut rest = input.as_str();
                let append = |output: &mut String, text: &str| -> Result<(), Failure> {
                    if output
                        .len()
                        .checked_add(text.len())
                        .is_none_or(|n| n > 16 * 1024 * 1024)
                    {
                        return Err(Failure::Windows(8));
                    }
                    output.push_str(text);
                    Ok(())
                };
                while let Some(start) = rest.find('%') {
                    append(&mut output, &rest[..start])?;
                    rest = &rest[start..];
                    let Some(end) = rest[1..].find('%').map(|n| n + 1) else {
                        break;
                    };
                    append(
                        &mut output,
                        self.variables.get(&rest[1..end]).unwrap_or(&rest[..=end]),
                    )?;
                    rest = &rest[end + 1..];
                }
                append(&mut output, rest)?;
                let bytes: Vec<_> = if id.wide() {
                    output
                        .encode_utf16()
                        .chain(Some(0))
                        .flat_map(u16::to_le_bytes)
                        .collect()
                } else {
                    crate::codepage::encode(&(output + "\0"), self.profile.codepage())
                        .map_err(Failure::Windows)?
                        .0
                };
                if bytes.len() > 16 * 1024 * 1024 {
                    return Err(Failure::Windows(8));
                }
                let required = bytes.len() / if id.wide() { 2 } else { 1 };
                if a[2] >= required as u64 {
                    memory.write(a[1], &bytes)?;
                }
                Ok(required as u64)
            }
            E::Block | E::BlockA | E::BlockW => {
                let wide = id == E::BlockW;
                let bytes = self.variables.wide_block();
                let bytes = if wide {
                    bytes
                } else {
                    let units: Vec<_> = bytes
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| u16::from_le_bytes(*b))
                        .collect();
                    crate::codepage::encode(
                        &String::from_utf16(&units).expect("environment Unicode"),
                        self.profile.codepage(),
                    )
                    .map_err(Failure::Windows)?
                    .0
                };
                let span = (bytes.len() as u64).div_ceil(PAGE) * PAGE;
                if self.snapshots.len() >= 4096 || self.snapshot_bytes + span > 64 * 1024 * 1024 {
                    return Err(Failure::Windows(8));
                }
                let block = allocate(memory, &bytes).map_err(Failure::Windows)?;
                self.snapshots.insert(block.base, (block.span, wide));
                self.snapshot_bytes += span;
                Ok(block.base)
            }
            E::FreeA | E::FreeW => {
                let &(span, wide) = self
                    .snapshots
                    .get(&a[0])
                    .filter(|(_, w)| *w == (id == E::FreeW))
                    .ok_or(Failure::Windows(87))?;
                memory
                    .check_unmap(a[0], span)
                    .map_err(|_| Failure::Windows(87))?;
                memory
                    .unmap(a[0], span)
                    .expect("preflighted environment snapshot release");
                self.snapshots.remove(&a[0]);
                self.snapshot_bytes -= span;
                debug_assert_eq!(wide, id == E::FreeW);
                Ok(1)
            }
            E::GetA | E::GetW | E::SetA | E::SetW => {
                if a[0] == 0 {
                    return Err(Failure::Windows(87));
                }
                let variable = name(memory, a[0], id.wide())?;
                if variable.is_empty() || variable.contains('=') {
                    return Err(Failure::Windows(87));
                }
                if id.set() {
                    let mut next = self.variables.clone();
                    if a[1] == 0 {
                        if !next.remove(&variable) {
                            return Err(Failure::Windows(203));
                        }
                    } else {
                        next.set(variable, name(memory, a[1], id.wide())?);
                    }
                    self.replace(memory, next)?;
                    Ok(1)
                } else {
                    let value = self.variables.get(&variable).ok_or(Failure::Windows(203))?;
                    let bytes = if id.wide() {
                        value
                            .encode_utf16()
                            .chain(Some(0))
                            .flat_map(u16::to_le_bytes)
                            .collect()
                    } else {
                        crate::codepage::encode(&(value.to_owned() + "\0"), self.profile.codepage())
                            .map_err(Failure::Windows)?
                            .0
                    };
                    let required = bytes.len() / if id.wide() { 2 } else { 1 };
                    if a[2] < required as u64 {
                        return Ok(required as u64);
                    }
                    memory.write(a[1], &bytes)?;
                    Ok(required as u64 - 1)
                }
            }
        }
    }
}
pub(super) fn dispatch(
    id: E,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    environment: &mut Environment,
) -> Result<u64, ServiceError> {
    match environment.call(id, call, memory) {
        Ok(value) => Ok(value),
        Err(Failure::Fault(fault)) => Err(ServiceError::Fault(fault)),
        Err(Failure::Windows(code)) => {
            memory.write_int(context::error_address(state, memory.mode()), 4, code.into())?;
            Ok(0)
        }
    }
}
