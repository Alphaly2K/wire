use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CryptoApi {
    AcquireA,
    AcquireW,
    AddRef,
    Release,
    CreateHash,
    HashData,
    GetHashParam,
    DuplicateHash,
    DestroyHash,
}

const fn declaration(
    id: CryptoApi,
    name: &'static str,
    parameters: &'static [Parameter],
) -> Contract {
    Contract::new(
        ApiId::Crypto(id), name, parameters, ReturnConvention::BoolLastError,
        "process-owned ephemeral RSA_FULL/RSA_AES hash contexts, MD5/SHA1 and RSA_AES SHA256/SHA384/SHA512; incrementally hashed input, independent duplicates, HP_ALGID/HASHSIZE/HASHVAL with full size/error/output preflight; HASHVAL completes on size query or short capacity; no keys, containers, signing or encryption; complete guest pointers and DWORD carriers, success preserves LastError",
        [ArchitecturePlan { dlls: &["advapi32.dll"], execution: ExecutionPolicy::Typed }; 2],
    )
}

use Parameter::{Pointer as P, Uint32 as D};
pub static CRYPTO_CONTRACTS: [Contract; 9] = [
    declaration(
        CryptoApi::AcquireA,
        "CryptAcquireContextA",
        &[P, P, P, D, D],
    ),
    declaration(
        CryptoApi::AcquireW,
        "CryptAcquireContextW",
        &[P, P, P, D, D],
    ),
    declaration(CryptoApi::AddRef, "CryptContextAddRef", &[P, P, D]),
    declaration(CryptoApi::Release, "CryptReleaseContext", &[P, D]),
    declaration(CryptoApi::CreateHash, "CryptCreateHash", &[P, D, P, D, P]),
    declaration(CryptoApi::HashData, "CryptHashData", &[P, P, D, D]),
    declaration(
        CryptoApi::GetHashParam,
        "CryptGetHashParam",
        &[P, D, P, P, D],
    ),
    declaration(
        CryptoApi::DuplicateHash,
        "CryptDuplicateHash",
        &[P, P, D, P],
    ),
    declaration(CryptoApi::DestroyHash, "CryptDestroyHash", &[P]),
];
