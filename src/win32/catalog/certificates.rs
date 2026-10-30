use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertificateApi {
    OpenStore,
    CloseStore,
    AddEncoded,
    FreeContext,
    GetChain,
    FreeChain,
    VerifyPolicy,
}

const fn declaration(
    id: CertificateApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(ApiId::Certificate(id), name, parameters, returns,
        "No certificate store, X.509 parser, chain builder, trust policy or certificate owner; constructors/validation explicitly fail, NULL frees are empty, foreign context releases stop without claiming success",
        [ArchitecturePlan { dlls: &["crypt32.dll"], execution: ExecutionPolicy::LegacyUnimplemented }; 2])
}
use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Pointer as R, Void as V};
pub static CERTIFICATE_CONTRACTS: [Contract; 7] = [
    declaration(
        CertificateApi::OpenStore,
        "CertOpenStore",
        &[P, D, P, D, P],
        R,
    ),
    declaration(CertificateApi::CloseStore, "CertCloseStore", &[P, D], B),
    declaration(
        CertificateApi::AddEncoded,
        "CertAddEncodedCertificateToStore",
        &[P, D, P, D, D, P],
        B,
    ),
    declaration(
        CertificateApi::FreeContext,
        "CertFreeCertificateContext",
        &[P],
        B,
    ),
    declaration(
        CertificateApi::GetChain,
        "CertGetCertificateChain",
        &[P, P, P, P, P, D, P, P],
        B,
    ),
    declaration(
        CertificateApi::FreeChain,
        "CertFreeCertificateChain",
        &[P],
        V,
    ),
    declaration(
        CertificateApi::VerifyPolicy,
        "CertVerifyCertificateChainPolicy",
        &[P, P, P, P],
        B,
    ),
];
