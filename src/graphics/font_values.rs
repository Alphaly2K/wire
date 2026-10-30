//! Logical LOGFONT values and encoding, independent of raster or platform fonts.
use super::GdiOperation as Op;
pub(crate) const STOCK_IDS: [u32; 7] = [10, 11, 12, 13, 14, 16, 17];
pub(crate) const SYSTEM_ID: u32 = 13;

pub(crate) trait Memory {
    type Error: From<u32>;
    fn read(&self, address: u32, bytes: &mut [u8]) -> Result<(), Self::Error>;
    fn text(&self, address: u32, wide: bool) -> Result<Option<Vec<u16>>, Self::Error>;
}
pub(crate) struct Font {
    pub logfont: [u8; 92],
    pub stock: bool,
}
impl Font {
    pub(crate) fn stock(id: u32) -> Self {
        let mut lf = [0; 92];
        lf[..4].copy_from_slice(&(-16i32).to_le_bytes());
        lf[16..20].copy_from_slice(&400u32.to_le_bytes());
        lf[23] = 128;
        let face = if matches!(id, 10 | 11 | 16) {
            "MS Gothic"
        } else {
            "MS UI Gothic"
        };
        for (i, c) in face.encode_utf16().enumerate() {
            lf[28 + i * 2..30 + i * 2].copy_from_slice(&c.to_le_bytes());
        }
        Self {
            logfont: lf,
            stock: true,
        }
    }
    pub(crate) fn output(&self, wide: bool) -> Result<Vec<u8>, u32> {
        if wide {
            return Ok(self.logfont.to_vec());
        }
        let mut out = vec![0; 60];
        out[..28].copy_from_slice(&self.logfont[..28]);
        let units: Vec<_> = self.logfont[28..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes(*b))
            .take_while(|&n| n != 0)
            .collect();
        let ansi = crate::codepage::encode(&String::from_utf16_lossy(&units), 932)
            .map_err(|_| 87u32)?
            .0;
        let count = ansi.len().min(31);
        out[28..28 + count].copy_from_slice(&ansi[..count]);
        Ok(out)
    }
}
pub(crate) fn create<T: Memory>(op: Op, a: &[u32; 16], memory: &T) -> Result<Font, T::Error> {
    let wide = matches!(op, Op::FontW | Op::FontIndirectW);
    let mut lf = [0; 92];
    if matches!(op, Op::FontIndirectA | Op::FontIndirectW) {
        if wide {
            memory.read(a[0], &mut lf)?;
        } else {
            let mut ansi = [0; 60];
            memory.read(a[0], &mut ansi)?;
            lf[..28].copy_from_slice(&ansi[..28]);
            let end = ansi[28..].iter().position(|&c| c == 0).unwrap_or(32);
            let text = crate::codepage::decode(&ansi[28..28 + end], 932).map_err(|_| 87u32)?;
            for (i, c) in text.encode_utf16().take(31).enumerate() {
                lf[28 + i * 2..30 + i * 2].copy_from_slice(&c.to_le_bytes());
            }
        }
        lf[90..].fill(0);
    } else {
        for (i, n) in a[..5].iter().enumerate() {
            lf[i * 4..i * 4 + 4].copy_from_slice(&n.to_le_bytes());
        }
        for i in 0..8 {
            lf[20 + i] = a[5 + i] as u8;
        }
        if let Some(text) = memory.text(a[13], wide)? {
            for (i, c) in text.into_iter().take(31).enumerate() {
                lf[28 + i * 2..30 + i * 2].copy_from_slice(&c.to_le_bytes());
            }
        }
    }
    Ok(Font {
        logfont: lf,
        stock: false,
    })
}
