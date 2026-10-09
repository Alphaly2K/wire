#include <mono/mini/jit.h> // Source-tree location of the public mono/jit/jit.h.
#include <mono/metadata/assembly.h>
#include <mono/metadata/mono-config.h>
#include <mono/metadata/mono-gc.h>
#include <mono/metadata/object.h>
#include <mono/metadata/reflection.h>
#include <mono/metadata/threads.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct { double x; int64_t y; } ProbePair;
static MonoDomain *domain;
static MonoClass *fixture;
static MonoMethod *reenter;
static _Thread_local MonoThread *thread_identity;
static int boundary_exceptions;
static MonoGCHandle strong_handle, weak_handle, finalizable_weak;
static MonoThread *worker_identities[2];

static void require(int success, const char *label) {
    if (!success) { fprintf(stderr, "FAIL: %s\n", label); exit(1); }
}

static double host_scale(double value, int factor) { return value * factor; }
static int64_t host_ref(ProbePair *pair, int *result) {
    pair->x += 1.25;
    pair->y += 7;
    *result = 73;
    return pair->y + 1;
}
static int host_bounce(int value) {
    require(mono_thread_current() == thread_identity, "stable Mono TLS across reentry");
    void *args[] = { &value };
    MonoObject *exception = NULL;
    MonoObject *result = mono_runtime_invoke(reenter, NULL, args, &exception);
    if (exception) {
        // No native unwind crosses the FFI boundary.
        __atomic_add_fetch(&boundary_exceptions, 1, __ATOMIC_RELAXED);
        return -777;
    }
    require(result != NULL, "nested invoke result");
    return *(int *)mono_object_unbox(result);
}
static int invoke_int(const char *name, void **args) {
    MonoMethod *method = mono_class_get_method_from_name(fixture, name, args ? 1 : 0);
    require(method != NULL, name);
    MonoObject *exception = NULL;
    MonoObject *result = mono_runtime_invoke(method, NULL, args, &exception);
    if (exception) {
        MonoClass *klass = mono_object_get_class(exception);
        fprintf(stderr, "Managed exception in %s: %s.%s\n", name,
                mono_class_get_namespace(klass), mono_class_get_name(klass));
        exit(1);
    }
    require(result != NULL, "boxed result");
    return *(int *)mono_object_unbox(result);
}
static void *worker(void *opaque) {
    int seed = (int)(intptr_t)opaque;
    MonoThread *attached = mono_thread_attach(domain);
    require(attached != NULL, "worker attach");
    thread_identity = attached;
    worker_identities[seed - 1] = attached;
    void *args[] = { &seed };
    int result = invoke_int("Worker", args);
    mono_thread_detach(attached);
    return (void *)(intptr_t)result;
}

static void *gc_create(void *unused) {
    (void)unused;
    MonoThread *attached = mono_thread_attach(domain);
    MonoObject *exception = NULL;
    MonoMethod *factory = mono_class_get_method_from_name(fixture, "NewObject", 0);
    MonoObject *object = mono_runtime_invoke(factory, NULL, NULL, &exception);
    require(object && !exception, "GC object factory");
    strong_handle = mono_gchandle_new_v2(object, 1);
    weak_handle = mono_gchandle_new_weakref_v2(object, 0);
    mono_gc_collect(mono_gc_max_generation());
    require(mono_gchandle_get_target_v2(strong_handle) == object &&
            mono_gchandle_get_target_v2(weak_handle) == object, "strong/pinned and weak survive GC");
    factory = mono_class_get_method_from_name(fixture, "NewFinalizable", 0);
    object = mono_runtime_invoke(factory, NULL, NULL, &exception);
    require(object && !exception, "finalizable factory");
    finalizable_weak = mono_gchandle_new_weakref_v2(object, 0);
    // End this thread so conservative stale stack words cannot preserve objects.
    mono_thread_detach(attached);
    return NULL;
}
static void *gc_collect(void *unused) {
    (void)unused;
    MonoThread *attached = mono_thread_attach(domain);
    for (int i = 0; i < 8; i++) {
        mono_gc_collect(mono_gc_max_generation());
        invoke_int("Finalized", NULL);
    }
    require(mono_gchandle_get_target_v2(weak_handle) == NULL, "released object collected");
    require(mono_gchandle_get_target_v2(finalizable_weak) == NULL, "finalizable weak cleared");
    require(invoke_int("Finalized", NULL) == 1, "finalizer executed exactly once");
    mono_thread_detach(attached);
    return NULL;
}
static void run_native_thread(void *(*entry)(void *)) {
    pthread_t thread;
    require(pthread_create(&thread, NULL, entry, NULL) == 0, "GC thread create");
    require(pthread_join(thread, NULL) == 0, "GC thread join");
}

