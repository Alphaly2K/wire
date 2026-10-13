#include <mono/mini/jit.h>
#include <mono/metadata/assembly.h>
#include <mono/metadata/mono-config.h>
#include <mono/metadata/mono-gc.h>
#include <mono/metadata/object.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef int (*ScalarRead)(MonoMethod *, MonoObject *, uint64_t, void *, size_t);
extern int wire_mono_install_scalar_read_adapter(uint32_t, MonoMethod *, ScalarRead);
extern int wire_mono_prepare_memory_adapter(uint32_t);
extern int wire_mono_field_fixed_buffer(uint32_t, MonoClass *, MonoClassField *, MonoType **, uint32_t *);
extern int wire_mono_class_is_unsafe_value(uint32_t, MonoClass *);
static MonoMethod *read_method;
static MonoMethod *value_method;
static MonoGCHandle receiver_root;
static int calls;
static uint32_t engine_header[8];
static void require(int ok, const char *label) {
    if (!ok) { fprintf(stderr, "FAIL: %s\n", label); exit(1); }
}
static int read_guest(MonoMethod *method, MonoObject *receiver, uint64_t address,
                      void *output, size_t width) {
    if ((method != read_method && method != value_method) || receiver != mono_gchandle_get_target_v2(receiver_root) ||
        address != UINT64_C(0x30100014) || !output || width != 4) return 1;
    memcpy(output, &engine_header[5], 4);
    ++calls;
    return 0;
}
static int invoke(MonoMethod *method, MonoObject *receiver, void **args, int failure) {
    MonoObject *exception = NULL;
    MonoObject *result = mono_runtime_invoke(method, receiver, args, &exception);
    if (failure) {
        require(exception && !strcmp(mono_class_get_name(mono_object_get_class(exception)),
                                   "AccessViolationException"), "rejected read has managed unwind");
        return 0;
    }
    require(result && !exception, "actual managed read result");
    return *(int *)mono_object_unbox(result);
}
int main(int argc, char **argv) {
    require(argc == 3, "classlib-root fixture.dll");
    mono_set_dirs(argv[1], argv[1]);
    mono_config_parse(NULL);
    require(wire_mono_prepare_memory_adapter(1) == 0, "preserve runtime-invoke method boundaries");
    mono_jit_set_aot_mode(MONO_AOT_MODE_INTERP);
    if (getenv("WIRE_MONO_UNITY_GC_PROFILE")) {
        char *options[] = { "--gc-params=incremental=3" };
        mono_jit_parse_options(1, options);
    }
    MonoDomain *domain = mono_jit_init_version("wire-engine-header-probe", "v4.0.30319");
    require(domain != NULL, "domain");
    MonoAssembly *assembly = mono_domain_assembly_open(domain, argv[2]);
    require(assembly != NULL, "assembly");
    MonoImage *image = mono_assembly_get_image(assembly);
    MonoClass *numeric = mono_class_from_name(image, "", "FixedNumericProbe");
    MonoClassField *planes = mono_class_get_field_from_name(numeric, "Planes");
    MonoType *element = NULL;
    uint32_t count = 0;
    require(wire_mono_field_fixed_buffer(1, numeric, planes, &element, &count) == 1 &&
            element && mono_type_get_type(element) == MONO_TYPE_R4 && count == 24,
            "actual fixed float element and complete count from metadata");
    MonoClass *storage = mono_class_from_mono_type(mono_field_get_type(planes));
    require(wire_mono_class_is_unsafe_value(1, storage) == 1 &&
            wire_mono_class_is_unsafe_value(1, numeric) == 0,
            "compiler unsafe storage requires its declaring fixed-buffer field");
    uint32_t alignment = 0;
    require(mono_class_value_size(storage, &alignment) == 96 && alignment == 4,
            "fixed storage contains every element, not first element plus padding");
    element = (MonoType *)(uintptr_t)0x1234;
    count = 9876;
    require(wire_mono_field_fixed_buffer(2, numeric, planes, &element, &count) == -1 &&
            element == (MonoType *)(uintptr_t)0x1234 && count == 9876,
            "metadata version rejection leaves output cells unchanged");
    require(wire_mono_field_fixed_buffer(1, numeric, mono_class_get_field_from_name(numeric, "Tag"),
            &element, &count) == 0 && !element && !count, "ordinary field has no fixed protocol");
    MonoClass *large = mono_class_from_name(image, "", "LargeFixedNumericProbe");
    require(wire_mono_field_fixed_buffer(1, large, mono_class_get_field_from_name(large, "Bytes"),
            &element, &count) == 1 && element && mono_type_get_type(element) == MONO_TYPE_U1 && count == 1024,
            "large fixed buffer declaration remains bounded complete metadata");
    element = (MonoType *)(uintptr_t)0x1234;
    count = 9876;
    require(wire_mono_field_fixed_buffer(1, large, planes,
            &element, &count) == -1 && element == (MonoType *)(uintptr_t)0x1234 && count == 9876,
            "foreign declaring class rejected without constructing attribute IL");
    MonoClass *base = mono_class_from_name(image, "", "EngineHeaderProbe");
    MonoClass *derived = mono_class_from_name(image, "", "DerivedEngineHeaderProbe");
    require(base && derived, "actual base and derived metadata");
    MonoObject *receiver = mono_object_new(domain, derived);
    receiver_root = mono_gchandle_new_v2(receiver, 1);
    MonoMethod *already = mono_class_get_method_from_name(base, "AlreadyRun", 0);
    require(invoke(already, receiver, NULL, 0) == 7, "ordinary method executes");
    require(wire_mono_install_scalar_read_adapter(1, already, read_guest) != 0,
            "cached unchecked transform rejected");
    read_method = mono_class_get_method_from_name(base, "Read", 0);
    require(read_method != NULL, "read metadata");
    require(wire_mono_install_scalar_read_adapter(3, read_method, read_guest) != 0,
            "invalid ABI version rejected");
    require(wire_mono_install_scalar_read_adapter(1, read_method, read_guest) == 0,
            "immutable exact method installed before first transform");
    require(wire_mono_install_scalar_read_adapter(1, read_method, read_guest) != 0,
            "duplicate installation rejected");
    value_method = mono_class_get_method_from_name(base, "ReadValue", 0);
    require(value_method != NULL, "actual value getter metadata");
    require(wire_mono_install_scalar_read_adapter(1, value_method, read_guest) != 0,
            "version one rejects value result");
    require(wire_mono_install_scalar_read_adapter(2, value_method, read_guest) == 0,
            "version two installs second exact method with four-byte value result");
    require(wire_mono_install_scalar_read_adapter(2, value_method, read_guest) != 0,
            "version two duplicate rejected");
    require(wire_mono_install_scalar_read_adapter(2,
            mono_class_get_method_from_name(base, "WideValue", 0), read_guest) != 0,
            "version two rejects wide result");
    MonoMethod *already_value = mono_class_get_method_from_name(base, "AlreadyValue", 0);
    require(invoke(already_value, receiver, NULL, 0) == 7, "ordinary value method executes");
    require(wire_mono_install_scalar_read_adapter(2, already_value, read_guest) != 0,
            "version two cached unchecked value transform rejected");
    mono_runtime_object_init(receiver);
    require(invoke(read_method, receiver, NULL, 0) == 0 && calls == 0,
            "original null behavior bypasses memory callback");
    require(invoke(value_method, receiver, NULL, 0) == 0 && calls == 0,
            "original value null helper bypasses memory callback");
    MonoClassField *cached = mono_class_get_field_from_name(base, "Cached");
    for (int i = 0; i < 2; ++i) {
        MonoClass *declaring = NULL;
        require(mono_field_from_token(image, mono_class_get_field_token(cached), &declaring, NULL) == cached && declaring == base,
                "field token resolution retains declaring-class output on cache hits");
    }
    uintptr_t pointer = UINT64_C(0x30100000);
    mono_field_set_value(receiver, cached, &pointer);
    engine_header[5] = UINT32_C(0xfedcba98);
    require((uint32_t)invoke(read_method, receiver, NULL, 0) == engine_header[5] && calls == 1,
            "derived receiver reads original guest I4 bits");
    mono_gc_collect(mono_gc_max_generation());
    receiver = mono_gchandle_get_target_v2(receiver_root);
    require((uint32_t)invoke(read_method, receiver, NULL, 0) == engine_header[5] && calls == 2,
            "rooted receiver identity survives GC");
    pointer += 8;
    mono_field_set_value(receiver, cached, &pointer);
    invoke(read_method, receiver, NULL, 1);
    require(calls == 2, "rejection never falls back to fake host address");
    MonoMethod *unscoped = mono_class_get_method_from_name(base, "Unscoped", 1);
    int native = 1234567;
    pointer = (uintptr_t)&native;
    void *args[] = { &pointer };
    require(invoke(unscoped, receiver, args, 0) == native && calls == 2,
            "other IL keeps actual native pointer semantics");
    pointer = UINT64_C(0x30100000);
    mono_field_set_value(receiver, cached, &pointer);
    require((uint32_t)invoke(value_method, receiver, NULL, 0) == engine_header[5] && calls == 3,
            "value getter executes original I4 conversion with original guest bits");
    require((uint32_t)invoke(read_method, receiver, NULL, 0) == engine_header[5] && calls == 4,
            "first method binding remains valid after second binding");
    mono_gc_collect(mono_gc_max_generation());
    receiver = mono_gchandle_get_target_v2(receiver_root);
    require((uint32_t)invoke(value_method, receiver, NULL, 0) == engine_header[5] && calls == 5,
            "value getter identity survives GC");
    pointer += 8;
    mono_field_set_value(receiver, cached, &pointer);
    invoke(value_method, receiver, NULL, 1);
    require(calls == 5, "value getter rejection has no host pointer fallback");
    mono_gchandle_free_v2(receiver_root);
    mono_jit_cleanup(domain);
    puts("PASS engine-header: exact method, original null IL, inherited receiver, GC, managed rejection, native read isolation");
    return 0;
}
