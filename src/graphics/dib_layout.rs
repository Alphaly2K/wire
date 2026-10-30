//! 32-bit DIB headers and pixel layout, independent of guest memory and handles.
#[derive(Clone, Copy)]
pub(crate) struct Layout {
    pub width: u32,
    pub height: u32,
    pub top_down: bool,
    pub compression: u32,
}

impl Layout {
    pub fn parse(f: [u32; 10]) -> Result<Self, u32> {
        if f[0] != 40 || f[3] != 0x00200001 || !matches!(f[4], 0 | 3) || f[8] != 0 {
            return Err(120);
        }
        let width = f[1] as i32;
        let height = f[2] as i32;
        if width <= 0 || height == 0 || height == i32::MIN {
            return Err(87);
        }
        Ok(Self {
            width: width as u32,
            height: height.unsigned_abs(),
            top_down: height < 0,
            compression: f[4],
        })
    }
    pub fn masks(masks: [u32; 3]) -> Result<(), u32> {
        if masks == [0xff0000, 0xff00, 0xff] {
            Ok(())
        } else {
            Err(120)
        }
    }
    pub fn offset(self, x: u32, y: u32) -> u64 {
        let y = if self.top_down {
            y
        } else {
            self.height - 1 - y
        };
        (u64::from(y) * u64::from(self.width) + u64::from(x)) * 4
    }
}

pub(crate) fn swap_rb(c: u32) -> u32 {
    (c & 0xff00ff00) | ((c & 255) << 16) | ((c >> 16) & 255)
}
