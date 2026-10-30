//! Bounded resident input namespace shared by the loader and Windows services.
//! Input payloads are shared until their first write. Metadata queries never copy them.
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::{Rc, Weak},
};
const FILE_LIMIT: usize = 4096;

#[derive(Clone, Copy)]
pub(crate) enum Mutation<'a> {
    Move {
        destination: Option<&'a str>,
        flags: u64,
    },
    DeleteFile,
    RemoveDirectory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileMetadata {
    pub attributes: u32,
    /// Windows FILETIME creation, last access and last write times.
    /// Unconfigured input metadata uses the virtual filesystem epoch (zero).
    pub times: [u64; 3],
    pub size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_exhaustion_does_not_create_or_truncate_files() {
        let mut files = Files::default();
        files.insert("C:\\a", vec![1, 2, 3]).unwrap();
        files.storage.borrow_mut().identity_serial = u64::MAX;
        let mut handles = crate::file_handles::Handles::for_files(&files);
        assert_eq!(
            handles.open(&mut files, "C:\\a", [0xc0000000, 7, 2, 0x80], false),
            Err(8)
        );
        assert_eq!(files.metadata("C:\\a").unwrap().size, 3);
        assert_eq!(
            handles.open(&mut files, "C:\\new", [0xc0000000, 7, 1, 0x80], false),
            Err(8)
        );
        assert_eq!(files.metadata("C:\\new"), Err(2));
        files.storage.borrow_mut().identity_serial = 0;
        let (handle, _) = handles
            .open(&mut files, "C:\\a", [0x80000000, 7, 3, 0x80], false)
            .unwrap();
        assert_eq!(handle, 0x100000);
        assert_eq!(handles.identity(handle), Ok(1));
    }
    #[test]
    fn shared_file_identities_follow_namespace_lifetimes_across_process_views() {
        let mut files = Files::default();
        files.insert("C:\\a", vec![1]).unwrap();
        files.insert("C:\\b", vec![2]).unwrap();
        let mut other = files.fork_process();
        let mut first = crate::file_handles::Handles::for_files(&files);
        let mut second = crate::file_handles::Handles::for_files(&other);
        let open = |h: &mut crate::file_handles::Handles, f: &mut Files, p: &str| {
            let handle = h.open(f, p, [0x80000000, 7, 3, 0x80], false).unwrap().0;
            let id = h.identity(handle).unwrap();
            h.close(handle).unwrap();
            id
        };
        let b = open(&mut first, &mut files, "C:\\b");
        let a = open(&mut second, &mut other, "C:\\a");
        assert_ne!(a, b);
        assert_eq!(open(&mut second, &mut other, "C:\\b"), b);
        other
            .mutate(
                "C:\\a",
                Mutation::Move {
                    destination: Some("C:\\b"),
                    flags: 1,
                },
            )
            .unwrap();
        assert_eq!(open(&mut first, &mut files, "C:\\b"), a);
        files.mutate("C:\\b", Mutation::DeleteFile).unwrap();
        other.create_file("C:\\b").unwrap();
        assert_ne!(open(&mut first, &mut files, "C:\\b"), a);
        let mut snapshot = files.clone();
        let identity = files.identity("C:\\b").unwrap();
        snapshot.mutate("C:\\b", Mutation::DeleteFile).unwrap();
        assert_eq!(files.identity("C:\\b").unwrap(), identity);
    }
    #[test]
    fn replacement_preserves_source_identity_backing_and_original_backup() {
        let mut files = Files::default();
        files.insert("C:\\old", vec![1, 2]).unwrap();
        files.insert_source("C:\\staging\\new", 1 << 40).unwrap();
        files.insert("C:\\backup", vec![7; 16]).unwrap();
        let old_id = files.identity("C:\\old").unwrap();
        let new_id = files.identity("C:\\staging\\new").unwrap();
        let backup_id = files.identity("C:\\backup").unwrap();
        let old = files.open_data("C:\\old").unwrap().node.unwrap();
        let new = files.open_data("C:\\staging\\new").unwrap().node.unwrap();
        old.borrow_mut().metadata.times = [11, 12, 13];
        new.borrow_mut().metadata.times = [21, 22, 23];
        let mut other = files.fork_process();
        files
            .replace_file("C:\\old", "C:\\staging\\new", Some("C:\\backup"))
            .unwrap();
        assert_eq!(files.metadata("C:\\staging\\new"), Err(2));
        assert_eq!(files.metadata("C:\\staging").unwrap().attributes, 0x10);
        assert_eq!(other.metadata("C:\\old").unwrap().size, 1 << 40);
        assert_eq!(other.metadata("C:\\old").unwrap().times, [11, 22, 23]);
        assert_eq!(other.metadata("C:\\backup").unwrap().size, 2);
        assert_eq!(other.identity("C:\\old"), Ok(new_id));
        assert_eq!(other.identity("C:\\backup"), Ok(old_id));
        assert_ne!(old_id, backup_id);
        assert!(Rc::ptr_eq(
            &new,
            &files.open_data("C:\\old").unwrap().node.unwrap()
        ));
        assert!(Rc::ptr_eq(
            &old,
            &files.open_data("C:\\backup").unwrap().node.unwrap()
        ));
        let storage = files.storage.borrow();
        assert_eq!(storage.bytes, 2);
        assert_eq!(storage.path_bytes, storage.recalculate_paths());
        assert!(files.pending_input().is_none());
    }
    #[test]
    fn replacement_failure_keeps_namespace_metadata_and_identities() {
        let mut files = Files::default();
        files.insert("C:\\old", vec![1]).unwrap();
        files.insert("C:\\new", vec![2, 3]).unwrap();
        files
            .open_data("C:\\old")
            .unwrap()
            .node
            .unwrap()
            .borrow_mut()
            .metadata
            .times = [1, 2, 3];
        files
            .open_data("C:\\new")
            .unwrap()
            .node
            .unwrap()
            .borrow_mut()
            .metadata
            .times = [4, 5, 6];
        let old = files.identity("C:\\old").unwrap();
        let new = files.identity("C:\\new").unwrap();
        let before = files.storage.borrow().bytes;
        for (replacement, backup, error) in [
            ("C:\\new", Some("C:\\missing\\backup"), 3),
            ("C:\\new", Some("C:\\old"), 87),
            ("C:\\new", Some("C:\\new"), 87),
            ("C:\\old", None, 87),
            ("C:\\absent", None, 2),
        ] {
            assert_eq!(
                files.replace_file("C:\\old", replacement, backup),
                Err(error)
            );
            assert_eq!(files.identity("C:\\old"), Ok(old));
            assert_eq!(files.identity("C:\\new"), Ok(new));
            assert_eq!(files.metadata("C:\\old").unwrap().size, 1);
            assert_eq!(files.metadata("C:\\old").unwrap().times, [1, 2, 3]);
            assert_eq!(files.metadata("C:\\new").unwrap().times, [4, 5, 6]);
            assert_eq!(
                files
                    .open_data("C:\\old")
                    .unwrap()
                    .node
                    .unwrap()
                    .borrow()
                    .data
                    .as_slice(),
                &[1]
            );
            assert_eq!(
                files
                    .open_data("C:\\new")
                    .unwrap()
                    .node
                    .unwrap()
                    .borrow()
                    .data
                    .as_slice(),
                &[2, 3]
            );
            assert_eq!(files.storage.borrow().bytes, before);
            assert_eq!(
                files.storage.borrow().path_bytes,
                files.storage.borrow().recalculate_paths()
            );
        }
    }
    #[test]
    fn recursive_directories_retain_parents_and_publish_all_or_none() {
        let mut files = Files::default();
        files.create_directory_all("C:\\Persist\\Leaf").unwrap();
        files
            .mutate("C:\\Persist\\Leaf", Mutation::RemoveDirectory)
            .unwrap();
        assert_eq!(files.metadata("C:\\Persist").unwrap().attributes, 0x10);
        files.insert("C:\\Implicit\\file", vec![]).unwrap();
        files.create_directory_all("C:\\Implicit").unwrap();
        files
            .mutate("C:\\Implicit\\file", Mutation::DeleteFile)
            .unwrap();
        assert_eq!(files.metadata("C:\\Implicit").unwrap().attributes, 0x10);
        files.insert("C:\\File", vec![]).unwrap();
        assert_eq!(files.create_directory_all("C:\\File\\child"), Err(3));
        let mut storage = files.storage.borrow_mut();
        storage.path_bytes = 4 * 1024 * 1024 - 20;
        let before = (
            storage.directories.clone(),
            storage.directory_names.clone(),
            storage.path_bytes,
        );
        assert_eq!(storage.create_directory_all("C:\\new\\leaf"), Err(8));
        assert_eq!(
            (
                storage.directories.clone(),
                storage.directory_names.clone(),
                storage.path_bytes
            ),
            before
        );
    }
    #[test]
    fn one_namespace_owns_case_paths_empty_directories_and_precise_missing_errors() {
        let mut files = Files::default();
        files.insert("C:\\lib\\code.dll", vec![1, 2, 3]).unwrap();
        files
            .set_metadata("C:\\empty", FileMetadata::directory())
            .unwrap();
        assert_eq!(files.metadata("c:/LIB/code.dll").unwrap().size, 3);
        for path in ["C:\\", "C:\\lib", "C:\\empty"] {
            assert_eq!(files.metadata(path).unwrap().attributes, 0x10);
        }
        assert_eq!(files.metadata("C:\\lib\\missing"), Err(2));
        assert_eq!(files.metadata("C:\\missing\\file"), Err(3));
        assert_eq!(files.metadata("D:\\lib"), Err(15));
        let before = (
            files.storage.borrow().bytes,
            files.storage.borrow().path_bytes,
        );
        assert!(files.insert("C:\\LIB", vec![9]).is_err());
        assert!(files.insert("C:\\lib\\code.dll\\child", vec![9]).is_err());
        assert!(files
            .set_metadata("C:\\lib\\code.dll", FileMetadata::directory())
            .is_err());
        assert_eq!(
            (
                files.storage.borrow().bytes,
                files.storage.borrow().path_bytes
            ),
            before
        );
    }
    #[test]
    fn executable_transfer_keeps_the_original_allocation_and_clones_share_bytes() {
        let mut files = Files::default();
        let data = vec![0x5a; 8192];
        let pointer = data.as_ptr();
        let source = files
            .register_owned_executable("C:\\app\\program.exe", data)
            .unwrap();
        assert_eq!(source.as_ptr(), pointer);
        files
            .register_executable("c:/APP/program.exe", &source)
            .unwrap();
        let copy = files.clone();
        let (_, copied) = copy.find(vec!["c:/app/PROGRAM.exe".into()]).unwrap();
        assert_eq!(copied.as_ptr(), pointer);
        assert_eq!(files.metadata("C:\\app").unwrap().attributes, 0x10);
        assert_eq!(files.metadata("C:\\app\\program.exe").unwrap().size, 8192);
    }
    #[test]
    fn configured_metadata_cannot_invent_contents_or_change_size_on_failure() {
        let mut files = Files::default();
        files.insert("C:\\file", vec![42; 3]).unwrap();
        let metadata = FileMetadata {
            attributes: 1,
            times: [123, 456, 789],
            size: 3,
        };
        files.set_metadata("C:\\file", metadata).unwrap();
        let before = files.metadata("C:\\file").unwrap();
        assert!(files
            .set_metadata(
                "C:\\file",
                FileMetadata {
                    size: 4,
                    ..metadata
                }
            )
            .is_err());
        assert!(files.set_metadata("C:\\absent", metadata).is_err());
        assert!(files
            .set_metadata(
                "C:\\file",
                FileMetadata {
                    attributes: 0x400,
                    ..metadata
                }
            )
            .is_err());
        assert_eq!(files.metadata("C:\\file").unwrap(), before);
        assert_eq!(
            files.find(vec!["C:\\file".into()]).unwrap().1.as_slice(),
            [42; 3]
        );
    }
    #[test]
    fn copying_uses_independent_nodes_and_shares_only_immutable_payloads() {
        let mut files = Files::default();
        files.insert("C:\\source", vec![1, 2, 3]).unwrap();
        files.copy_file("C:\\source", "C:\\target", true).unwrap();
        let source = files.open_data("C:\\source").unwrap().node.unwrap();
        let target = files.open_data("C:\\target").unwrap().node.unwrap();
        assert!(!Rc::ptr_eq(&source, &target));
        assert!(Rc::ptr_eq(&source.borrow().data, &target.borrow().data));
        files.write("C:\\target", 0, &[9]).unwrap();
        assert_eq!(source.borrow().data.as_slice(), [1, 2, 3]);
        assert_eq!(target.borrow().data.as_slice(), [9, 2, 3]);
        assert_eq!(files.copy_file("C:\\source", "C:\\target", true), Err(80));
        files.copy_file("C:\\source", "C:\\target", false).unwrap();
        assert_eq!(target.borrow().data.as_slice(), [1, 2, 3]);
        for flags in [1, 2] {
            let mut metadata = files.metadata("C:\\target").unwrap();
            metadata.attributes = flags;
            files.set_metadata("C:\\target", metadata).unwrap();
            assert_eq!(files.copy_file("C:\\source", "C:\\target", false), Err(5));
            assert_eq!(target.borrow().metadata, metadata);
        }
    }
    #[test]
    fn process_views_share_disk_changes_and_keep_input_snapshots_isolated() {
        let mut parent = Files::default();
        parent.insert("C:\\app\\data", vec![1, 2, 3]).unwrap();
        let snapshot = parent.clone();
        let mut child = parent.fork_process();
        child.set_cwd("C:\\app").unwrap();
        assert_eq!(parent.cwd(), "C:\\");
        assert_eq!(child.cwd(), "C:\\app");
        let before = child.find(vec!["data".into()]).unwrap().1;
        parent.write("C:\\app\\data", 1, &[9]).unwrap();
        assert_eq!(
            child.find(vec!["data".into()]).unwrap().1.as_slice(),
            [1, 9, 3]
        );
        assert_eq!(before.as_slice(), [1, 2, 3]);
        assert_eq!(
            snapshot
                .find(vec!["C:\\app\\data".into()])
                .unwrap()
                .1
                .as_slice(),
            [1, 2, 3]
        );
        child.create_file("new").unwrap();
        child.write("new", 0, &[4, 5]).unwrap();
        assert_eq!(parent.metadata("C:\\app\\new").unwrap().size, 2);
        child.set_attributes("new", 0x2000).unwrap();
        assert_eq!(parent.metadata("C:\\app\\new").unwrap().attributes, 0x2000);
        parent
            .mutate(
                "C:\\app\\new",
                Mutation::Move {
                    destination: Some("C:\\app\\renamed"),
                    flags: 0,
                },
            )
            .unwrap();
        assert_eq!(child.metadata("new"), Err(2));
        assert_eq!(
            child.find(vec!["renamed".into()]).unwrap().1.as_slice(),
            [4, 5]
        );
        child.mutate("renamed", Mutation::DeleteFile).unwrap();
        assert_eq!(parent.metadata("C:\\app\\renamed"), Err(2));
        assert_eq!(snapshot.metadata("C:\\app\\new"), Err(2));
        assert_eq!(
            parent.storage.borrow().path_bytes,
            parent.recalculate_paths()
        );
    }

    #[test]
    fn live_process_directories_block_moves_and_release_without_cross_snapshot_leases() {
        let mut parent = Files::default();
        parent.create_directory_all("C:\\one\\inside").unwrap();
        parent.create_directory("C:\\two").unwrap();
        let mut child = parent.fork_process();
        child.set_cwd("C:\\one\\inside").unwrap();
        let snapshot = child.clone();
        assert_eq!(
            parent.mutate(
                "C:\\one",
                Mutation::Move {
                    destination: Some("C:\\moved"),
                    flags: 0,
                }
            ),
            Err(32)
        );
        child.set_cwd("C:\\two").unwrap();
        parent
            .mutate(
                "C:\\one",
                Mutation::Move {
                    destination: Some("C:\\moved"),
                    flags: 0,
                },
            )
            .unwrap();
        assert_eq!(snapshot.cwd(), "C:\\one\\inside");
        assert!(snapshot.metadata(".").is_ok());
        assert_eq!(parent.mutate("C:\\two", Mutation::RemoveDirectory), Err(32));
        drop(child);
        parent.mutate("C:\\two", Mutation::RemoveDirectory).unwrap();
        assert_eq!(parent.metadata("C:\\two"), Err(2));
    }

    #[test]
    fn directory_and_file_limits_leave_the_namespace_unchanged() {
        let mut files = Files::default();
        for n in 0..FILE_LIMIT {
            files.insert(&format!("C:\\file{n}"), vec![1]).unwrap();
        }
        let before = (
            files.storage.borrow().bytes,
            files.storage.borrow().path_bytes,
        );
        assert!(files.insert("C:\\extra", vec![2]).is_err());
        assert_eq!(files.copy_file("C:\\file0", "C:\\extra", false), Err(8));
        let mut child = files.fork_process();
        assert_eq!(child.create_file("C:\\extra"), Err(8));
        assert_eq!(child.copy_file("C:\\file0", "C:\\extra", false), Err(8));
        assert_eq!(child.metadata("C:\\extra"), Err(2));
        assert_eq!(
            (
                files.storage.borrow().bytes,
                files.storage.borrow().path_bytes
            ),
            before
        );
        for n in 0..4096 {
            files
                .set_metadata(&format!("C:\\dir{n}"), FileMetadata::directory())
                .unwrap();
        }
        let before = files.storage.borrow().path_bytes;
        assert!(files
            .set_metadata("C:\\extra", FileMetadata::directory())
            .is_err());
        assert_eq!(files.storage.borrow().path_bytes, before);
        assert_eq!(files.metadata("C:\\extra"), Err(2));
    }
    #[test]
    fn executable_rename_accounts_for_paths_before_changing_the_namespace() {
        let mut files = Files::default();
        files
            .register_owned_executable("C:\\app.exe", vec![1, 2, 3])
            .unwrap();
        for n in 0..64 {
            files
                .set_metadata(
                    &format!("C:\\{n:02}{}", "d".repeat(32690)),
                    FileMetadata::directory(),
                )
                .unwrap();
        }
        assert_eq!(files.storage.borrow().path_bytes, files.recalculate_paths());
        let before = files.storage.borrow().path_bytes;
        let target = format!("C:\\{}", "x".repeat(20000));
        assert_eq!(
            files.mutate(
                "C:\\app.exe",
                Mutation::Move {
                    destination: Some(&target),
                    flags: 0
                }
            ),
            Err(8)
        );
        assert_eq!(files.storage.borrow().path_bytes, before);
        assert_eq!(files.metadata("C:\\app.exe").unwrap().size, 3);
        assert_eq!(files.metadata(&target), Err(2));
        files
            .mutate(
                "C:\\app.exe",
                Mutation::Move {
                    destination: Some("C:\\renamed.exe"),
                    flags: 0,
                },
            )
            .unwrap();
        assert_eq!(files.storage.borrow().path_bytes, files.recalculate_paths());
        assert_eq!(files.metadata("C:\\renamed.exe").unwrap().size, 3);
    }
}
impl FileMetadata {
    fn file(size: usize) -> Self {
        Self {
            attributes: 0x80,
            times: [0; 3],
            size: size as u64,
        }
    }
    fn directory() -> Self {
        Self {
            attributes: 0x10,
            times: [0; 3],
            size: 0,
        }
    }
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() != 36 {
            return Err("invalid resident metadata length".into());
        }
        let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        Ok(Self {
            attributes: word(0),
            times: std::array::from_fn(|n| {
                u64::from_le_bytes(bytes[4 + n * 8..12 + n * 8].try_into().unwrap())
            }),
            size: (word(28) as u64) << 32 | word(32) as u64,
        })
    }
    fn validate(self) -> Result<(), String> {
        // Data, directories and metadata flags are represented; reparse,
        // device, compression and encryption semantics are not advertised.
        let allowed = 1 | 2 | 4 | 0x10 | 0x20 | 0x80 | 0x100 | 0x2000;
        if self.attributes == 0
            || self.attributes & !allowed != 0
            || self.attributes & 0x80 != 0 && self.attributes != 0x80
            || self.attributes & 0x10 != 0 && self.size != 0
        {
            return Err("unsupported resident file metadata".into());
        }
        Ok(())
    }
}

