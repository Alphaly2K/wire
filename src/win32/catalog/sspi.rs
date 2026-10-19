//! SSPI service discovery exposes an empty security-package registry.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SspiApi {
    InterfaceA,
    InterfaceW,
    EnumerateA,
    EnumerateW,
    PackageA,
    PackageW,
    AcquireA,
    AcquireW,
    InitializeA,
    InitializeW,
    AddA,
    AddW,
    Accept,
    FreeBuffer,
    FreeCredentials,
    DeleteContext,
    Impersonate,
    Revert,
    CredentialsAttributesA,
    CredentialsAttributesW,
    ContextAttributesA,
    ContextAttributesW,
    Apply,
    Complete,
    ContextToken,
    Sign,
    Verify,
    Encrypt,
    Decrypt,
    Export,
    ImportA,
    ImportW,
    SetAttributesA,
    SetAttributesW,
}
impl SspiApi {
    pub(crate) const fn wide(self) -> bool {
        matches!(
            self,
            Self::InterfaceW
                | Self::EnumerateW
                | Self::PackageW
                | Self::AcquireW
                | Self::InitializeW
                | Self::AddW
                | Self::CredentialsAttributesW
                | Self::ContextAttributesW
                | Self::ImportW
                | Self::SetAttributesW
        )
    }
}

const fn declaration(
    id: SspiApi,
    name: &'static str,
    parameters: &'static [Parameter],
) -> Contract {
    Contract::new(
        ApiId::Sspi(id), name, parameters,
        if matches!(id, SspiApi::InterfaceA | SspiApi::InterfaceW) { ReturnConvention::Pointer } else { ReturnConvention::Hresult },
        "PE32 absent/unknown ABI; Win64 static secur32/kernel32/kernelbase and dynamic secur32 or registered gateway lookup; stable per-process read-only A/W function tables with original slots, reserved NULLs and allocation/registration order; empty package list preflights both outputs, package lookup returns PACKAGE_NOT_FOUND with original name-read order, no credential/context handles or authentication/TLS, NULL FreeContextBuffer succeeds, other operations preserve outputs/LastError and report INVALID_HANDLE",
        [ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent },
         ArchitecturePlan { dlls: &["secur32.dll", "kernel32.dll", "kernelbase.dll"], execution: ExecutionPolicy::Typed }],
    ).with_lookup_dlls([&[], &["secur32.dll"]])
}

use Parameter::{Pointer as P, Uint32 as D};
pub static SSPI_CONTRACTS: [Contract; 34] = [
    declaration(SspiApi::InterfaceA, "InitSecurityInterfaceA", &[]),
    declaration(SspiApi::InterfaceW, "InitSecurityInterfaceW", &[]),
    declaration(SspiApi::EnumerateA, "EnumerateSecurityPackagesA", &[P, P]),
    declaration(SspiApi::EnumerateW, "EnumerateSecurityPackagesW", &[P, P]),
    declaration(SspiApi::PackageA, "QuerySecurityPackageInfoA", &[P, P]),
    declaration(SspiApi::PackageW, "QuerySecurityPackageInfoW", &[P, P]),
    declaration(
        SspiApi::AcquireA,
        "AcquireCredentialsHandleA",
        &[P, P, D, P, P, P, P, P, P],
    ),
    declaration(
        SspiApi::AcquireW,
        "AcquireCredentialsHandleW",
        &[P, P, D, P, P, P, P, P, P],
    ),
    declaration(
        SspiApi::InitializeA,
        "InitializeSecurityContextA",
        &[P, P, P, D, D, D, P, D, P, P, P, P],
    ),
    declaration(
        SspiApi::InitializeW,
        "InitializeSecurityContextW",
        &[P, P, P, D, D, D, P, D, P, P, P, P],
    ),
    declaration(SspiApi::AddA, "AddCredentialsA", &[P, P, P, D, P, P, P, P]),
    declaration(SspiApi::AddW, "AddCredentialsW", &[P, P, P, D, P, P, P, P]),
    declaration(
        SspiApi::Accept,
        "AcceptSecurityContext",
        &[P, P, P, D, D, P, P, P, P],
    ),
    declaration(SspiApi::FreeBuffer, "FreeContextBuffer", &[P]),
    declaration(SspiApi::FreeCredentials, "FreeCredentialsHandle", &[P]),
    declaration(SspiApi::DeleteContext, "DeleteSecurityContext", &[P]),
    declaration(SspiApi::Impersonate, "ImpersonateSecurityContext", &[P]),
    declaration(SspiApi::Revert, "RevertSecurityContext", &[P]),
    declaration(
        SspiApi::CredentialsAttributesA,
        "QueryCredentialsAttributesA",
        &[P, D, P],
    ),
    declaration(
        SspiApi::CredentialsAttributesW,
        "QueryCredentialsAttributesW",
        &[P, D, P],
    ),
    declaration(
        SspiApi::ContextAttributesA,
        "QueryContextAttributesA",
        &[P, D, P],
    ),
    declaration(
        SspiApi::ContextAttributesW,
        "QueryContextAttributesW",
        &[P, D, P],
    ),
    declaration(SspiApi::Apply, "ApplyControlToken", &[P, P]),
    declaration(SspiApi::Complete, "CompleteAuthToken", &[P, P]),
    declaration(SspiApi::ContextToken, "QuerySecurityContextToken", &[P, P]),
    declaration(SspiApi::Sign, "MakeSignature", &[P, D, P, D]),
    declaration(SspiApi::Verify, "VerifySignature", &[P, P, D, P]),
    declaration(SspiApi::Encrypt, "EncryptMessage", &[P, D, P, D]),
    declaration(SspiApi::Decrypt, "DecryptMessage", &[P, P, D, P]),
    declaration(SspiApi::Export, "ExportSecurityContext", &[P, D, P, P]),
    declaration(SspiApi::ImportA, "ImportSecurityContextA", &[P, P, P, P]),
    declaration(SspiApi::ImportW, "ImportSecurityContextW", &[P, P, P, P]),
    declaration(
        SspiApi::SetAttributesA,
        "SetContextAttributesA",
        &[P, D, P, D],
    ),
    declaration(
        SspiApi::SetAttributesW,
        "SetContextAttributesW",
        &[P, D, P, D],
    ),
];
