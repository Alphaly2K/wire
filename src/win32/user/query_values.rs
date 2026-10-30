//! Window lookup, POINT translation and WINDOWPLACEMENT bytes without memory owners.
pub(crate) const DESKTOP: u32 = 0x4000fffc;

pub(crate) struct Search {
    atom: u32,
    class: Option<String>,
    title: Option<String>,
}
impl Search {
    pub(crate) fn new(atom: u32, class: Option<String>, title: Option<String>) -> Self {
        Self {
            atom,
            class: class.map(|s| s.to_lowercase()),
            title: title.map(|s| s.to_lowercase()),
        }
    }
    pub(crate) fn matches(
        &self,
        style: u32,
        parent: u64,
        atom: u16,
        class: &str,
        title: &str,
    ) -> bool {
        style & 0x40000000 == 0
            && !super::hierarchy::message_parent(parent)
            && (self.atom == 0
                || if let Some(key) = &self.class {
                    class.to_lowercase() == *key
                } else {
                    u32::from(atom) == self.atom
                })
            && self
                .title
                .as_ref()
                .is_none_or(|key| title.to_lowercase() == *key)
    }
}

pub(crate) fn map_points(bytes: &mut [u8], delta: [i32; 2]) -> u32 {
    for (i, chunk) in bytes.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        *chunk = i32::from_le_bytes(*chunk)
            .wrapping_add(delta[i % 2])
            .to_le_bytes();
    }
    (delta[0] as u32 & 0xffff) | ((delta[1] as u32) << 16)
}

pub(crate) fn placement(rect: [i32; 4]) -> [u8; 44] {
    let mut bytes = [0; 44];
    bytes[..4].copy_from_slice(&44u32.to_le_bytes());
    bytes[8..12].copy_from_slice(&1u32.to_le_bytes());
    bytes[12..28].fill(0xff);
    for (i, value) in rect.into_iter().enumerate() {
        bytes[28 + i * 4..32 + i * 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}
