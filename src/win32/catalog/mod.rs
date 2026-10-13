//! Export identity and ABI plans are resolved during binding, not execution.
//! The GUID pilot deliberately leaves unconverted families on legacy routes.
mod guid;
pub use guid::{Contract, GuidApi, GUID_CONTRACTS};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Architecture {
    X86,
    X64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parameter {
    Pointer,
    Int32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReturnConvention {
    CharacterCount,
    Hresult,
}

#[derive(Clone, Copy, Debug)]
pub enum Resolution {
    Bound(&'static Contract),
    WrongDll(&'static Contract),
    Legacy,
}

pub fn resolve(architecture: Architecture, dll: &str, name: &str) -> Resolution {
    let Some(contract) = by_name(name) else {
        return Resolution::Legacy;
    };
    if contract
        .dlls(architecture)
        .iter()
        .any(|owner| dll.eq_ignore_ascii_case(owner))
    {
        Resolution::Bound(contract)
    } else {
        Resolution::WrongDll(contract)
    }
}

pub(crate) fn by_name(name: &str) -> Option<&'static Contract> {
    GUID_CONTRACTS.iter().find(|contract| contract.name == name)
}
