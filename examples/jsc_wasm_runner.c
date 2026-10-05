// Public-JSC-API runtime/oracle runner for examples/wasm_bench.rs (macOS).
// The imported gateway is a measurement shim, not wire's Win32 dispatcher.
#include <JavaScriptCore/JavaScript.h>
#include <inttypes.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

enum { BANK_BYTES = 512, IO_OFFSET = 4096 };
static struct {
    JSObjectRef view, add;
    FILE *file;
    uint64_t calls;
    bool borrowed;
} bridge;

static double now(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec + t.tv_nsec / 1e9;
}
static void fail(const char *message) {
    fprintf(stderr, "%s\n", message);
    exit(1);
}
static void print_exception(JSContextRef ctx, JSValueRef error) {
    JSStringRef text = JSValueToStringCopy(ctx, error, NULL);
    char message[2048];
    JSStringGetUTF8CString(text, message, sizeof(message));
    fprintf(stderr, "JSC: %s\n", message);
    JSStringRelease(text);
}
static JSValueRef evaluate(JSGlobalContextRef ctx, const char *source) {
    JSStringRef s = JSStringCreateWithUTF8CString(source);
    JSValueRef error = NULL;
    JSValueRef value = JSEvaluateScript(ctx, s, NULL, NULL, 1, &error);
    JSStringRelease(s);
    if (error) { print_exception(ctx, error); fail("JS evaluation failed"); }
    return value;
}
static void raise_error(JSContextRef ctx, JSValueRef *error, const char *message) {
    JSStringRef text = JSStringCreateWithUTF8CString(message);
    JSValueRef arg = JSValueMakeString(ctx, text);
    JSStringRelease(text);
    *error = JSObjectMakeError(ctx, 1, &arg, NULL);
}
static JSValueRef gateway(JSContextRef ctx, JSObjectRef function,
                         JSObjectRef this_object, size_t count,
                         const JSValueRef args[], JSValueRef *error) {
    (void)function; (void)this_object;
    if (bridge.borrowed || count != 2) {
        raise_error(ctx, error, "gateway reentry during a native memory borrow");
        return JSValueMakeUndefined(ctx);
    }
    uint32_t tag = (uint32_t)JSValueToNumber(ctx, args[0], NULL);
    uint32_t value = (uint32_t)(int64_t)JSValueToNumber(ctx, args[1], NULL);
    bridge.calls++;
    if (tag == 0)
        return JSValueMakeNumber(ctx, (int32_t)(value ^ 0xa5a5a5a5u));
    if (tag == 2) {
        JSValueRef callback_args[2] = {args[1], JSValueMakeNumber(ctx, 17)};
        return JSObjectCallAsFunction(ctx, bridge.add, NULL, 2, callback_args, error);
    }
    if (tag != 1) {
        raise_error(ctx, error, "unknown gateway measurement ID");
        return JSValueMakeUndefined(ctx);
    }
    size_t size = JSObjectGetTypedArrayByteLength(ctx, bridge.view, error);
    if (*error) return JSValueMakeUndefined(ctx);
    if (size < IO_OFFSET || value > size - IO_OFFSET) {
        raise_error(ctx, error, "I/O buffer is detached or outside linear memory");
        return JSValueMakeUndefined(ctx);
    }
    if (fseek(bridge.file, 0, SEEK_SET) != 0) {
        raise_error(ctx, error, "fixture seek failed");
        return JSValueMakeUndefined(ctx);
    }
    unsigned char *ptr = JSObjectGetTypedArrayBytesPtr(ctx, bridge.view, error);
    if (*error || !ptr) return JSValueMakeUndefined(ctx);
    // No JSC call, callback, grow, or asynchronous ownership while borrowed.
    bridge.borrowed = true;
    size_t read = fread(ptr + IO_OFFSET, 1, value, bridge.file);
    bridge.borrowed = false;
    ptr = NULL;
    if (ferror(bridge.file)) {
        raise_error(ctx, error, "fixture read failed");
        return JSValueMakeUndefined(ctx);
    }
    return JSValueMakeNumber(ctx, read);
}
static JSObjectRef function(JSGlobalContextRef ctx, const char *name) {
    char source[128];
    snprintf(source, sizeof(source), "instance.exports.%s", name);
    JSObjectRef object = JSValueToObject(ctx, evaluate(ctx, source), NULL);
    if (!object || !JSObjectIsFunction(ctx, object)) fail("missing Wasm export");
    JSValueProtect(ctx, object);
    return object;
}
static JSValueRef call(JSGlobalContextRef ctx, JSObjectRef fn, uint32_t n,
                       JSValueRef *error) {
    JSValueRef arg = JSValueMakeNumber(ctx, n);
    return JSObjectCallAsFunction(ctx, fn, NULL, 1, &arg, error);
}
static uint32_t invoke(JSGlobalContextRef ctx, JSObjectRef fn, uint32_t n) {
    JSValueRef error = NULL;
    JSValueRef result = call(ctx, fn, n, &error);
    if (error) { print_exception(ctx, error); fail("Wasm call failed"); }
    return (uint32_t)(int64_t)JSValueToNumber(ctx, result, NULL);
}
static size_t view_size(JSGlobalContextRef ctx) {
    JSValueRef error = NULL;
    size_t size = JSObjectGetTypedArrayByteLength(ctx, bridge.view, &error);
    if (error) { print_exception(ctx, error); fail("view length failed"); }
    return size;
}
static void transfer(JSGlobalContextRef ctx, unsigned char bank[BANK_BYTES], bool write) {
    if (view_size(ctx) < BANK_BYTES) fail("view is too small/detached");
    JSValueRef error = NULL;
    void *ptr = JSObjectGetTypedArrayBytesPtr(ctx, bridge.view, &error);
    if (error || !ptr) fail("memory pointer unavailable");
    bridge.borrowed = true;
    if (write) memcpy(ptr, bank, BANK_BYTES);
    else memcpy(bank, ptr, BANK_BYTES);
    bridge.borrowed = false;
}
static void refresh_view(JSGlobalContextRef ctx) {
    if (bridge.view) JSValueUnprotect(ctx, bridge.view);
    bridge.view = JSValueToObject(ctx,
        evaluate(ctx, "new Uint8Array(instance.exports.memory.buffer)"), NULL);
    JSValueProtect(ctx, bridge.view);
}
static uint32_t little32(const unsigned char *b) {
    return (uint32_t)b[0] | ((uint32_t)b[1] << 8) | ((uint32_t)b[2] << 16) | ((uint32_t)b[3] << 24);
}
static void read_exact(FILE *file, void *dst, size_t count) {
    if (fread(dst, 1, count, file) != count) fail("truncated input");
}
static unsigned char *read_module(const char *path, size_t *size) {
    FILE *file = fopen(path, "rb");
    if (!file) fail("cannot open Wasm module");
    if (fseek(file, 0, SEEK_END) != 0) fail("module seek failed");
    long length = ftell(file);
    if (length < 8 || length > 16 * 1024 * 1024) fail("invalid module size");
    rewind(file);
    unsigned char *data = malloc((size_t)length);
    if (!data) fail("module allocation failed");
    read_exact(file, data, (size_t)length);
    fclose(file);
    *size = (size_t)length;
    return data;
}

