/*
 * Copyright 1993, 1994, 1996 Alexandre Julliard
 * LGPL-2.1-or-later; LGPL 2.1 selected for wire's combined work.
 * Decimal control conversion retains the Wine USER derivation.
 */
//! Decimal conversion used after a control's WM_GETTEXT returns.
pub(crate) fn integer(text: &str, signed: bool) -> Option<u32> {
    let text = text.trim_start_matches([' ', '\t', '\r', '\n']);
    let negative = text.starts_with('-');
    let digits: String = text
        .strip_prefix(['-', '+'])
        .unwrap_or(text)
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    let value = digits.parse::<u64>().ok()?;
    if signed {
        let value = i64::try_from(value).ok()?;
        i32::try_from(if negative { -value } else { value })
            .ok()
            .map(|n| n as u32)
    } else {
        u32::try_from(value)
            .ok()
            .map(|n| if negative { n.wrapping_neg() } else { n })
    }
}
pub(crate) fn text(value: u32, signed: bool) -> String {
    if signed {
        (value as i32).to_string()
    } else {
        value.to_string()
    }
}
