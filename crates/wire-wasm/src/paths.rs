//! Logical C-drive names without host path discovery or symlink traversal.
pub(crate) fn logical_path(name: &str) -> Result<String, u32> {
    logical_path_at(name, "C:\\")
}
pub(crate) fn logical_path_at(name: &str, cwd: &str) -> Result<String, u32> {
    if name.is_empty() || name.contains('\0') {
        return Err(123);
    }
    let name = if let Some(local) = name.strip_prefix("\\\\?\\") {
        let bytes = local.as_bytes();
        if bytes.len() < 3
            || !bytes[0].eq_ignore_ascii_case(&b'c')
            || bytes[1] != b':'
            || bytes[2] != b'\\'
        {
            return Err(5);
        }
        local
    } else {
        name
    };
    let mut name = name.replace('\\', "/");
    if name.starts_with("//") {
        return Err(5);
    }
    let mut absolute = name.starts_with('/');
    if name.as_bytes().get(1) == Some(&b':') {
        if !name[..1].eq_ignore_ascii_case("c") {
            return Err(15);
        }
        absolute = name.as_bytes().get(2) == Some(&b'/');
        name = name[2..].to_owned();
    }
    let mut parts: Vec<&str> = if absolute {
        Vec::new()
    } else {
        cwd[3..].split('\\').filter(|p| !p.is_empty()).collect()
    };
    for part in name.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop().ok_or(5u32)?;
            }
            _ if part.contains(':') => return Err(123),
            _ => parts.push(part),
        }
    }
    Ok(format!("C:\\{}", parts.join("\\")))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn logical_paths_keep_local_drive_and_root_escape_rules() {
        for (input, expected) in [
            ("C:\\", "C:\\"),
            ("c:/plugins/../lib", "C:\\lib"),
            ("lib/./日本", "C:\\lib\\日本"),
            ("\\lib", "C:\\lib"),
            ("C:lib", "C:\\lib"),
            ("\\\\?\\C:\\lib", "C:\\lib"),
        ] {
            assert_eq!(logical_path(input).unwrap(), expected);
        }
        for (input, error) in [
            ("", 123),
            ("a\0b", 123),
            ("../lib", 5),
            ("C:/../lib", 5),
            ("D:/lib", 15),
            ("//server/share", 5),
            ("\\\\?\\D:\\lib", 5),
            ("a:b/c", 15),
            ("abc:def", 123),
        ] {
            assert_eq!(logical_path(input), Err(error));
        }
    }
}
