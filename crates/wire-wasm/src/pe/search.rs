use crate::{filesystem::Files, paths};

#[derive(Clone)]
pub(crate) struct Search {
    pub executable: String,
    pub directory: Option<String>,
}

pub(crate) fn filename(name: &str) -> Result<String, u32> {
    if name.is_empty() || name.contains('\0') {
        return Err(123);
    }
    if name.len() > 32768 {
        return Err(206);
    }
    let mut name = name.replace('/', "\\");
    let base = name.rsplit('\\').next().unwrap();
    if base.is_empty() || matches!(base, "." | "..") {
        return Err(123);
    }
    if !base.contains('.') {
        name.push_str(".dll");
    }
    Ok(name)
}

pub(crate) fn absolute(name: &str) -> bool {
    name.as_bytes().get(1) == Some(&b':')
        && name
            .as_bytes()
            .get(2)
            .is_some_and(|b| matches!(b, b'/' | b'\\'))
        || name.starts_with("\\\\?\\C:\\")
        || name.starts_with("\\\\?\\c:\\")
}

pub(crate) fn qualified(name: &str) -> bool {
    name.contains(['\\', '/', ':'])
}

pub(crate) fn parent(path: &str) -> &str {
    let at = path.rfind('\\').unwrap();
    if at == 2 {
        &path[..3]
    } else {
        &path[..at]
    }
}

impl Search {
    pub fn candidates(
        &self,
        name: &str,
        flags: u32,
        files: &Files,
        altered: Option<&str>,
    ) -> Result<Vec<String>, u32> {
        let name = filename(name)?;
        if qualified(&name) {
            return Ok(vec![files.resolve(&name)?]);
        }
        let bits = if flags & 0x1000 != 0 {
            flags | 0xe00
        } else {
            flags
        };
        let legacy = flags & !8 == 0;
        let mut dirs = Vec::new();
        if flags & (8 | 0x100) != 0 {
            if let Some(dir) = altered {
                dirs.push(dir.to_owned());
            }
        }
        if (legacy && flags != 8) || bits & 0x200 != 0 {
            dirs.push(parent(&self.executable).to_owned());
        }
        if legacy || bits & 0x400 != 0 {
            if let Some(dir) = self.directory.as_ref().filter(|s| !s.is_empty()) {
                dirs.push(dir.clone());
            }
        }
        if legacy || bits & 0x800 != 0 {
            dirs.push("C:\\Windows\\System32".into());
        }
        if legacy {
            dirs.push("C:\\Windows".into());
            if self.directory.is_none() {
                dirs.push(files.cwd().to_owned());
            }
        }
        let mut result = Vec::new();
        for dir in dirs {
            let path = paths::logical_path_at(
                &format!("{}\\{name}", dir.trim_end_matches('\\')),
                files.cwd(),
            )?;
            if !result
                .iter()
                .any(|p: &String| p.eq_ignore_ascii_case(&path))
            {
                result.push(path);
            }
        }
        Ok(result)
    }

    pub fn provider(&self, name: &str, files: &Files) -> Result<Option<String>, u32> {
        let name = filename(name)?;
        let base = name.rsplit('\\').next().unwrap().to_ascii_lowercase();
        if !qualified(&name)
            || files
                .resolve(&name)?
                .eq_ignore_ascii_case(&format!("C:\\Windows\\System32\\{base}"))
        {
            Ok(Some(base))
        } else {
            Ok(None)
        }
    }
}
