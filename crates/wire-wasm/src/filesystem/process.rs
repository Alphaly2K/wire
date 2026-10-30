//! Process directory views over a shared virtual disk; input cloning remains a snapshot.
use super::*;

pub struct Files {
    pub(super) storage: Rc<RefCell<Storage>>,
    cwd: Rc<String>,
}

impl Default for Files {
    fn default() -> Self {
        Self::from_storage(Storage::default(), Rc::new("C:\\".into()))
    }
}

impl Clone for Files {
    fn clone(&self) -> Self {
        Self::from_storage(self.storage.borrow().clone(), Rc::new((*self.cwd).clone()))
    }
}

impl Files {
    pub(crate) fn check_identity(&self, path: &str) -> Result<(), u32> {
        let key = self.resolve(path)?.to_lowercase();
        self.storage.borrow().check_identity(&key)
    }
    pub(crate) fn identity(&mut self, path: &str) -> Result<u64, u32> {
        let key = self.resolve(path)?.to_lowercase();
        let mut storage = self.storage.borrow_mut();
        storage.check_identity(&key)?;
        if let Some(&identity) = storage.identities.get(&key) {
            return Ok(identity);
        }
        storage.identity_serial += 1;
        let identity = storage.identity_serial;
        storage.identities.insert(key, identity);
        Ok(identity)
    }
    pub(crate) fn sharing(&self) -> Rc<Sharing> {
        self.storage.borrow().sharing.clone()
    }

    pub(crate) fn shared_processes(&self) -> bool {
        self.storage
            .borrow()
            .current_directories
            .iter()
            .map(Weak::strong_count)
            .sum::<usize>()
            > 1
    }
    fn from_storage(storage: Storage, cwd: Rc<String>) -> Self {
        let files = Self {
            storage: Rc::new(RefCell::new(storage)),
            cwd,
        };
        files.retain_directory();
        files
    }

    fn retain_directory(&self) {
        let mut storage = self.storage.borrow_mut();
        storage
            .current_directories
            .retain(|cwd| cwd.strong_count() != 0);
        let directory = Rc::downgrade(&self.cwd);
        if !storage
            .current_directories
            .iter()
            .any(|cwd| cwd.ptr_eq(&directory))
        {
            storage.current_directories.push(directory);
        }
    }

    pub(crate) fn fork_process(&self) -> Self {
        let files = Self {
            storage: self.storage.clone(),
            cwd: self.cwd.clone(),
        };
        files.retain_directory();
        files
    }

    pub(crate) fn cwd(&self) -> &str {
        &self.cwd
    }

    pub(crate) fn resolve(&self, path: &str) -> Result<String, u32> {
        if path.len() > 32768 {
            return Err(206);
        }
        crate::paths::logical_path_at(path, self.cwd())
    }

    pub(crate) fn set_cwd(&mut self, path: &str) -> Result<(), u32> {
        let path = self.resolve(path)?;
        if self.storage.borrow().metadata(&path)?.attributes & 0x10 == 0 {
            return Err(3);
        }
        self.cwd = Rc::new(path);
        self.retain_directory();
        Ok(())
    }

