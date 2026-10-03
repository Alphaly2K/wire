//! Windows code pages via the host's iconv (no guest/native x86 execution).
use std::ffi::{c_char, c_void, CString};

#[cfg_attr(target_vendor = "apple", link(name = "iconv"))]
extern "C" {
    fn iconv_open(to: *const c_char, from: *const c_char) -> *mut c_void;
    fn iconv(
        cd: *mut c_void,
        input: *mut *mut c_char,
        inleft: *mut usize,
        output: *mut *mut c_char,
        outleft: *mut usize,
    ) -> usize;
    fn iconv_close(cd: *mut c_void) -> i32;
}

fn encoding(cp: u32) -> Result<&'static str, u32> {
    match cp {
        0 | 1 | 3 | 932 => Ok("CP932"),
        1252 => Ok("WINDOWS-1252"),
        65001 => Ok("UTF-8"),
        _ => Err(87),
    }
}

fn convert(input: &[u8], from: &str, to: &str) -> Result<Vec<u8>, u32> {
    let from = CString::new(from).unwrap();
    let to = CString::new(to).unwrap();
    let mut out = vec![
        0u8;
        input
            .len()
            .checked_mul(4)
            .and_then(|n| n.checked_add(16))
            .ok_or(8u32)?
    ];
    unsafe {
        let cd = iconv_open(to.as_ptr(), from.as_ptr());
        if cd == usize::MAX as *mut c_void {
            return Err(87);
        }
        let mut src = input.as_ptr() as *mut c_char;
        let mut dst = out.as_mut_ptr() as *mut c_char;
        let mut inleft = input.len();
        let mut outleft = out.len();
        let r = iconv(cd, &mut src, &mut inleft, &mut dst, &mut outleft);
        iconv_close(cd);
        if r == usize::MAX || inleft != 0 {
            return Err(1113);
        }
        out.truncate(out.len() - outleft);
    }
    Ok(out)
}

pub fn decode(input: &[u8], cp: u32) -> Result<String, u32> {
    String::from_utf8(convert(input, encoding(cp)?, "UTF-8")?).map_err(|_| 1113)
}

pub fn decode_lossy(input: &[u8], cp: u32) -> Result<String, u32> {
    if let Ok(s) = decode(input, cp) {
        return Ok(s);
    }
    encoding(cp)?;
    if cp == 65001 {
        return Ok(String::from_utf8_lossy(input).into_owned());
    }
    let mut out = String::new();
    let mut i = 0;
    while i < input.len() {
        let lead = matches!(cp, 0 | 1 | 3 | 932)
            && ((0x81..=0x9f).contains(&input[i]) || (0xe0..=0xfc).contains(&input[i]));
        let n = if lead && i + 1 < input.len() { 2 } else { 1 };
        match decode(&input[i..i + n], cp) {
            Ok(s) => {
                out.push_str(&s);
                i += n;
            }
            Err(_) => {
                out.push('\u{fffd}');
                i += 1;
            }
        }
    }
    Ok(out)
}

pub fn encode(input: &str, cp: u32) -> Result<(Vec<u8>, bool), u32> {
    let to = encoding(cp)?;
    if let Ok(bytes) = convert(input.as_bytes(), "UTF-8", to) {
        return Ok((bytes, false));
    }
    let mut out = Vec::new();
    let mut used = false;
    for ch in input.chars() {
        let mut buf = [0; 4];
        match convert(ch.encode_utf8(&mut buf).as_bytes(), "UTF-8", to) {
            Ok(b) => out.extend(b),
            Err(_) => {
                out.push(b'?');
                used = true;
            }
        }
    }
    Ok((out, used))
}
