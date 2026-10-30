//! Fixed Windows file information layouts, independent of guest execution.
#[derive(Clone, Copy)]
pub(crate) struct Metadata {
    pub attributes: u32,
    pub size: u64,
    pub times: [Result<u64, u32>; 3],
    pub change: Result<u64, u32>,
    pub identity: Result<(u64, u64, u32, u64), u32>,
}
impl Metadata {
    pub fn is_dir(&self) -> bool {
        self.attributes & 0x10 != 0
    }
    pub fn len(&self) -> u64 {
        self.size
    }
    pub fn created(&self) -> Result<u64, u32> {
        self.times[0]
    }
    pub fn accessed(&self) -> Result<u64, u32> {
        self.times[1]
    }
    pub fn modified(&self) -> Result<u64, u32> {
        self.times[2]
    }
}

pub(crate) fn identity(metadata: &Metadata) -> Result<(u64, u64, u32, u64), u32> {
    metadata.identity
}
pub(crate) fn change_time(metadata: &Metadata) -> Result<u64, u32> {
    metadata.change
}
pub(crate) fn filetime(value: Result<u64, u32>) -> Result<u64, u32> {
    value
}

#[derive(Clone, Copy)]
pub(crate) enum Information {
    Handle,
    Times([bool; 3]),
    Class(u32),
}
pub(crate) fn information(
    meta: Metadata,
    information: Information,
    capacity: u64,
) -> Result<Vec<u8>, u32> {
    let attributes = meta.attributes;
    let kind = match information {
        Information::Handle => u64::MAX,
        Information::Times(_) => u64::MAX - 1,
        Information::Class(kind) => u64::from(kind),
    };
    let mut data = match kind {
        n if n == u64::MAX - 1 => vec![0; 24],
        u64::MAX => vec![0; 52],
        0 => vec![0; 40],  // FILE_BASIC_INFO
        1 => vec![0; 24],  // FILE_STANDARD_INFO
        9 => vec![0; 8],   // FILE_ATTRIBUTE_TAG_INFO.
        18 => vec![0; 24], // FILE_ID_INFO.
        _ => return Err(50),
    };
    if matches!(information, Information::Class(_)) && capacity < data.len() as u64 {
        return Err(24);
    }
    if kind >= u64::MAX - 1 || kind == 0 {
        let offset = if kind == u64::MAX { 4 } else { 0 };
        for (n, time) in [meta.created(), meta.accessed(), meta.modified()]
            .into_iter()
            .enumerate()
        {
            // GetFileTime permits independent null outputs.
            if !matches!(information, Information::Times(selected) if !selected[n]) {
                data[offset + n * 8..offset + n * 8 + 8]
                    .copy_from_slice(&filetime(time)?.to_le_bytes());
            }
        }
    }
    match kind {
        n if n == u64::MAX - 1 => {}
        u64::MAX => {
            let (volume, index, links, _) = identity(&meta)?;
            data[..4].copy_from_slice(&attributes.to_le_bytes());
            data[28..32].copy_from_slice(&(volume as u32).to_le_bytes());
            data[32..36].copy_from_slice(&((meta.len() >> 32) as u32).to_le_bytes());
            data[36..40].copy_from_slice(&(meta.len() as u32).to_le_bytes());
            data[40..44].copy_from_slice(&links.to_le_bytes());
            data[44..48].copy_from_slice(&((index >> 32) as u32).to_le_bytes());
            data[48..52].copy_from_slice(&(index as u32).to_le_bytes());
        }
        0 => {
            data[24..32].copy_from_slice(&change_time(&meta)?.to_le_bytes());
            data[32..36].copy_from_slice(&attributes.to_le_bytes());
        }
        1 => {
            let (_, _, links, allocated) = identity(&meta)?;
            data[..8].copy_from_slice(&allocated.to_le_bytes());
            data[8..16].copy_from_slice(&meta.len().to_le_bytes());
            data[16..20].copy_from_slice(&links.to_le_bytes());
            // FILE_FLAG_DELETE_ON_CLOSE is rejected at open, so pending is false.
            data[21] = meta.is_dir() as u8;
        }
        9 => data[..4].copy_from_slice(&attributes.to_le_bytes()),
        18 => {
            let (volume, index, _, _) = identity(&meta)?;
            data[..8].copy_from_slice(&volume.to_le_bytes());
            data[8..16].copy_from_slice(&index.to_le_bytes());
        }
        _ => unreachable!(),
    }
    Ok(data)
}

/// FILE_FULL_DIR_INFORMATION. Data layout and missing timestamp policy match
/// the original directory adapter; storage identity never becomes a guest pointer.
pub(crate) fn directory_record(metadata: Metadata, name: &str) -> Result<Vec<u8>, u32> {
    let name: Vec<_> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut data = vec![0; (68 + name.len()).next_multiple_of(8)];
    for (n, time) in metadata.times.into_iter().enumerate() {
        let ticks = match time {
            Ok(ticks) => ticks,
            Err(50) => 0,
            Err(error) => return Err(error),
        };
        data[8 + n * 8..16 + n * 8].copy_from_slice(&ticks.to_le_bytes());
    }
    data[32..40].copy_from_slice(&metadata.change?.to_le_bytes());
    let size = if metadata.is_dir() { 0 } else { metadata.size };
    data[40..48].copy_from_slice(&size.to_le_bytes());
    data[48..56].copy_from_slice(&metadata.identity?.3.to_le_bytes());
    data[56..60].copy_from_slice(&metadata.attributes.to_le_bytes());
    data[60..64].copy_from_slice(&(name.len() as u32).to_le_bytes());
    data[68..68 + name.len()].copy_from_slice(&name);
    Ok(data)
}
