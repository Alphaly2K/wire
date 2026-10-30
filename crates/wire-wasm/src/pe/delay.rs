use super::*;

pub struct Delay {
    pub dll: String,
    pub handle: u32,
    pub unload: Option<u32>,
    pub functions: Vec<ImportFunc>,
}

impl Image {
    pub(crate) fn delay_imports(
        &self,
        image: &[u8],
        base: u64,
        maximum: usize,
    ) -> Result<Vec<Delay>, String> {
        let image = bytes(image, 0, self.size as usize)?;
        let directory = self.dir(13);
        if directory.rva == 0 {
            return Ok(Vec::new());
        }
        if directory.size < 32 {
            return Err("truncated PE delay directory".into());
        }
        let width = self.mode as usize / 8;
        let flag = 1u64 << (width * 8 - 1);
        let mut result = Vec::new();
        let mut total = 0;
        let end = directory.rva as usize + directory.size as usize;
        for descriptor in 0..=512 {
            let at = directory.rva as usize + descriptor * 32;
            if at + 32 > end && at != end {
                return Err("unterminated PE delay directory".into());
            }
            let header = bytes(image, at, 32)?;
            if header.iter().all(|&b| b == 0) {
                return Ok(result);
            }
            // GNU delay libraries omit the null sentinel from directory size.
            if at == end {
                return Err("unterminated PE delay directory".into());
            }
            if descriptor == 512 {
                return Err("PE delay descriptor capacity exceeded".into());
            }
            let attributes = word(header, 0, 4)?;
            if attributes > 1 {
                return Err("invalid PE delay attributes".into());
            }
            let rva = |value: u64| -> Result<u32, String> {
                let relative = if attributes == 1 {
                    value
                } else {
                    value
                        .checked_sub(base)
                        .ok_or("PE delay VA precedes image")?
                };
                let relative =
                    u32::try_from(relative).map_err(|_| "PE delay RVA exceeds 32 bits")?;
                if relative >= self.size {
                    return Err("PE delay address outside image".into());
                }
                Ok(relative)
            };
            let required = |offset| -> Result<u32, String> {
                let value = word(header, offset, 4)?;
                if value == 0 {
                    return Err("missing PE delay address".into());
                }
                rva(value)
            };
            let name = required(4)?;
            let handle = required(8)?;
            let iat = required(12)?;
            let names = required(16)?;
            let optional = |offset| -> Result<Option<u32>, String> {
                let value = word(header, offset, 4)?;
                if value == 0 {
                    Ok(None)
                } else {
                    rva(value).map(Some)
                }
            };
            let bound = optional(20)?;
            let unload = optional(24)?;
            bytes(image, handle as usize, width)?;
            let mut functions = Vec::new();
            for index in 0..=4096 {
                let offset = index * width;
                let source = (names as usize)
                    .checked_add(offset)
                    .ok_or("PE delay thunk overflow")?;
                let dest = (iat as usize)
                    .checked_add(offset)
                    .ok_or("PE delay IAT overflow")?;
                let value = word(image, source, width)?;
                bytes(image, dest, width)?;
                for table in [bound, unload].into_iter().flatten() {
                    bytes(image, table as usize + offset, width)?;
                }
                if value == 0 {
                    break;
                }
                if total == maximum || index == 4096 {
                    return Err("PE delay import capacity exceeded".into());
                }
                total += 1;
                let (name, ordinal) = if value & flag != 0 {
                    if value & !(flag | 65535) != 0 {
                        return Err("invalid PE delay ordinal".into());
                    }
                    (None, Some(value as u16))
                } else {
                    let name = rva(value)? as usize;
                    bytes(image, name, 2)?;
                    (Some(super::tables::string(image, name + 2)?), None)
                };
                functions.push(ImportFunc {
                    iat_rva: dest as u32,
                    name,
                    ordinal,
                });
            }
            result.push(Delay {
                dll: linker::module_name(&super::tables::string(image, name as usize)?)?,
                handle,
                unload,
                functions,
            });
        }
        Err("PE delay descriptor capacity exceeded".into())
    }
}
