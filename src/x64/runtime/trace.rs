//! Bounded, owned diagnostic history. Calls retain values, not formatted text;
//! printing preserves the existing trace syntax. Names are interned once.
use std::{
    collections::{vec_deque, BTreeMap, VecDeque},
    fmt,
    sync::Arc,
};

#[derive(Debug)]
enum Arguments {
    Inline { values: [u64; 4], len: u8 },
    Extra(Box<[u64]>),
}
impl Arguments {
    fn new(args: &[u64]) -> Self {
        // Scalar gateways have at most four values. Express their bounded
        // copies directly instead of calling memcpy for 0–32 bytes per API.
        let values = match args {
            [] => [0; 4],
            [a] => [*a, 0, 0, 0],
            [a, b] => [*a, *b, 0, 0],
            [a, b, c] => [*a, *b, *c, 0],
            [a, b, c, d] => [*a, *b, *c, *d],
            _ => return Self::Extra(args.into()),
        };
        Self::Inline {
            values,
            len: args.len() as u8,
        }
    }
    fn slice(&self) -> &[u64] {
        match self {
            Self::Inline { values, len } => &values[..*len as usize],
            Self::Extra(values) => values,
        }
    }
}

#[derive(Debug)]
pub struct TraceRecord(Record);
#[derive(Debug)]
enum Record {
    Note(String),
    Call {
        export: Arc<str>,
        args: Arguments,
        ret: u64,
    },
}
impl From<String> for TraceRecord {
    fn from(value: String) -> Self {
        Self(Record::Note(value))
    }
}
impl fmt::Display for TraceRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Record::Note(message) => f.write_str(message),
            Record::Call { export, args, ret } => {
                write!(f, "{export} args={:x?} -> {ret:#x}", args.slice())
            }
        }
    }
}
impl TraceRecord {
    pub fn contains(&self, text: &str) -> bool {
        self.to_string().contains(text)
    }
}

#[derive(Default)]
pub struct Trace {
    records: VecDeque<TraceRecord>,
    names: BTreeMap<String, BTreeMap<String, Arc<str>>>,
}
impl Trace {
    pub fn len(&self) -> usize {
        self.records.len()
    }
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
    pub fn iter(&self) -> vec_deque::Iter<'_, TraceRecord> {
        self.records.iter()
    }
    pub fn back(&self) -> Option<&TraceRecord> {
        self.records.back()
    }
    pub fn pop_front(&mut self) -> Option<TraceRecord> {
        self.records.pop_front()
    }
    pub fn push_back(&mut self, value: impl Into<TraceRecord>) {
        self.evict_if_full();
        self.records.push_back(value.into());
    }
    fn evict_if_full(&mut self) {
        if self.records.len() == 4096 {
            self.records.pop_front();
        }
    }
    pub(super) fn call(&mut self, dll: &str, name: &str, args: &[u64], ret: u64) {
        let export = if let Some(export) = self.names.get(dll).and_then(|m| m.get(name)) {
            export.clone()
        } else {
            let export: Arc<str> = format!("{dll}!{name}").into();
            self.names
                .entry(dll.into())
                .or_default()
                .insert(name.into(), export.clone());
            export
        };
        self.call_bound(export, args, ret);
    }
    pub(super) fn call_bound(&mut self, export: Arc<str>, args: &[u64], ret: u64) {
        // Own borrowed arguments before eviction, but assemble the complete
        // record afterwards so it need not survive the old record's drop.
        let args = Arguments::new(args);
        self.evict_if_full();
        self.records
            .push_back(TraceRecord(Record::Call { export, args, ret }));
    }
}
impl<'a> IntoIterator for &'a Trace {
    type Item = &'a TraceRecord;
    type IntoIter = vec_deque::Iter<'a, TraceRecord>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn structured_calls_keep_owned_arguments_formatting_and_bounded_order() {
        let mut trace = Trace::default();
        for count in 0..=16 {
            let mut args: Vec<u64> = (0..count).map(|n| 0xdeadbeef00000000 + n).collect();
            let expected = format!("kernel32.dll!Example args={args:x?} -> 0x123");
            trace.call("kernel32.dll", "Example", &args, 0x123);
            args.fill(0);
            assert_eq!(trace.back().unwrap().to_string(), expected);
        }
        trace.push_back("note".to_string());
        assert_eq!(trace.back().unwrap().to_string(), "note");
        for value in 0..5000 {
            trace.call("kernel32.dll", "Example", &[value], value);
        }
        assert_eq!(trace.len(), 4096);
        assert_eq!(
            trace.iter().next().unwrap().to_string(),
            "kernel32.dll!Example args=[388] -> 0x388"
        );
        assert_eq!(
            trace.back().unwrap().to_string(),
            "kernel32.dll!Example args=[1387] -> 0x1387"
        );
    }
}