extern MonoCustomAttrInfo *wire_mono_custom_attrs_from_class_checked(MonoClass *, uint16_t *);
extern MonoCustomAttrInfo *wire_mono_custom_attrs_from_field_checked(MonoClass *, MonoClassField *, uint16_t *);
extern MonoObject *wire_mono_custom_attrs_get_attr_checked(MonoCustomAttrInfo *, MonoClass *, uint16_t *);
extern MonoArray *wire_mono_custom_attrs_construct_checked(MonoCustomAttrInfo *, uint16_t *);
static void check_tag(MonoObject *object, MonoClass *tag, int expected, int toggle) {
    require(object && mono_object_get_class(object) == tag, "real custom attribute type");
    int number = 0;
    uint8_t actual_toggle = 255;
    mono_field_get_value(object, mono_class_get_field_from_name(tag, "Number"), &number);
    mono_field_get_value(object, mono_class_get_field_from_name(tag, "Toggle"), &actual_toggle);
    require(number == expected && actual_toggle == toggle, "real attribute constructor and named field values");
}
extern MonoGCHandle wire_mono_object_new_rooted_checked(MonoDomain *, MonoClass *, uint16_t *);
static void run_objects(MonoImage *image) {
    MonoClass *klass = mono_class_from_name(image, "", "ProbeAllocated");
    require(klass != NULL, "allocated class");
    uint16_t status = 65535;
    MonoGCHandle root = wire_mono_object_new_rooted_checked(domain, klass, &status);
    require(root && status == 0, "checked allocation returns already strong-rooted object");
    MonoObject *object = mono_gchandle_get_target_v2(root);
    require(object && mono_object_get_class(object) == klass, "allocated type identity");
    MonoClassField *number_field = mono_class_get_field_from_name(klass, "Number");
    MonoClassField *text_field = mono_class_get_field_from_name(klass, "Text");
    int number = -1;
    MonoString *text = (MonoString *)1;
    mono_field_get_value(object, number_field, &number);
    mono_field_get_value(object, text_field, &text);
    require(number == 0 && text == NULL, "allocation does not invoke instance constructor");
    mono_gc_collect(mono_gc_max_generation());
    require(mono_gchandle_get_target_v2(root) == object, "pre-constructor root survives GC");
    MonoObject *exception = NULL;
    MonoMethod *ctor = mono_class_get_method_from_name(klass, ".ctor", 0);
    require(ctor != NULL, "allocated constructor");
    require(mono_runtime_invoke(ctor, object, NULL, &exception) == NULL && !exception, "separate real instance constructor");
    mono_gc_collect(mono_gc_max_generation());
    object = mono_gchandle_get_target_v2(root);
    require(object && mono_object_get_class(object) == klass, "constructed root survives GC");
    mono_field_get_value(object, number_field, &number);
    mono_field_get_value(object, text_field, &text);
    require(number == 37 && text && mono_string_length(text) == 24, "constructor scalar and managed reference survive GC");
    mono_gchandle_free_v2(root);
    require(wire_mono_object_new_rooted_checked(NULL, klass, &status) == NULL && status != 0, "NULL domain rejected");
    require(wire_mono_object_new_rooted_checked(domain, NULL, &status) == NULL && status != 0, "NULL class rejected");
    require(wire_mono_object_new_rooted_checked(domain, klass, NULL) == NULL, "missing status rejected");
    for (int index = 0; index < 3; index++) {
        MonoClass *invalid = index == 2 ? mono_array_class_get(klass, 1) :
            mono_class_from_name(image, "", index == 0 ? "ProbeAbstractAllocated" : "ProbeValueAllocated");
        require(invalid && wire_mono_object_new_rooted_checked(domain, invalid, &status) == NULL && status != 0,
                "abstract/value/array object shapes rejected with owned status");
    }
}
static void run_attrs(MonoImage *image) {
    MonoClass *target = mono_class_from_name(image, "", "ProbeAttrTarget");
    MonoClass *tag = mono_class_from_name(image, "", "ProbeTagAttribute");
    require(target && tag, "attribute fixture classes");
    uint16_t status = 65535;
    MonoCustomAttrInfo *info = wire_mono_custom_attrs_from_class_checked(target, &status);
    require(info && status == 0 && mono_custom_attrs_has_attr(info, tag), "checked class attributes");
    MonoObject *object = wire_mono_custom_attrs_get_attr_checked(info, tag, &status);
    require(object && status == 0, "checked attribute construction");
    MonoGCHandle root = mono_gchandle_new_v2(object, 1);
    require(root != NULL, "attribute strong root");
    MonoArray *array = wire_mono_custom_attrs_construct_checked(info, &status);
    require(array && status == 0 && mono_array_length(array) == 1, "checked real attribute array");
    MonoGCHandle array_root = mono_gchandle_new_v2((MonoObject *)array, 1);
    require(array_root != NULL, "attribute array strong root");
    for (int cycle = 0; cycle < 2; ++cycle) {
        check_tag(mono_gchandle_get_target_v2(root), tag, 71, 1);
        array = (MonoArray *)mono_gchandle_get_target_v2(array_root);
        check_tag(*(MonoObject **)mono_array_addr_with_size(array, sizeof(MonoObject *), 0), tag, 71, 1);
        mono_gc_collect(mono_gc_max_generation());
    }
    mono_custom_attrs_free(info);
    mono_gchandle_free_v2(array_root);
    mono_gchandle_free_v2(root);
    info = wire_mono_custom_attrs_from_field_checked(target, mono_class_get_field_from_name(target, "Value"), &status);
    require(info && status == 0, "checked field attributes");
    check_tag(wire_mono_custom_attrs_get_attr_checked(info, tag, &status), tag, 89, 0);
    require(status == 0, "field attribute constructor success");
    mono_custom_attrs_free(info);
    info = wire_mono_custom_attrs_from_class_checked(mono_class_from_name(image, "", "ProbeAttrThrowTarget"), &status);
    require(info && status == 0, "throwing attribute metadata query succeeds");
    tag = mono_class_from_name(image, "", "ProbeThrowAttribute");
    require(wire_mono_custom_attrs_get_attr_checked(info, tag, &status) == NULL && status != 0, "constructor exception returned as owned error code");
    require(wire_mono_custom_attrs_construct_checked(info, &status) == NULL && status != 0, "array constructor exception returned as owned error code");
    mono_custom_attrs_free(info);
    require(wire_mono_custom_attrs_from_class_checked(NULL, &status) == NULL && status != 0, "invalid class refused without fake success");
}

