//! Immutable input windows and private writes over a sparse file.
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    rc::Rc,
};

pub(crate) const PAGE: u64 = 65536;
pub(crate) const TRANSFER_LIMIT: usize = 256 * 1024 * 1024;
const CACHE_LIMIT: usize = 64 * 1024 * 1024;
type CachedRange = (u64, Rc<Vec<u8>>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadRequest {
    pub source: u32,
    pub offset: u64,
    pub length: usize,
}
#[derive(Debug)]
pub enum ReadError {
    Windows(u32),
    Input(ReadRequest),
}
impl From<u32> for ReadError {
    fn from(code: u32) -> Self {
        Self::Windows(code)
    }
}
impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Windows(code) => write!(f, "file error {code}"),
            Self::Input(_) => f.write_str("file input is required"),
        }
    }
}
impl ReadError {
    pub(crate) fn code(self) -> u32 {
        match self {
            Self::Windows(code) => code,
            Self::Input(_) => 50,
        }
    }
}

struct Window {
    request: ReadRequest,
    bytes: Rc<Vec<u8>>,
}
pub(crate) struct Cache {
    limit: usize,
    next: u32,
    windows: VecDeque<Window>,
    bytes: usize,
    pending: Option<ReadRequest>,
    pinned: bool,
}
impl Default for Cache {
    fn default() -> Self {
        Self {
            limit: CACHE_LIMIT,
            next: 1,
            windows: VecDeque::new(),
            bytes: 0,
            pending: None,
            pinned: false,
        }
    }
}
impl Cache {
    pub(crate) fn source(&mut self) -> Result<u32, u32> {
        let id = self.next;
        if id > 4096 {
            return Err(8);
        }
        self.next = id.checked_add(1).ok_or(8u32)?;
        Ok(id)
    }
    fn get(&mut self, source: u32, offset: u64, length: usize) -> Option<(u64, Rc<Vec<u8>>)> {
        let at = self.windows.iter().position(|window| {
            window.request.source == source
                && window.request.offset <= offset
                && offset + length as u64 <= window.request.offset + window.request.length as u64
        })?;
        let window = self.windows.remove(at).unwrap();
        let value = (window.request.offset, window.bytes.clone());
        self.windows.push_back(window);
        Some(value)
    }
    fn request(&mut self, request: ReadRequest) -> ReadError {
        let replaced = self
            .windows
            .iter()
            .filter(|window| Self::contains(request, window.request))
            .map(|window| window.bytes.len())
            .sum::<usize>();
        if self.pinned && self.bytes - replaced + request.length > 512 * 1024 * 1024 {
            return ReadError::Windows(8);
        }
        self.pending = Some(request);
        ReadError::Input(request)
    }
    pub(crate) fn pending(&self) -> Option<ReadRequest> {
        self.pending
    }
    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }
    pub(crate) fn supply(&mut self, request: ReadRequest, bytes: Vec<u8>) -> Result<(), String> {
        if self.pending != Some(request) || bytes.len() != request.length {
            return Err("file input does not match the pending range".into());
        }
        self.windows.retain(|window| {
            if Self::contains(request, window.request) {
                self.bytes -= window.bytes.len();
                false
            } else {
                true
            }
        });
        self.bytes += bytes.len();
        self.windows.push_back(Window {
            request,
            bytes: Rc::new(bytes),
        });
        self.pending = None;
        if !self.pinned {
            self.trim(true);
        }
        Ok(())
    }
    fn trim(&mut self, retain_last: bool) {
        while self.bytes > self.limit && self.windows.len() > usize::from(retain_last) {
            self.bytes -= self.windows.pop_front().unwrap().bytes.len();
        }
    }
    fn contains(a: ReadRequest, b: ReadRequest) -> bool {
        a.source == b.source
            && a.offset <= b.offset
            && b.offset + b.length as u64 <= a.offset + a.length as u64
    }
    pub(crate) fn finish(&mut self) {
        self.pinned = false;
        self.trim(false);
    }
}

