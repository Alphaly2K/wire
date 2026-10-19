//! Guest RNG providers own only their handles; guest memory stays checked.
use super::{Memory, TEB, WRITE};
use crate::win32::{catalog::BcryptApi, x64::text::utf16};
use std::collections::BTreeSet;

#[derive(Default)]
pub(super) struct RngProviders {
    handles: BTreeSet<u64>,
}

impl RngProviders {
    pub(super) fn call(
        &mut self,
        mem: &mut Memory,
        id: BcryptApi,
        args: &[u64],
    ) -> Result<u64, String> {
        Ok(match id {
            BcryptApi::Generate => {
                if args[3] & !3 != 0 || args[3] & 2 != 0 && args[0] != 0 {
                    0xc000000d
                } else if args[3] & 2 == 0 && args[0] != 0x81 && !self.handles.contains(&args[0]) {
                    0xc0000008
                } else if args[2] > 16 * 1024 * 1024 {
                    0xc0000017
                } else {
                    mem.check(args[1], args[2] as usize, WRITE)?;
                    let mut data = vec![0; args[2] as usize];
                    let mut success = true;
                    for bytes in data.chunks_mut(256) {
                        if unsafe { libc::getentropy(bytes.as_mut_ptr().cast(), bytes.len()) } != 0
                        {
                            success = false;
                            break;
                        }
                    }
                    if success {
                        mem.write(args[1], &data)?;
                        0
                    } else {
                        0xc0000001
                    }
                }
            }
            BcryptApi::Open => {
                let algorithm = utf16(mem, args[1])?;
                let implementation = if args[2] == 0 {
                    None
                } else {
                    Some(utf16(mem, args[2])?)
                };
                if args[3] != 0 {
                    0xc000000d
                } else if algorithm != "RNG"
                    || implementation
                        .as_ref()
                        .is_some_and(|name| name != "Microsoft Primitive Provider")
                {
                    0xc00000bb
                } else {
                    mem.check(args[0], 8, WRITE)?;
                    let handle = (0..1024u64)
                        .map(|n| TEB + 0x700000 + n * 16)
                        .find(|h| !self.handles.contains(h));
                    if let Some(handle) = handle {
                        mem.write_int(args[0], 8, handle)?;
                        self.handles.insert(handle);
                        0
                    } else {
                        0xc0000017
                    }
                }
            }
            BcryptApi::Close => {
                if args[1] != 0 {
                    0xc000000d
                } else if self.handles.remove(&args[0]) {
                    0
                } else {
                    0xc0000008
                }
            }
        })
    }
}
