//! Existing SSPI static/dynamic provider scopes, independent of catalog metadata.
use super::*;

#[test]
fn sspi_preserves_static_providers_dynamic_secur32_and_registered_gateway_fallback() {
    let (mut mem, mut cpu, mut rt) = startup();
    let text = STACK + 0x100;
    for dll in ["secur32.dll", "kernel32.dll", "kernelbase.dll"] {
        rt.register_provider(dll);
        mem.write(text, format!("{dll}\0").as_bytes()).unwrap();
        let module = startup_call(&mut mem, &mut cpu, &mut rt, "LoadLibraryA", &[text]);
        for name in [
            "InitSecurityInterfaceA",
            "InitSecurityInterfaceW",
            "EnumerateSecurityPackagesA",
            "EnumerateSecurityPackagesW",
            "QuerySecurityPackageInfoA",
            "QuerySecurityPackageInfoW",
            "AcquireCredentialsHandleA",
            "AcquireCredentialsHandleW",
            "InitializeSecurityContextA",
            "InitializeSecurityContextW",
            "AddCredentialsA",
            "AddCredentialsW",
            "AcceptSecurityContext",
            "FreeContextBuffer",
            "FreeCredentialsHandle",
            "DeleteSecurityContext",
            "ImpersonateSecurityContext",
            "RevertSecurityContext",
            "QueryCredentialsAttributesA",
            "QueryCredentialsAttributesW",
            "QueryContextAttributesA",
            "QueryContextAttributesW",
            "ApplyControlToken",
            "CompleteAuthToken",
            "QuerySecurityContextToken",
            "MakeSignature",
            "VerifySignature",
            "EncryptMessage",
            "DecryptMessage",
            "ExportSecurityContext",
            "ImportSecurityContextA",
            "ImportSecurityContextW",
            "SetContextAttributesA",
            "SetContextAttributesW",
        ] {
            assert!(wire::x64::runtime::import_supported(dll, name));
            assert!(!wire::x64::runtime::import_supported("sspicli.dll", name));
            mem.write(text, format!("{name}\0").as_bytes()).unwrap();
            let initial = startup_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "GetProcAddress",
                &[module, text],
            );
            assert_eq!(initial != 0, dll == "secur32.dll", "{dll}!{name}");
            let address = rt.register(dll, name);
            assert_eq!(
                startup_call(
                    &mut mem,
                    &mut cpu,
                    &mut rt,
                    "GetProcAddress",
                    &[module, text]
                ),
                address
            );
        }
    }
}
