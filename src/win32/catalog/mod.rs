//! Export identity and ABI plans are resolved during binding, not execution.
//! Unconverted families remain on explicit legacy routes.
mod guid;
mod identity;
pub use guid::{GuidApi, GUID_CONTRACTS};
pub use identity::{IdentityApi, IDENTITY_CONTRACTS};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Architecture {
    X86,
    X64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parameter {
    Pointer,
    Int32,
    Uint32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReturnConvention {
    CharacterCount,
    Hresult,
    BoolLastError,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiId {
    Guid(GuidApi),
    Identity(IdentityApi),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionPolicy {
    Typed,
    /// ABI is known; preserve the PE32 unimplemented diagnostic/return policy.
    LegacyUnimplemented,
}

#[derive(Clone, Copy, Debug)]
pub struct ArchitecturePlan {
    pub dlls: &'static [&'static str],
    pub execution: ExecutionPolicy,
}

#[derive(Debug)]
pub struct Contract {
    pub id: ApiId,
    pub name: &'static str,
    pub parameters: &'static [Parameter],
    pub returns: ReturnConvention,
    pub limits: &'static str,
    plans: [ArchitecturePlan; 2],
    dwords: [usize; 16],
    dword_count: usize,
}

impl Contract {
    pub(super) const fn new(
        id: ApiId,
        name: &'static str,
        parameters: &'static [Parameter],
        returns: ReturnConvention,
        limits: &'static str,
        plans: [ArchitecturePlan; 2],
    ) -> Self {
        assert!(parameters.len() <= 16, "scalar gateway argument limit");
        let mut dwords = [0; 16];
        let mut dword_count = 0;
        let mut i = 0;
        while i < parameters.len() {
            if matches!(parameters[i], Parameter::Int32 | Parameter::Uint32) {
                dwords[dword_count] = i;
                dword_count += 1;
            }
            i += 1;
        }
        Self {
            id,
            name,
            parameters,
            returns,
            limits,
            plans,
            dwords,
            dword_count,
        }
    }
    pub fn dlls(&self, architecture: Architecture) -> &'static [&'static str] {
        match architecture {
            Architecture::X86 => self.plans[0].dlls,
            Architecture::X64 => self.plans[1].dlls,
        }
    }
    pub fn execution(&self, architecture: Architecture) -> ExecutionPolicy {
        self.plans[match architecture {
            Architecture::X86 => 0,
            Architecture::X64 => 1,
        }]
        .execution
    }
    pub fn stdcall_bytes(&self) -> u32 {
        self.parameters.len() as u32 * 4
    }
    pub fn win64_dwords(&'static self) -> &'static [usize] {
        &self.dwords[..self.dword_count]
    }
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
    contracts().find(|contract| contract.name == name)
}

pub fn contracts() -> impl Iterator<Item = &'static Contract> {
    GUID_CONTRACTS.iter().chain(IDENTITY_CONTRACTS.iter())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn declarations_have_unique_exports_and_valid_architecture_plans() {
        let mut names = std::collections::HashSet::new();
        for contract in contracts() {
            assert!(
                names.insert(contract.name),
                "duplicate export {}",
                contract.name
            );
            assert!(!contract.limits.is_empty());
            assert!(contract
                .win64_dwords()
                .iter()
                .all(|&i| i < contract.parameters.len()));
            for architecture in [Architecture::X86, Architecture::X64] {
                let mut owners = std::collections::HashSet::new();
                for dll in contract.dlls(architecture) {
                    assert_eq!(*dll, dll.to_ascii_lowercase());
                    assert!(owners.insert(*dll), "duplicate owner {}", dll);
                }
                assert!(!owners.is_empty());
            }
        }
    }
}
