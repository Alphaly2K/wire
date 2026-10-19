//! SID guest-memory adapter. Fault/error and partial-effect order is deliberate.
use crate::{
    win32::catalog::SidApi,
    x64::memory::{Memory, READ, WRITE},
};

fn read_sid(mem: &Memory, address: u64) -> Result<Vec<u8>, u32> {
    let mut header = [0; 8];
    mem.read(address, &mut header).map_err(|_| 1337u32)?;
    if header[0] != 1 || header[1] > 15 {
        return Err(1337);
    }
    let mut data = vec![0; 8 + header[1] as usize * 4];
    mem.read(address, &mut data).map_err(|_| 1337u32)?;
    Ok(data)
}

pub(crate) fn call(
    mem: &mut Memory,
    id: SidApi,
    args: &[u64],
    last_error: u64,
) -> Result<u64, String> {
    if id == SidApi::Initialize && args[2] as u8 <= 15 {
        mem.check(args[0], 8 + args[2] as u8 as usize * 4, WRITE)?;
        mem.check(args[1], 6, READ)?;
    }
    if id == SidApi::Copy {
        if let Ok(sid) = read_sid(mem, args[2]) {
            if args[0] >= sid.len() as u64 {
                mem.check(args[1], sid.len(), WRITE)?;
            }
        }
    }
    let result = (|| -> Result<u64, u32> {
        if id == SidApi::LengthRequired {
            return Ok(8 + (args[0] as u8 as u64) * 4);
        }
        if id == SidApi::Initialize {
            let count = args[2] as u8;
            if count > 15 {
                return Err(87);
            }
            let mut header = [1, count, 0, 0, 0, 0, 0, 0];
            mem.read(args[1], &mut header[2..]).map_err(|_| 1337u32)?;
            mem.check(args[0], 8 + count as usize * 4, WRITE)
                .map_err(|_| 1337u32)?;
            mem.write(args[0], &header).map_err(|_| 1337u32)?;
            return Ok(1);
        }
        let source = if id == SidApi::Copy { args[2] } else { args[0] };
        let sid = read_sid(mem, source)?;
        Ok(match id {
            SidApi::Valid => 1,
            SidApi::Length => sid.len() as u64,
            SidApi::IdentifierAuthority => source + 2,
            SidApi::SubAuthorityCount => source + 1,
            SidApi::SubAuthority => {
                if args[1] >= sid[1] as u64 {
                    return Err(87);
                }
                source + 8 + args[1] * 4
            }
            SidApi::Equal => (sid == read_sid(mem, args[1])?) as u64,
            SidApi::Copy => {
                if args[0] < sid.len() as u64 {
                    return Err(122);
                }
                mem.write(args[1], &sid).map_err(|_| 1337u32)?;
                1
            }
            SidApi::LengthRequired | SidApi::Initialize => unreachable!(),
        })
    })();
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            mem.write_int(last_error, 4, error as u64)?;
            Ok(0)
        }
    }
}
