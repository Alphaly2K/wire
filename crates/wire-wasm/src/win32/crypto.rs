//! Ephemeral CryptoAPI contexts and process-owned incremental hashes.
use super::{
    call::Call,
    catalog::CryptoApi as C,
    context,
    modules::{name, Failure},
    ServiceError,
};
use crate::{
    machine::{Mode, State},
    memory::{Memory, READ, WRITE},
    process::Loaded,
};
use sha2::Digest;
use std::collections::BTreeMap;

const BAD_UID: u32 = 0x80090001;
const BAD_HASH: u32 = 0x80090002;
const BAD_FLAGS: u32 = 0x80090009;
const BAD_TYPE: u32 = 0x8009000a;
const VERIFY: u32 = 0xf0000000;

#[derive(Clone)]
enum Hasher {
    Md5(md5::Md5),
    Sha1(sha1::Sha1),
    Sha256(sha2::Sha256),
    Sha384(sha2::Sha384),
    Sha512(sha2::Sha512),
}
impl Hasher {
    fn new(algorithm: u32, aes: bool) -> Result<Self, Failure> {
        Ok(match algorithm {
            0x8003 => Self::Md5(md5::Md5::new()),
            0x8004 => Self::Sha1(sha1::Sha1::new()),
            0x800c if aes => Self::Sha256(sha2::Sha256::new()),
            0x800d if aes => Self::Sha384(sha2::Sha384::new()),
            0x800e if aes => Self::Sha512(sha2::Sha512::new()),
            _ => return Err(Failure::Windows(0x80090008)),
        })
    }
    fn update(&mut self, bytes: &[u8]) {
        match self {
            Self::Md5(h) => h.update(bytes),
            Self::Sha1(h) => h.update(bytes),
            Self::Sha256(h) => h.update(bytes),
            Self::Sha384(h) => h.update(bytes),
            Self::Sha512(h) => h.update(bytes),
        }
    }
    fn finish(self) -> Vec<u8> {
        match self {
            Self::Md5(h) => h.finalize().to_vec(),
            Self::Sha1(h) => h.finalize().to_vec(),
            Self::Sha256(h) => h.finalize().to_vec(),
            Self::Sha384(h) => h.finalize().to_vec(),
            Self::Sha512(h) => h.finalize().to_vec(),
        }
    }
}
struct Provider {
    aes: bool,
    references: u32,
}
#[derive(Clone)]
struct Hash {
    provider: u64,
    algorithm: u32,
    hasher: Hasher,
    finished: Option<Vec<u8>>,
}
impl Hash {
    fn size(&self) -> u32 {
        match self.algorithm {
            0x8003 => 16,
            0x8004 => 20,
            0x800c => 32,
            0x800d => 48,
            0x800e => 64,
            _ => unreachable!(),
        }
    }
}

