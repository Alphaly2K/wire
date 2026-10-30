//! Logical font objects share LOGFONT rules; no platform realization is implied.
use super::{user, Failure, Gdi, O};
use crate::memory::{Memory, WRITE};
use O as GdiOperation;
#[path = "../../../../src/graphics/font_values.rs"]
pub(super) mod values;

struct Adapter<'a>(&'a mut Memory);
impl user::menus::Memory for Adapter<'_> {
    type Error = Failure;
    fn read(&self, p: u32, bytes: &mut [u8]) -> Result<(), Failure> {
        self.0.read(p.into(), bytes).map_err(Into::into)
    }
    fn check_write(&self, p: u32, n: usize) -> Result<(), Failure> {
        self.0.check(p.into(), n, WRITE).map_err(Into::into)
    }
    fn write(&mut self, p: u32, bytes: &[u8]) -> Result<(), Failure> {
        self.0.write(p.into(), bytes).map_err(Into::into)
    }
}
impl values::Memory for Adapter<'_> {
    type Error = Failure;
    fn read(&self, p: u32, bytes: &mut [u8]) -> Result<(), Failure> {
        user::menus::Memory::read(self, p, bytes)
    }
    fn text(&self, p: u32, wide: bool) -> Result<Option<Vec<u16>>, Failure> {
        user::menus::read_text(self, p, wide)
    }
}
pub(super) fn dispatch(
    id: O,
    a: &[u64; 16],
    memory: &mut Memory,
    g: &mut Gdi,
) -> Option<Result<u64, Failure>> {
    Some((|| {
        Ok(match id {
            O::FontA | O::FontW | O::FontIndirectA | O::FontIndirectW => {
                let value = values::create(id, &a.map(|v| v as u32), &Adapter(memory))?;
                let handle = g.handle(1)?;
                g.fonts.insert(handle, value);
                g.next += 16;
                handle
            }
            O::TextMetricsW => {
                let font = g.dcs.state(a[0])?.font;
                g.fonts.get(&font).ok_or(6u32)?;
                return Err(Failure::Windows(120));
            }
            O::StockObject if values::STOCK_IDS.contains(&(a[0] as u32)) && !g.fonts.is_empty() => {
                g.font_base + a[0] * 4
            }
            O::Select if g.fonts.contains_key(&a[1]) => {
                std::mem::replace(&mut g.dcs.state_mut(a[0])?.font, a[1])
            }
            O::Delete if g.fonts.contains_key(&a[0]) => {
                if !g.fonts[&a[0]].stock {
                    if g.dcs.states().any(|(_, d)| d.font == a[0]) {
                        return Err(Failure::Windows(170));
                    }
                    g.fonts.remove(&a[0]);
                }
                1
            }
            O::ObjectType if g.fonts.contains_key(&a[0]) => 6,
            O::CurrentObject if a[1] == 6 && !g.fonts.is_empty() => g.dcs.state(a[0])?.font,
            O::ObjectA | O::ObjectW if g.fonts.contains_key(&a[0]) => {
                let size = if id == O::ObjectW { 92 } else { 60 };
                if a[2] != 0 {
                    if a[1] < size {
                        return Err(Failure::Windows(87));
                    }
                    memory.write(a[2], &g.fonts[&a[0]].output(id == O::ObjectW)?)?;
                }
                size
            }
            _ => return Err(Failure::Windows(u32::MAX)),
        })
    })())
    .filter(|result| !matches!(result, Err(Failure::Windows(u32::MAX))))
}