pub(crate) struct Contents {
    pub data: Rc<Vec<u8>>,
    pub metadata: FileMetadata,
    pub source: Option<backing::Source>,
}
impl Contents {
    pub(crate) fn charge(&self) -> usize {
        self.source
            .as_ref()
            .map_or(self.data.len(), backing::Source::charge)
    }
    pub(crate) fn read(&self, offset: u64, length: usize) -> Result<Vec<u8>, ReadError> {
        let length = (self.metadata.size.saturating_sub(offset)).min(length as u64) as usize;
        if let Some(source) = &self.source {
            source.read(offset, length)
        } else {
            let offset = offset.min(self.data.len() as u64) as usize;
            Ok(self.data[offset..offset + length].to_vec())
        }
    }
    pub(crate) fn materialize(&self) -> Result<Rc<Vec<u8>>, ReadError> {
        if self.source.is_none() {
            return Ok(self.data.clone());
        }
        self.source
            .as_ref()
            .unwrap()
            .materialize(self.metadata.size)
    }
}
#[derive(Clone)]
struct File {
    name: String,
    node: Rc<RefCell<Contents>>,
}
impl File {
    fn data(&self) -> Result<Rc<Vec<u8>>, ReadError> {
        self.node.borrow().materialize()
    }
    fn metadata(&self) -> FileMetadata {
        self.node.borrow().metadata
    }
}

