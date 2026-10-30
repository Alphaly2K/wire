use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecurityApi {
    AllocateSid,
    FreeSid,
    Trustee,
    LookupSid,
    DuplicateToken,
    Impersonate,
    Revert,
    EffectiveRights,
    GetNamed,
    SetNamed,
    SetEntries,
    LogonProcess,
}
const fn declaration(
    id: SecurityApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(ApiId::Security(id), name, parameters, returns,
        "Shared virtual SID/token identity and process-heap ownership; no host credentials, ACL backend or authenticated process launch; unsupported operations return explicit errors without publishing rights or objects",
        [ArchitecturePlan { dlls: &["advapi32.dll"], execution: ExecutionPolicy::LegacyUnimplemented }; 2])
}
use Parameter::{Pointer as P, Uint32 as D, Uint8 as C};
use ReturnConvention::{BoolLastError as B, Pointer as R, Uint32 as U, Void as V};
pub static SECURITY_CONTRACTS: [Contract; 12] = [
    declaration(
        SecurityApi::AllocateSid,
        "AllocateAndInitializeSid",
        &[P, C, D, D, D, D, D, D, D, D, P],
        B,
    ),
    declaration(SecurityApi::FreeSid, "FreeSid", &[P], R),
    declaration(SecurityApi::Trustee, "BuildTrusteeWithSidW", &[P, P], V),
    declaration(
        SecurityApi::LookupSid,
        "LookupAccountSidW",
        &[P, P, P, P, P, P, P],
        B,
    ),
    declaration(SecurityApi::DuplicateToken, "DuplicateToken", &[P, D, P], B),
    declaration(SecurityApi::Impersonate, "ImpersonateLoggedOnUser", &[P], B),
    declaration(SecurityApi::Revert, "RevertToSelf", &[], B),
    declaration(
        SecurityApi::EffectiveRights,
        "GetEffectiveRightsFromAclW",
        &[P, P, P],
        U,
    ),
    declaration(
        SecurityApi::GetNamed,
        "GetNamedSecurityInfoW",
        &[P, D, D, P, P, P, P, P],
        U,
    ),
    declaration(
        SecurityApi::SetNamed,
        "SetNamedSecurityInfoW",
        &[P, D, D, P, P, P, P],
        U,
    ),
    declaration(
        SecurityApi::SetEntries,
        "SetEntriesInAclW",
        &[D, P, P, P],
        U,
    ),
    declaration(
        SecurityApi::LogonProcess,
        "CreateProcessWithLogonW",
        &[P, P, P, D, P, P, D, P, P, P, P],
        B,
    ),
];
