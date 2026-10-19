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
    pub fn user_directories(&self) -> [&'static str; 4] {
        [
            self.local(),
            self.local_low(),
            self.roaming(),
            self.documents(),
        ]
    }
}

#[derive(Default)]
pub(crate) struct EnvironmentVariables {
    values: BTreeMap<String, (String, String)>,
}
impl EnvironmentVariables {
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
        if bytes.len() == 2 {
            bytes.extend_from_slice(&[0, 0]);
        }
        bytes
    }
}
