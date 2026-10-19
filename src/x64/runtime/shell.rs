//! Shell allocation commits use the existing local/task ownership and heap.
use super::{Memory, Runtime64};
use crate::win32::{catalog::ShellApi, x64::shell};

impl Runtime64 {
    pub(super) fn shell_call(
        &mut self,
        mem: &mut Memory,
        id: ShellApi,
        args: &[u64],
    ) -> Result<u64, String> {
        Ok(match id {
            ShellApi::Arguments => {
                let shell::Arguments {
                    strings,
                    table_size,
                    size,
                    count,
                } = shell::arguments(mem, args, &self.exe_path)?;
                let block = self.heap_alloc(mem, size as u64)?;
                if block == 0 {
                    0
                } else {
                    let mut data = vec![0; size];
                    let mut at = table_size;
                    for (n, string) in strings.iter().enumerate() {
                        data[n * 8..n * 8 + 8].copy_from_slice(&(block + at as u64).to_le_bytes());
                        data[at..at + string.len()].copy_from_slice(string);
                        at += string.len();
                    }
                    mem.write(block, &data)?;
                    mem.write_int(args[1], 4, count)?;
                    self.local_blocks.insert(block);
                    block
                }
            }
            ShellApi::KnownFolder => {
                let data = shell::known_folder(mem, args, &self.profile, self.files.filesystem())?;
                match data {
                    Err(error) => {
                        mem.write_int(args[3], 8, 0)?;
                        0x80070000 | error as u64
                    }
                    Ok(data) => {
                        let p = self.heap_alloc(mem, data.len() as u64)?;
                        if p == 0 {
                            mem.write_int(args[3], 8, 0)?;
                            0x8007000e
                        } else {
                            mem.write(p, &data)?;
                            mem.write_int(args[3], 8, p)?;
                            self.task_memory.track(p);
                            0
                        }
                    }
                }
            }
            _ => unreachable!("PE32-only shell directory shim"),
        })
    }
}
