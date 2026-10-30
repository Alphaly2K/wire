//! PE metadata shared by image readers and portable loaders.
#[derive(Debug, Clone)]
pub struct Section {
    pub name: String,
    pub virtual_size: u32,
    pub virtual_address: u32,
    pub raw_size: u32,
    pub raw_ptr: u32,
    pub characteristics: u32,
}
impl Section {
    pub fn is_executable(&self) -> bool {
        self.characteristics & 0x2000_0000 != 0
    }
    pub fn is_writable(&self) -> bool {
        self.characteristics & 0x8000_0000 != 0
    }
    pub fn contains_rva(&self, rva: u32) -> bool {
        u64::from(rva) >= u64::from(self.virtual_address)
            && u64::from(rva)
                < u64::from(self.virtual_address) + u64::from(self.virtual_size.max(self.raw_size))
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub struct DataDir {
    pub rva: u32,
    pub size: u32,
}
#[derive(Debug, Clone)]
pub struct ImportFunc {
    pub iat_rva: u32,
    pub name: Option<String>,
    pub ordinal: Option<u16>,
}
#[derive(Debug, Clone)]
pub struct ImportDll {
    pub dll: String,
    pub functions: Vec<ImportFunc>,
}
