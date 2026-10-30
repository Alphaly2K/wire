//! Pure Windows timezone rules and an explicit VM-owned zone profile.
use super::calendar::{calendar_to_ticks, ticks_to_calendar};
const MINUTE: i128 = 600000000;
pub(crate) fn fields(bytes: &[u8]) -> [u16; 8] {
    std::array::from_fn(|n| u16::from_le_bytes(bytes[n * 2..n * 2 + 2].try_into().unwrap()))
}
pub(crate) fn ticks(fields: [u16; 8]) -> Result<u64, u32> {
    calendar_to_ticks(fields).ok_or(87)
}
pub(crate) fn calendar(ticks: i128) -> Result<[u16; 8], u32> {
    u64::try_from(ticks)
        .ok()
        .and_then(ticks_to_calendar)
        .ok_or(87)
}
pub(crate) struct Zone {
    standard: i64,
    daylight: i64,
    standard_date: [u16; 8],
    daylight_date: [u16; 8],
}
impl Zone {
    pub(crate) fn parse(bytes: &[u8; 172]) -> Result<Self, u32> {
        let bias = |at| i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as i64;
        let standard_date = fields(&bytes[68..84]);
        let daylight_date = fields(&bytes[152..168]);
        if (standard_date[1] == 0) != (daylight_date[1] == 0) {
            return Err(87);
        }
        for date in [standard_date, daylight_date] {
            if date[1] != 0
                && (date[1] > 12
                    || date[2] > 6
                    || date[3] == 0
                    || date[0] == 0 && date[3] > 5
                    || date[4] > 23
                    || date[5] > 59
                    || date[6] > 59
                    || date[7] > 999)
            {
                return Err(87);
            }
        }
        for date in [standard_date, daylight_date] {
            if date[1] != 0 && date[0] != 0 {
                ticks(date)?;
            }
        }
        Ok(Self {
            // Without transition dates only Bias applies; Windows ignores both extra biases.
            standard: bias(0) + if standard_date[1] == 0 { 0 } else { bias(84) },
            daylight: bias(0) + bias(168),
            standard_date,
            daylight_date,
        })
    }
    fn transition(date: [u16; 8], year: u16) -> Result<u64, u32> {
        let mut date = date;
        if date[0] == 0 {
            date[0] = year;
            let occurrence = date[3];
            date[3] = 1;
            let first = calendar(ticks(date)? as i128)?[2];
            date[3] = 1 + (date[2] + 7 - first) % 7 + (occurrence - 1) * 7;
            if occurrence == 5 && calendar_to_ticks(date).is_none() {
                date[3] -= 7;
            }
        }
        ticks(date)
    }
    fn daylight_at(&self, utc: u64) -> Result<bool, u32> {
        if self.standard_date[1] == 0 {
            return Ok(false);
        }
        let year = calendar(utc as i128 - self.standard as i128 * MINUTE)?[0];
        let daylight =
            Self::transition(self.daylight_date, year)? as i128 + self.standard as i128 * MINUTE;
        let standard =
            Self::transition(self.standard_date, year)? as i128 + self.daylight as i128 * MINUTE;
        let utc = utc as i128;
        let summer = if daylight < standard {
            utc >= daylight && utc < standard
        } else {
            utc >= daylight || utc < standard
        };
        Ok(summer)
    }
    fn bias_at(&self, utc: u64) -> Result<i64, u32> {
        Ok(if self.daylight_at(utc)? {
            self.daylight
        } else {
            self.standard
        })
    }
    #[allow(dead_code)] // Used by the resident provider; native discovery owns its mode.
    pub(crate) fn mode_at(&self, utc: u64) -> Result<u64, u32> {
        Ok(if self.standard_date[1] == 0 {
            0
        } else if self.daylight_at(utc)? {
            2
        } else {
            1
        })
    }
    pub(crate) fn to_local(&self, utc: u64) -> Result<[u16; 8], u32> {
        calendar(utc as i128 - self.bias_at(utc)? as i128 * MINUTE)
    }
    pub(crate) fn to_utc(&self, local: u64) -> Result<[u16; 8], u32> {
        // Resolve an overlap to standard time. A spring-forward gap has no UTC match.
        for bias in [self.standard, self.daylight] {
            let candidate = local as i128 + bias as i128 * MINUTE;
            if let Ok(utc) = u64::try_from(candidate) {
                if self.bias_at(utc)? == bias {
                    return calendar(candidate);
                }
            }
        }
        Err(87)
    }
}

