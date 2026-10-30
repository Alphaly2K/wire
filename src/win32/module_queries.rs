//! Module query outputs are prepared before either memory adapter publishes them.
use super::catalog::ModuleQueryApi as Q;

pub(crate) struct Module<'a> {
    pub base: u64,
    pub name: &'a str,
    pub path: Option<&'a str>,
    pub image: Option<(u32, u64)>,
}

pub(crate) struct Output {
    pub value: u64,
    pub writes: Vec<(u64, Vec<u8>)>,
}

pub(crate) fn prepare(
    id: Q,
    a: &[u64],
    width: usize,
    main: u64,
    modules: &[Module<'_>],
) -> Result<Output, u32> {
    let (value, writes) = if matches!(id, Q::EnumProcessModules | Q::EnumProcessModulesEx) {
        let filter = if id == Q::EnumProcessModulesEx {
            a[4] as u32
        } else {
            0
        };
        if filter > 3 {
            return Err(87);
        }
        let empty = filter == if width == 4 { 2 } else { 1 };
        let total = if empty { 0 } else { modules.len() * width };
        let count = (a[2] as u32 as usize / width).min(total / width);
        let mut writes = Vec::new();
        if count != 0 {
            writes.push((
                a[1],
                modules[..count]
                    .iter()
                    .flat_map(|m| m.base.to_le_bytes()[..width].to_vec())
                    .collect(),
            ));
        }
        writes.push((a[3], (total as u32).to_le_bytes().to_vec()));
        (1, writes)
    } else {
        let base = if a[1] == 0 && id != Q::GetModuleInformation {
            main
        } else {
            a[1]
        };
        let module = modules.iter().find(|m| m.base == base).ok_or(6u32)?;
        if id == Q::GetModuleInformation {
            let size = if width == 4 { 12 } else { 24 };
            if (a[3] as u32) < size {
                return Err(122);
            }
            let (image_size, entry) = module.image.ok_or(50u32)?;
            let mut bytes = vec![0; size as usize];
            bytes[..width].copy_from_slice(&base.to_le_bytes()[..width]);
            bytes[width..width + 4].copy_from_slice(&image_size.to_le_bytes());
            let at = if width == 4 { 8 } else { 16 };
            bytes[at..at + width].copy_from_slice(&entry.to_le_bytes()[..width]);
            (1, vec![(a[2], bytes)])
        } else {
            let capacity = a[3] as u32 as usize;
            if capacity == 0 {
                return Err(122);
            }
            let path;
            let value = if matches!(id, Q::GetModuleBaseNameA | Q::GetModuleBaseNameW) {
                module.name
            } else if let Some(path) = module.path {
                path
            } else {
                path = format!("C:\\Windows\\System32\\{}", module.name);
                &path
            };
            let wide = id.wide();
            let mut bytes: Vec<u8> = if wide {
                value
                    .encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect()
            } else {
                crate::codepage::encode(&(value.to_owned() + "\0"), 932)
                    .map_err(|_| 1113u32)?
                    .0
            };
            let unit = if wide { 2 } else { 1 };
            let units = bytes.len() / unit;
            let count = units.min(capacity);
            bytes.truncate(count * unit);
            if count < units {
                bytes[(count - 1) * unit..].fill(0);
            }
            (
                if count < units { count } else { units - 1 } as u64,
                vec![(a[2], bytes)],
            )
        }
    };
    Ok(Output { value, writes })
}
