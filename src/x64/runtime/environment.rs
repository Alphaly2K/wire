//! Variable values and issued environment blocks have one private owner.
use super::{Memory, Runtime64};
use crate::win32::catalog::EnvironmentApi;
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
        Ok(match id {
            EnvironmentApi::GetA
            | EnvironmentApi::GetW
            | EnvironmentApi::SetA
            | EnvironmentApi::SetW => {
                let wide = id.wide();
                let variable = Self::path_name(mem, args[0], wide)?;
                if variable.is_empty() || variable.contains('=') {
                    self.set_error(mem, 87)?;
                    0
                } else if id.set() {
                    if args[1] == 0 {
                        if self.environment.variables.remove(&variable) {
                            1
                        } else {
                            self.set_error(mem, 203)?;
                            0
                        }
                    } else {
                        let value = Self::path_name(mem, args[1], wide)?;
                        self.environment.variables.set(variable, value);
                        1
                    }
                } else if let Some(value) = self.environment.variables.get(&variable) {
                    let bytes: Vec<u8> = if wide {
                        value
                            .encode_utf16()
                            .chain(Some(0))
                            .flat_map(u16::to_le_bytes)
                            .collect()
                    } else {
                        crate::codepage::encode(&(value.to_owned() + "\0"), self.profile.codepage())
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
                    self.set_error(mem, 203)?;
                    0
                }
            }
            EnvironmentApi::BlockW => {
                let bytes = self.environment.variables.wide_block();
                let ptr = self.heap_alloc(mem, bytes.len() as u64)?;
                if ptr != 0 {
                    mem.write(ptr, &bytes)?;
                    self.environment.blocks.insert(ptr);
                }
                ptr
            }
            EnvironmentApi::FreeW => {
                if !self.environment.blocks.contains(&args[0])
                    || !self.allocations.contains_key(&args[0])
                {
                    self.set_error(mem, 87)?;
                    0
                } else {
                    let (_, span) = self.allocations[&args[0]];
                    mem.unmap(args[0], span)?;
                    self.environment.blocks.remove(&args[0]);
                    self.allocations.remove(&args[0]);
                    1
                }
            }
            _ => unreachable!("ANSI environment block exports remain absent on Win64"),
        })
    }
}
