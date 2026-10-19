//! Guest function-table registration and lookup. This is metadata, never host executable code.
//! Process controls perform guest callbacks; this owner handles checked metadata only.
use super::{Memory, Runtime64, READ, WRITE};
use crate::win32::catalog::ExceptionApi;
use std::collections::BTreeMap;
enum Table {
    Array {
        base: u64,
        count: u32,
        permanent: bool,
    },
    Callback {
        base: u64,
        length: u32,
        target: u64,
        context: u64,
        _out_of_process: Option<String>,
    },
}
pub(super) enum Result64 {
    Value(u64),
    Callback {
        base: u64,
        target: u64,
        context: u64,
    },
}
#[derive(Default)]
pub(super) struct FunctionTables {
    tables: BTreeMap<u64, Table>,
}
fn entries(mem: &Memory, table: u64, count: u32, base: u64) -> Result<bool, String> {
    if count == 0 || count > 1_000_000 || base >= 1 << 47 {
        return Ok(false);
    }
    mem.check(table, count as usize * 12, READ)?;
    let mut previous_end = 0;
    for n in 0..count as u64 {
        let start = mem.read_int(table + n * 12, 4)?;
        let end = mem.read_int(table + n * 12 + 4, 4)?;
        if start < previous_end
            || start >= end
            || base.checked_add(end).is_none_or(|end| end > 1 << 47)
        {
            return Ok(false);
        }
        previous_end = end;
    }
    Ok(true)
}
impl FunctionTables {
    pub(super) fn arrays(&self) -> impl Iterator<Item = (u64, u64, u32)> + '_ {
        self.tables.iter().filter_map(|(&table, info)| match info {
            Table::Array { base, count, .. } => Some((table, *base, *count)),
            Table::Callback { .. } => None,
        })
    }
    fn register_static_functions(
        &mut self,
        mem: &Memory,
        base: u64,
        rva: u32,
        size: u32,
    ) -> Result<(), String> {
        if size == 0 {
            return Ok(());
        }
        if !size.is_multiple_of(12) || !entries(mem, base + rva as u64, size / 12, base)? {
            return Err("invalid AMD64 image runtime function table".into());
        }
        self.tables.insert(
            base + rva as u64,
            Table::Array {
                base,
                count: size / 12,
                permanent: true,
            },
        );
        Ok(())
    }
    pub(crate) fn finish_function_lookup(
        mem: &mut Memory,
        entry: u64,
        base: u64,
        pc: u64,
        output: u64,
    ) -> Result<u64, String> {
        mem.check(output, 8, WRITE)?;
        if entry == 0 {
            mem.write_int(output, 8, 0)?;
            return Ok(0);
        }
        mem.check(entry, 12, READ)?;
        let start = mem.read_int(entry, 4)?;
        let end = mem.read_int(entry + 4, 4)?;
        if start >= end || pc < base || pc - base < start || pc - base >= end {
            return Err(
                "AMD64 function-table callback returned an entry outside the queried PC".into(),
            );
        }
        mem.write_int(output, 8, base)?;
        Ok(entry)
    }
    pub(super) fn call(
        &mut self,
        mem: &mut Memory,
        id: ExceptionApi,
        args: &[u64],
        callbacks_enabled: bool,
    ) -> Result<Result64, String> {
        let value = match id {
            ExceptionApi::AddTable => {
                if self.tables.len() >= 128
                    || self.tables.contains_key(&args[0])
                    || !entries(mem, args[0], args[1] as u32, args[2])?
                {
                    0
                } else {
                    self.tables.insert(
                        args[0],
                        Table::Array {
                            base: args[2],
                            count: args[1] as u32,
                            permanent: false,
                        },
                    );
                    1
                }
            }
            ExceptionApi::InstallCallback => {
                if args[0] & 3 != 3
                    || args[2] == 0
                    || args[3] == 0
                    || args[1].checked_add(args[2]).is_none_or(|end| end > 1 << 47)
                    || self.tables.len() >= 128
                    || self.tables.contains_key(&args[0])
                {
                    0
                } else {
                    let out_of_process = if args[5] == 0 {
                        None
                    } else {
                        Some(crate::win32::x64::text::utf16(mem, args[5])?)
                    };
                    self.tables.insert(
                        args[0],
                        Table::Callback {
                            base: args[1],
                            length: args[2] as u32,
                            target: args[3],
                            context: args[4],
                            _out_of_process: out_of_process,
                        },
                    );
                    1
                }
            }
            ExceptionApi::DeleteTable => {
                if matches!(
                    self.tables.get(&args[0]),
                    None | Some(Table::Array {
                        permanent: true,
                        ..
                    })
                ) {
                    0
                } else {
                    self.tables.remove(&args[0]);
                    1
                }
            }
            ExceptionApi::LookupEntry => {
                mem.check(args[1], 8, WRITE)?;
                if args[2] != 0 {
                    return Err("AMD64 unwind history table cache is not implemented".into());
                }
                for (&table, info) in &self.tables {
                    match info {
                        Table::Array { base, count, .. }
                            if args[0] >= *base && args[0] - *base <= u32::MAX as u64 =>
                        {
                            let rva = args[0] - *base;
                            let mut lo = 0;
                            let mut hi = *count as u64;
                            while lo < hi {
                                let mid = (lo + hi) / 2;
                                if mem.read_int(table + mid * 12, 4)? <= rva {
                                    lo = mid + 1;
                                } else {
                                    hi = mid;
                                }
                            }
                            if lo != 0 {
                                let entry = table + (lo - 1) * 12;
                                if rva < mem.read_int(entry + 4, 4)? {
                                    return Ok(Result64::Value(Self::finish_function_lookup(
                                        mem, entry, *base, args[0], args[1],
                                    )?));
                                }
                            }
                        }
                        Table::Callback {
                            base,
                            length,
                            target,
                            context,
                            ..
                        } if args[0] >= *base && args[0] - *base < *length as u64 => {
                            if !callbacks_enabled {
                                return Err("AMD64 function-table lookup callback requires process execution".into());
                            }
                            return Ok(Result64::Callback {
                                base: *base,
                                target: *target,
                                context: *context,
                            });
                        }
                        _ => {}
                    }
                }
                mem.write_int(args[1], 8, 0)?;
                0
            }
            _ => unreachable!(),
        };
        Ok(Result64::Value(value))
    }
}

impl Runtime64 {
    pub(crate) fn register_static_functions(
        &mut self,
        mem: &Memory,
        base: u64,
        rva: u32,
        size: u32,
    ) -> Result<(), String> {
        self.function_tables
            .register_static_functions(mem, base, rva, size)
    }
    pub(crate) fn finish_function_lookup(
        &self,
        mem: &mut Memory,
        entry: u64,
        base: u64,
        pc: u64,
        output: u64,
    ) -> Result<u64, String> {
        FunctionTables::finish_function_lookup(mem, entry, base, pc, output)
    }
}
