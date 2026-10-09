//! Owned managed-image bytes and opaque image/assembly identities. Provider
//! references prevent native pointer reuse until same-runtime retirement.
use super::{c_char, CString, Object};
use std::{collections::BTreeMap, ptr};
const IMAGE_BASE: u64 = 0x6ffe20000000;
const ASSEMBLY_BASE: u64 = 0x6ffe30000000;
const LIMIT: usize = 512;
type Open = unsafe extern "C" fn(*mut c_char, u32, i32, *mut i32, i32, *const c_char) -> Object;
#[derive(Clone, Copy)]
pub(super) struct Api {
    open: Open,
    addref: unsafe extern "C" fn(Object),
    close: unsafe extern "C" fn(Object),
    load: unsafe extern "C" fn(Object, *const c_char, *mut i32, i32) -> Object,
    image: unsafe extern "C" fn(Object) -> Object,
}
impl Api {
    pub(super) fn load(library: Object) -> Result<Self, String> {
        macro_rules! sym {
            ($name:literal,$ty:ty) => {{
                let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing Mono image symbol: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        Ok(Self {
            open: sym!("mono_image_open_from_data_with_name", Open),
            addref: sym!("mono_image_addref", unsafe extern "C" fn(Object)),
            close: sym!("mono_image_close", unsafe extern "C" fn(Object)),
            load: sym!(
                "mono_assembly_load_from_full",
                unsafe extern "C" fn(Object, *const c_char, *mut i32, i32) -> Object
            ),
            image: sym!(
                "mono_assembly_get_image",
                unsafe extern "C" fn(Object) -> Object
            ),
        })
    }
}
struct Image {
    native: Object,
    name: CString,
    hash: String,
    user_refs: usize,
    borrowed: bool,
}
struct Assembly {
    native: Object,
    image: u64,
}
#[derive(Default)]
pub(super) struct Registry {
    images: BTreeMap<u64, Image>,
    assemblies: BTreeMap<u64, Assembly>,
    opens: usize,
    loads: usize,
    user_closes: usize,
    getters: usize,
    retired: bool,
}
impl Registry {
    pub(super) fn unique_native_image(&self, hash: &str) -> Result<Object, String> {
        if self.retired {
            return Err("image lookup after retirement".into());
        }
        let mut matches = self.images.values().filter(|image| image.hash == hash);
        let image = matches
            .next()
            .ok_or("missing fingerprinted image identity")?;
        if matches.next().is_some() {
            return Err("ambiguous fingerprinted image identity".into());
        }
        Ok(image.native)
    }
    pub(super) fn open(
        &mut self,
        api: Api,
        mut bytes: Vec<u8>,
        name: CString,
        hash: String,
    ) -> Result<(u64, i32), String> {
        if self.retired
            || self.images.len() >= LIMIT
            || bytes.is_empty()
            || bytes.len() > 64 * 1024 * 1024
        {
            return Err("managed image open outside live bounded registry".into());
        }
        let mut status = 0;
        // Native always copies owned bytes. No guest/native borrowed payload can
        // remain referenced after this call, even when native caches the image.
        let image = unsafe {
            (api.open)(
                bytes.as_mut_ptr().cast(),
                bytes.len() as u32,
                1,
                &mut status,
                0,
                name.as_ptr(),
            )
        };
        if image.is_null() {
            return Ok((0, status));
        }
        self.opens += 1;
        if let Some((token, entry)) = self
            .images
            .iter_mut()
            .find(|(_, entry)| entry.native == image)
        {
            entry.user_refs += 1;
            return Ok((*token, status));
        }
        unsafe {
            (api.addref)(image);
        }
        let token = IMAGE_BASE + self.images.len() as u64 * 256;
        self.images.insert(
            token,
            Image {
                native: image,
                name,
                hash,
                user_refs: 1,
                borrowed: false,
            },
        );
        Ok((token, status))
    }
    pub(super) fn load_assembly(
        &mut self,
        api: Api,
        image: u64,
        name: CString,
    ) -> Result<(u64, i32), String> {
        if self.retired || self.assemblies.len() >= LIMIT {
            return Err("assembly registry is closed/full".into());
        }
        let entry = self
            .images
            .get_mut(&image)
            .ok_or("unknown managed image token")?;
        if entry.user_refs == 0 {
            return Err("assembly load requires live caller image reference".into());
        }
        let mut status = 0;
        let assembly = unsafe { (api.load)(entry.native, name.as_ptr(), &mut status, 0) };
        if assembly.is_null() {
            return Ok((0, status));
        }
        let actual = unsafe { (api.image)(assembly) };
        if actual != entry.native {
            return Err(
                "assembly resolved to another native image; typed alias not admitted".into(),
            );
        }
        entry.borrowed = true;
        self.loads += 1;
        if let Some((token, _)) = self
            .assemblies
            .iter()
            .find(|(_, entry)| entry.native == assembly)
        {
            return Ok((*token, status));
        }
        let token = ASSEMBLY_BASE + self.assemblies.len() as u64 * 256;
        self.assemblies.insert(
            token,
            Assembly {
                native: assembly,
                image,
            },
        );
        Ok((token, status))
    }
    pub(super) fn close(&mut self, api: Api, token: u64) -> Result<(), String> {
        if self.retired {
            return Err("image close after registry retirement".into());
        }
        let entry = self
            .images
            .get_mut(&token)
            .ok_or("image close has unknown token")?;
        if entry.user_refs == 0 {
            return Err("managed caller image reference already closed".into());
        }
        unsafe {
            (api.close)(entry.native);
        }
        entry.user_refs -= 1;
        self.user_closes += 1;
        Ok(())
    }
    pub(super) fn borrow_assembly(&mut self, api: Api, native: Object) -> Result<u64, String> {
        if self.retired {
            return Err("assembly result after registry retirement".into());
        }
        if native.is_null() {
            return Ok(0);
        }
        if let Some((token, _)) = self
            .assemblies
            .iter()
            .find(|(_, entry)| entry.native == native)
        {
            return Ok(*token);
        }
        if self.assemblies.len() >= LIMIT {
            return Err("borrowed assembly registry capacity exceeded".into());
        }
        let image = self.borrow_registered(unsafe { (api.image)(native) })?;
        let token = ASSEMBLY_BASE + self.assemblies.len() as u64 * 256;
        self.assemblies.insert(token, Assembly { native, image });
        Ok(token)
    }
    pub(super) fn assembly_image(&mut self, api: Api, token: u64) -> Result<u64, String> {
        if self.retired {
            return Err("assembly getter after registry retirement".into());
        }
        let assembly = self
            .assemblies
            .get(&token)
            .ok_or("unknown assembly token")?;
        let image = self
            .images
            .get(&assembly.image)
            .ok_or("assembly image identity lost")?;
        if !image.borrowed || unsafe { (api.image)(assembly.native) } != image.native {
            return Err("assembly image identity changed".into());
        }
        self.getters += 1;
        Ok(assembly.image)
    }
    pub(super) fn retire(&mut self, api: Api) {
        if self.retired {
            return;
        }
        self.retired = true;
        for entry in self.images.values_mut() {
            for _ in 0..=entry.user_refs {
                unsafe {
                    (api.close)(entry.native);
                }
            }
            entry.user_refs = 0;
            entry.borrowed = false;
            entry.native = ptr::null_mut();
        }
        // Assembly refs are held by native domain/runtime. Never fabricate
        // extra mono_assembly_close calls for borrowed assembly handles.
        for entry in self.assemblies.values_mut() {
            entry.native = ptr::null_mut();
        }
    }
    pub(super) fn borrowed_image(
        &mut self,
        api: Api,
        native: Object,
        name: CString,
        hash: String,
    ) -> Result<u64, String> {
        if self.retired || native.is_null() {
            return Err("borrowed native image has no live runtime owner".into());
        }
        if let Some((token, entry)) = self
            .images
            .iter_mut()
            .find(|(_, entry)| entry.native == native)
        {
            entry.borrowed = true;
            return Ok(*token);
        }
        if self.images.len() >= LIMIT {
            return Err("borrowed image registry capacity exceeded".into());
        }
        unsafe {
            (api.addref)(native);
        }
        let token = IMAGE_BASE + self.images.len() as u64 * 256;
        self.images.insert(
            token,
            Image {
                native,
                name,
                hash,
                user_refs: 0,
                borrowed: true,
            },
        );
        Ok(token)
    }
    pub(super) fn borrow_registered(&mut self, native: Object) -> Result<u64, String> {
        if self.retired || native.is_null() {
            return Err("class image has no live native registry".into());
        }
        let (token, entry) = self
            .images
            .iter_mut()
            .find(|(_, entry)| entry.native == native)
            .ok_or("class image is not a verified registered input")?;
        // Each entry retains its provider-owned addref until retire. Reuse that
        // exact identity instead of inventing an image or another user ref.
        entry.borrowed = true;
        Ok(*token)
    }
    pub(super) fn native_image(&self, token: u64) -> Result<Object, String> {
        if self.retired {
            return Err("metadata image registry retired".into());
        }
        let entry = self
            .images
            .get(&token)
            .ok_or("metadata query has unknown image")?;
        if entry.native.is_null() || (entry.user_refs == 0 && !entry.borrowed) {
            return Err("metadata query has no live image owner".into());
        }
        Ok(entry.native)
    }
    pub(super) fn completed(&self) -> usize {
        self.getters
    }
    pub(super) fn report(&self) -> String {
        format!("native-images-opened={}\nnative-assemblies-loaded={}\nguest-image-refs-closed={}\nnative-assembly-image-getters={}\nnative-image-registry-retired={}\n",self.opens,self.loads,self.user_closes,self.getters,self.retired)
    }
    pub(super) fn tsv(&self) -> String {
        let mut output =
            String::from("image_token\tname\tsha256\tcaller_refs\tassembly_borrowed\tretired\n");
        for (token, entry) in &self.images {
            output.push_str(&format!(
                "{token:#x}\t{}\t{}\t{}\t{}\t{}\n",
                entry
                    .name
                    .to_string_lossy()
                    .replace('\t', "\\t")
                    .replace('\n', "\\n"),
                entry.hash,
                entry.user_refs,
                entry.borrowed,
                self.retired
            ));
        }
        output
    }
}
pub(super) fn handles(function: &str) -> bool {
    matches!(
        function,
        "mono_image_open_from_data_with_name"
            | "mono_assembly_load_from_full"
            | "mono_image_close"
            | "mono_assembly_get_image"
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    thread_local! { static REFERENCES:Cell<usize> = const {Cell::new(0)}; }
    const IMAGE: usize = 0x10001000;
    const ASSEMBLY: usize = 0x10002000;
    unsafe extern "C" fn open(
        _: *mut c_char,
        _: u32,
        _: i32,
        status: *mut i32,
        _: i32,
        _: *const c_char,
    ) -> Object {
        unsafe {
            status.write(0);
        }
        REFERENCES.with(|r| r.set(r.get() + 1));
        IMAGE as Object
    }
    unsafe extern "C" fn addref(_: Object) {
        REFERENCES.with(|r| r.set(r.get() + 1));
    }
    unsafe extern "C" fn close(_: Object) {
        REFERENCES.with(|r| r.set(r.get() - 1));
    }
    unsafe extern "C" fn load(_: Object, _: *const c_char, status: *mut i32, _: i32) -> Object {
        unsafe {
            status.write(0);
        }
        REFERENCES.with(|r| r.set(r.get() + 1));
        ASSEMBLY as Object
    }
    unsafe extern "C" fn image(_: Object) -> Object {
        IMAGE as Object
    }
    #[test]
    fn opaque_images_reject_forged_and_double_closed_handles_and_keep_provider_reference_until_retirement(
    ) {
        REFERENCES.with(|r| r.set(0));
        let api = Api {
            open,
            addref,
            close,
            load,
            image,
        };
        let mut registry = Registry::default();
        let (image, status) = registry
            .open(
                api,
                vec![1],
                CString::new("authored.dll").unwrap(),
                "hash".into(),
            )
            .unwrap();
        assert_eq!(status, 0);
        assert_ne!(image, IMAGE as u64);
        assert_eq!(REFERENCES.with(Cell::get), 2);
        assert!(registry.close(api, IMAGE as u64).is_err());
        let (assembly, status) = registry
            .load_assembly(api, image, CString::new("authored.dll").unwrap())
            .unwrap();
        assert_eq!(status, 0);
        assert_ne!(assembly, ASSEMBLY as u64);
        registry.close(api, image).unwrap();
        assert_eq!(REFERENCES.with(Cell::get), 2);
        assert!(registry.close(api, image).is_err());
        assert_eq!(registry.assembly_image(api, assembly).unwrap(), image);
        assert_eq!(registry.borrow_registered(IMAGE as Object).unwrap(), image);
        assert_eq!(REFERENCES.with(Cell::get), 2);
        assert!(registry.borrow_registered((IMAGE + 1) as Object).is_err());
        assert!(registry.assembly_image(api, ASSEMBLY as u64).is_err());
        registry.retire(api);
        // Domain retains the assembly image. Only the provider's independent
        // hold retires here; domain cleanup owns the last reference.
        assert_eq!(REFERENCES.with(Cell::get), 1);
        registry.retire(api);
        assert_eq!(REFERENCES.with(Cell::get), 1);
        assert!(registry.assembly_image(api, assembly).is_err());
        assert!(registry.close(api, image).is_err());
        assert!(registry.borrow_registered(IMAGE as Object).is_err());
    }
}