pub(crate) struct OpenData {
    pub path: String,
    pub node: Option<Rc<RefCell<Contents>>>,
    pub metadata: FileMetadata,
}

#[derive(Default)]
struct Storage {
    cache: Rc<RefCell<backing::Cache>>,
    sharing: Rc<Sharing>,
    entries: BTreeMap<String, File>,
    directories: BTreeMap<String, FileMetadata>,
    executable: Option<(String, File)>,
    bytes: usize,
    path_bytes: usize,
    directory_names: BTreeMap<String, String>,
    current_directories: Vec<Weak<String>>,
    identities: BTreeMap<String, u64>,
    identity_serial: u64,
}
impl Storage {
    pub(crate) fn set_attributes(&mut self, path: &str, flags: u32) -> Result<(), u32> {
        if flags & !0x2090 != 0 {
            return Err(50);
        }
        let mut metadata = self.metadata(path)?;
        metadata.attributes = (metadata.attributes & !(0x80 | 0x2000)) | (flags & 0x2000);
        if metadata.attributes == 0 {
            metadata.attributes = 0x80;
        }
        let key = self.resolve(path)?.to_lowercase();
        if metadata.attributes & 0x10 != 0 {
            self.set_metadata(&key, metadata).map_err(|_| 8u32)?;
        } else {
            if let Some(file) = self.entries.get_mut(&key) {
                file.node.borrow_mut().metadata = metadata;
            }
            if let Some((_, file)) = self.executable.as_mut().filter(|e| e.0 == key) {
                file.node.borrow_mut().metadata = metadata;
            }
        }
        Ok(())
    }
    pub(crate) fn create_directory(&mut self, path: &str) -> Result<(), u32> {
        match self.metadata(path) {
            Ok(_) => return Err(183),
            Err(2) => {}
            Err(error) => return Err(error),
        }
        self.set_metadata(path, FileMetadata::directory())
            .map_err(|_| 8)
    }
    /// Create a bounded empty virtual directory tree, with no host I/O.
    pub(crate) fn create_directory_all(&mut self, path: &str) -> Result<(), u32> {
        match self.metadata(path) {
            Ok(metadata) if metadata.attributes & 0x10 != 0 => {}
            Ok(_) => return Err(183),
            Err(2 | 3) => {}
            Err(error) => return Err(error),
        }
        let (_, name) = Self::path(path).map_err(|_| 3u32)?;
        let mut staged = Vec::new();
        let mut path_bytes = self.path_bytes;
        for at in name
            .match_indices('\\')
            .filter(|&(at, _)| at > 2)
            .map(|(at, _)| at)
            .chain(Some(name.len()))
        {
            let part = &name[..at];
            let key = part.to_lowercase();
            if self.metadata(part).is_ok_and(|m| m.attributes & 0x10 == 0) {
                return Err(3);
            }
            if key == "c:\\" || self.directories.contains_key(&key) {
                continue;
            }
            self.reserve_path(&key).map_err(|_| 8u32)?;
            path_bytes += key.len() * 2;
            if self.directories.len() + staged.len() >= 4096 || path_bytes > 4 * 1024 * 1024 {
                return Err(8);
            }
            staged.push((key, part.to_owned()));
        }
        for (key, name) in staged {
            self.directory_names.insert(key.clone(), name);
            self.directories.insert(key, FileMetadata::directory());
        }
        self.path_bytes = path_bytes;
        Ok(())
    }
    fn path(path: &str) -> Result<(String, String), String> {
        if path.is_empty() || path.len() > 32768 {
            return Err("resident file path capacity exceeded".into());
        }
        let path = crate::paths::logical_path(path)
            .map_err(|e| format!("resident file path error {e}"))?;
        Ok((path.to_lowercase(), path))
    }
    fn reserve_path(&self, key: &str) -> Result<usize, String> {
        let total = self
            .path_bytes
            .checked_add(key.len() * 2)
            .ok_or("resident path capacity")?;
        if total > 4 * 1024 * 1024 {
            return Err("resident path capacity exceeded".into());
        }
        for (at, _) in key.match_indices('\\').filter(|&(at, _)| at > 2) {
            let parent = &key[..at];
            if self.entries.contains_key(parent)
                || self.executable.as_ref().is_some_and(|e| e.0 == parent)
            {
                return Err("resident file ancestor is not a directory".into());
            }
        }
        Ok(total)
    }
    fn directory(&self, key: &str) -> Option<FileMetadata> {
        if let Some(&metadata) = self.directories.get(key) {
            return Some(metadata);
        }
        let prefix = format!("{}\\", key.trim_end_matches('\\'));
        let contains = |name: &str| name.starts_with(&prefix);
        (key == "c:\\"
            || self.executable.as_ref().is_some_and(|e| contains(&e.0))
            || self
                .entries
                .range(prefix.clone()..)
                .next()
                .is_some_and(|(name, _)| contains(name))
            || self
                .directories
                .range(prefix.clone()..)
                .next()
                .is_some_and(|(name, _)| contains(name)))
        .then(FileMetadata::directory)
    }
    fn prepare(&self, path: &str, size: usize) -> Result<(String, String, usize, usize), String> {
        let (key, path) = Self::path(path)?;
        if self.entries.contains_key(&key)
            || self.directory(&key).is_some()
            || self.executable.as_ref().is_some_and(|e| e.0 == key)
        {
            return Err("resident file path is a directory or duplicate".into());
        }
        let path_bytes = self.reserve_path(&key)?;
        let total = self
            .bytes
            .checked_add(size)
            .ok_or("resident file capacity")?;
        if self.entries.len() >= FILE_LIMIT || total > 256 * 1024 * 1024 {
            return Err("resident file capacity exceeded".into());
        }
        Ok((key, path, total, path_bytes))
    }
    pub fn insert(&mut self, path: &str, bytes: Vec<u8>) -> Result<(), String> {
        let (key, name, total, path_bytes) = self.prepare(path, bytes.len())?;
        let metadata = FileMetadata::file(bytes.len());
        self.entries.insert(
            key,
            File {
                name,
                node: Rc::new(RefCell::new(Contents {
                    data: Rc::new(bytes),
                    source: None,
                    metadata,
                })),
            },
        );
        self.bytes = total;
        self.path_bytes = path_bytes;
        Ok(())
    }
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn insert_slice(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        self.prepare(path, bytes.len())?;
        let mut data = Vec::new();
        data.try_reserve_exact(bytes.len())
            .map_err(|_| "resident file allocation failed")?;
        data.extend_from_slice(bytes);
        self.insert(path, data)
    }
    fn insert_source(&mut self, path: &str, size: u64) -> Result<u32, String> {
        if size > i64::MAX as u64 {
            return Err("file source size is outside the supported range".into());
        }
        let (key, name, _, path_bytes) = self.prepare(path, 0)?;
        let id = self
            .cache
            .borrow_mut()
            .source()
            .map_err(|_| "file source identity capacity")?;
        let mut metadata = FileMetadata::file(0);
        metadata.size = size;
        let source = backing::Source::new(id, size, self.cache.clone());
        self.entries.insert(
            key,
            File {
                name,
                node: Rc::new(RefCell::new(Contents {
                    data: Rc::new(Vec::new()),
                    metadata,
                    source: Some(source),
                })),
            },
        );
        self.path_bytes = path_bytes;
        Ok(id)
    }
    /// Configure an existing file, or declare an empty directory. File size
    /// must agree with its input bytes. No guest execution performs host stat.
    pub fn set_metadata(&mut self, path: &str, metadata: FileMetadata) -> Result<(), String> {
        metadata.validate()?;
        let (key, _) = Self::path(path)?;
        if metadata.attributes & 0x10 != 0 {
            if self.entries.contains_key(&key)
                || self.executable.as_ref().is_some_and(|e| e.0 == key)
            {
                return Err("resident directory path is a file".into());
            }
            if let Some(slot) = self.directories.get_mut(&key) {
                *slot = metadata;
            } else {
                if self.directories.len() >= 4096 {
                    return Err("resident directory capacity exceeded".into());
                }
                let path_bytes = self.reserve_path(&key)?;
                self.directory_names
                    .insert(key.clone(), Self::path(path)?.1);
                self.directories.insert(key, metadata);
                self.path_bytes = path_bytes;
            }
        } else {
            let file = self
                .entries
                .get_mut(&key)
                .ok_or("resident metadata file not supplied")?;
            if metadata.size != file.metadata().size {
                return Err("resident metadata size differs from input".into());
            }
            file.node.borrow_mut().metadata = metadata;
        }
        Ok(())
    }
    pub(crate) fn register_executable(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() > 256 * 1024 * 1024 {
            return Err("resident executable input capacity exceeded".into());
        }
        let (key, _) = Self::path(path)?;
        if let Some((_, file)) = self.executable.as_ref().filter(|e| e.0 == key) {
            if file.data().map_err(|e| e.to_string())?.as_slice() == bytes {
                return Ok(());
            }
        }
        if let Some(file) = self.entries.get(&key) {
            if file.data().map_err(|e| e.to_string())?.as_slice() != bytes {
                return Err("resident executable differs from supplied file".into());
            }
            let path_bytes = self.executable_path_bytes(&key)?;
            self.executable = Some((key, file.clone()));
            self.path_bytes = path_bytes;
            return Ok(());
        }
        if self.directory(&key).is_some() {
            return Err("resident executable path is a directory".into());
        }
        self.reserve_path(&key)?;
        let mut data = Vec::new();
        data.try_reserve_exact(bytes.len())
            .map_err(|_| "resident executable allocation failed")?;
        data.extend_from_slice(bytes);
        self.register_owned_executable(path, data)?;
        Ok(())
    }
    pub(crate) fn register_owned_executable(
        &mut self,
        path: &str,
        bytes: Vec<u8>,
    ) -> Result<Rc<Vec<u8>>, String> {
        let (key, name) = Self::path(path)?;
        if bytes.len() > 256 * 1024 * 1024 {
            return Err("resident executable input capacity exceeded".into());
        }
        if self.directory(&key).is_some() {
            return Err("resident executable path is a directory".into());
        }
        self.reserve_path(&key)?;
        let path_bytes = self.executable_path_bytes(&key)?;
        let file = if let Some(file) = self.entries.get(&key) {
            if file.data().map_err(|e| e.to_string())?.as_slice() != bytes {
                return Err("resident executable differs from supplied file".into());
            }
            file.clone()
        } else {
            File {
                name,
                node: Rc::new(RefCell::new(Contents {
                    metadata: FileMetadata::file(bytes.len()),
                    data: Rc::new(bytes),
                    source: None,
                })),
            }
        };
        let data = file.data().map_err(|e| e.to_string())?;
        self.executable = Some((key, file));
        self.path_bytes = path_bytes;
        Ok(data)
    }
    fn executable_path_bytes(&self, key: &str) -> Result<usize, String> {
        let total = self.recalculate_paths()
            - self.executable.as_ref().map_or(0, |e| e.0.len() * 2)
            + key.len() * 2;
        if total > 4 * 1024 * 1024 {
            return Err("resident path capacity exceeded".into());
        }
        Ok(total)
    }
    pub(crate) fn open_data(&self, path: &str) -> Result<OpenData, u32> {
        let metadata = self.metadata(path)?;
        let key = self.resolve(path)?.to_lowercase();
        let node = self
            .entries
            .get(&key)
            .or_else(|| {
                self.executable
                    .as_ref()
                    .filter(|e| e.0 == key)
                    .map(|e| &e.1)
            })
            .map(|file| file.node.clone());
        Ok(OpenData {
            path: key,
            node,
            metadata,
        })
    }
    pub fn metadata(&self, path: &str) -> Result<FileMetadata, u32> {
        self.query_metadata(path)
    }
    fn query_metadata(&self, path: &str) -> Result<FileMetadata, u32> {
        let key = self.resolve(path)?.to_lowercase();
        if let Some(file) = self.entries.get(&key) {
            return Ok(file.metadata());
        }
        if let Some((_, file)) = self.executable.as_ref().filter(|e| e.0 == key) {
            return Ok(file.metadata());
        }
        if let Some(metadata) = self.directory(&key) {
            return Ok(metadata);
        }
        let parent = key.rsplit_once('\\').ok_or(3u32)?.0;
        let parent = if parent == "c:" { "c:\\" } else { parent };
        if self.directory(parent).is_some() {
            Err(2)
        } else {
            Err(3)
        }
    }
    pub(crate) fn probe_file(&self, path: &str) -> Result<bool, u32> {
        match self.metadata(path) {
            Ok(metadata) if metadata.attributes & 0x10 == 0 => Ok(true),
            Ok(_) => Err(5),
            Err(2) => Ok(false),
            Err(error) => Err(error),
        }
    }
    pub(crate) fn find(&self, paths: Vec<String>) -> Result<(String, Rc<Vec<u8>>), u32> {
        for path in paths {
            let path = self.resolve(&path)?.to_lowercase();
            if let Some(file) = self.entries.get(&path).or_else(|| {
                self.executable
                    .as_ref()
                    .filter(|e| e.0 == path)
                    .map(|e| &e.1)
            }) {
                return Ok((file.name.clone(), file.data().map_err(ReadError::code)?));
            }
        }
        Err(126)
    }
}

