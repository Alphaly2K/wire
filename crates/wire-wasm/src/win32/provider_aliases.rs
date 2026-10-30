//! Exact provider declarations; service admission still belongs to Services.
use crate::machine::Mode;

struct Set {
    dll: &'static str,
    exports: &'static [(&'static str, u8)],
}

const SETS: &[Set] = include!("../../../../third_party/mingw-w64/provider-aliases.rs");

fn find(dll: &str) -> Option<&'static Set> {
    SETS.binary_search_by(|set| {
        set.dll
            .bytes()
            .cmp(dll.bytes().map(|c| c.to_ascii_lowercase()))
    })
    .ok()
    .map(|index| &SETS[index])
}

fn mask(mode: Mode) -> u8 {
    if mode == Mode::X86 {
        1
    } else {
        2
    }
}

pub(super) fn known(dll: &str) -> bool {
    find(dll).is_some()
}

pub(super) fn provider(dll: &str, mode: Mode) -> bool {
    find(dll).is_some_and(|set| set.exports.iter().any(|(_, bits)| bits & mask(mode) != 0))
}

pub(super) fn declares(dll: &str, name: &str, mode: Mode) -> bool {
    find(dll).is_some_and(|set| {
        set.exports
            .binary_search_by_key(&name, |(name, _)| *name)
            .is_ok_and(|index| set.exports[index].1 & mask(mode) != 0)
    })
}

/// Declared DLL identities, including contracts whose operation is unavailable.
pub fn providers() -> impl Iterator<Item = &'static str> {
    SETS.iter().map(|set| set.dll)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::win32::{catalog, Services};
    use std::collections::BTreeSet;

    #[test]
    fn provider_identities_fit_the_reserved_area_and_stop_at_its_boundary() {
        use crate::win32::gateways;
        let names: BTreeSet<_> = catalog::contracts()
            .flat_map(|c| {
                c.dlls(catalog::Architecture::X86)
                    .iter()
                    .chain(c.dlls(catalog::Architecture::X64))
            })
            .copied()
            .chain(providers())
            .collect();
        assert!(names.len() > 64 && names.len() < gateways::PROVIDER_CAPACITY);
        for mode in [Mode::X86, Mode::X64] {
            let area = gateways::provider_area(mode);
            let calls = gateways::layout(mode).services;
            let delays = gateways::delay_area(mode);
            assert!(area.end <= calls.start || calls.end <= area.start);
            assert!(area.end <= delays.start || delays.end <= area.start);
            let mut services = Services::new(mode);
            for name in &names {
                let handle = services.register_provider(name).unwrap();
                assert!(area.contains(&handle));
                assert_eq!(services.provider_name(handle), Some(*name));
            }
            for n in names.len()..gateways::PROVIDER_CAPACITY {
                services
                    .register_provider(&format!("capacity-{n}.dll"))
                    .unwrap();
            }
            let count = services.providers().count();
            assert!(services.register_provider("overflow.dll").is_err());
            assert_eq!(services.providers().count(), count);
            let first = *names.first().unwrap();
            assert!(services.register_provider(first).is_ok());
            assert_eq!(services.provider_name(area.end), None);
        }
    }

    #[test]
    fn aliases_keep_canonical_admission_and_gateway_identity() {
        assert!(SETS.windows(2).all(|pair| pair[0].dll < pair[1].dll));
        let owners: BTreeSet<_> = catalog::contracts()
            .flat_map(|c| {
                c.dlls(catalog::Architecture::X86)
                    .iter()
                    .chain(c.dlls(catalog::Architecture::X64))
            })
            .copied()
            .filter(|owner| !known(owner))
            .collect();
        for set in SETS {
            assert!(set.exports.windows(2).all(|pair| pair[0].0 < pair[1].0));
            for &(name, bits) in set.exports {
                assert!(catalog::by_name(name).is_some());
                assert!((1..=3).contains(&bits));
                for mode in [Mode::X86, Mode::X64] {
                    let mut services = Services::new(mode);
                    let canonical = owners
                        .iter()
                        .find_map(|owner| services.bind(owner, name).ok());
                    let alias = services.bind(set.dll, name).ok();
                    assert_eq!(
                        alias,
                        canonical.filter(|_| bits & mask(mode) != 0),
                        "{}!{name} in {}-bit mode",
                        set.dll,
                        mode as u32
                    );
                    if alias.is_some() {
                        assert_eq!(services.lookup(set.dll, name).ok(), alias);
                    }
                }
            }
        }
        for mode in [Mode::X86, Mode::X64] {
            let mut services = Services::new(mode);
            assert!(services
                .bind("api-ms-win-core-heap-l1-1-0.dll", "ReadFile")
                .is_err());
            assert!(services
                .bind("api-ms-win-core-file-l1-1-0.dll", "HeapAlloc")
                .is_err());
            assert!(services
                .bind("api-ms-win-core-file-l1-1-0.dll", "#8")
                .is_err());
            assert!(services
                .bind("api-ms-win-core-heap-l1-1-0.dll", "heapalloc")
                .is_err());
            assert!(services
                .bind("api-ms-win-core-libraryloader-l1-2-0.dll", "LoadLibraryW")
                .is_err());
            assert!(services
                .bind("api-ms-win-core-libraryloader-l1-2-1.dll", "LoadLibraryW")
                .is_ok());
            assert_eq!(
                services.bind("API-MS-WIN-CORE-HEAP-L1-1-0.DLL", "HeapAlloc"),
                services.bind("kernel32.dll", "HeapAlloc")
            );
            assert!(!crate::pe::linker::provider(
                "api-ms-win-core-file-l1-999-0.dll",
                mode
            ));
        }
    }
}
