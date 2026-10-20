//! Native font measurement uses host values; guest font and output layouts stay in GDI.
extern "C" {
    fn wire_font_metrics(
        face: *const u16,
        length: u32,
        height: i32,
        width: i32,
        weight: i32,
        italic: i32,
        values: *mut i32,
        characters: *mut u16,
        pitch: *mut u8,
    ) -> i32;
}

pub(crate) struct Metrics {
    pub(crate) values: [i32; 11],
    pub(crate) characters: [u16; 4],
    pub(crate) pitch: u8,
}

pub(crate) fn metrics(
    face: &[u16],
    height: i32,
    width: i32,
    weight: i32,
    italic: i32,
) -> Option<Metrics> {
    let mut result = Metrics {
        values: [0; 11],
        characters: [0; 4],
        pitch: 0,
    };
    if unsafe {
        wire_font_metrics(
            face.as_ptr(),
            face.len() as u32,
            height,
            width,
            weight,
            italic,
            result.values.as_mut_ptr(),
            result.characters.as_mut_ptr(),
            &mut result.pitch,
        )
    } == 0
    {
        return None;
    }
    Some(result)
}
