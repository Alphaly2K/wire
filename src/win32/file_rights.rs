//! Access masks and token allocation shared by file storage adapters.
pub(crate) fn open_rights(values: [u32; 4]) -> Result<u32, u32> {
    let [access, share, disposition, attributes] = values;
    if access & !0xc012019f != 0
        || access & 4 != 0 && access & 0x40000002 == 0
        || attributes & !0x1a000080 != 0
    {
        return Err(50);
    }
    if share & !7 != 0 || !(1..=5).contains(&disposition) {
        return Err(87);
    }
    Ok(((access & 0x80000001 != 0) as u32) | (((access & 0x40000002 != 0) as u32) << 1))
}
pub(crate) fn duplicate_rights(original: u32, desired: Option<u32>) -> Result<u32, u32> {
    let Some(access) = desired else {
        return Ok(original);
    };
    if access & !0xc012019b != 0 {
        return Err(50);
    }
    let rights = ((access & 0x80000001 != 0) as u32) | (((access & 0x40000002 != 0) as u32) << 1);
    if rights & !original != 0 {
        return Err(5);
    }
    Ok(rights)
}
pub(crate) fn allocate_handle(next: &mut u64) -> u64 {
    let handle = *next;
    *next += 1;
    if *next == 0x180000 {
        *next = 0x190000;
    }
    handle
}