// Input snapshots own independent metadata nodes while retaining immutable
// payloads. Aliases within a snapshot keep the same node.
impl Clone for Storage {
    fn clone(&self) -> Self {
        let mut nodes = BTreeMap::new();
        let mut clone_file = |file: &File| {
            let key = Rc::as_ptr(&file.node) as usize;
            let node = nodes
                .entry(key)
                .or_insert_with(|| {
                    let data = file.node.borrow();
                    Rc::new(RefCell::new(Contents {
                        data: data.data.clone(),
                        source: data.source.clone(),
                        metadata: data.metadata,
                    }))
                })
                .clone();
            File {
                name: file.name.clone(),
                node,
            }
        };
        let entries = self
            .entries
            .iter()
            .map(|(key, file)| (key.clone(), clone_file(file)))
            .collect();
        let executable = self
            .executable
            .as_ref()
            .map(|(key, file)| (key.clone(), clone_file(file)));
        Self {
            cache: self.cache.clone(),
            sharing: Default::default(),
            entries,
            executable,
            directories: self.directories.clone(),
            bytes: self.bytes,
            path_bytes: self.path_bytes,
            directory_names: self.directory_names.clone(),
            current_directories: Vec::new(),
            identities: self.identities.clone(),
            identity_serial: self.identity_serial,
        }
    }
}

#[path = "filesystem/operations.rs"]
mod operations;
#[path = "filesystem/process.rs"]
mod process;

#[path = "filesystem/backing.rs"]
mod backing;
pub use backing::{ReadError, ReadRequest};
pub use process::Files;
#[path = "filesystem/reservations.rs"]
mod reservations;
pub(crate) use reservations::{Reservation, Sharing};
