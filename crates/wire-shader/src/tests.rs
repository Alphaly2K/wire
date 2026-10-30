use super::*;
const VERTEX: &[u8] = include_bytes!("../tests/fixtures/varying.vs.spv");
const PIXEL: &[u8] = include_bytes!("../tests/fixtures/varying.ps.spv");
#[test]
fn strict_target_preserves_draw_base_binding_and_stage_interfaces() {
    let vertex = translate(VERTEX, 1).unwrap();
    assert!(vertex.bindings.is_empty());
    assert!(vertex.inputs.is_empty());
    assert_eq!(vertex.internal_bindings.len(), 1);
    assert_eq!(vertex.internal_bindings[0].parameter, "baseVertex");
    assert_eq!(vertex.internal_bindings[0].set, 3);
    assert_eq!(vertex.internal_bindings[0].binding, 0);
    assert_eq!(vertex.output_interface[0].location, 1);
    assert_eq!(vertex.output_interface[0].components, 4);
    let pixel = translate(PIXEL, 2).unwrap();
    assert_eq!(pixel.inputs, [1]);
    assert_eq!(pixel.input_interface[0].components, 3);
    assert!(pixel.internal_bindings.is_empty());
    assert_eq!(pixel.output_interface[0].location, 0);
    assert_eq!(pixel.output_interface[0].components, 4);
}
#[test]
fn shader_entry_stage_cannot_be_relabelled_by_caller() {
    assert!(translate(PIXEL, 1).unwrap_err().contains("stage mismatch"));
    assert!(translate(VERTEX, 8).unwrap_err().contains("stage"));
}
#[test]
fn unsupported_physical_addressing_and_excessive_ids_stay_rejected() {
    let source = include_bytes!("../tests/fixtures/physical.vs.spv");
    assert!(translate(source, 1)
        .unwrap_err()
        .contains("PhysicalStorageBufferAddresses"));
    let mut source = VERTEX.to_vec();
    source[12..16].copy_from_slice(&1_000_001u32.to_le_bytes());
    assert!(translate(&source, 1).unwrap_err().contains("id capacity"));
}

#[test]
fn compute_store_uses_legal_storage_binding_and_literal_group_size() {
    let result = translate(include_bytes!("../tests/fixtures/store.cs.spv"), 4).unwrap();
    assert_eq!(result.workgroup_size, [1, 1, 1]);
    assert_eq!(result.bindings.len(), 1);
    assert_eq!(result.bindings[0].set, 2);
    assert_eq!(result.bindings[0].binding, 158);
    assert_eq!(result.bindings[0].descriptor_type, 7);
    assert!(result.internal_bindings.is_empty());
}

#[test]
fn stage_link_preserves_body_and_selects_pixel_consumed_components() {
    let result = link_vertex(VERTEX, PIXEL).unwrap();
    assert_eq!(result.output_interface.len(), 1);
    assert_eq!(result.output_interface[0].components, 3);
    assert_eq!(result.output_interface[0].location, 1);
    assert_eq!(result.internal_bindings.len(), 1);
    assert!(result.code.contains("wire_vertex_body"));
    assert!(result.code.contains("@builtin(position)"));
}

#[test]
fn stage_link_rejects_missing_short_and_wrong_scalar_outputs() {
    for name in ["missing.ps.spv", "integer.ps.spv"] {
        let bytes = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(name),
        )
        .unwrap();
        assert!(link_vertex(VERTEX, &bytes)
            .unwrap_err()
            .contains("location mismatch"));
    }
    assert!(
        link_vertex(include_bytes!("../tests/fixtures/short.vs.spv"), PIXEL)
            .unwrap_err()
            .contains("location mismatch")
    );
}

#[test]
fn stage_link_handles_scalar_all_vector_widths_and_unused_outputs() {
    for width in 1..=4 {
        let vertex = parse_module(VERTEX, 1).unwrap();
        let mut pixel = parse_module(PIXEL, 2).unwrap();
        // Change only the consumed interface type; the original fixture's
        // code reads xyz, so use an empty typed entry to avoid invalid loads.
        let scalar = naga::Scalar {
            kind: naga::ScalarKind::Float,
            width: 4,
        };
        let inner = if width == 1 {
            naga::TypeInner::Scalar(scalar)
        } else {
            naga::TypeInner::Vector {
                size: match width {
                    2 => naga::VectorSize::Bi,
                    3 => naga::VectorSize::Tri,
                    _ => naga::VectorSize::Quad,
                },
                scalar,
            }
        };
        let ty = pixel
            .types
            .insert(naga::Type { name: None, inner }, naga::Span::UNDEFINED);
        let binding = naga::Binding::Location {
            location: 1,
            interpolation: Some(naga::Interpolation::Linear),
            sampling: Some(naga::Sampling::Centroid),
            blend_src: None,
            per_primitive: false,
        };
        pixel.entry_points[0].function = naga::Function {
            arguments: vec![naga::FunctionArgument {
                name: None,
                ty,
                binding: Some(binding),
            }],
            ..naga::Function::default()
        };
        let result = link::link_modules(vertex, pixel).unwrap();
        assert_eq!(result.output_interface[0].components, width);
        assert_eq!(
            result.output_interface[0].interpolation.as_deref(),
            Some("Linear")
        );
        assert_eq!(
            result.output_interface[0].sampling.as_deref(),
            Some("Centroid")
        );
    }
    let vertex = parse_module(VERTEX, 1).unwrap();
    let mut pixel = parse_module(PIXEL, 2).unwrap();
    pixel.entry_points[0].function = naga::Function::default();
    let result = link::link_modules(vertex, pixel).unwrap();
    assert!(result.output_interface.is_empty());
    assert!(result.code.contains("@builtin(position)"));
}