    pub fn insert(&mut self, path: &str, bytes: Vec<u8>) -> Result<(), String> {
        self.storage.borrow_mut().insert(path, bytes)
    }
    pub(crate) fn insert_source(&mut self, path: &str, size: u64) -> Result<u32, String> {
        self.storage.borrow_mut().insert_source(path, size)
    }
    pub(crate) fn pending_input(&self) -> Option<ReadRequest> {
        self.storage.borrow().cache.borrow().pending()
    }
    pub(crate) fn supply_input(
        &mut self,
        request: ReadRequest,
        bytes: Vec<u8>,
    ) -> Result<(), String> {
        self.storage
            .borrow()
            .cache
            .borrow_mut()
            .supply(request, bytes)
    }
    pub(crate) fn finish_input(&self) {
        self.storage.borrow().cache.borrow_mut().finish();
    }
    pub(crate) fn input_cache_bytes(&self) -> usize {
        self.storage.borrow().cache.borrow().bytes()
    }
    pub(crate) fn owned_bytes(&self) -> usize {
        self.storage.borrow().bytes
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn insert_slice(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        self.storage.borrow_mut().insert_slice(path, bytes)
    }

    pub fn set_metadata(&mut self, path: &str, metadata: FileMetadata) -> Result<(), String> {
        self.storage.borrow_mut().set_metadata(path, metadata)
    }

    pub fn metadata(&self, path: &str) -> Result<FileMetadata, u32> {
        let path = self.resolve(path)?;
        self.storage.borrow().metadata(&path)
    }

    pub(crate) fn set_attributes(&mut self, path: &str, flags: u32) -> Result<(), u32> {
        let path = self.resolve(path)?;
        self.storage.borrow_mut().set_attributes(&path, flags)
    }

    pub(crate) fn create_directory(&mut self, path: &str) -> Result<(), u32> {
        let path = self.resolve(path)?;
        self.storage.borrow_mut().create_directory(&path)
    }

    pub(crate) fn create_directory_all(&mut self, path: &str) -> Result<(), u32> {
        let path = self.resolve(path)?;
        self.storage.borrow_mut().create_directory_all(&path)
    }

    pub(crate) fn create_file(&mut self, path: &str) -> Result<(), u32> {
        let path = self.resolve(path)?;
        self.storage.borrow_mut().create_file(&path)
    }

    pub(crate) fn resize(&mut self, path: &str, size: usize) -> Result<(), u32> {
        let path = self.resolve(path)?;
        self.storage.borrow_mut().resize(&path, size)
    }
    pub(crate) fn resize64(&mut self, path: &str, size: u64) -> Result<(), u32> {
        let path = self.resolve(path)?;
        self.storage.borrow_mut().modify64(&path, size, None)
    }

    pub(crate) fn copy_file(
        &mut self,
        source: &str,
        destination: &str,
        fail_existing: bool,
    ) -> Result<(), u32> {
        let source = self.resolve(source)?;
        let destination = self.resolve(destination)?;
        self.storage
            .borrow_mut()
            .copy_file(&source, &destination, fail_existing)
    }
    pub(crate) fn replace_file(
        &mut self,
        original: &str,
        replacement: &str,
        backup: Option<&str>,
    ) -> Result<(), u32> {
        let original = self.resolve(original)?;
        let replacement = self.resolve(replacement)?;
        let backup = backup.map(|s| self.resolve(s)).transpose()?;
        self.storage
            .borrow_mut()
            .replace_file(&original, &replacement, backup.as_deref())
    }

    pub(crate) fn write(&mut self, path: &str, offset: u64, data: &[u8]) -> Result<usize, u32> {
        let path = self.resolve(path)?;
        self.storage.borrow_mut().write(&path, offset, data)
    }

    pub(crate) fn mutate(&mut self, path: &str, mutation: Mutation<'_>) -> Result<(), u32> {
        let path = self.resolve(path)?;
        let destination = match mutation {
            Mutation::Move {
                destination: Some(path),
                ..
            } => Some(self.resolve(path)?),
            _ => None,
        };
        let mutation = match mutation {
            Mutation::Move { flags, .. } => Mutation::Move {
                destination: destination.as_deref(),
                flags,
            },
            other => other,
        };
        self.storage.borrow_mut().mutate(&path, mutation)
    }

    pub(crate) fn entries(&self, directory: &str) -> Result<Vec<String>, u32> {
        let directory = self.resolve(directory)?;
        self.storage.borrow().entries(&directory)
    }

    pub(crate) fn register_executable(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        self.storage.borrow_mut().register_executable(path, bytes)
    }

    pub(crate) fn register_owned_executable(
        &mut self,
        path: &str,
        bytes: Vec<u8>,
    ) -> Result<Rc<Vec<u8>>, String> {
        self.storage
            .borrow_mut()
            .register_owned_executable(path, bytes)
    }

    pub(crate) fn open_data(&self, path: &str) -> Result<OpenData, u32> {
        let path = self.resolve(path)?;
        self.storage.borrow().open_data(&path)
    }

    pub(crate) fn probe_file(&self, path: &str) -> Result<bool, u32> {
        let path = self.resolve(path)?;
        self.storage.borrow().probe_file(&path)
    }

    pub(crate) fn find(&self, paths: Vec<String>) -> Result<(String, Rc<Vec<u8>>), u32> {
        let paths = paths
            .iter()
            .map(|path| self.resolve(path))
            .collect::<Result<_, _>>()?;
        self.storage.borrow().find(paths)
    }

    #[cfg(test)]
    pub(super) fn recalculate_paths(&self) -> usize {
        self.storage.borrow().recalculate_paths()
    }
}