int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IONBF, 0);
    if (argc != 3) fail("usage: jsc_wasm_runner module.wasm oracle.bin");
    size_t module_size;
    unsigned char *module = read_module(argv[1], &module_size);
    double setup = now();
    JSGlobalContextRef ctx = JSGlobalContextCreate(NULL);
    JSValueRef error = NULL;
    JSObjectRef bytes = JSObjectMakeTypedArray(ctx, kJSTypedArrayTypeUint8Array, module_size, &error);
    if (error || !bytes) fail("module typed array failed");
    void *ptr = JSObjectGetTypedArrayBytesPtr(ctx, bytes, &error);
    if (error || !ptr) fail("module bytes unavailable");
    memcpy(ptr, module, module_size);
    free(module);
    ptr = NULL;
    JSObjectRef global = JSContextGetGlobalObject(ctx);
    JSStringRef key = JSStringCreateWithUTF8CString("moduleBytes");
    JSObjectSetProperty(ctx, global, key, bytes, kJSPropertyAttributeNone, NULL);
    JSStringRelease(key);
    key = JSStringCreateWithUTF8CString("gateway");
    JSObjectRef native = JSObjectMakeFunctionWithCallback(ctx, key, gateway);
    JSObjectSetProperty(ctx, global, key, native, kJSPropertyAttributeNone, NULL);
    JSStringRelease(key);
    evaluate(ctx, "var instance=new WebAssembly.Instance(new WebAssembly.Module(moduleBytes),"
                  "{wire:{gateway:gateway}});");
    refresh_view(ctx);
    JSObjectRef run = function(ctx, "run"), tick = function(ctx, "native_loop");
    JSObjectRef io = function(ctx, "read_file"), load = function(ctx, "load");
    JSObjectRef grow = function(ctx, "grow"), callbacks = function(ctx, "callback_loop");
    bridge.add = function(ctx, "add");
    printf("JSC context+module setup: %.3f ms; module %zu bytes\n", (now() - setup) * 1e3, module_size);

    FILE *cases = fopen(argv[2], "rb");
    if (!cases) fail("cannot open oracle");
    unsigned char header[12];
    read_exact(cases, header, sizeof(header));
    if (memcmp(header, "WJCASE01", 8)) fail("invalid oracle header");
    uint32_t case_count = little32(header + 8);
    if (!case_count || case_count > 2048) fail("invalid oracle count");
    unsigned char initial[BANK_BYTES], expected[BANK_BYTES], actual[BANK_BYTES], spec[8];
    uint32_t bench_budget = 0, bench_reason = 0;
    for (uint32_t n = 0; n < case_count; n++) {
        read_exact(cases, spec, 8);
        read_exact(cases, initial, BANK_BYTES);
        read_exact(cases, expected, BANK_BYTES);
        uint32_t budget = little32(spec), reason = little32(spec + 4);
        transfer(ctx, initial, true);
        uint32_t result = invoke(ctx, run, budget);
        transfer(ctx, actual, false);
        if (result != reason || memcmp(expected, actual, BANK_BYTES)) {
            fprintf(stderr, "oracle case %u failed: budget=%u reason=%u expected=%u\n", n, budget, result, reason);
            for (size_t slot = 0; slot < 64; slot++) {
                if (memcmp(expected + 8 * slot, actual + 8 * slot, 8))
                    fprintf(stderr, "slot %zu: actual=%08x expected=%08x\n", slot,
                            little32(actual + 8 * slot), little32(expected + 8 * slot));
            }
            return 1;
        }
        if (n == case_count - 1) { bench_budget = budget; bench_reason = reason; }
    }
    if (fgetc(cases) != EOF) fail("trailing oracle data");
    fclose(cases);
    printf("%u oracle cases: GPR/FLAGS/PC/retired/untouched vregs match\n", case_count);
    // The last oracle is the benchmark. Warm it separately from measurement.
    transfer(ctx, initial, true);
    invoke(ctx, run, bench_budget);
    for (unsigned round = 0; round < 3; round++) {
        transfer(ctx, initial, true);
        double start = now();
        uint32_t reason = invoke(ctx, run, bench_budget);
        double elapsed = now() - start;
        transfer(ctx, actual, false);
        if (reason != bench_reason || memcmp(actual, expected, BANK_BYTES)) fail("benchmark state differs");
        uint32_t retired = little32(actual + 10 * 8);
        printf("Wasm %.2f guest MIPS (%.6fs, %u retired, full FLAGS/budget checks)\n",
               retired / elapsed / 1e6, elapsed, retired);
    }

    const uint32_t crossing_count = 250000;
    for (unsigned mode = 0; mode < 2; mode++) {
        JSObjectRef fn = mode ? callbacks : tick;
        uint32_t checksum = 0;
        for (uint32_t i = 0; i < crossing_count; i++) checksum ^= mode ? i + 17 : i ^ 0xa5a5a5a5u;
        invoke(ctx, fn, 10000);
        for (unsigned round = 0; round < 3; round++) {
            bridge.calls = 0;
            double start = now();
            uint32_t result = invoke(ctx, fn, crossing_count);
            double elapsed = now() - start;
            if (result != checksum || bridge.calls != crossing_count) fail("gateway checksum/count differs");
            printf("%s %.2f ns/call (%.2f Mcalls/s, %" PRIu64 " calls)\n",
                   mode ? "Wasm->native->Wasm callback" : "Wasm->native gateway",
                   elapsed * 1e9 / crossing_count, crossing_count / elapsed / 1e6, bridge.calls);
        }
    }

    bridge.file = tmpfile();
    if (!bridge.file) fail("temporary I/O fixture failed");
    unsigned char fixture[65536];
    for (size_t i = 0; i < sizeof(fixture); i++) fixture[i] = (unsigned char)(i * 37 ^ (i >> 3));
    if (fwrite(fixture, 1, sizeof(fixture), bridge.file) != sizeof(fixture)) fail("fixture write failed");
    if (invoke(ctx, io, 32768) != 32768 || invoke(ctx, load, IO_OFFSET) != little32(fixture)) fail("native I/O readback differs");
    JSValueRef caught = NULL;
    call(ctx, load, (uint32_t)view_size(ctx), &caught);
    if (!caught) fail("out-of-bounds load did not trap");
    caught = NULL;
    call(ctx, io, 65536, &caught);
    if (!caught) fail("out-of-bounds native I/O was accepted");
    if (invoke(ctx, grow, 1) != 1 || view_size(ctx) != 0) fail("old view was not detached after grow");
    caught = NULL;
    call(ctx, io, 4, &caught);
    if (!caught) fail("detached native I/O view was accepted");
    refresh_view(ctx);
    if (view_size(ctx) != 131072 || invoke(ctx, load, IO_OFFSET) != little32(fixture)) fail("grow lost existing bytes");
    if (invoke(ctx, io, 65537) != 65536) fail("short read/EOF differs");
    for (unsigned round = 0; round < 3; round++) {
        double start = now();
        for (unsigned i = 0; i < 1024; i++) {
            if (invoke(ctx, io, 32768) != 32768) fail("I/O benchmark short read");
        }
        double elapsed = now() - start;
        printf("native fread->Wasm %.2f MiB/s (32 MiB, synchronous temporary borrow)\n", 32 / elapsed);
    }
    fclose(bridge.file);
    puts("bounds, grow/detached-view, refresh, short-read, nested callback checks passed");
    JSValueUnprotect(ctx, bridge.view);
    for (size_t i = 0; i < 7; i++) {
        JSObjectRef refs[] = {run, tick, io, load, grow, callbacks, bridge.add};
        JSValueUnprotect(ctx, refs[i]);
    }
    JSGlobalContextRelease(ctx);
    return 0;
}