int main(int argc, char **argv) {
    require(argc == 5, "usage: embed_probe classlib-root fixture.exe core|interop|threads|gc|layout|remoting|collation|static-roots|attrs|objects interp|interp-only");
    mono_set_dirs(argv[1], argv[1]);
    mono_config_parse(NULL);
    mono_jit_set_aot_mode(strcmp(argv[4], "interp-only") == 0 ? MONO_AOT_MODE_INTERP_ONLY : MONO_AOT_MODE_INTERP);
    if (getenv("WIRE_MONO_UNITY_GC_PROFILE")) {
        char *options[] = { "--gc-params=incremental=3" };
        mono_jit_parse_options(1, options);
    }
    domain = mono_jit_init_version("wire-native-mono-probe", "v4.0.30319");
    require(domain != NULL, "domain");
    thread_identity = mono_thread_current();
    mono_add_internal_call("NativeProbe::HostScale", (const void *)host_scale);
    mono_add_internal_call("NativeProbe::HostRef", (const void *)host_ref);
    mono_add_internal_call("NativeProbe::HostBounce", (const void *)host_bounce);
    MonoAssembly *assembly = mono_domain_assembly_open(domain, argv[2]);
    require(assembly != NULL, "fixture assembly");
    fixture = mono_class_from_name(mono_assembly_get_image(assembly), "", "NativeProbe");
    require(fixture != NULL, "fixture class");
    reenter = mono_class_get_method_from_name(fixture, "Reenter", 1);
    require(reenter != NULL, "nested target");
    if (strcmp(argv[3], "objects") == 0) {
        run_objects(mono_assembly_get_image(assembly));
    } else if (strcmp(argv[3], "attrs") == 0) {
        run_attrs(mono_assembly_get_image(assembly));
    } else if (strcmp(argv[3], "static-roots") == 0) {
        require(invoke_int("StaticRoots", NULL) == 0, "static/thread/generic reference roots under allocation pressure");
    } else if (strcmp(argv[3], "collation") == 0) {
        require(invoke_int("Collation", NULL) == 0, "real corelib invariant/en-US/ja-JP collation across GC");
    } else if (strcmp(argv[3], "core") == 0) {
        require(invoke_int("Core", NULL) == 0, "core IL checksum, generics, reflection, delegate, GC");
        MonoMethod *method = mono_class_get_method_from_name(fixture, "ThrowBoundary", 0);
        MonoObject *exception = NULL;
        mono_runtime_invoke(method, NULL, NULL, &exception);
        require(exception != NULL, "exception returned to embedding caller");
    } else if (strcmp(argv[3], "remoting") == 0) {
        require(invoke_int("Remoting", NULL) == 0, "MarshalByRefObject field/ref CAS and AppDomain event lifecycle");
    } else if (strcmp(argv[3], "interop") == 0) {
        require(invoke_int("Interop", NULL) == 0, "float, struct, ref/out, callback, exception reentry");
        require(boundary_exceptions == 1, "one nested exception");
    } else if (strcmp(argv[3], "threads") == 0) {
        pthread_t threads[2];
        for (int i = 0; i < 2; i++) require(pthread_create(&threads[i], NULL, worker, (void *)(intptr_t)(i + 1)) == 0, "pthread create");
        for (int i = 0; i < 2; i++) {
            void *result = NULL;
            require(pthread_join(threads[i], &result) == 0 && result == NULL, "worker completion");
        }
        require(worker_identities[0] != worker_identities[1] &&
                worker_identities[0] != thread_identity && worker_identities[1] != thread_identity,
                "distinct Mono identities for two native threads");
    } else if (strcmp(argv[3], "gc") == 0) {
        mono_gc_set_incremental(1);
        require(mono_gc_is_incremental(), "Unity BDWGC incremental collection enabled");
        run_native_thread(gc_create);
        mono_gchandle_free_v2(strong_handle);
        run_native_thread(gc_collect);
        mono_gchandle_free_v2(weak_handle);
        mono_gchandle_free_v2(finalizable_weak);
    } else if (strcmp(argv[3], "layout") == 0) {
        require(sizeof(void *) == 8 && sizeof(MonoObject) == 16, "64-bit object header");
        MonoArray *array = mono_array_new(domain, mono_get_int32_class(), 7);
        MonoGCHandle root = mono_gchandle_new_v2((MonoObject *)array, 1);
        uintptr_t stored_length;
        memcpy(&stored_length, (char *)array + 24, sizeof(stored_length));
        require(stored_length == 7 && mono_array_length(array) == 7, "array length at offset 24");
        require(mono_array_addr_with_size(array, 4, 0) == (char *)array + 32, "array data at offset 32");
        int value = 37;
        MonoObject *boxed = mono_value_box(domain, mono_get_int32_class(), &value);
        require((char *)mono_object_unbox(boxed) == (char *)boxed + 16, "unbox at offset 16");
        MonoString *string = mono_string_new(domain, "layout");
        int stored_string_length;
        memcpy(&stored_string_length, (char *)string + 16, sizeof(stored_string_length));
        require(stored_string_length == 6 && (char *)mono_string_chars(string) == (char *)string + 20,
                "string length and UTF16 layout");
        mono_gchandle_free_v2(root);
    } else {
        require(0, "unknown probe");
    }
    printf("PASS %s: native ARM64 Unity Mono IL interpreter\n", argv[3]);
    fflush(stdout);
    mono_jit_cleanup(domain);
    return 0;
}
