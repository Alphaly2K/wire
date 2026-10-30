use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Symbol {
    Name(String),
    Ordinal(u16),
}
impl Symbol {
    pub(crate) fn provider_name(&self) -> std::borrow::Cow<'_, str> {
        match self {
            Self::Name(name) => std::borrow::Cow::Borrowed(name),
            Self::Ordinal(ordinal) => std::borrow::Cow::Owned(format!("#{ordinal}")),
        }
    }
}
#[derive(Clone, Debug)]
pub enum Export {
    Address(u32),
    Forward { dll: String, symbol: Symbol },
}
#[derive(Default, Debug)]
pub struct Exports {
    names: BTreeMap<String, u16>,
    ordinals: BTreeMap<u16, Export>,
}
impl Exports {
    pub fn get(&self, symbol: &Symbol) -> Option<&Export> {
        let ordinal = match symbol {
            Symbol::Name(name) => *self.names.get(name)?,
            Symbol::Ordinal(ordinal) => *ordinal,
        };
        self.ordinals.get(&ordinal)
    }
}

fn string(image: &[u8], at: usize, end: usize) -> Result<String, String> {
    let tail = bytes(
        image,
        at,
        end.checked_sub(at)
            .ok_or("PE export string outside directory")?,
    )?;
    let length = tail
        .iter()
        .take(4096)
        .position(|b| *b == 0)
        .ok_or("unterminated PE export string")?;
    if !tail[..length].is_ascii() {
        return Err("non-ASCII PE export identity".into());
    }
    Ok(std::str::from_utf8(&tail[..length]).unwrap().to_owned())
}

impl Image {
    pub fn exports(&self, image: &[u8]) -> Result<Exports, String> {
        let image = bytes(image, 0, self.size as usize)?;
        let dir = self.dir(0);
        if dir.rva == 0 {
            return Ok(Exports::default());
        }
        if dir.size < 40 {
            return Err("truncated PE export directory".into());
        }
        let at = dir.rva as usize;
        let first = word(image, at + 16, 4)?;
        let count = word(image, at + 20, 4)? as usize;
        let names = word(image, at + 24, 4)? as usize;
        let functions = word(image, at + 28, 4)? as usize;
        let strings = word(image, at + 32, 4)? as usize;
        let ordinals = word(image, at + 36, 4)? as usize;
        if count > 65536 || names > 65536 {
            return Err("PE export capacity exceeded".into());
        }
        bytes(image, functions, count * 4)?;
        bytes(image, strings, names * 4)?;
        bytes(image, ordinals, names * 2)?;
        let mut result = Exports::default();
        for index in 0..count {
            let rva = word(image, functions + index * 4, 4)? as u32;
            if rva == 0 {
                continue;
            }
            let ordinal = u16::try_from(first + index as u64)
                .map_err(|_| "PE export ordinal exceeds 16 bits")?;
            let value = if u64::from(rva) >= u64::from(dir.rva)
                && u64::from(rva) < u64::from(dir.rva) + u64::from(dir.size)
            {
                let forward = string(image, rva as usize, dir.rva as usize + dir.size as usize)?;
                let (dll, name) = forward
                    .rsplit_once('.')
                    .filter(|(dll, name)| !dll.is_empty() && !name.is_empty())
                    .ok_or("invalid PE export forwarder")?;
                let symbol = if let Some(ordinal) = name.strip_prefix('#') {
                    Symbol::Ordinal(
                        ordinal
                            .parse()
                            .map_err(|_| "invalid PE forwarded ordinal")?,
                    )
                } else {
                    Symbol::Name(name.to_owned())
                };
                Export::Forward {
                    dll: linker::module_name(dll)?,
                    symbol,
                }
            } else {
                if rva >= self.size {
                    return Err("PE export target outside image".into());
                }
                Export::Address(rva)
            };
            result.ordinals.insert(ordinal, value);
        }
        for index in 0..names {
            let name = string(
                image,
                word(image, strings + index * 4, 4)? as usize,
                image.len(),
            )?;
            let relative = word(image, ordinals + index * 2, 2)? as usize;
            if relative >= count {
                return Err("PE named export ordinal outside table".into());
            }
            let ordinal = u16::try_from(first + relative as u64)
                .map_err(|_| "PE named export ordinal exceeds 16 bits")?;
            if !result.ordinals.contains_key(&ordinal) {
                return Err("PE named export targets an empty slot".into());
            }
            if result.names.insert(name, ordinal).is_some() {
                return Err("duplicate PE export name".into());
            }
        }
        Ok(result)
    }
}
