//! Immutable virtual identity/defaults and guest-owned environment variables.
//! Host accounts, locale and environment variables are never inherited here.
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug)]
pub struct WindowsVersion {
    pub major: u32,
    pub minor: u32,
    pub build: u32,
    pub platform: u32,
    pub suite_mask: u16,
    pub product_type: u8,
    pub service_pack_major: u16,
    pub service_pack_minor: u16,
}
impl WindowsVersion {
    pub fn packed(self) -> u32 {
        (self.build << 16) | (self.minor << 8) | self.major
    }
}

#[derive(Clone, Copy, Debug)]
pub struct GuestEnvironment {
    version: WindowsVersion,
}
impl GuestEnvironment {
    pub const fn pe32() -> Self {
        Self {
            version: WindowsVersion {
                major: 6,
                minor: 1,
                build: 7601,
                platform: 2,
                suite_mask: 0,
                product_type: 1,
                service_pack_major: 1,
                service_pack_minor: 0,
            },
        }
    }
    pub const fn amd64() -> Self {
        Self {
            version: WindowsVersion {
                major: 10,
                minor: 0,
                build: 19045,
                platform: 2,
                suite_mask: 0x100,
                product_type: 1,
                service_pack_major: 0,
                service_pack_minor: 0,
            },
        }
    }
    pub fn version(&self) -> WindowsVersion {
        self.version
    }
    pub fn computer_name(&self) -> &'static str {
        "WIRE"
    }
    pub fn dns_hostname(&self) -> &'static str {
        "wire"
    }
    pub fn dns_domain(&self) -> &'static str {
        ""
    }
    pub fn user_name(&self) -> &'static str {
        "wire"
    }
    pub fn codepage(&self) -> u32 {
        932
    }
    pub fn locale_id(&self) -> u32 {
        0x411
    }
    pub fn home(&self) -> &'static str {
        "C:\\Users\\wire"
    }
    pub fn local(&self) -> &'static str {
        "C:\\Users\\wire\\AppData\\Local"
    }
    pub fn local_low(&self) -> &'static str {
        "C:\\Users\\wire\\AppData\\LocalLow"
    }
    pub fn roaming(&self) -> &'static str {
        "C:\\Users\\wire\\AppData\\Roaming"
    }
    pub fn documents(&self) -> &'static str {
        "C:\\Users\\wire\\Documents"
    }
    pub fn windows_directory(&self) -> &'static str {
        "C:\\Windows"
    }
    pub fn system_directory(&self) -> &'static str {
        "C:\\Windows\\System32"
    }
    pub fn temporary_directory(&self) -> &'static str {
        "C:\\Temp\\"
    }
    pub fn csidl(&self, folder: u32) -> Option<&'static str> {
        match folder {
            0x05 => Some(self.documents()),
            0x1a => Some(self.roaming()),
            0x1c => Some(self.local()),
            0x24 => Some(self.windows_directory()),
            0x25 => Some(self.system_directory()),
            0x28 => Some(self.home()),
            _ => None,
        }
    }
    pub fn known_folder(&self, guid: [u8; 16]) -> Option<&'static str> {
        match guid {
            [0xa4, 0xa1, 0x20, 0xa5, 0x80, 0x17, 0xf6, 0x4f, 0xbd, 0x18, 0x16, 0x73, 0x43, 0xc5, 0xaf, 0x16] => {
                Some(self.local_low())
            }
            [0x85, 0x27, 0xb3, 0xf1, 0xba, 0x6f, 0xcf, 0x4f, 0x9d, 0x55, 0x7b, 0x8e, 0x7f, 0x15, 0x70, 0x91] => {
                Some(self.local())
            }
            [0xdb, 0x85, 0xb6, 0x3e, 0xf9, 0x65, 0xf6, 0x4c, 0xa0, 0x3a, 0xe3, 0xef, 0x65, 0x72, 0x9f, 0x3d] => {
                Some(self.roaming())
            }
            [0x8f, 0x85, 0x6c, 0x5e, 0x22, 0x0e, 0x60, 0x47, 0x9a, 0xfe, 0xea, 0x33, 0x17, 0xb6, 0x71, 0x73] => {
                Some(self.home())
            }
            [0xd0, 0x9a, 0xd3, 0xfd, 0x8f, 0x23, 0xaf, 0x46, 0xad, 0xb4, 0x6c, 0x85, 0x48, 0x03, 0x69, 0xc7] => {
                Some(self.documents())
            }
            _ => None,
        }
    }
    pub fn user_directories(&self) -> [&'static str; 4] {
        [
            self.local(),
            self.local_low(),
            self.roaming(),
            self.documents(),
        ]
    }
}

#[derive(Clone, Default)]
pub(crate) struct EnvironmentVariables {
    values: BTreeMap<String, (String, String)>,
}
impl EnvironmentVariables {
    pub fn len(&self) -> usize {
        self.values.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn set(&mut self, name: String, value: String) {
        self.values.insert(name.to_lowercase(), (name, value));
    }
    pub fn remove(&mut self, name: &str) -> bool {
        self.values.remove(&name.to_lowercase()).is_some()
    }
    pub fn get(&self, name: &str) -> Option<&str> {
        self.values
            .get(&name.to_lowercase())
            .map(|(_, value)| value.as_str())
    }
    pub fn wide_block(&self) -> Vec<u8> {
        let mut bytes: Vec<_> = self
            .values
            .values()
            .flat_map(|(name, value)| {
                format!("{name}={value}\0")
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>()
            })
            .collect();
        bytes.extend_from_slice(&[0, 0]);
        if self.is_empty() {
            bytes.extend_from_slice(&[0, 0]);
        }
        bytes
    }
}
