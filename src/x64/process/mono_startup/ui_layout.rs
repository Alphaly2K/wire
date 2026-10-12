//! Fixed Canvas / RectTransform / hierarchy ABIs, validated as a whole before
//! registration. The original Unity engine owns layout and rendering behavior.
use super::{bindings, callbacks, engine_objects, managed_calls, value_layout, Object};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    ffi::{CStr, CString},
    sync::OnceLock,
};

#[derive(Clone, Copy)]
enum Scalar {
    Boolean,
    Integer,
    Float,
    Enum(&'static CStr),
}
#[derive(Clone, Copy)]
enum Shape {
    Get(Scalar),
    Set(Scalar),
    ValueGet(&'static CStr, usize),
    ValueSet(&'static CStr, usize),
    Handle(&'static CStr, bool),
    SetHandle,
    SetIndexedHandle,
    StaticHandle(&'static CStr),
    Void,
    StaticBoolean,
    Parent,
}
struct Spec {
    owner: &'static CStr,
    member: &'static CStr,
    target: u64,
    shape: Shape,
}
struct Binding {
    library: usize,
    targets: Vec<u64>,
    returns: Vec<usize>,
}
static BINDING: OnceLock<Binding> = OnceLock::new();
thread_local! {
    static CALLS: Cell<usize> = const { Cell::new(0) };
    static SEEN: RefCell<BTreeSet<usize>> = const { RefCell::new(BTreeSet::new()) };
}
macro_rules! shape {
    (bool_get) => {
        Shape::Get(Scalar::Boolean)
    };
    (bool_set) => {
        Shape::Set(Scalar::Boolean)
    };
    (int_get) => {
        Shape::Get(Scalar::Integer)
    };
    (int_set) => {
        Shape::Set(Scalar::Integer)
    };
    (float_get) => {
        Shape::Get(Scalar::Float)
    };
    (float_set) => {
        Shape::Set(Scalar::Float)
    };
    (enum_get, $class:expr) => {
        Shape::Get(Scalar::Enum($class))
    };
    (enum_set, $class:expr) => {
        Shape::Set(Scalar::Enum($class))
    };
    (value_get, $class:expr, $width:expr) => {
        Shape::ValueGet($class, $width)
    };
    (value_set, $class:expr, $width:expr) => {
        Shape::ValueSet($class, $width)
    };
    (handle_get, $class:expr) => {
        Shape::Handle($class, false)
    };
    (handle_at, $class:expr) => {
        Shape::Handle($class, true)
    };
    (handle_set) => {
        Shape::SetHandle
    };
    (handle_index_set) => {
        Shape::SetIndexedHandle
    };
    (static_handle, $class:expr) => {
        Shape::StaticHandle($class)
    };
    (void) => {
        Shape::Void
    };
    (static_bool) => {
        Shape::StaticBoolean
    };
    (parent) => {
        Shape::Parent
    };
}
macro_rules! adapter {
    ($name:ident, bool_get) => {
        unsafe extern "C" fn $name(p: usize) -> u8 {
            scalar(Slot::$name as usize, p, None).0 as u8
        }
    };
    ($name:ident, bool_set) => {
        unsafe extern "C" fn $name(p: usize, value: u8) {
            scalar(Slot::$name as usize, p, Some(value as u64));
        }
    };
    ($name:ident, int_get $(, $class:expr)?) => {
        unsafe extern "C" fn $name(p: usize) -> i32 {
            scalar(Slot::$name as usize, p, None).0 as i32
        }
    };
    ($name:ident, int_set $(, $class:expr)?) => {
        unsafe extern "C" fn $name(p: usize, value: i32) {
            scalar(Slot::$name as usize, p, Some(value as u32 as u64));
        }
    };
    ($name:ident, enum_get, $class:expr) => {
        adapter!($name, int_get);
    };
    ($name:ident, enum_set, $class:expr) => {
        adapter!($name, int_set);
    };
    ($name:ident, float_get) => {
        unsafe extern "C" fn $name(p: usize) -> f32 {
            f32::from_bits(scalar(Slot::$name as usize, p, None).1 as u32)
        }
    };
    ($name:ident, float_set) => {
        unsafe extern "C" fn $name(p: usize, value: f32) {
            scalar(Slot::$name as usize, p, Some(value.to_bits() as u64));
        }
    };
    ($name:ident, value_get, $class:expr, $width:expr) => {
        unsafe extern "C" fn $name(p: usize, output: *mut u8) {
            checked(|| unsafe { value_call(Slot::$name as usize, p, output, $width, false) });
        }
    };
    ($name:ident, value_set, $class:expr, $width:expr) => {
        unsafe extern "C" fn $name(p: usize, input: *const u8) {
            checked(|| unsafe {
                value_call(Slot::$name as usize, p, input.cast_mut(), $width, true)
            });
        }
    };
    ($name:ident, handle_get, $class:expr) => {
        unsafe extern "C" fn $name(p: usize) -> usize {
            handle(Slot::$name as usize, Some(p), None)
        }
    };
    ($name:ident, handle_at, $class:expr) => {
        unsafe extern "C" fn $name(p: usize, at: i32) -> usize {
            handle(Slot::$name as usize, Some(p), Some(at))
        }
    };
    ($name:ident, static_handle, $class:expr) => {
        unsafe extern "C" fn $name() -> usize {
            handle(Slot::$name as usize, None, None)
        }
    };
    ($name:ident, handle_set) => {
        unsafe extern "C" fn $name(p: usize, value: usize) {
            checked(|| {
                let b = binding()?;
                let p = engine_objects::engine_pointer(p)?;
                let value = if value == 0 {
                    0
                } else {
                    engine_objects::engine_pointer(value)?
                };
                callbacks::guest_arguments(b.targets[Slot::$name as usize], &[p, value])?;
                note(Slot::$name as usize);
                Ok(())
            });
        }
    };
    ($name:ident, handle_index_set) => {
        unsafe extern "C" fn $name(p: usize, value: usize, index: i32) {
            checked(|| {
                let b = binding()?;
                let p = engine_objects::engine_pointer(p)?;
                let value = if value == 0 {
                    0
                } else {
                    engine_objects::engine_pointer(value)?
                };
                callbacks::guest_arguments(
                    b.targets[Slot::$name as usize],
                    &[p, value, index as u32 as u64],
                )?;
                note(Slot::$name as usize);
                Ok(())
            });
        }
    };
    ($name:ident, void) => {
        unsafe extern "C" fn $name(p: usize) {
            scalar(Slot::$name as usize, p, None);
        }
    };
    ($name:ident, static_bool) => {
        unsafe extern "C" fn $name(value: u8) {
            scalar(Slot::$name as usize, 0, Some(value as u64));
        }
    };
    ($name:ident, parent) => {
        unsafe extern "C" fn $name(p: usize, parent: usize, world: u8) {
            checked(|| {
                let b = binding()?;
                boolean(world as u64)?;
                let p = engine_objects::engine_pointer(p)?;
                let parent = if parent == 0 {
                    0
                } else {
                    engine_objects::engine_pointer(parent)?
                };
                callbacks::guest_arguments(
                    b.targets[Slot::$name as usize],
                    &[p, parent, world as u64],
                )?;
                callbacks::with_metadata_service(|route, memory| route.sync_guest(memory))?;
                note(Slot::$name as usize);
                Ok(())
            });
        }
    };
}
macro_rules! apis {
    ($( $name:ident, $owner:expr, $member:expr, $target:expr, ($($shape:tt)*); )*) => {
        #[allow(non_camel_case_types)]
        enum Slot { $( $name, )* }
        const SPECS: &[Spec] = &[$(Spec { owner: $owner, member: $member, target: $target, shape: shape!($($shape)*) },)*];
        $(adapter!($name, $($shape)*);)*
        fn adapters() -> Vec<Object> { vec![$($name as *const () as Object,)*] }
    };
}
apis! {
    canvas_render, c"Canvas", c"get_renderMode_Injected", 0x1801a4e80, (enum_get, c"RenderMode");
    set_canvas_render, c"Canvas", c"set_renderMode_Injected", 0x1801a4ed0, (enum_set, c"RenderMode");
    root_canvas_state, c"Canvas", c"get_isRootCanvas_Injected", 0x1801a4ee0, (bool_get);
    pixel_rect, c"Canvas", c"get_pixelRect_Injected", 0x1801a4ef0, (value_get, c"Rect", 16);
    scale_factor, c"Canvas", c"get_scaleFactor_Injected", 0x1801a2d60, (float_get);
    set_scale_factor, c"Canvas", c"set_scaleFactor_Injected", 0x1801a4f10, (float_set);
    reference_pixels, c"Canvas", c"get_referencePixelsPerUnit_Injected", 0x1801a2d80, (float_get);
    set_reference_pixels, c"Canvas", c"set_referencePixelsPerUnit_Injected", 0x1801a4f80, (float_set);
    override_pixel, c"Canvas", c"get_overridePixelPerfect_Injected", 0x1801a4f90, (bool_get);
    set_override_pixel, c"Canvas", c"set_overridePixelPerfect_Injected", 0x1801a4fa0, (bool_set);
    gamma_vertex, c"Canvas", c"get_vertexColorAlwaysGammaSpace_Injected", 0x1801a4fc0, (bool_get);
    set_gamma_vertex, c"Canvas", c"set_vertexColorAlwaysGammaSpace_Injected", 0x1801a4fd0, (bool_set);
    pixel_perfect, c"Canvas", c"get_pixelPerfect_Injected", 0x1801a5050, (bool_get);
    set_pixel_perfect, c"Canvas", c"set_pixelPerfect_Injected", 0x1801a5080, (bool_set);
    plane_distance, c"Canvas", c"get_planeDistance_Injected", 0x1801a2da0, (float_get);
    set_plane_distance, c"Canvas", c"set_planeDistance_Injected", 0x1801a5100, (float_set);
    render_order, c"Canvas", c"get_renderOrder_Injected", 0x1801a5180, (int_get);
    override_sort, c"Canvas", c"get_overrideSorting_Injected", 0x1801a51c0, (bool_get);
    set_override_sort, c"Canvas", c"set_overrideSorting_Injected", 0x1801a51d0, (bool_set);
    sort_order, c"Canvas", c"get_sortingOrder_Injected", 0x1801a5260, (int_get);
    set_sort_order, c"Canvas", c"set_sortingOrder_Injected", 0x1801a52a0, (int_set);
    display, c"Canvas", c"get_targetDisplay_Injected", 0x1801a5390, (int_get);
    set_display, c"Canvas", c"set_targetDisplay_Injected", 0x1801a53b0, (int_set);
    sort_layer, c"Canvas", c"get_sortingLayerID_Injected", 0x1801a5470, (int_get);
    set_sort_layer, c"Canvas", c"set_sortingLayerID_Injected", 0x1801a54a0, (int_set);
    cached_sort_layer, c"Canvas", c"get_cachedSortingLayerValue_Injected", 0x1801a54b0, (int_get);
    channels, c"Canvas", c"get_additionalShaderChannels_Injected", 0x18009ee70, (enum_get, c"AdditionalCanvasShaderChannels");
    set_channels, c"Canvas", c"set_additionalShaderChannels_Injected", 0x1801a54c0, (enum_set, c"AdditionalCanvasShaderChannels");
    root_canvas, c"Canvas", c"get_rootCanvas_Injected", 0x1801a59c0, (handle_get, c"Canvas");
    display_size, c"Canvas", c"get_renderingDisplaySize_Injected", 0x1801a5a10, (value_get, c"Vector2", 8);
    standalone, c"Canvas", c"get_updateRectTransformForStandalone_Injected", 0x1801a5a40, (enum_get, c"StandaloneRenderResize");
    set_standalone, c"Canvas", c"set_updateRectTransformForStandalone_Injected", 0x1801a5a50, (enum_set, c"StandaloneRenderResize");
    external_canvas, c"Canvas", c"SetExternalCanvasEnabled", 0x1801a5a60, (static_bool);
    camera, c"Canvas", c"get_worldCamera_Injected", 0x1801a5a70, (handle_get, c"Camera");
    set_camera, c"Canvas", c"set_worldCamera_Injected", 0x1801a5b60, (handle_set);
    grid_size, c"Canvas", c"get_normalizedSortingGridSize_Injected", 0x1801a5ba0, (float_get);
    set_grid_size, c"Canvas", c"set_normalizedSortingGridSize_Injected", 0x1801a5be0, (float_set);
    grid_size_old, c"Canvas", c"get_sortingGridNormalizedSize_Injected", 0x1801a5bf0, (int_get);
    set_grid_size_old, c"Canvas", c"set_sortingGridNormalizedSize_Injected", 0x1801a5c40, (int_set);
    default_text_material, c"Canvas", c"GetDefaultCanvasTextMaterial_Injected", 0x1801a5c50, (static_handle, c"Material");
    default_material, c"Canvas", c"GetDefaultCanvasMaterial_Injected", 0x1801a5c50, (static_handle, c"Material");
    etc_material, c"Canvas", c"GetETC1SupportedCanvasMaterial_Injected", 0x1801a5c90, (static_handle, c"Material");
    update_canvas_rect, c"Canvas", c"UpdateCanvasRectTransform_Injected", 0x1801a5d60, (bool_set);
    rect, c"RectTransform", c"get_rect_Injected", 0x1800cba20, (value_get, c"Rect", 16);
    anchor_min, c"RectTransform", c"get_anchorMin_Injected", 0x1800cba70, (value_get, c"Vector2", 8);
    set_anchor_min, c"RectTransform", c"set_anchorMin_Injected", 0x1800cba80, (value_set, c"Vector2", 8);
    anchor_max, c"RectTransform", c"get_anchorMax_Injected", 0x1800cba90, (value_get, c"Vector2", 8);
    set_anchor_max, c"RectTransform", c"set_anchorMax_Injected", 0x1800cbaa0, (value_set, c"Vector2", 8);
    anchored_position, c"RectTransform", c"get_anchoredPosition_Injected", 0x1800cbab0, (value_get, c"Vector2", 8);
    set_anchored_position, c"RectTransform", c"set_anchoredPosition_Injected", 0x1800cbaf0, (value_set, c"Vector2", 8);
    size_delta, c"RectTransform", c"get_sizeDelta_Injected", 0x1800cbb00, (value_get, c"Vector2", 8);
    set_size_delta, c"RectTransform", c"set_sizeDelta_Injected", 0x1800cbb10, (value_set, c"Vector2", 8);
    pivot, c"RectTransform", c"get_pivot_Injected", 0x1800cbb20, (value_get, c"Vector2", 8);
    set_pivot, c"RectTransform", c"set_pivot_Injected", 0x1800cbb30, (value_set, c"Vector2", 8);
    driven_object, c"RectTransform", c"get_drivenByObject_Injected", 0x180060050, (handle_get, c"Object");
    set_driven_object, c"RectTransform", c"set_drivenByObject_Injected", 0x18005fee0, (handle_set);
    driven_properties, c"RectTransform", c"get_drivenProperties_Injected", 0x180060050, (enum_get, c"DrivenTransformProperties");
    set_driven_properties, c"RectTransform", c"set_drivenProperties_Injected", 0x18005fee0, (enum_set, c"DrivenTransformProperties");
    dimensions_change, c"RectTransform", c"get_sendChildDimensionsChange_Injected", 0x1800cbb40, (bool_get);
    set_dimensions_change, c"RectTransform", c"set_sendChildDimensionsChange_Injected", 0x1800cbb50, (bool_set);
    update_rect, c"RectTransform", c"ForceUpdateRectTransforms_Injected", 0x1800cbb60, (void);
    transform_parent, c"Transform", c"GetParent_Injected", 0x1800cbe70, (handle_get, c"Transform");
    set_parent, c"Transform", c"SetParent_Injected", 0x1800cbeb0, (parent);
    transform_root, c"Transform", c"GetRoot_Injected", 0x1800cc730, (handle_get, c"Transform");
    child_count, c"Transform", c"get_childCount_Injected", 0x1800cc7a0, (int_get);
    first_sibling, c"Transform", c"SetAsFirstSibling_Injected", 0x1800cc7c0, (void);
    last_sibling, c"Transform", c"SetAsLastSibling_Injected", 0x1800cc7d0, (void);
    set_sibling, c"Transform", c"SetSiblingIndex_Injected", 0x1800cc7e0, (int_set);
    sibling, c"Transform", c"GetSiblingIndex_Injected", 0x1800cc8e0, (int_get);
    changed, c"Transform", c"get_hasChanged_Injected", 0x1800ccd80, (bool_get);
    set_changed, c"Transform", c"set_hasChanged_Injected", 0x1800cce00, (bool_set);
    child, c"Transform", c"GetChild_Injected", 0x1800cd150, (handle_at, c"Transform");
    old_child_count, c"Transform", c"GetChildCount_Injected", 0x1800cc7a0, (int_get);
    group_alpha, c"CanvasGroup", c"get_alpha_Injected", 0x18009c230, (float_get);
    set_group_alpha, c"CanvasGroup", c"set_alpha_Injected", 0x1801a2df0, (float_set);
    group_interactable, c"CanvasGroup", c"get_interactable_Injected", 0x180139640, (bool_get);
    set_group_interactable, c"CanvasGroup", c"set_interactable_Injected", 0x1801a2ea0, (bool_set);
    group_raycasts, c"CanvasGroup", c"get_blocksRaycasts_Injected", 0x18013c2e0, (bool_get);
    set_group_raycasts, c"CanvasGroup", c"set_blocksRaycasts_Injected", 0x1801a2f30, (bool_set);
    ignore_groups, c"CanvasGroup", c"get_ignoreParentGroups_Injected", 0x1801a2fc0, (bool_get);
    set_ignore_groups, c"CanvasGroup", c"set_ignoreParentGroups_Injected", 0x1801a2fd0, (bool_set);
    renderer_pop, c"CanvasRenderer", c"get_hasPopInstruction_Injected", 0x1801a3060, (bool_get);
    set_renderer_pop, c"CanvasRenderer", c"set_hasPopInstruction_Injected", 0x1801a3070, (bool_set);
    materials, c"CanvasRenderer", c"get_materialCount_Injected", 0x180075430, (int_get);
    set_materials, c"CanvasRenderer", c"set_materialCount_Injected", 0x1801a30a0, (int_set);
    pop_materials, c"CanvasRenderer", c"get_popMaterialCount_Injected", 0x1800d0880, (int_get);
    set_pop_materials, c"CanvasRenderer", c"set_popMaterialCount_Injected", 0x1801a30e0, (int_set);
    absolute_depth, c"CanvasRenderer", c"get_absoluteDepth_Injected", 0x1801a3120, (int_get);
    renderer_moved, c"CanvasRenderer", c"get_hasMoved_Injected", 0x1801a3140, (bool_get);
    cull_transparent, c"CanvasRenderer", c"get_cullTransparentMesh_Injected", 0x1801a3170, (bool_get);
    set_cull_transparent, c"CanvasRenderer", c"set_cullTransparentMesh_Injected", 0x1801a3180, (bool_set);
    has_clipping, c"CanvasRenderer", c"get_hasRectClipping_Injected", 0x1801a31a0, (bool_get);
    relative_depth, c"CanvasRenderer", c"get_relativeDepth_Injected", 0x1801a31b0, (int_get);
    renderer_cull, c"CanvasRenderer", c"get_cull_Injected", 0x1801a31d0, (bool_get);
    set_renderer_cull, c"CanvasRenderer", c"set_cull_Injected", 0x1801a31e0, (bool_set);
    set_renderer_color, c"CanvasRenderer", c"SetColor_Injected", 0x1801a3200, (value_set, c"Color", 16);
    renderer_color, c"CanvasRenderer", c"GetColor_Injected", 0x1801a32c0, (value_get, c"Color", 16);
    enable_clipping, c"CanvasRenderer", c"EnableRectClipping_Injected", 0x1801a32f0, (value_set, c"Rect", 16);
    clipping_softness, c"CanvasRenderer", c"get_clippingSoftness_Injected", 0x1801a33c0, (value_get, c"Vector2", 8);
    set_clipping_softness, c"CanvasRenderer", c"set_clippingSoftness_Injected", 0x1801a33e0, (value_set, c"Vector2", 8);
    disable_clipping, c"CanvasRenderer", c"DisableRectClipping_Injected", 0x1801a3420, (void);
    set_renderer_material, c"CanvasRenderer", c"SetMaterial_Injected", 0x1801a3440, (handle_index_set);
    renderer_material, c"CanvasRenderer", c"GetMaterial_Injected", 0x1801a3470, (handle_at, c"Material");
    set_renderer_pop_material, c"CanvasRenderer", c"SetPopMaterial_Injected", 0x1801a34c0, (handle_index_set);
    renderer_pop_material, c"CanvasRenderer", c"GetPopMaterial_Injected", 0x1801a34f0, (handle_at, c"Material");
    set_texture, c"CanvasRenderer", c"SetTexture_Injected", 0x1801a3550, (handle_set);
    secondary_textures, c"CanvasRenderer", c"GetSecondaryTextureCount_Injected", 0x180083dc0, (int_get);
    set_secondary_textures, c"CanvasRenderer", c"SetSecondaryTextureCount_Injected", 0x1801a3670, (int_set);
    secondary_texture, c"CanvasRenderer", c"GetSecondaryTexture_Injected", 0x1801a3ba0, (handle_at, c"Texture");
    set_alpha_texture, c"CanvasRenderer", c"SetAlphaTexture_Injected", 0x1801a3fb0, (handle_set);
    set_mesh, c"CanvasRenderer", c"SetMesh_Injected", 0x1801a3fe0, (handle_set);
    mesh, c"CanvasRenderer", c"GetMesh_Injected", 0x1801a3ff0, (handle_get, c"Mesh");
    clear_renderer, c"CanvasRenderer", c"Clear_Injected", 0x1801a4160, (void);
    inherited_alpha, c"CanvasRenderer", c"GetInheritedAlpha_Injected", 0x1801a4170, (float_get);
}
pub(super) fn installed() -> bool {
    BINDING.get().is_some()
}
pub(super) fn report() -> String {
    format!("native-ui-layout-calls={}\n", CALLS.with(Cell::get))
}
pub(super) fn install(
    library: Object,
    images: &[Object],
    registry: &bindings::Registry,
) -> Result<(), String> {
    macro_rules! sym {
        ($name:literal, $ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing UI layout API: ", $name).into());
            }
            unsafe { std::mem::transmute::<Object, $ty>(p) }
        }};
    }
    let find = sym!(
        "mono_class_from_name",
        unsafe extern "C" fn(Object, *const libc::c_char, *const libc::c_char) -> Object
    );
    let method = sym!(
        "mono_class_get_method_from_name",
        unsafe extern "C" fn(Object, *const libc::c_char, i32) -> Object
    );
    let add = sym!(
        "mono_add_internal_call",
        unsafe extern "C" fn(*const libc::c_char, Object)
    );
    let image = sym!(
        "mono_class_get_image",
        unsafe extern "C" fn(Object) -> Object
    );
    let is_enum = sym!("mono_class_is_enum", unsafe extern "C" fn(Object) -> i32);
    let enum_base = sym!(
        "mono_class_enum_basetype",
        unsafe extern "C" fn(Object) -> Object
    );
    let kind = sym!("mono_type_get_type", unsafe extern "C" fn(Object) -> u32);
    let size = sym!(
        "mono_class_value_size",
        unsafe extern "C" fn(Object, *mut u32) -> i32
    );
    let class = |namespace: &CStr, name: &CStr| -> Result<Object, String> {
        let mut matches: Vec<_> = images
            .iter()
            .map(|&image| unsafe { find(image, namespace.as_ptr(), name.as_ptr()) })
            .filter(|p| !p.is_null())
            .collect();
        matches.sort_unstable_by_key(|p| *p as usize);
        matches.dedup();
        if matches.len() != 1 {
            return Err(format!(
                "UI layout class is not unique: {namespace:?}.{name:?}"
            ));
        }
        Ok(matches[0])
    };
    let pointer = (0x18, class(c"System", c"IntPtr")?, false);
    let void = (1, class(c"System", c"Void")?, false);
    let integer = (8, class(c"System", c"Int32")?, false);
    let scalar = |scalar: Scalar| -> Result<(u32, Object, bool), String> {
        Ok(match scalar {
            Scalar::Boolean => (2, class(c"System", c"Boolean")?, false),
            Scalar::Integer => integer,
            Scalar::Float => (0xc, class(c"System", c"Single")?, false),
            Scalar::Enum(name) => {
                let c = class(c"UnityEngine", name)?;
                let mut alignment = 0;
                if unsafe { is_enum(c) } == 0
                    || unsafe { kind(enum_base(c)) } != 8
                    || unsafe { size(c, &mut alignment) } != 4
                    || alignment != 4
                {
                    return Err("UI layout enum is not the actual Int32 ABI".into());
                }
                (0x11, c, false)
            }
        })
    };
    let api = managed_calls::Api::load(library)?;
    let mut targets = Vec::new();
    let mut returns = vec![0; SPECS.len()];
    let mut names = Vec::new();
    for (index, spec) in SPECS.iter().enumerate() {
        let owner = class(c"UnityEngine", spec.owner)?;
        let signature = match spec.shape {
            Shape::Get(s) => vec![scalar(s)?, pointer],
            Shape::Set(s) => vec![void, pointer, scalar(s)?],
            Shape::ValueGet(name, width) | Shape::ValueSet(name, width) => {
                let c = class(c"UnityEngine", name)?;
                if value_layout::Api::load(library, unsafe { image(c) })?
                    .layout(c)?
                    .scalar_inline()?
                    != (width, 4)
                {
                    return Err("UI layout value ABI lacks exact scalar coverage".into());
                }
                vec![void, pointer, (0x11, c, true)]
            }
            Shape::Handle(name, indexed) => {
                returns[index] = class(c"UnityEngine", name)? as usize;
                let mut signature = vec![pointer, pointer];
                if indexed {
                    signature.push(integer);
                }
                signature
            }
            Shape::StaticHandle(name) => {
                returns[index] = class(c"UnityEngine", name)? as usize;
                vec![pointer]
            }
            Shape::SetHandle => vec![void, pointer, pointer],
            Shape::SetIndexedHandle => vec![void, pointer, pointer, integer],
            Shape::Void => vec![void, pointer],
            Shape::StaticBoolean => vec![void, scalar(Scalar::Boolean)?],
            Shape::Parent => vec![void, pointer, pointer, scalar(Scalar::Boolean)?],
        };
        let name = CString::new(format!(
            "UnityEngine.{}::{}",
            spec.owner.to_str().unwrap(),
            spec.member.to_str().unwrap()
        ))
        .unwrap();
        api.fixed_typed_icall(
            unsafe { method(owner, spec.member.as_ptr(), (signature.len() - 1) as i32) },
            owner,
            &signature,
            images,
        )
        .map_err(|error| format!("{}: {error}", name.to_string_lossy()))?;
        targets.push(registry.exact_target(name.to_str().unwrap(), spec.target)?);
        names.push(name);
    }
    BINDING
        .set(Binding {
            library: library as usize,
            targets,
            returns,
        })
        .map_err(|_| "UI layout adapters already installed")?;
    for (name, adapter) in names.iter().zip(adapters()) {
        unsafe { add(name.as_ptr(), adapter) };
    }
    Ok(())
}
fn binding() -> Result<&'static Binding, String> {
    BINDING
        .get()
        .ok_or_else(|| "UI layout adapters absent".into())
}
fn checked<T>(work: impl FnOnce() -> Result<T, String>) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            eprintln!("NATIVE_MONO_UI_LAYOUT_REJECT: {error}");
            std::process::abort()
        }
        Err(_) => {
            eprintln!("NATIVE_MONO_UI_LAYOUT_REJECT: adapter panicked");
            std::process::abort()
        }
    }
}
fn note(index: usize) {
    if SEEN.with(|seen| seen.borrow_mut().insert(index)) {
        eprintln!(
            "NATIVE_MONO_UI_LAYOUT_VERIFIED: owner={:?}; member={:?}; original-engine=true",
            SPECS[index].owner, SPECS[index].member
        );
    }
    CALLS.with(|c| c.set(c.get() + 1));
}
fn boolean(value: u64) -> Result<(), String> {
    if value > 1 {
        Err("UI layout Boolean is not canonical".into())
    } else {
        Ok(())
    }
}
fn scalar(index: usize, pointer: usize, argument: Option<u64>) -> (u64, u64) {
    checked(|| {
        let b = binding()?;
        let spec = &SPECS[index];
        let mut args = if matches!(spec.shape, Shape::StaticBoolean) {
            vec![]
        } else {
            vec![engine_objects::engine_pointer(pointer)?]
        };
        let mut floating = Vec::new();
        if let Some(value) = argument {
            if matches!(
                spec.shape,
                Shape::Set(Scalar::Boolean) | Shape::StaticBoolean
            ) {
                boolean(value)?;
            }
            let at = args.len();
            args.push(value);
            if matches!(spec.shape, Shape::Set(Scalar::Float)) {
                floating.push((at, value));
            }
        }
        let result = callbacks::guest_scalars(b.targets[index], &args, &floating)?;
        if matches!(spec.shape, Shape::Get(Scalar::Boolean)) {
            boolean(result.0 & 255)?;
        }
        note(index);
        Ok(result)
    })
}
fn handle(index: usize, pointer: Option<usize>, at: Option<i32>) -> usize {
    checked(|| {
        let b = binding()?;
        let mut args = Vec::new();
        if let Some(pointer) = pointer {
            args.push(engine_objects::engine_pointer(pointer)?);
        }
        args.extend(at.map(|at| at as u32 as u64));
        let result = callbacks::guest_arguments(b.targets[index], &args)?;
        let handle =
            engine_objects::engine_handle(b.library as Object, b.returns[index] as Object, result)?;
        note(index);
        Ok(handle)
    })
}
unsafe fn value_call(
    index: usize,
    pointer: usize,
    data: *mut u8,
    width: usize,
    input: bool,
) -> Result<(), String> {
    let b = binding()?;
    if data.is_null() || !(data as usize).is_multiple_of(4) {
        return Err("UI layout value pointer is NULL/misaligned".into());
    }
    let pointer = engine_objects::engine_pointer(pointer)?;
    let bytes = if input {
        unsafe { std::slice::from_raw_parts(data, width) }.to_vec()
    } else {
        vec![0; width]
    };
    let (_, result) =
        callbacks::guest_byte_arguments(b.targets[index], &[pointer, 0], &[(1, &bytes)])?;
    if !input {
        unsafe { std::ptr::copy_nonoverlapping(result[0].as_ptr(), data, width) };
    }
    note(index);
    Ok(())
}
