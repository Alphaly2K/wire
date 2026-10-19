//! Complete PE32 API trace, frequency counts and bounded crash context.
//! No dependency on a guest CPU, API catalog or runtime state.
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{BufWriter, Write};

pub struct ApiLogger {
    trace: BufWriter<File>,
    call_keys: Vec<String>,
    pub freq: HashMap<String, u64>,
    pub unimpl_freq: HashMap<String, u64>,
    pub recent: VecDeque<String>,
    pub call_count: u64,
}

impl ApiLogger {
    pub fn new(path: &std::path::Path) -> std::io::Result<ApiLogger> {
        Ok(ApiLogger {
            trace: BufWriter::with_capacity(64 * 1024, File::create(path)?),
            call_keys: Vec::new(),
            freq: HashMap::new(),
            unimpl_freq: HashMap::new(),
            recent: VecDeque::new(),
            call_count: 0,
        })
    }

    fn emit(&mut self, line: String) {
        self.trace
            .write_all(line.as_bytes())
            .and_then(|()| self.trace.write_all(b"\n"))
            .ok();
        self.recent.push_back(line);
        while self.recent.len() > 64 {
            self.recent.pop_front();
        }
    }

    pub fn note(&mut self, msg: &str) {
        self.emit(format!("; {msg}"));
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn log_call(
        &mut self,
        idx: usize,
        dll: &str,
        name: &str,
        args: &[u32],
        ret: u32,
        implemented: bool,
        note: &str,
    ) {
        self.call_count += 1;
        if self.call_keys.len() <= idx {
            self.call_keys.resize_with(idx + 1, String::new);
        }
        let key = &mut self.call_keys[idx];
        if key.split_once('!') != Some((dll, name)) {
            key.clear();
            key.push_str(dll);
            key.push('!');
            key.push_str(name);
        }
        if let Some(count) = self.freq.get_mut(key.as_str()) {
            *count += 1;
        } else {
            self.freq.insert(key.clone(), 1);
        }
        if !implemented {
            if let Some(count) = self.unimpl_freq.get_mut(key.as_str()) {
                *count += 1;
            } else {
                self.unimpl_freq.insert(key.clone(), 1);
            }
        }
        // Reuse the evicted diagnostic line and format directly into it.
        // Keep the complete trace and the same 64-call crash context.
        let mut line = if self.recent.len() == 64 {
            self.recent.pop_front().unwrap()
        } else {
            String::with_capacity(160)
        };
        line.clear();
        line.push('#');
        let digits = push_decimal(&mut line, self.call_count);
        for _ in digits..8 {
            line.push(' ');
        }
        line.push(' ');
        line.push_str(key);
        line.push('(');
        for (i, a) in args.iter().enumerate() {
            if i != 0 {
                line.push_str(", ");
            }
            push_hex(&mut line, *a);
        }
        line.push_str(") = ");
        push_hex(&mut line, ret);
        if !implemented {
            line.push_str(" [UNIMPLEMENTED]");
        }
        if !note.is_empty() {
            line.push_str(" ; ");
            line.push_str(note);
        }
        self.emit(line);
    }

    pub fn finish(&mut self) {
        self.emit("; ---- API call frequency (all) ----".into());
        let mut v: Vec<(String, u64)> = self.freq.iter().map(|(k, c)| (k.clone(), *c)).collect();
        v.sort_by_key(|a| std::cmp::Reverse(a.1));
        for (k, c) in &v {
            self.emit(format!("; {c:>8}  {k}"));
        }
        self.emit("; ---- UNIMPLEMENTED API frequency ----".into());
        let mut u: Vec<(String, u64)> = self
            .unimpl_freq
            .iter()
            .map(|(k, c)| (k.clone(), *c))
            .collect();
        u.sort_by_key(|a| std::cmp::Reverse(a.1));
        for (k, c) in &u {
            self.emit(format!("; {c:>8}  {k}"));
        }
        self.trace.flush().ok();
    }
}

// These are the two fixed integer formats used by the complete API trace.
// Stack buffers cover every u64/u32 value; String storage is still reused.
fn push_decimal(line: &mut String, mut value: u64) -> usize {
    let mut bytes = [0u8; 20];
    let mut start = bytes.len();
    loop {
        start -= 1;
        bytes[start] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    line.push_str(std::str::from_utf8(&bytes[start..]).unwrap());
    bytes.len() - start
}

fn push_hex(line: &mut String, mut value: u32) {
    let mut bytes = [0u8; 10];
    let mut start = bytes.len();
    loop {
        start -= 1;
        bytes[start] = b"0123456789abcdef"[(value & 15) as usize];
        value >>= 4;
        if value == 0 {
            break;
        }
    }
    start -= 2;
    bytes[start] = b'0';
    bytes[start + 1] = b'x';
    line.push_str(std::str::from_utf8(&bytes[start..]).unwrap());
}

#[cfg(test)]
mod tests {
    use super::ApiLogger;
    use std::io::Write;

    #[test]
    fn api_log_preserves_numeric_format_and_complete_trace() {
        let path =
            std::env::temp_dir().join(format!("wire-api-log-numeric-{}", std::process::id()));
        let mut log = ApiLogger::new(&path).unwrap();
        let args = [0, 1, 15, 16, 255, 256, 65535, 65536, 0x80000000, u32::MAX];
        let mut expected = String::new();
        for count in [1, 9, 10, 9999999, 10000000, 99999999, u64::MAX] {
            log.call_count = count - 1;
            for (i, value) in args.iter().enumerate() {
                // The maximum counter is tested separately without overflowing.
                if log.call_count == u64::MAX {
                    break;
                }
                let count = count + i as u64;
                let values = &args[..i];
                log.log_call(7, "gdi32.dll", "Value", values, *value, i % 2 == 0, "");
                let values = values
                    .iter()
                    .map(|a| format!("{a:#x}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let marker = if i % 2 == 0 { "" } else { " [UNIMPLEMENTED]" };
                expected.push_str(&format!(
                    "#{count:<8} gdi32.dll!Value({values}) = {value:#x}{marker}\n"
                ));
            }
        }
        log.trace.flush().unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
        assert_eq!(
            log.recent.back().unwrap(),
            "#18446744073709551615 gdi32.dll!Value() = 0x0"
        );
        drop(log);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn api_log_keeps_mutable_identity_counts_notes_and_recent_eviction() {
        let path =
            std::env::temp_dir().join(format!("wire-api-log-history-{}", std::process::id()));
        let mut log = ApiLogger::new(&path).unwrap();
        let mut expected = Vec::new();
        for i in 1..=100 {
            let (dll, name) = if i < 50 {
                ("user32.dll", "A")
            } else {
                ("other!provider", "B!C")
            };
            log.log_call(3, dll, name, &[u32::MAX], 0x123, false, "diagnostic 日本語");
            expected.push(format!(
                "#{i:<8} {dll}!{name}(0xffffffff) = 0x123 [UNIMPLEMENTED] ; diagnostic 日本語"
            ));
            if i % 9 == 0 {
                log.note("interleaved note");
                expected.push("; interleaved note".into());
            }
        }
        log.trace.flush().unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            expected.join("\n") + "\n"
        );
        assert_eq!(
            log.recent.iter().collect::<Vec<_>>(),
            expected[expected.len() - 64..].iter().collect::<Vec<_>>()
        );
        assert_eq!(log.freq["user32.dll!A"], 49);
        assert_eq!(log.freq["other!provider!B!C"], 51);
        assert_eq!(log.freq, log.unimpl_freq);
        assert_eq!(log.call_count, 100);
        drop(log);
        std::fs::remove_file(path).unwrap();
    }
}
