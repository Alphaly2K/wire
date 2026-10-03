//! Guest C:\ drive confined to one canonical host directory.
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct GuestFs {
    root: PathBuf,
    cwd: PathBuf,
}

pub fn win_error(e: &std::io::Error) -> u32 {
    use std::io::ErrorKind::*;
    match e.kind() {
        NotFound => 2,
        PermissionDenied => 5,
        AlreadyExists => 80,
        InvalidInput => 87,
        _ => 31,
    }
}

impl GuestFs {
    pub fn new(root: &Path) -> std::io::Result<Self> {
        let root = root.canonicalize()?;
        if !root.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "guest cwd must be a directory",
            ));
        }
        Ok(Self {
            cwd: root.clone(),
            root,
        })
    }
    pub fn cwd(&self) -> String {
        self.guest_path(&self.cwd)
    }
    pub fn guest_path(&self, path: &Path) -> String {
        format!(
            "C:\\{}",
            path.strip_prefix(&self.root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('/', "\\")
        )
    }
    pub fn set_cwd(&mut self, name: &str) -> Result<(), u32> {
        let p = self.resolve(name)?;
        if !p.is_dir() {
            return Err(3);
        }
        self.cwd = p;
        Ok(())
    }
    pub fn resolve(&self, name: &str) -> Result<PathBuf, u32> {
        if name.is_empty() || name.contains('\0') {
            return Err(123);
        }
        // Accept host paths returned by earlier wire versions only inside root.
        let host = Path::new(name);
        if host.is_absolute() && host.starts_with(&self.root) {
            return self.confined(host.to_path_buf());
        }
        let mut s = name.replace('\\', "/");
        if s.starts_with("//") {
            return Err(5);
        } // UNC/device paths
        let mut absolute = s.starts_with('/');
        if s.as_bytes().get(1) == Some(&b':') {
            if !s[..1].eq_ignore_ascii_case("c") {
                return Err(15);
            }
            absolute = s.as_bytes().get(2) == Some(&b'/');
            s = s[2..].to_string();
        }
        let mut p = if absolute {
            self.root.clone()
        } else {
            self.cwd.clone()
        };
        for part in s.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    if p == self.root {
                        return Err(5);
                    }
                    p.pop();
                }
                _ => {
                    if part.contains(':') {
                        return Err(123);
                    }
                    let mut component = part.to_string();
                    if let Ok(entries) = std::fs::read_dir(&p) {
                        if let Some(found) = entries.flatten().find(|e| {
                            e.file_name().to_string_lossy().to_lowercase() == part.to_lowercase()
                        }) {
                            component = found.file_name().to_string_lossy().into_owned();
                        }
                    }
                    p.push(component);
                }
            }
        }
        self.confined(p)
    }
    fn confined(&self, p: PathBuf) -> Result<PathBuf, u32> {
        // Validate the nearest existing ancestor for new files as well as
        // existing ones, so symlinks cannot redirect writes outside the root.
        let mut ancestor = p.as_path();
        loop {
            match std::fs::symlink_metadata(ancestor) {
                Ok(_) => {
                    let real = ancestor.canonicalize().map_err(|e| win_error(&e))?;
                    if !real.starts_with(&self.root) {
                        return Err(5);
                    }
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    ancestor = ancestor.parent().ok_or(5u32)?;
                }
                Err(e) => return Err(win_error(&e)),
            }
        }
        if !p.starts_with(&self.root) {
            return Err(5);
        }
        Ok(p)
    }
}
