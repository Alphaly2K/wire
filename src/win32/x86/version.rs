//! PE32 version query compatibility, including historical cross-encoding sizes.
use crate::{mem::GuestMem, runtime::environment::GuestEnvironment, win32::catalog::VersionApi};

pub(crate) struct Value {
    pub value: u32,
    pub note: &'static str,
}
pub(crate) fn call(
    id: VersionApi,
    args: &[u32; 16],
    mem: &GuestMem,
    environment: &GuestEnvironment,
    last_error: &mut u32,
) -> Result<Value, String> {
    let version = environment.version();
    if id == VersionApi::GetVersion {
        return Ok(Value {
            value: version.packed(),
            note: "fake Windows 7 (6.1.7601)",
        });
    }
    assert!(
        matches!(
            id,
            VersionApi::GetVersionExA | VersionApi::GetVersionExW | VersionApi::RtlGetVersion
        ),
        "ABI-only version call reached PE32 query adapter"
    );
    let size = mem.read_u32(args[0]).map_err(|e| e.to_string())?;
    let value = if matches!(size, 148 | 156 | 276 | 284) {
        let mut info = vec![0; size as usize];
        for (i, value) in [
            size,
            version.major,
            version.minor,
            version.build,
            version.platform,
        ]
        .iter()
        .enumerate()
        {
            info[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        if matches!(size, 156 | 284) {
            info[size as usize - 2] = version.product_type;
        }
        mem.write_bytes(args[0], &info).map_err(|e| e.to_string())?;
        u32::from(id != VersionApi::RtlGetVersion)
    } else {
        *last_error = 87;
        0
    };
    Ok(Value { value, note: "" })
}
