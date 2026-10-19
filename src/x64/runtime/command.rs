//! Shell command-line quoting, including the special executable argument.
use super::{Memory, Runtime64, COMMAND_A, COMMAND_W, PAGE, PARAMS, WRITE};
fn quote(value: &str) -> Result<String, String> {
    if value.contains('\0') {
        return Err("guest argument contains NUL".into());
    }
    let mut out = String::from("\"");
    let mut slashes = 0;
    for c in value.chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        out.extend(std::iter::repeat_n(
            '\\',
            if c == '"' { slashes * 2 + 1 } else { slashes },
        ));
        slashes = 0;
        out.push(c);
    }
    out.extend(std::iter::repeat_n('\\', slashes * 2));
    out.push('"');
    Ok(out)
}
impl Runtime64 {
    /// Append literal guest arguments before starting the process. No shell runs.
    pub fn append_guest_arguments(&self, mem: &mut Memory, args: &[String]) -> Result<(), String> {
        if args.is_empty() {
            return Ok(());
        }
        let mut command = Self::utf16(mem, COMMAND_W)?;
        for arg in args {
            command.push(' ');
            command.push_str(&quote(arg)?);
        }
        let mut ansi = crate::codepage::encode(&command, 932)
            .map_err(|e| format!("guest command-line encoding error {e}"))?
            .0;
        ansi.push(0);
        let wide: Vec<u8> = command
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        if ansi.len() > PAGE as usize || wide.len() > PAGE as usize {
            return Err("AMD64 command line exceeds startup buffer".into());
        }
        mem.check(COMMAND_A, ansi.len(), WRITE)?;
        mem.check(COMMAND_W, wide.len(), WRITE)?;
        mem.check(PARAMS + 0x70, 4, WRITE)?;
        mem.write(COMMAND_A, &ansi)?;
        mem.write(COMMAND_W, &wide)?;
        mem.write_int(PARAMS + 0x70, 2, wide.len() as u64 - 2)?;
        mem.write_int(PARAMS + 0x72, 2, wide.len() as u64)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::win32::kernel::command_line::arguments;
    use crate::x64::memory::READ;
    #[test]
    fn guest_arguments_preserve_quotes_unicode_executable_and_atomic_startup_buffers() {
        let mut m = Memory::default();
        const STACK: u64 = 0x700000000;
        m.map(STACK, 8192, READ | WRITE).unwrap();
        let r = Runtime64::new(&mut m, 0x140000000, STACK, 8192, "\"C:\\a b.exe\"").unwrap();
        let args: Vec<String> = [
            "-force-windows-7-like-d3d11",
            "a b",
            "",
            "a\"b",
            "C:\\a b\\",
            "日本語",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        r.append_guest_arguments(&mut m, &args).unwrap();
        let wide = Runtime64::utf16(&m, COMMAND_W).unwrap();
        let mut expected = vec!["C:\\a b.exe".to_owned()];
        expected.extend(args);
        assert_eq!(arguments(&wide), expected);
        assert!(wide.contains("\"-force-windows-7-like-d3d11\""));
        let ansi = Runtime64::string_bytes(&m, COMMAND_A, u32::MAX as u64, 1).unwrap();
        assert_eq!(
            crate::codepage::decode(&ansi[..ansi.len() - 1], 932).unwrap(),
            wide
        );
        assert_eq!(
            m.read_int(PARAMS + 0x70, 2).unwrap(),
            (wide.encode_utf16().count() * 2) as u64
        );
        for arg in ["bad\0value".to_owned(), "x".repeat(4096)] {
            assert!(r.append_guest_arguments(&mut m, &[arg]).is_err());
            assert_eq!(Runtime64::utf16(&m, COMMAND_W).unwrap(), wide);
        }
        m.protect(COMMAND_W, PAGE, READ).unwrap();
        assert!(r.append_guest_arguments(&mut m, &["new".into()]).is_err());
        assert_eq!(
            Runtime64::string_bytes(&m, COMMAND_A, u32::MAX as u64, 1).unwrap(),
            ansi
        );
        assert_eq!(Runtime64::utf16(&m, COMMAND_W).unwrap(), wide);
        assert_eq!(
            m.read_int(PARAMS + 0x72, 2).unwrap(),
            (wide.encode_utf16().count() * 2 + 2) as u64
        );
    }
}
