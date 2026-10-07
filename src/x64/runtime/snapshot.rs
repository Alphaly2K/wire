//! Toolhelp snapshots expose the isolated guest process, never host applications.
pub(super) struct Snapshot {
    pub process: bool,
    pub enumerate_threads: bool,
    pub threads: Vec<(u32, i32)>,
    pub name: String,
    pub process_done: bool,
    pub thread_at: usize,
    pub flags: u32,
}
impl Snapshot {
    pub fn process_entry(&self, wide: bool) -> Result<Vec<u8>, u32> {
        let mut data = vec![0; if wide { 568 } else { 304 }];
        let size = data.len() as u32;
        data[..4].copy_from_slice(&size.to_le_bytes());
        data[8..12].copy_from_slice(&1u32.to_le_bytes());
        data[28..32].copy_from_slice(&(self.threads.len() as u32).to_le_bytes());
        data[36..40].copy_from_slice(&8i32.to_le_bytes());
        if wide {
            let name: Vec<_> = self.name.encode_utf16().collect();
            if name.len() >= 260 {
                return Err(206);
            }
            for (n, u) in name.iter().enumerate() {
                data[44 + n * 2..46 + n * 2].copy_from_slice(&u.to_le_bytes());
            }
        } else {
            let name = crate::codepage::encode(&self.name, 932)?.0;
            if name.len() >= 260 {
                return Err(206);
            }
            data[44..44 + name.len()].copy_from_slice(&name);
        }
        Ok(data)
    }
    pub fn thread_entry(&self, index: usize) -> Option<Vec<u8>> {
        if !self.enumerate_threads {
            return None;
        }
        let &(id, priority) = self.threads.get(index)?;
        let base = match priority {
            -15 => 1,
            15 => 15,
            _ => 8 + priority,
        };
        Some(
            [28u32, 0, id, 1, base as u32, 0, 0]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect(),
        )
    }
}