pub(crate) fn encode(fields: [u16; 8]) -> [u8; 16] {
    let mut bytes = [0; 16];
    for (n, value) in fields.into_iter().enumerate() {
        bytes[n * 2..n * 2 + 2].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}
#[allow(dead_code)] // Shared resident profile; native host providers do not construct it.
pub(crate) struct Profile {
    bytes: [u8; 172],
    zone: Zone,
}
impl Default for Profile {
    fn default() -> Self {
        let mut bytes = [0; 172];
        for (n, value) in "UTC".encode_utf16().enumerate() {
            bytes[4 + n * 2..6 + n * 2].copy_from_slice(&value.to_le_bytes());
        }
        Self::new(bytes).expect("UTC profile")
    }
}
#[allow(dead_code)] // Referenced by the separately compiled resident crate.
impl Profile {
    pub fn seasonal(&self) -> bool {
        self.zone.standard_date[1] != 0
    }
    pub fn crt_values(&self) -> Result<[u32; 2], u32> {
        let seconds = i32::try_from(self.zone.standard * 60).map_err(|_| 87u32)?;
        Ok([seconds as u32, u32::from(self.seasonal())])
    }
    pub fn new(bytes: [u8; 172]) -> Result<Self, u32> {
        Ok(Self {
            zone: Zone::parse(&bytes)?,
            bytes,
        })
    }
    pub fn information(&self, utc: u64) -> Result<(u64, [u8; 172]), u32> {
        Ok((self.zone.mode_at(utc)?, self.bytes))
    }
    pub fn convert(&self, fields: [u16; 8], reverse: bool) -> Result<[u16; 8], u32> {
        let value = ticks(fields)?;
        if reverse {
            self.zone.to_utc(value)
        } else {
            self.zone.to_local(value)
        }
    }
    pub fn local(&self, utc: u64) -> Result<[u16; 8], u32> {
        self.zone.to_local(utc)
    }
    pub fn local_filetime(&self, input: u64, now: u64) -> Result<u64, u32> {
        if input >= 1 << 63 {
            return Err(87);
        }
        // FileTimeToLocalFileTime applies the current seasonal bias, including
        // when the input belongs to a different season. Preserve all 100 ns bits.
        u64::try_from(input as i128 - self.zone.bias_at(now)? as i128 * MINUTE)
            .ok()
            .filter(|&ticks| ticks < 1 << 63)
            .ok_or(87)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn last_week_transition_and_southern_season_obey_windows_rules() {
        let mut bytes = [0; 172];
        bytes[..4].copy_from_slice(&(-60i32).to_le_bytes());
        bytes[168..].copy_from_slice(&(-60i32).to_le_bytes());
        bytes[68..84].copy_from_slice(&encode([0, 10, 0, 5, 3, 0, 0, 0]));
        bytes[152..168].copy_from_slice(&encode([0, 3, 0, 5, 2, 0, 0, 0]));
        let zone = Profile::new(bytes).unwrap();
        for (utc, mode, local) in [
            (
                [2024, 10, 0, 27, 0, 59, 59, 999],
                2,
                [2024, 10, 0, 27, 2, 59, 59, 999],
            ),
            (
                [2024, 10, 0, 27, 1, 0, 0, 0],
                1,
                [2024, 10, 0, 27, 2, 0, 0, 0],
            ),
        ] {
            let utc = ticks(utc).unwrap();
            assert_eq!(zone.information(utc).unwrap().0, mode);
            assert_eq!(zone.local(utc).unwrap(), local);
        }
        bytes[..4].copy_from_slice(&(-600i32).to_le_bytes());
        bytes[68..84].copy_from_slice(&encode([0, 4, 0, 1, 3, 0, 0, 0]));
        bytes[152..168].copy_from_slice(&encode([0, 10, 0, 1, 2, 0, 0, 0]));
        let zone = Profile::new(bytes).unwrap();
        assert_eq!(
            zone.information(ticks([2024, 1, 1, 15, 0, 0, 0, 0]).unwrap())
                .unwrap()
                .0,
            2
        );
        assert_eq!(
            zone.information(ticks([2024, 7, 1, 15, 0, 0, 0, 0]).unwrap())
                .unwrap()
                .0,
            1
        );
    }
}
