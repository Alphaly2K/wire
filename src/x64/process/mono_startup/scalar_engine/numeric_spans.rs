//! Typed pinned buffers stay in native memory. Snapshot them into a temporary
//! guest scope and validate all outputs before committing any native bytes.
use super::super::logging;
use super::{callbacks, managed_calls::NumericSpanLayout, Argument, Payload};
use std::ptr;

struct Span {
    position: usize,
    header: usize,
    address: usize,
    count: usize,
    layout: NumericSpanLayout,
    bytes: Vec<u8>,
    alias: Option<usize>,
}
#[derive(Default)]
pub(super) struct NumericFrame {
    spans: Vec<Span>,
}
fn overlap(a: usize, width: usize, b: usize, length: usize) -> bool {
    width != 0 && length != 0 && a < b + length && b < a + width
}
impl NumericFrame {
    pub(super) unsafe fn prepare(
        kinds: &[Argument],
        arguments: &[u64],
        layouts: &[Option<Payload>],
    ) -> Result<Self, String> {
        if kinds.len() != arguments.len() || kinds.len() != layouts.len() {
            return Err("numeric span ABI arity mismatch".into());
        }
        let mut result = Self::default();
        for (position, (&kind, &header)) in kinds.iter().zip(arguments).enumerate() {
            let Argument::NumericSpan(layout) = kind else {
                continue;
            };
            let header = header as usize;
            if header == 0
                || !header.is_multiple_of(8)
                || header.checked_add(16).is_none()
                || !(1..=256).contains(&layout.stride)
                || !matches!(layout.alignment, 1 | 2 | 4 | 8)
                || !layout.stride.is_multiple_of(layout.alignment)
            {
                return Err("numeric span header/element layout invalid".into());
            }
            let native = unsafe { (header as *const logging::Span).read() };
            let count =
                usize::try_from(native.length).map_err(|_| "negative pinned numeric span")?;
            let length = count
                .checked_mul(layout.stride)
                .ok_or("numeric span size overflow")?;
            let address = native.begin as usize;
            if count > 65536
                || length > 0x20000
                || address.checked_add(length).is_none()
                || count > 0 && (address <= 1 || !address.is_multiple_of(layout.alignment))
            {
                return Err("numeric span pinned element range invalid".into());
            }
            result.spans.push(Span {
                position,
                header,
                address,
                count,
                layout,
                bytes: vec![0; length],
                alias: None,
            });
        }
        // Validate every header and data range before reading a pinned element.
        for i in 0..result.spans.len() {
            let current = &result.spans[i];
            for (&argument, &layout) in arguments.iter().zip(layouts) {
                if let Some(layout) = layout {
                    let address = argument as usize;
                    address
                        .checked_add(layout.width)
                        .ok_or("numeric/byref range overflow")?;
                    if overlap(current.header, 16, address, layout.width)
                        || overlap(current.address, current.bytes.len(), address, layout.width)
                    {
                        return Err("numeric span overlaps ordinary byref storage".into());
                    }
                }
            }
            let mut alias = None;
            for (j, other) in result.spans.iter().enumerate() {
                if overlap(current.address, current.bytes.len(), other.header, 16)
                    || i != j && overlap(current.header, 16, other.header, 16)
                {
                    return Err("numeric span data/header ranges overlap".into());
                }
                if j < i
                    && overlap(
                        current.address,
                        current.bytes.len(),
                        other.address,
                        other.bytes.len(),
                    )
                {
                    if current.address != other.address
                        || current.count != other.count
                        || current.layout != other.layout
                    {
                        return Err(
                            "numeric span buffers partially overlap or have different types".into(),
                        );
                    }
                    alias = Some(other.alias.unwrap_or(j));
                }
            }
            result.spans[i].alias = alias;
        }
        for span in &mut result.spans {
            if !span.bytes.is_empty() {
                unsafe {
                    ptr::copy_nonoverlapping(
                        span.address as *const u8,
                        span.bytes.as_mut_ptr(),
                        span.bytes.len(),
                    )
                };
            }
        }
        Ok(result)
    }
    pub(super) fn inputs(&self) -> Vec<callbacks::NumericSpan<'_>> {
        self.spans
            .iter()
            .map(|span| callbacks::NumericSpan {
                position: span.position,
                count: span.count,
                stride: span.layout.stride,
                bytes: &span.bytes,
                alias: span.alias,
            })
            .collect()
    }
    pub(super) fn validate(&self, outputs: &[Vec<u8>]) -> Result<(), String> {
        if outputs.len() != self.spans.len()
            || outputs.iter().zip(&self.spans).any(|(bytes, span)| {
                bytes.len() != span.bytes.len()
                    || span.alias.is_some_and(|source| bytes != &outputs[source])
            })
        {
            return Err(
                "numeric span output length/alias mismatch; native storage unchanged".into(),
            );
        }
        Ok(())
    }
    /// All outputs were validated together; pinned arrays remain fixed until
    /// the actual managed wrapper returns. No fallible operation follows here.
    pub(super) unsafe fn commit(&self, outputs: &[Vec<u8>]) {
        for (span, bytes) in self.spans.iter().zip(outputs) {
            if span.alias.is_none() && !bytes.is_empty() {
                unsafe {
                    ptr::copy_nonoverlapping(bytes.as_ptr(), span.address as *mut u8, bytes.len())
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Binding, Return, Scalar};
    use super::*;
    use crate::x64::{
        memory::{Memory, EXECUTE, READ, WRITE},
        process::STACK,
        runtime::Runtime64,
    };
    use std::ffi::CString;
    fn layout(element: usize) -> NumericSpanLayout {
        NumericSpanLayout {
            element,
            stride: 4,
            alignment: 4,
        }
    }
    fn addresses(first: &logging::Span, second: &logging::Span) -> [u64; 2] {
        [
            first as *const logging::Span as u64,
            second as *const logging::Span as u64,
        ]
    }
    #[test]
    fn pinned_numeric_arrays_preserve_aliases_float_and_string_positions_and_atomic_faults() {
        let mut memory = Memory::default();
        memory.map(0x1000, 4096, READ | WRITE | EXECUTE).unwrap();
        // Read XMM2, add the first element and verify both headers share data.
        // Write the second element and byref result; add the fifth UTF16 count.
        memory
            .write(
                0x1000,
                &[
                    0x66, 0x0f, 0x7e, 0xd0, 0x48, 0x8b, 0x09, 0x03, 0x01, 0x48, 0x8b, 0x12, 0x48,
                    0x39, 0xca, 0x0f, 0x94, 0xc2, 0x0f, 0xb6, 0xd2, 0x01, 0xd0, 0x89, 0x41, 0x04,
                    0x41, 0x89, 0x01, 0x4c, 0x8b, 0x54, 0x24, 0x28, 0x41, 0x03, 0x42, 0x08, 0xc3,
                ],
            )
            .unwrap();
        memory
            .write(
                0x1100,
                &[
                    0x48, 0x8b, 0x01, 0xc7, 0x00, 99, 0, 0, 0, 0x41, 0xc7, 0x01, 77, 0, 0, 0, 0xb8,
                    2, 0, 0, 0, 0xc3,
                ],
            )
            .unwrap();
        memory
            .write(
                0x1200,
                &[0x48, 0x8b, 0x01, 0xc7, 0x00, 99, 0, 0, 0, 0x0f, 0x0b],
            )
            .unwrap();
        memory
            .write(0x1300, &[0xc7, 0x41, 0x08, 5, 0, 0, 0, 0xc3])
            .unwrap();
        memory.map(STACK, 65536, READ | WRITE).unwrap();
        let mut runtime =
            Runtime64::new(&mut memory, 0x140000000, STACK, 65536, "numeric-arrays").unwrap();
        let mut data = [10u32, 20];
        let first = logging::Span {
            begin: data.as_ptr().cast(),
            length: 2,
        };
        let second = logging::Span {
            begin: first.begin,
            length: 2,
        };
        let text_units = [0x3042, 0x3044];
        let text = logging::Span {
            begin: text_units.as_ptr(),
            length: 2,
        };
        let mut output = 0xabcdef01u32;
        let args = [
            (&first as *const logging::Span) as u64,
            (&second as *const logging::Span) as u64,
            0x80000000,
            (&mut output as *mut u32) as u64,
            (&text as *const logging::Span) as u64,
        ];
        let kinds = vec![
            Argument::NumericSpan(layout(1)),
            Argument::NumericSpan(layout(1)),
            Argument::Scalar(Scalar::from_kind(12).unwrap()),
            Argument::Value(Payload {
                width: 4,
                alignment: 4,
                output: true,
            }),
            Argument::Utf16Span,
        ];
        let mut binding = Binding {
            library: 0,
            name: CString::new("DifferentOwner::NumericArrays").unwrap(),
            target: 0x1000,
            arguments: kinds,
            returns: Return::Scalar(Scalar::from_kind(8).unwrap()),
            work: callbacks::Work::Ordinary,
            keyword_producer: None,
            numeric_values: Vec::new(),
        };
        let (result, _) = callbacks::with_owner(&mut memory, &mut runtime, [0; 7], || {
            assert_eq!(super::super::execute(&binding, &args)?, (0x8000000d, 0));
            assert_eq!(data, [10, 0x8000000b]);
            assert_eq!(output, 0x8000000b);
            data[0] = 42;
            output = 0xabcdef01;
            binding.target = 0x1100;
            binding.returns = Return::Scalar(Scalar::from_kind(2).unwrap());
            assert!(super::super::execute(&binding, &args).is_err());
            assert_eq!(data, [42, 0x8000000b]);
            assert_eq!(output, 0xabcdef01);
            for target in [0x1200, 0x1300] {
                binding.target = target;
                assert!(super::super::execute(&binding, &args).is_err());
                assert_eq!(data, [42, 0x8000000b]);
                assert_eq!(output, 0xabcdef01);
            }
            assert!(
                callbacks::with_service(|memory, _| memory.read_int(0x6ff900000000, 8)).is_err()
            );
            Ok::<_, String>(())
        })
        .unwrap();
        result.unwrap();
        assert!(memory.read_int(0x6ff900000000, 8).is_err());
    }
    #[test]
    fn pinned_ranges_reject_partial_overlap_element_conflicts_and_byref_aliases_before_copy() {
        let data = [1u32, 2, 3];
        let first = logging::Span {
            begin: data.as_ptr().cast(),
            length: 2,
        };
        let mut second = logging::Span {
            begin: unsafe { data.as_ptr().add(1).cast() },
            length: 2,
        };
        let kinds = [
            Argument::NumericSpan(layout(1)),
            Argument::NumericSpan(layout(1)),
        ];
        let args = addresses(&first, &second);
        assert!(unsafe { NumericFrame::prepare(&kinds, &args, &[None, None]) }.is_err());
        second.begin = first.begin;
        let conflicting = [kinds[0], Argument::NumericSpan(layout(2))];
        assert!(unsafe {
            NumericFrame::prepare(&conflicting, &addresses(&first, &second), &[None, None])
        }
        .is_err());
        let mut arguments = args.to_vec();
        arguments.push(first.begin as u64);
        let mut kinds = kinds.to_vec();
        kinds.push(Argument::Value(Payload {
            width: 4,
            alignment: 4,
            output: false,
        }));
        assert!(unsafe {
            NumericFrame::prepare(
                &kinds,
                &arguments,
                &[
                    None,
                    None,
                    Some(Payload {
                        width: 4,
                        alignment: 4,
                        output: false,
                    }),
                ],
            )
        }
        .is_err());
        second.begin = ptr::dangling::<u8>().cast();
        assert!(unsafe {
            NumericFrame::prepare(&kinds[..2], &addresses(&first, &second), &[None, None])
        }
        .is_err());
        second.length = -1;
        assert!(unsafe {
            NumericFrame::prepare(&kinds[..2], &addresses(&first, &second), &[None, None])
        }
        .is_err());
        second.length = 0;
        assert!(unsafe {
            NumericFrame::prepare(&kinds[..2], &addresses(&first, &second), &[None, None])
        }
        .is_ok());
    }
}