#[derive(Clone)]
pub(crate) struct Source {
    id: u32,
    length: u64,
    limit: u64,
    pages: BTreeMap<u64, Rc<Vec<u8>>>,
    cache: Rc<RefCell<Cache>>,
}
impl Source {
    pub(crate) fn new(id: u32, length: u64, cache: Rc<RefCell<Cache>>) -> Self {
        Self {
            id,
            length,
            limit: length,
            pages: BTreeMap::new(),
            cache,
        }
    }
    pub(crate) fn charge(&self) -> usize {
        self.pages.len() * PAGE as usize
    }
    pub(crate) fn resize_charge(&self, size: u64) -> usize {
        self.pages.range(..size.div_ceil(PAGE)).count() * PAGE as usize
    }
    pub(crate) fn materialize(&self, size: u64) -> Result<Rc<Vec<u8>>, ReadError> {
        if size > TRANSFER_LIMIT as u64 {
            return Err(8.into());
        }
        self.cache.borrow_mut().pinned = true;
        if self.pages.is_empty() && self.limit >= size && size != 0 {
            if let Some((start, bytes)) = self.base(0, size as usize)? {
                if start == 0 && bytes.len() == size as usize {
                    return Ok(bytes);
                }
            }
        }
        Ok(Rc::new(self.read(0, size as usize)?))
    }
    pub(crate) fn write_charge(&self, offset: u64, length: usize) -> Result<usize, u32> {
        if length == 0 {
            return Ok(self.charge());
        }
        let last = offset.checked_add(length as u64 - 1).ok_or(8u32)? / PAGE;
        let first = offset / PAGE;
        let added = last - first + 1 - self.pages.range(first..=last).count() as u64;
        self.charge()
            .checked_add(usize::try_from(added * PAGE).map_err(|_| 8u32)?)
            .ok_or(8)
    }
    fn base(&self, offset: u64, length: usize) -> Result<Option<CachedRange>, ReadError> {
        let end = (offset + length as u64).min(self.limit);
        let mut first = None;
        let mut last = 0;
        let mut at = offset;
        while at < end {
            let next = ((at / PAGE + 1) * PAGE).min(end);
            if !self.pages.contains_key(&(at / PAGE)) {
                first.get_or_insert(at);
                last = next;
            }
            at = next;
        }
        let Some(first) = first else {
            return Ok(None);
        };
        let mut cache = self.cache.borrow_mut();
        if let Some(window) = cache.get(self.id, first, (last - first) as usize) {
            return Ok(Some(window));
        }
        let start = if last - first < TRANSFER_LIMIT as u64 - PAGE {
            first / PAGE * PAGE
        } else {
            first
        };
        let end = if last - start < TRANSFER_LIMIT as u64 - PAGE {
            last.div_ceil(PAGE).saturating_mul(PAGE).min(self.length)
        } else {
            last
        };
        Err(cache.request(ReadRequest {
            source: self.id,
            offset: start,
            length: (end - start) as usize,
        }))
    }
    pub(crate) fn read(&self, offset: u64, length: usize) -> Result<Vec<u8>, ReadError> {
        if length > TRANSFER_LIMIT || offset.checked_add(length as u64).is_none() {
            return Err(8.into());
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| ReadError::Windows(8))?;
        let base = self.base(offset, length)?;
        bytes.resize(length, 0);
        let mut at = offset;
        let end = offset + length as u64;
        while at < end {
            let next = ((at / PAGE + 1) * PAGE).min(end);
            let target = &mut bytes[(at - offset) as usize..(next - offset) as usize];
            if let Some(page) = self.pages.get(&(at / PAGE)) {
                let within = (at % PAGE) as usize;
                target.copy_from_slice(&page[within..within + target.len()]);
            } else if at < self.limit {
                let count = (next.min(self.limit) - at) as usize;
                let (start, data) = base.as_ref().unwrap();
                let within = (at - start) as usize;
                target[..count].copy_from_slice(&data[within..within + count]);
            }
            at = next;
        }
        Ok(bytes)
    }
    pub(crate) fn write(&self, offset: u64, bytes: &[u8]) -> Result<Self, ReadError> {
        let end = offset
            .checked_add(bytes.len() as u64)
            .ok_or(ReadError::Windows(8))?;
        if end > i64::MAX as u64 || bytes.len() > TRANSFER_LIMIT {
            return Err(8.into());
        }
        let mut next = self.clone();
        let mut at = offset;
        while at < end {
            let page = at / PAGE;
            let start = page * PAGE;
            let to = ((page + 1) * PAGE).min(end);
            let full = at == start && to == start + PAGE;
            let mut data = if full {
                let mut data = Vec::new();
                data.try_reserve_exact(PAGE as usize)
                    .map_err(|_| ReadError::Windows(8))?;
                data.resize(PAGE as usize, 0);
                data
            } else {
                self.read(start, PAGE as usize)?
            };
            data[(at - start) as usize..(to - start) as usize]
                .copy_from_slice(&bytes[(at - offset) as usize..(to - offset) as usize]);
            next.pages.insert(page, Rc::new(data));
            at = to;
        }
        Ok(next)
    }
    pub(crate) fn resize(&self, size: u64) -> Result<Self, ReadError> {
        let mut next = self.clone();
        next.limit = next.limit.min(size);
        next.pages.retain(|&page, _| page * PAGE < size);
        if !size.is_multiple_of(PAGE) {
            if let Some(page) = next.pages.get_mut(&(size / PAGE)) {
                let mut data = Vec::new();
                data.try_reserve_exact(PAGE as usize)
                    .map_err(|_| ReadError::Windows(8))?;
                data.extend_from_slice(page);
                data[(size % PAGE) as usize..].fill(0);
                *page = Rc::new(data);
            }
        }
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bytes(offset: u64, length: usize) -> Vec<u8> {
        (0..length)
            .map(|n| {
                let at = offset + n as u64;
                (at.wrapping_mul(37) ^ (at >> 7)) as u8
            })
            .collect()
    }
    fn finish<T>(source: &Source, mut run: impl FnMut() -> Result<T, ReadError>) -> T {
        loop {
            match run() {
                Ok(value) => return value,
                Err(ReadError::Input(request)) => source
                    .cache
                    .borrow_mut()
                    .supply(request, bytes(request.offset, request.length))
                    .unwrap(),
                Err(error) => panic!("{error}"),
            }
        }
    }
    #[test]
    fn sparse_writes_copy_on_write_and_truncation_match_dense_bytes() {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let mut source = Source::new(1, PAGE * 3 + 7, cache);
        let original = source.clone();
        let mut expected = bytes(0, (PAGE * 3 + 7) as usize);
        for (offset, input) in [
            (PAGE - 3, vec![91; 11]),
            (PAGE * 2 + 8, vec![0x5a; 19]),
            (4, vec![0x31; PAGE as usize + 21]),
        ] {
            source = finish(&source, || source.write(offset, &input));
            expected[offset as usize..offset as usize + input.len()].copy_from_slice(&input);
            assert_eq!(finish(&source, || source.read(0, expected.len())), expected);
        }
        assert_eq!(
            finish(&original, || original.read(0, expected.len())),
            bytes(0, expected.len())
        );
        let shortened = PAGE + 9;
        source = source.resize(shortened).unwrap();
        expected.truncate(shortened as usize);
        expected.resize((PAGE * 4) as usize, 0);
        source = source.resize(PAGE * 4).unwrap();
        assert_eq!(finish(&source, || source.read(0, expected.len())), expected);
        assert_eq!(source.charge(), PAGE as usize * 2);
    }
    #[test]
    fn high_offsets_and_holes_do_not_allocate_file_length() {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let source = Source::new(7, 1 << 40, cache.clone());
        let high = (1u64 << 36) + 19;
        assert_eq!(finish(&source, || source.read(high, 33)), bytes(high, 33));
        assert_eq!(cache.borrow().bytes, PAGE as usize);
        assert_eq!(source.charge(), 0);
        let changed = finish(&source, || source.write(high, &[2, 4, 6]));
        assert_eq!(changed.charge(), PAGE as usize);
        let shrunk = changed.resize(4).unwrap();
        let grown = shrunk.resize(1 << 40).unwrap();
        assert_eq!(grown.read(high, 33).unwrap(), vec![0; 33]);
        assert_eq!(grown.charge(), 0);
    }
    #[test]
    fn input_validation_and_transient_windows_keep_the_cache_bounded() {
        let limit = 31;
        let mut cache = Cache {
            limit,
            ..Default::default()
        };
        let mut owner = None;
        for n in 0..3 {
            let request = ReadRequest {
                source: 1,
                offset: n * limit as u64,
                length: limit,
            };
            cache.request(request);
            assert!(cache.supply(request, vec![1; 3]).is_err());
            assert_eq!(cache.pending(), Some(request));
            cache.supply(request, vec![n as u8; limit]).unwrap();
            owner = cache.get(1, request.offset, 1).map(|(_, bytes)| bytes);
            assert!(cache.bytes <= limit);
        }
        let request = ReadRequest {
            source: 2,
            offset: 0,
            length: limit + 1,
        };
        cache.request(request);
        cache.supply(request, vec![5; limit + 1]).unwrap();
        assert_eq!(cache.bytes, limit + 1);
        cache.finish();
        assert_eq!(cache.bytes, 0);
        assert_eq!(owner.unwrap()[0], 2);
    }
    #[test]
    fn image_graph_retries_keep_earlier_inputs_until_publication() {
        let cache = Rc::new(RefCell::new(Cache {
            limit: 31,
            ..Default::default()
        }));
        let a = Source::new(1, 64, cache.clone());
        let b = Source::new(2, 64, cache.clone());
        let first = finish(&a, || a.materialize(64));
        let second = finish(&b, || b.materialize(64));
        assert!(Rc::ptr_eq(&first, &a.materialize(64).unwrap()));
        assert!(Rc::ptr_eq(&second, &b.materialize(64).unwrap()));
        assert_eq!(cache.borrow().bytes, 128);
        cache.borrow_mut().finish();
        assert_eq!(cache.borrow().bytes, 0);
        assert_eq!(&*first, &bytes(0, 64));
        assert_eq!(&*second, &bytes(0, 64));
    }
}
