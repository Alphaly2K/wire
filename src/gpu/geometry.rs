//! Host data for geometry expansion; never contains guest or GPU pointers.
//! Expansion preserves per-invocation strip boundaries and triangle winding.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeometryIo {
    pub location: u32,
    pub component: u32,
    pub built_in: u32,
    pub scalar_type: u32,
    pub array_size: u32,
    pub word_offset: u32,
    pub vector_size: u32,
    pub array_dimensions: u32,
}
#[repr(C)]
pub(super) struct View {
    pub inputs: *const GeometryIo,
    pub outputs: *const GeometryIo,
    pub input_count: usize,
    pub output_count: usize,
    pub input_vertices: u32,
    pub output_primitive: u32,
    pub max_vertices: u32,
    pub instances: u32,
    pub input_words: u32,
    pub vertex_words: u32,
    pub invocation_words: u32,
    pub reserved: u32,
}
#[derive(Clone, Debug)]
pub struct GeometryInfo {
    pub inputs: Vec<GeometryIo>,
    pub outputs: Vec<GeometryIo>,
    pub input_vertices: u32,
    /// 0 = points, 1 = lines, 3 = triangles (dxbc-spirv primitive enum).
    pub output_primitive: u32,
    pub max_vertices: u32,
    pub instances: u32,
    pub input_words: u32,
    pub vertex_words: u32,
    pub invocation_words: u32,
}
#[derive(Debug, PartialEq, Eq)]
pub struct GeometryBatch {
    /// Interleaved output payload, vertex_words words per vertex.
    pub vertices: Vec<u32>,
    /// List indices with winding restored; incomplete strips do not rasterize.
    pub indices: Vec<u32>,
    /// Emitted vertices per (primitive, GS instance), including empty ones.
    pub counts: Vec<u32>,
    pub output_primitive: u32,
    pub vertex_words: u32,
}
impl GeometryInfo {
    pub(super) fn assemble(&self, raw: &[u32]) -> Result<GeometryBatch, String> {
        let stride = self.invocation_words as usize;
        if stride == 0 || !raw.len().is_multiple_of(stride) {
            return Err("GS output record size".into());
        }
        let mut batch = GeometryBatch {
            vertices: Vec::new(),
            indices: Vec::new(),
            counts: Vec::new(),
            output_primitive: self.output_primitive,
            vertex_words: self.vertex_words,
        };
        let mut vertex = 0u32;
        for invocation in raw.chunks_exact(stride) {
            let count = invocation[0];
            if count > self.max_vertices {
                return Err("GS emitted more than declared max vertices".into());
            }
            batch.counts.push(count);
            let mut strip_vertices = 0u32;
            for n in 0..count as usize {
                let start = 1 + n * (self.vertex_words as usize + 1);
                let record = &invocation[start..start + 1 + self.vertex_words as usize];
                if record[0] > 1 || (n == 0 && record[0] != 1) {
                    return Err("GS invalid strip marker".into());
                }
                if record[0] == 1 {
                    strip_vertices = 0;
                }
                batch.vertices.extend_from_slice(&record[1..]);
                strip_vertices += 1;
                match self.output_primitive {
                    0 => batch.indices.push(vertex),
                    1 if strip_vertices >= 2 => {
                        batch.indices.extend_from_slice(&[vertex - 1, vertex]);
                    }
                    3 if strip_vertices >= 3 => {
                        let mut a = vertex - 2;
                        let mut b = vertex - 1;
                        if strip_vertices.is_multiple_of(2) {
                            std::mem::swap(&mut a, &mut b);
                        }
                        batch.indices.extend_from_slice(&[a, b, vertex]);
                    }
                    1 | 3 => {}
                    _ => return Err("GS output topology".into()),
                }
                vertex = vertex.checked_add(1).ok_or("GS vertex count capacity")?;
            }
        }
        Ok(batch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn info(primitive: u32) -> GeometryInfo {
        GeometryInfo {
            inputs: vec![],
            outputs: vec![],
            input_vertices: 3,
            output_primitive: primitive,
            max_vertices: 6,
            instances: 1,
            input_words: 0,
            vertex_words: 1,
            invocation_words: 15,
        }
    }
    #[test]
    fn strip_cut_winding_and_invocations_are_preserved() {
        let raw = [
            6, 1, 10, 0, 11, 0, 12, 0, 13, 0, 14, 0, 15, 0, 0, 5, 1, 20, 0, 21, 1, 22, 0, 23, 0,
            24, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let triangle = info(3).assemble(&raw).unwrap();
        assert_eq!(
            triangle.vertices,
            [10, 11, 12, 13, 14, 15, 20, 21, 22, 23, 24]
        );
        assert_eq!(
            triangle.indices,
            [0, 1, 2, 2, 1, 3, 2, 3, 4, 4, 3, 5, 8, 9, 10]
        );
        assert_eq!(triangle.counts, [6, 5, 0]);
        let line = info(1).assemble(&raw).unwrap();
        assert_eq!(
            line.indices,
            [0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 6, 7, 8, 9, 9, 10]
        );
        assert_eq!(
            info(0).assemble(&raw).unwrap().indices,
            (0..11).collect::<Vec<_>>()
        );
    }
    #[test]
    fn corrupt_records_and_overflow_are_rejected() {
        let mut raw = [0; 15];
        raw[0] = 7;
        assert!(info(3).assemble(&raw).is_err());
        raw[0] = 1;
        assert!(info(3).assemble(&raw).is_err());
        raw[1] = 2;
        assert!(info(3).assemble(&raw).is_err());
        raw[1] = 1;
        assert!(info(3).assemble(&raw[..14]).is_err());
    }
}