pub(crate) struct Crypto {
    providers: BTreeMap<u64, Provider>,
    hashes: BTreeMap<u64, Hash>,
    next: u64,
    end: u64,
}
impl Crypto {
    pub(crate) fn new(mode: Mode) -> Self {
        let start = if mode == Mode::X86 {
            0xa0030000
        } else {
            0x7ffd07000000
        };
        Self {
            providers: BTreeMap::new(),
            hashes: BTreeMap::new(),
            next: start,
            end: start + 65536,
        }
    }
    pub(crate) fn clear(&mut self) {
        self.providers.clear();
        self.hashes.clear();
    }
    fn vacant(&self) -> Result<u64, Failure> {
        if self.providers.len() + self.hashes.len() >= 1024 || self.next >= self.end {
            Err(Failure::Windows(8))
        } else {
            Ok(self.next)
        }
    }
    fn provider(&self, handle: u64) -> Result<&Provider, Failure> {
        self.providers.get(&handle).ok_or(Failure::Windows(BAD_UID))
    }
    fn hash(&self, handle: u64) -> Result<&Hash, Failure> {
        self.hashes.get(&handle).ok_or(Failure::Windows(BAD_HASH))
    }
    fn acquire(&mut self, id: C, a: &[u64; 16], memory: &mut Memory) -> Result<u64, Failure> {
        let flags = a[4] as u32;
        if a[0] == 0 {
            return Err(Failure::Windows(87));
        }
        if flags & !(VERIFY | 0x60) != 0 || flags & VERIFY != 0 && flags & VERIFY != VERIFY {
            return Err(Failure::Windows(BAD_FLAGS));
        }
        let kind = a[3] as u32;
        if !matches!(kind, 1 | 24) {
            return Err(Failure::Windows(if kind == 0 {
                0x80090014
            } else {
                0x80090017
            }));
        }
        let wide = id == C::AcquireW;
        if a[2] != 0 {
            let provider = name(memory, a[2], wide)?;
            let supported: &[&str] = if kind == 24 {
                &["Microsoft Enhanced RSA and AES Cryptographic Provider"]
            } else {
                &[
                    "Microsoft Enhanced Cryptographic Provider v1.0",
                    "Microsoft Base Cryptographic Provider v1.0",
                    "Microsoft Strong Cryptographic Provider",
                ]
            };
            if !supported.iter().any(|s| s.eq_ignore_ascii_case(&provider)) {
                return Err(Failure::Windows(0x80090013));
            }
        }
        if flags & VERIFY == 0 || a[1] != 0 && !name(memory, a[1], wide)?.is_empty() {
            return Err(Failure::Windows(0x80090016));
        }
        let handle = self.vacant()?;
        memory.write_int(a[0], memory.mode() as usize / 8, handle)?;
        self.providers.insert(
            handle,
            Provider {
                aes: kind == 24,
                references: 1,
            },
        );
        self.next += 1;
        Ok(1)
    }
    fn query(&mut self, a: &[u64; 16], memory: &mut Memory, error: u64) -> Result<u64, Failure> {
        let hash = self.hash(a[0])?;
        if a[4] as u32 != 0 {
            return Err(Failure::Windows(BAD_FLAGS));
        }
        if a[3] == 0 {
            return Err(Failure::Windows(87));
        }
        let param = a[1] as u32;
        let bytes = match param {
            1 => hash.algorithm.to_le_bytes().to_vec(),
            2 => hash
                .finished
                .clone()
                .unwrap_or_else(|| hash.hasher.clone().finish()),
            4 => hash.size().to_le_bytes().to_vec(),
            _ => return Err(Failure::Windows(BAD_TYPE)),
        };
        memory.check(a[3], 4, WRITE)?;
        let short = a[2] != 0 && memory.read_int(a[3], 4)? < bytes.len() as u64;
        if short {
            memory.check(error, 4, WRITE)?;
        } else if a[2] != 0 {
            memory.check(a[2], bytes.len(), WRITE)?;
        }
        if !short && a[2] != 0 {
            memory.write(a[2], &bytes)?;
        }
        memory.write_int(a[3], 4, bytes.len() as u64)?;
        if short {
            memory.write_int(error, 4, 234)?;
        }
        // A valid HASHVAL query completes the hash even when only size is requested.
        if param == 2 {
            self.hashes.get_mut(&a[0]).unwrap().finished = Some(bytes);
        }
        Ok(u64::from(!short))
    }
    fn run(
        &mut self,
        id: C,
        a: &[u64; 16],
        memory: &mut Memory,
        error: u64,
    ) -> Result<u64, Failure> {
        match id {
            C::AcquireA | C::AcquireW => return self.acquire(id, a, memory),
            C::GetHashParam => return self.query(a, memory, error),
            C::AddRef => {
                let p = self.provider(a[0])?;
                if a[1] != 0 {
                    return Err(Failure::Windows(87));
                }
                if a[2] as u32 != 0 {
                    return Err(Failure::Windows(BAD_FLAGS));
                }
                let refs = p.references.checked_add(1).ok_or(Failure::Windows(8))?;
                self.providers.get_mut(&a[0]).unwrap().references = refs;
            }
            C::Release => {
                let p = self.provider(a[0])?;
                if a[1] as u32 != 0 {
                    return Err(Failure::Windows(BAD_FLAGS));
                }
                if p.references == 1 {
                    self.providers.remove(&a[0]);
                    self.hashes.retain(|_, hash| hash.provider != a[0]);
                } else {
                    self.providers.get_mut(&a[0]).unwrap().references -= 1;
                }
            }
            C::CreateHash => {
                let p = self.provider(a[0])?;
                let hasher = Hasher::new(a[1] as u32, p.aes)?;
                if a[3] as u32 != 0 {
                    return Err(Failure::Windows(BAD_FLAGS));
                }
                if a[2] != 0 {
                    return Err(Failure::Windows(0x80090003));
                }
                if a[4] == 0 {
                    return Err(Failure::Windows(87));
                }
                let handle = self.vacant()?;
                memory.write_int(a[4], memory.mode() as usize / 8, handle)?;
                self.hashes.insert(
                    handle,
                    Hash {
                        provider: a[0],
                        algorithm: a[1] as u32,
                        hasher,
                        finished: None,
                    },
                );
                self.next += 1;
            }
            C::DuplicateHash => {
                let hash = self.hash(a[0])?;
                if a[1] != 0 || a[2] as u32 != 0 || a[3] == 0 {
                    return Err(Failure::Windows(87));
                }
                let handle = self.vacant()?;
                let copy = hash.clone();
                memory.write_int(a[3], memory.mode() as usize / 8, handle)?;
                self.hashes.insert(handle, copy);
                self.next += 1;
            }
            C::DestroyHash => {
                self.hash(a[0])?;
                self.hashes.remove(&a[0]);
            }
            C::HashData => {
                if a[3] as u32 & !1 != 0 {
                    return Err(Failure::Windows(BAD_FLAGS));
                }
                let hash = self.hash(a[0])?;
                if hash.finished.is_some() {
                    return Err(Failure::Windows(0x8009000c));
                }
                let length = a[2] as u32 as usize;
                if length != 0 {
                    if a[1] == 0 {
                        return Err(Failure::Windows(87));
                    }
                    memory.check(a[1], length, READ)?;
                    let hash = self.hashes.get_mut(&a[0]).unwrap();
                    let mut scratch = [0; 4096];
                    for offset in (0..length).step_by(scratch.len()) {
                        let n = (length - offset).min(scratch.len());
                        memory
                            .read(a[1] + offset as u64, &mut scratch[..n])
                            .expect("preflighted hash input");
                        hash.hasher.update(&scratch[..n]);
                    }
                }
            }
        }
        Ok(1)
    }
}

pub(super) fn dispatch(
    id: C,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
) -> Result<u64, ServiceError> {
    let crypto = &mut image
        .ok_or(ServiceError::Unavailable(
            "CryptoAPI requires a loaded image",
        ))?
        .crypto;
    let error = context::error_address(state, memory.mode());
    match crypto.run(id, &call.arguments, memory, error) {
        Ok(value) => Ok(value),
        Err(Failure::Fault(f)) => Err(f.into()),
        Err(Failure::Windows(code)) => {
            memory.write_int(error, 4, code.into())?;
            Ok(0)
        }
    }
}
