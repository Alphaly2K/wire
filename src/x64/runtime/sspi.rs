//! SSPI service discovery with an empty security-package registry.
//! Win64 ABI checked against Wine 10.0 include/sspi.h and secur32.spec;
//! no Wine implementation is copied. Authentication/TLS are not available yet.
use super::{Memory, Runtime64, PAGE, READ, WRITE};
const INVALID_HANDLE: u64 = 0x80090301;
const PACKAGE_NOT_FOUND: u64 = 0x80090305;
const INVALID_PARAMETER: u64 = 0x8009035d;

pub(super) fn arity(name: &str) -> Option<usize> {
    Some(match name {
        "InitSecurityInterfaceA" | "InitSecurityInterfaceW" => 0,
        "EnumerateSecurityPackagesA"
        | "EnumerateSecurityPackagesW"
        | "QuerySecurityPackageInfoA"
        | "QuerySecurityPackageInfoW"
        | "ApplyControlToken"
        | "CompleteAuthToken"
        | "QuerySecurityContextToken" => 2,
        "QueryCredentialsAttributesA"
        | "QueryCredentialsAttributesW"
        | "QueryContextAttributesA"
        | "QueryContextAttributesW" => 3,
        "AcquireCredentialsHandleA" | "AcquireCredentialsHandleW" | "AcceptSecurityContext" => 9,
        "InitializeSecurityContextA" | "InitializeSecurityContextW" => 12,
        "AddCredentialsA" | "AddCredentialsW" => 8,
        "FreeContextBuffer"
        | "FreeCredentialsHandle"
        | "DeleteSecurityContext"
        | "ImpersonateSecurityContext"
        | "RevertSecurityContext" => 1,
        "MakeSignature"
        | "VerifySignature"
        | "EncryptMessage"
        | "DecryptMessage"
        | "ExportSecurityContext"
        | "ImportSecurityContextA"
        | "ImportSecurityContextW"
        | "SetContextAttributesA"
        | "SetContextAttributesW" => 4,
        _ => return None,
    })
}
pub(super) fn dwords(name: &str) -> &'static [usize] {
    match name {
        "AcquireCredentialsHandleA" | "AcquireCredentialsHandleW" => &[2],
        "InitializeSecurityContextA" | "InitializeSecurityContextW" => &[3, 4, 5, 7],
        "AcceptSecurityContext" => &[3, 4],
        "QueryCredentialsAttributesA"
        | "QueryCredentialsAttributesW"
        | "QueryContextAttributesA"
        | "QueryContextAttributesW"
        | "ExportSecurityContext" => &[1],
        "MakeSignature" | "EncryptMessage" => &[1, 3],
        "VerifySignature" | "DecryptMessage" => &[2],
        "SetContextAttributesA" | "SetContextAttributesW" => &[1, 3],
        "AddCredentialsA" | "AddCredentialsW" => &[3],
        _ => &[],
    }
}

impl Runtime64 {
    fn security_table(&mut self, mem: &mut Memory, wide: bool) -> Result<u64, String> {
        let index = wide as usize;
        if self.security_tables[index] != 0 {
            return Ok(self.security_tables[index]);
        }
        // Dedicated runtime-owned page: never entered in the guest heap or in
        // FreeContextBuffer's ownership set. It survives module refcount changes.
        let address = self.next_alloc;
        mem.map(address, PAGE, READ | WRITE)?;
        let mut table = [0; 224];
        table[..4].copy_from_slice(&1u32.to_le_bytes());
        for (slot, name) in [
            (0, "EnumerateSecurityPackages"),
            (1, "QueryCredentialsAttributes"),
            (2, "AcquireCredentialsHandle"),
            (3, "FreeCredentialsHandle"),
            (5, "InitializeSecurityContext"),
            (6, "AcceptSecurityContext"),
            (7, "CompleteAuthToken"),
            (8, "DeleteSecurityContext"),
            (9, "ApplyControlToken"),
            (10, "QueryContextAttributes"),
            (11, "ImpersonateSecurityContext"),
            (12, "RevertSecurityContext"),
            (13, "MakeSignature"),
            (14, "VerifySignature"),
            (15, "FreeContextBuffer"),
            (16, "QuerySecurityPackageInfo"),
            (19, "ExportSecurityContext"),
            (20, "ImportSecurityContext"),
            (21, "AddCredentials"),
            (23, "QuerySecurityContextToken"),
            (24, "EncryptMessage"),
            (25, "DecryptMessage"),
            (26, "SetContextAttributes"),
        ] {
            let variant = if matches!(slot, 0 | 1 | 2 | 5 | 10 | 16 | 20 | 21 | 26) {
                format!("{name}{}", if wide { 'W' } else { 'A' })
            } else {
                name.to_owned()
            };
            let gateway = self.register("secur32.dll", &variant);
            table[8 + slot * 8..16 + slot * 8].copy_from_slice(&gateway.to_le_bytes());
        }
        mem.write(address, &table)?;
        mem.protect(address, PAGE, READ)?;
        self.next_alloc += PAGE;
        self.security_tables[index] = address;
        Ok(address)
    }
    pub(super) fn security_call(
        &mut self,
        mem: &mut Memory,
        name: &str,
        args: &[u64],
    ) -> Result<u64, String> {
        match name {
            "InitSecurityInterfaceA" | "InitSecurityInterfaceW" => {
                self.security_table(mem, name.ends_with('W'))
            }
            "EnumerateSecurityPackagesA" | "EnumerateSecurityPackagesW" => {
                // Preflight both fields before committing either. The package
                // registry is empty, so there is no allocated buffer to free.
                mem.check(args[0], 4, WRITE)?;
                mem.check(args[1], 8, WRITE)?;
                mem.write_int(args[0], 4, 0)?;
                mem.write_int(args[1], 8, 0)?;
                Ok(0)
            }
            "QuerySecurityPackageInfoA"
            | "QuerySecurityPackageInfoW"
            | "AcquireCredentialsHandleA"
            | "AcquireCredentialsHandleW"
            | "ImportSecurityContextA"
            | "ImportSecurityContextW" => {
                let package = args[if name.starts_with("Acquire") { 1 } else { 0 }];
                if package == 0 {
                    return Ok(INVALID_PARAMETER);
                }
                Self::path_name(mem, package, name.ends_with('W'))?;
                // No Schannel/NTLM/Kerberos package is advertised. Preserve all
                // caller outputs and LastError on SECURITY_STATUS failure.
                Ok(PACKAGE_NOT_FOUND)
            }
            "FreeContextBuffer" if args[0] == 0 => Ok(0),
            // No credential/context handles have ever been issued. Do not report
            // authentication, signatures, encryption or impersonation success.
            _ => Ok(INVALID_HANDLE),
        }
    }
}
