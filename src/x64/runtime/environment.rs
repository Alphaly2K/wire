//! Variable values and issued environment blocks have one private owner.
use super::{allocation::Pages, threads::Threads, Memory, Runtime64};
use crate::win32::catalog::EnvironmentApi;
use std::collections::BTreeMap;
#[derive(Default)]
pub(super) struct Environment {
    variables: crate::runtime::environment::EnvironmentVariables,
    blocks: std::collections::BTreeSet<u64>,
}
impl Runtime64 {
    pub(super) fn environment_call(
        &mut self,
        mem: &mut Memory,
        id: EnvironmentApi,
        args: &[u64],
    ) -> Result<u64, String> {
        match id {
            EnvironmentApi::BlockW => {
                let last_error = self.teb() + 0x68;
                let mut pages = Pages {
                    next: &mut self.next_alloc,
                    allocations: &mut self.allocations,
                    last_error,
                    trace: &mut self.trace,
                };
                self.environment.block_call(mem, &mut pages)
            }
            EnvironmentApi::FreeW => {
                self.environment
                    .free_block_call(mem, args[0], &mut self.allocations, &self.threads)
            }
            _ => self.environment.variable_call(
                mem,
                id,
                args,
                self.profile.codepage(),
                &self.threads,
            ),
        }
    }
}

impl Environment {
    fn variable_call(
        &mut self,
        mem: &mut Memory,
        id: EnvironmentApi,
        args: &[u64],
        codepage: u32,
        threads: &Threads,
    ) -> Result<u64, String> {
        Ok(match id {
            EnvironmentApi::GetA
            | EnvironmentApi::GetW
            | EnvironmentApi::SetA
            | EnvironmentApi::SetW => {
                let wide = id.wide();
                let variable = crate::win32::x64::text::path_name(mem, args[0], wide)?;
                if variable.is_empty() || variable.contains('=') {
                    threads.set_error(mem, 87)?;
                    0
                } else if id.set() {
                    if args[1] == 0 {
                        if self.variables.remove(&variable) {
                            1
                        } else {
                            threads.set_error(mem, 203)?;
                            0
                        }
                    } else {
                        let value = crate::win32::x64::text::path_name(mem, args[1], wide)?;
                        self.variables.set(variable, value);
                        1
                    }
                } else if let Some(value) = self.variables.get(&variable) {
                    let bytes: Vec<u8> = if wide {
                        value
                            .encode_utf16()
                            .chain(Some(0))
                            .flat_map(u16::to_le_bytes)
                            .collect()
                    } else {
                        crate::codepage::encode(&(value.to_owned() + "\0"), codepage)
                            .map_err(|e| format!("environment codepage error {e}"))?
                            .0
                    };
                    let required = bytes.len() / if wide { 2 } else { 1 };
                    if args[2] < required as u64 {
                        required as u64
                    } else {
                        mem.write(args[1], &bytes)?;
                        required as u64 - 1
                    }
                } else {
                    threads.set_error(mem, 203)?;
                    0
                }
            }
            _ => unreachable!("ANSI environment block exports remain absent on Win64"),
        })
    }

    fn block_call(&mut self, mem: &mut Memory, pages: &mut Pages<'_>) -> Result<u64, String> {
        let bytes = self.variables.wide_block();
        let ptr = pages.allocate(mem, bytes.len() as u64)?;
        if ptr != 0 {
            mem.write(ptr, &bytes)?;
            self.blocks.insert(ptr);
        }
        Ok(ptr)
    }

    fn free_block_call(
        &mut self,
        mem: &mut Memory,
        pointer: u64,
        allocations: &mut BTreeMap<u64, (usize, u64)>,
        threads: &Threads,
    ) -> Result<u64, String> {
        Ok(
            if !self.blocks.contains(&pointer) || !allocations.contains_key(&pointer) {
                threads.set_error(mem, 87)?;
                0
            } else {
                let (_, span) = allocations[&pointer];
                mem.unmap(pointer, span)?;
                self.blocks.remove(&pointer);
                allocations.remove(&pointer);
                1
            },
        )
    }
}
