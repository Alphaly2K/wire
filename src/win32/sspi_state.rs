//! SSPI discovery for an empty security-package registry.
use crate::win32::catalog::SspiApi;
pub(crate) trait Backend {
    type Error;
    fn width(&self) -> usize;
    fn install(&mut self, wide: bool) -> Result<u64, Self::Error>;
    fn check_write(&self, address: u64, length: usize) -> Result<(), Self::Error>;
    fn write_int(&mut self, address: u64, width: usize, value: u64) -> Result<(), Self::Error>;
    fn check_name(&self, address: u64, wide: bool) -> Result<(), Self::Error>;
}
#[derive(Default)]
pub(crate) struct Security {
    tables: [u64; 2],
}
pub(crate) fn exports(wide: bool) -> Vec<(usize, String)> {
    [
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
    ]
    .into_iter()
    .map(|(slot, name)| {
        let name = if matches!(slot, 0 | 1 | 2 | 5 | 10 | 16 | 20 | 21 | 26) {
            format!("{name}{}", if wide { 'W' } else { 'A' })
        } else {
            name.to_owned()
        };
        (slot, name)
    })
    .collect()
}
pub(crate) fn encode(width: usize, gateways: &[(usize, u64)]) -> Vec<u8> {
    let mut bytes = vec![0; 28 * width];
    bytes[..4].copy_from_slice(&1u32.to_le_bytes());
    for &(slot, gateway) in gateways {
        bytes[(slot + 1) * width..(slot + 2) * width]
            .copy_from_slice(&gateway.to_le_bytes()[..width]);
    }
    bytes
}
impl Security {
    pub(crate) fn invoke<B: Backend>(
        &mut self,
        mem: &mut B,
        id: SspiApi,
        args: &[u64],
    ) -> Result<u64, B::Error> {
        use SspiApi::*;
        match id {
            InterfaceA | InterfaceW => {
                let index = usize::from(id.wide());
                if self.tables[index] == 0 {
                    self.tables[index] = mem.install(id.wide())?;
                }
                Ok(self.tables[index])
            }
            EnumerateA | EnumerateW => {
                mem.check_write(args[0], 4)?;
                mem.check_write(args[1], mem.width())?;
                mem.write_int(args[0], 4, 0)?;
                mem.write_int(args[1], mem.width(), 0)?;
                Ok(0)
            }
            PackageA | PackageW | AcquireA | AcquireW | ImportA | ImportW => {
                let package = args[if matches!(id, AcquireA | AcquireW) {
                    1
                } else {
                    0
                }];
                if package == 0 {
                    return Ok(0x8009035d);
                }
                mem.check_name(package, id.wide())?;
                Ok(0x80090305)
            }
            FreeBuffer if args[0] == 0 => Ok(0),
            _ => Ok(0x80090301),
        }
    }
}
