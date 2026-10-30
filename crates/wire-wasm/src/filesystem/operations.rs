//! Storage operations over normalized disk paths. No guest ABI lives here.
use super::*;

fn under(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|tail| tail.starts_with('\\'))
}
fn parent(path: &str) -> &str {
    let parent = path.rsplit_once('\\').map_or("c:\\", |(parent, _)| parent);
    if parent == "c:" {
        "c:\\"
    } else {
        parent
    }
}
impl Storage {
    pub(super) fn check_identity(&self, key: &str) -> Result<(), u32> {
        if !self.identities.contains_key(key)
            && (self.identities.len() >= 4096 || self.identity_serial == u64::MAX)
        {
            Err(8)
        } else {
            Ok(())
        }
    }
    fn remap_identities(&mut self, source: &str, destination: Option<&str>) {
        let ids: Vec<_> = self
            .identities
            .iter()
            .filter(|(p, _)| under(p, source))
            .map(|(p, &id)| (p.clone(), id))
            .collect();
        self.identities
            .retain(|p, _| !under(p, source) && destination.is_none_or(|to| p != to));
        if let Some(to) = destination {
            for (old, id) in ids {
                self.identities
                    .insert(format!("{to}{}", &old[source.len()..]), id);
            }
        }
    }
    pub(crate) fn resolve(&self, path: &str) -> Result<String, u32> {
        if path.len() > 32768 {
            return Err(206);
        }
        crate::paths::logical_path(path)
    }
    fn node(&self, key: &str) -> Option<&Rc<RefCell<Contents>>> {
        self.entries
            .get(key)
            .or_else(|| {
                self.executable
                    .as_ref()
                    .filter(|e| e.0 == key)
                    .map(|e| &e.1)
            })
            .map(|f| &f.node)
    }
    pub(crate) fn entries(&self, directory: &str) -> Result<Vec<String>, u32> {
        let directory = self.resolve(directory)?;
        if self.metadata(&directory)?.attributes & 0x10 == 0 {
            return Err(31);
        }
        let prefix = format!("{}\\", directory.trim_end_matches('\\')).to_lowercase();
        let mut entries = BTreeMap::new();
        let names = self
            .entries
            .values()
            .map(|f| f.name.as_str())
            .chain(self.executable.iter().map(|(_, f)| f.name.as_str()))
            .chain(self.directory_names.values().map(String::as_str));
        for name in names {
            let key = name.to_lowercase();
            if !key.starts_with(&prefix) {
                continue;
            }
            // Case folding may change UTF-8 length. Match components instead of slicing a lowercased offset.
            let count = directory.trim_end_matches('\\').split('\\').count();
            let mut components = name.split('\\');
            let head: Vec<_> = components.by_ref().take(count + 1).collect();
            if head.len() != count + 1 || head.last() == Some(&"") {
                continue;
            }
            let child = head.join("\\");
            entries.entry(child.to_lowercase()).or_insert(child);
            if entries.len() > 65536 {
                return Err(8);
            }
        }
        Ok(entries.into_values().collect())
    }
    pub(super) fn recalculate_paths(&self) -> usize {
        self.entries
            .keys()
            .chain(self.directories.keys())
            .chain(self.executable.iter().map(|e| &e.0))
            .map(|p| p.len() * 2)
            .sum()
    }
    fn retain_parent(&mut self, key: &str) -> Result<(), u32> {
        let path = parent(key);
        if path == "c:\\" || self.directories.contains_key(path) {
            return Ok(());
        }
        let metadata = self.metadata(path)?;
        self.set_metadata(path, metadata).map_err(|_| 8u32)
    }
    pub(crate) fn create_file(&mut self, path: &str) -> Result<(), u32> {
        let name = self.resolve(path)?;
        let key = name.to_lowercase();
        if self.metadata(parent(&key))?.attributes & 0x10 == 0 {
            return Err(3);
        }
        if self.entries.len() >= 4096 {
            return Err(8);
        }
        let bytes = self
            .recalculate_paths()
            .checked_add(key.len() * 2)
            .ok_or(8u32)?;
        if bytes > 4 * 1024 * 1024 {
            return Err(8);
        }
        self.entries.insert(
            key,
            File {
                name,
                node: Rc::new(RefCell::new(Contents {
                    source: None,
                    data: Rc::new(Vec::new()),
                    metadata: FileMetadata::file(0),
                })),
            },
        );
        self.path_bytes = bytes;
        Ok(())
    }
    pub(crate) fn resize(&mut self, path: &str, size: usize) -> Result<(), u32> {
        self.modify64(path, size as u64, None)
    }
    pub(crate) fn copy_file(
        &mut self,
        source: &str,
        destination: &str,
        fail_existing: bool,
    ) -> Result<(), u32> {
        let source = self.resolve(source)?.to_lowercase();
        let destination = self.resolve(destination)?;
        let key = destination.to_lowercase();
        if source == key {
            return Err(32);
        }
        let opened = self.open_data(&source)?;
        let source = opened.node.ok_or(5u32)?;
        let metadata = source.borrow().metadata;
        let data = source.borrow().data.clone();
        let backing = source.borrow().source.clone();
        let charge = source.borrow().charge();
        let previous = match self.metadata(&destination) {
            Ok(metadata) => Some(metadata),
            Err(2) => None,
            Err(error) => return Err(error),
        };
        if previous.is_some_and(|m| m.attributes & (1 | 2 | 0x10) != 0) {
            return Err(5);
        }
        if previous.is_some() && fail_existing {
            return Err(80);
        }
        if self.metadata(parent(&key))?.attributes & 0x10 == 0 {
            return Err(3);
        }
        if previous.is_none() && self.entries.len() >= FILE_LIMIT {
            return Err(8);
        }
        let old = self
            .entries
            .get(&key)
            .map_or(0, |f| f.node.borrow().charge());
        let bytes = if previous.is_none() || self.entries.contains_key(&key) {
            self.bytes
                .checked_sub(old)
                .and_then(|n| n.checked_add(charge))
                .ok_or(8u32)?
        } else {
            self.bytes
        };
        let paths = self
            .recalculate_paths()
            .checked_add(if previous.is_none() { key.len() * 2 } else { 0 })
            .ok_or(8u32)?;
        if bytes > 256 * 1024 * 1024 || paths > 4 * 1024 * 1024 {
            return Err(8);
        }
        if let Some(node) = self.node(&key) {
            *node.borrow_mut() = Contents {
                data,
                metadata,
                source: backing,
            };
        } else {
            self.entries.insert(
                key,
                File {
                    name: destination,
                    node: Rc::new(RefCell::new(Contents {
                        data,
                        metadata,
                        source: backing,
                    })),
                },
            );
        }
        self.bytes = bytes;
        self.path_bytes = paths;
        Ok(())
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
        let target = original.to_lowercase();
        let source = replacement.to_lowercase();
        let saved = backup.as_ref().map(|s| s.to_lowercase());
        if target == source || saved.as_ref().is_some_and(|s| s == &target || s == &source) {
            return Err(87);
        }
        let old = self.open_data(&original)?;
        let new = self.open_data(&replacement)?;
        if (old.metadata.attributes | new.metadata.attributes) & (1 | 0x10) != 0 {
            return Err(5);
        }
        let old = self.entries.get(&target).ok_or(5u32)?.clone();
        let mut new = self.entries.get(&source).ok_or(5u32)?.clone();
        let previous = if let Some(path) = &saved {
            if self
                .metadata(parent(path))
                .map_err(|e| if e == 2 { 3 } else { e })?
                .attributes
                & 0x10
                == 0
            {
                return Err(3);
            }
            match self.metadata(path) {
                Ok(m) if m.attributes & (1 | 0x10) != 0 => return Err(5),
                Ok(_) => self.entries.get(path).cloned(),
                Err(2) => None,
                Err(e) => return Err(e),
            }
        } else {
            None
        };
        let parent_extra =
            if parent(&source) != "c:\\" && !self.directories.contains_key(parent(&source)) {
                parent(&source).len() * 2
            } else {
                0
            };
        let path_bytes = self.recalculate_paths()
            - (target.len()
                + source.len()
                + if previous.is_some() {
                    saved.as_ref().unwrap().len()
                } else {
                    0
                })
                * 2
            + (target.len() + saved.as_ref().map_or(0, |s| s.len())) * 2
            + parent_extra;
        if path_bytes > 4 * 1024 * 1024 {
            return Err(8);
        }
        self.retain_parent(&source)?;
        let original_metadata = old.metadata();
        let mut metadata = new.metadata();
        metadata.times[0] = original_metadata.times[0];
        metadata.attributes =
            (metadata.attributes & !0x4800) | (original_metadata.attributes & 0x4800);
        new.node.borrow_mut().metadata = metadata;
        new.name = original;
        self.entries.remove(&target);
        self.entries.remove(&source);
        self.entries.insert(target.clone(), new);
        if let Some(path) = saved.as_ref() {
            if let Some(previous) = previous {
                self.bytes -= previous.node.borrow().charge();
            }
            let mut old = old;
            old.name = backup.unwrap();
            self.entries.insert(path.clone(), old);
        } else {
            self.bytes -= old.node.borrow().charge();
        }
        self.remap_identities(&target, saved.as_deref());
        self.remap_identities(&source, Some(&target));
        self.path_bytes = path_bytes;
        Ok(())
    }
    pub(crate) fn write(&mut self, path: &str, offset: u64, data: &[u8]) -> Result<usize, u32> {
        if data.is_empty() {
            return Ok(0);
        }
        let end = offset.checked_add(data.len() as u64).ok_or(8u32)?;
        let old = self.metadata(path)?.size;
        self.modify64(path, old.max(end), Some((offset, data)))?;
        Ok(data.len())
    }
    pub(super) fn modify64(
        &mut self,
        path: &str,
        size: u64,
        write: Option<(u64, &[u8])>,
    ) -> Result<(), u32> {
        if size > i64::MAX as u64 {
            return Err(8);
        }
        let key = self.resolve(path)?.to_lowercase();
        let node = self.node(&key).ok_or(2u32)?.clone();
        let mut file = node.borrow_mut();
        if file.metadata.attributes & (1 | 0x10) != 0 {
            return Err(5);
        }
        if let Some(source) = &file.source {
            let charged = if let Some((offset, bytes)) = write {
                source.write_charge(offset, bytes.len())?
            } else {
                source.resize_charge(size)
            };
            let total = self
                .bytes
                .checked_sub(file.charge())
                .and_then(|n| n.checked_add(charged))
                .filter(|&n| n <= backing::TRANSFER_LIMIT)
                .ok_or(8u32)?;
            let next = if let Some((offset, bytes)) = write {
                source.write(offset, bytes)
            } else {
                source.resize(size)
            }
            .map_err(ReadError::code)?;
            file.source = Some(next);
            file.metadata.size = size;
            self.bytes = total;
            return Ok(());
        }
        let size = usize::try_from(size).map_err(|_| 8u32)?;
        if size > backing::TRANSFER_LIMIT {
            return Err(8);
        }
        let write = write.map(|(offset, bytes)| (offset as usize, bytes));
        let old = file.data.len();
        let bytes = if self.entries.contains_key(&key) {
            let bytes = self
                .bytes
                .checked_sub(old)
                .ok_or(8u32)?
                .checked_add(size)
                .ok_or(8u32)?;
            if bytes > 256 * 1024 * 1024 {
                return Err(8);
            }
            Some(bytes)
        } else {
            None
        };
        // Try all growth before publishing data or metadata. Handles and sections
        // own the node, so they do not force a full payload copy on each write.
        if Rc::get_mut(&mut file.data).is_none() {
            let mut next = Vec::new();
            next.try_reserve_exact(size).map_err(|_| 8u32)?;
            next.extend_from_slice(&file.data[..old.min(size)]);
            next.resize(size, 0);
            if let Some((offset, data)) = write {
                next[offset..offset + data.len()].copy_from_slice(data);
            }
            file.data = Rc::new(next);
        } else {
            let payload = Rc::get_mut(&mut file.data).unwrap();
            payload
                .try_reserve_exact(size.saturating_sub(payload.len()))
                .map_err(|_| 8u32)?;
            payload.resize(size, 0);
            if let Some((offset, data)) = write {
                payload[offset..offset + data.len()].copy_from_slice(data);
            }
            // Release substantially oversized backing after truncation.
            if size < old {
                payload.shrink_to_fit();
            }
        }
        file.metadata.size = size as u64;
        if let Some(bytes) = bytes {
            self.bytes = bytes;
        }
        Ok(())
    }
    pub(crate) fn mutate(&mut self, path: &str, mutation: Mutation<'_>) -> Result<(), u32> {
        let source = self.resolve(path)?;
        let key = source.to_lowercase();
        let (destination, flags, remove) = match mutation {
            Mutation::Move { destination, flags } => (destination, flags, false),
            Mutation::RemoveDirectory => (None, 0, true),
            Mutation::DeleteFile => (None, 0, false),
        };
        let moving = matches!(mutation, Mutation::Move { .. });
        if flags & !1 != 0 || moving && destination.is_none() {
            return Err(50);
        }
        if under("c:\\", &key)
            || self
                .current_directories
                .iter()
                .filter_map(Weak::upgrade)
                .any(|cwd| under(&cwd.to_lowercase(), &key))
        {
            return Err(32);
        }
        let metadata = self.metadata(&source)?;
        if !moving {
            if remove {
                if metadata.attributes & 0x10 == 0 {
                    return Err(267);
                }
                if !self.entries(&source)?.is_empty() {
                    return Err(145);
                }
                self.directories.remove(&key);
                self.directory_names.remove(&key);
            } else {
                if metadata.attributes & (1 | 0x10) != 0 {
                    return Err(5);
                }
                self.retain_parent(&key)?;
                if let Some(file) = self.entries.remove(&key) {
                    self.bytes -= file.node.borrow().charge();
                }
                if self.executable.as_ref().is_some_and(|e| e.0 == key) {
                    self.executable = None;
                }
            }
            self.remap_identities(&key, None);
            self.path_bytes = self.recalculate_paths();
            return Ok(());
        }
        let target = self.resolve(destination.unwrap())?;
        let target_key = target.to_lowercase();
        if key == target_key {
            return Ok(());
        }
        if metadata.attributes & 0x10 != 0 && under(&target_key, &key) {
            return Err(87);
        }
        if let Ok(existing) = self.metadata(&target) {
            if flags & 1 == 0 {
                return Err(183);
            }
            if (existing.attributes | metadata.attributes) & 0x10 != 0
                || existing.attributes & 1 != 0
            {
                return Err(5);
            }
        }
        if !self
            .metadata(parent(&target_key))
            .is_ok_and(|m| m.attributes & 0x10 != 0)
        {
            return Err(3);
        }
        // Construct the complete rename plan before changing the namespace.
        let remap = |old: &str| -> String { format!("{target_key}{}", &old[key.len()..]) };
        let display = |old: &str| -> String {
            let suffix: Vec<_> = old.split('\\').skip(source.split('\\').count()).collect();
            if suffix.is_empty() {
                target.clone()
            } else {
                format!("{target}\\{}", suffix.join("\\"))
            }
        };
        let files: Vec<_> = self
            .entries
            .iter()
            .filter(|(p, _)| under(p, &key))
            .map(|(p, f)| {
                let mut f = f.clone();
                f.name = display(&f.name);
                (p.clone(), remap(p), f)
            })
            .collect();
        let dirs: Vec<_> = self
            .directories
            .iter()
            .filter(|(p, _)| under(p, &key))
            .map(|(p, m)| {
                let name = self
                    .directory_names
                    .get(p)
                    .map_or(p.as_str(), String::as_str);
                (p.clone(), remap(p), display(name), *m)
            })
            .collect();
        let executable = self
            .executable
            .as_ref()
            .filter(|e| under(&e.0, &key))
            .map(|(p, f)| {
                let mut f = f.clone();
                f.name = display(&f.name);
                (remap(p), f)
            });
        let removed: usize = files.iter().map(|(p, _, _)| p.len() * 2).sum::<usize>()
            + dirs.iter().map(|(p, _, _, _)| p.len() * 2).sum::<usize>()
            + self
                .executable
                .as_ref()
                .filter(|e| under(&e.0, &key) || e.0 == target_key)
                .map_or(0, |e| e.0.len() * 2)
            + self
                .entries
                .get(&target_key)
                .map_or(0, |_| target_key.len() * 2);
        let added: usize = files.iter().map(|(_, p, _)| p.len() * 2).sum::<usize>()
            + dirs.iter().map(|(_, p, _, _)| p.len() * 2).sum::<usize>()
            + executable.as_ref().map_or(0, |e| e.0.len() * 2)
            + if parent(&key) != "c:\\" && !self.directories.contains_key(parent(&key)) {
                parent(&key).len() * 2
            } else {
                0
            };
        if self
            .recalculate_paths()
            .saturating_sub(removed)
            .saturating_add(added)
            > 4 * 1024 * 1024
        {
            return Err(8);
        }
        self.retain_parent(&key)?;
        if self.executable.as_ref().is_some_and(|e| e.0 == target_key) {
            self.executable = None;
        }
        if let Some(replaced) = self.entries.remove(&target_key) {
            self.bytes -= replaced.node.borrow().charge();
        }
        for (old, new, file) in files {
            self.entries.remove(&old);
            self.entries.insert(new, file);
        }
        for (old, new, name, metadata) in dirs {
            self.directories.remove(&old);
            self.directory_names.remove(&old);
            self.directories.insert(new.clone(), metadata);
            self.directory_names.insert(new, name);
        }
        if let Some(executable) = executable {
            self.executable = Some(executable);
        }
        self.remap_identities(&key, Some(&target_key));
        self.path_bytes = self.recalculate_paths();
        Ok(())
    }
}
