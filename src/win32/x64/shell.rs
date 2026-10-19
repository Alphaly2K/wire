//! Win64 shell preparation borrows checked memory, the guest profile and confined filesystem.
use super::text::utf16;
use crate::{
    fs::GuestFs,
    runtime::environment::GuestEnvironment,
    x64::memory::{Memory, WRITE},
};

pub(crate) struct Arguments {
    pub strings: Vec<Vec<u8>>,
    pub table_size: usize,
    pub size: usize,
    pub count: u64,
}
pub(crate) fn arguments(mem: &Memory, args: &[u64], exe_path: &str) -> Result<Arguments, String> {
    mem.check(args[1], 4, WRITE)?;
    let line = utf16(mem, args[0])?;
    let words = if line.is_empty() {
        vec![exe_path.to_owned()]
    } else {
        crate::win32::kernel::command_line::arguments(&line)
    };
    let strings: Vec<Vec<u8>> = words
        .iter()
        .map(|word| {
            word.encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect()
        })
        .collect();
    let table_size = strings.len() * 8;
    let size = table_size + strings.iter().map(Vec::len).sum::<usize>();
    Ok(Arguments {
        strings,
        table_size,
        size,
        count: words.len() as u64,
    })
}
pub(crate) fn known_folder(
    mem: &Memory,
    args: &[u64],
    profile: &GuestEnvironment,
    filesystem: Option<&GuestFs>,
) -> Result<Result<Vec<u8>, u32>, String> {
    mem.check(args[3], 8, WRITE)?;
    let mut guid = [0; 16];
    mem.read(args[0], &mut guid)?;
    let path = match guid {
        [0xa4, 0xa1, 0x20, 0xa5, 0x80, 0x17, 0xf6, 0x4f, 0xbd, 0x18, 0x16, 0x73, 0x43, 0xc5, 0xaf, 0x16] => {
            Some(profile.local_low())
        }
        [0x85, 0x27, 0xb3, 0xf1, 0xba, 0x6f, 0xcf, 0x4f, 0x9d, 0x55, 0x7b, 0x8e, 0x7f, 0x15, 0x70, 0x91] => {
            Some(profile.local())
        }
        [0xdb, 0x85, 0xb6, 0x3e, 0xf9, 0x65, 0xf6, 0x4c, 0xa0, 0x3a, 0xe3, 0xef, 0x65, 0x72, 0x9f, 0x3d] => {
            Some(profile.roaming())
        }
        [0x8f, 0x85, 0x6c, 0x5e, 0x22, 0x0e, 0x60, 0x47, 0x9a, 0xfe, 0xea, 0x33, 0x17, 0xb6, 0x71, 0x73] => {
            Some(profile.home())
        }
        [0xd0, 0x9a, 0xd3, 0xfd, 0x8f, 0x23, 0xaf, 0x46, 0xad, 0xb4, 0x6c, 0x85, 0x48, 0x03, 0x69, 0xc7] => {
            Some(profile.documents())
        }
        _ => None,
    };
    let data = (|| -> Result<Vec<u8>, u32> {
        if args[2] != 0 || args[1] & !0xc000 != 0 {
            return Err(50);
        }
        let path = path.ok_or(50u32)?;
        let host = filesystem.ok_or(3u32)?.resolve(path)?;
        if args[1] & 0x8000 != 0 {
            std::fs::create_dir_all(&host).map_err(|e| crate::fs::win_error(&e))?;
        }
        if args[1] & 0x4000 == 0 && !host.is_dir() {
            return Err(3);
        }
        Ok(path
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect())
    })();
    Ok(data)
}
