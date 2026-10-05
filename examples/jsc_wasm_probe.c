// Standalone public-JSC-API probe, not a wire execution backend.
// macOS: xcrun clang examples/jsc_wasm_probe.c -framework JavaScriptCore -o /tmp/wire-jsc-probe
#include <JavaScriptCore/JavaScript.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

static void report_exception(JSContextRef ctx, JSValueRef exception) {
    JSStringRef text = JSValueToStringCopy(ctx, exception, NULL);
    char message[2048];
    JSStringGetUTF8CString(text, message, sizeof(message));
    fprintf(stderr, "JSC: %s\n", message);
    JSStringRelease(text);
}

static JSValueRef evaluate(JSGlobalContextRef ctx, const char *source) {
    JSStringRef script = JSStringCreateWithUTF8CString(source);
    JSValueRef exception = NULL;
    JSValueRef result = JSEvaluateScript(ctx, script, NULL, NULL, 1, &exception);
    JSStringRelease(script);
    if (exception) {
        report_exception(ctx, exception);
        return NULL;
    }
    return result;
}

int main(void) {
    JSGlobalContextRef ctx = JSGlobalContextCreate(NULL);
    // Authored Wasm module: add(i32,i32), read(i32), and a one-page memory.
    // The globals keep the instance and view alive for the native access.
    const char *source =
        "var bytes=new Uint8Array([0,97,115,109,1,0,0,0,"
        "1,12,2,96,2,127,127,1,127,96,1,127,1,127,"
        "3,3,2,0,1,5,3,1,0,1,"
        "7,23,3,3,97,100,100,0,0,4,114,101,97,100,0,1,6,109,101,109,111,114,121,2,0,"
        "10,17,2,7,0,32,0,32,1,106,11,7,0,32,0,40,2,0,11]);"
        "var instance=new WebAssembly.Instance(new WebAssembly.Module(bytes));"
        "var view=new Uint8Array(instance.exports.memory.buffer);"
        "instance.exports.add(19,23);";
    int status = 1;
    JSValueRef result = evaluate(ctx, source);
    if (!result)
        goto done;
    double sum = JSValueToNumber(ctx, result, NULL);
    printf("Wasm add(19,23): %.0f\n", sum);
    if (sum != 42)
        goto done;
    result = evaluate(ctx, "view");
    if (!result)
        goto done;
    JSValueRef exception = NULL;
    JSObjectRef view = JSValueToObject(ctx, result, &exception);
    if (exception) {
        report_exception(ctx, exception);
        goto done;
    }
    size_t size = JSObjectGetTypedArrayByteLength(ctx, view, &exception);
    if (exception) {
        report_exception(ctx, exception);
        goto done;
    }
    if (size < 4)
        goto done;
    void *ptr = JSObjectGetTypedArrayBytesPtr(ctx, view, &exception);
    if (exception) {
        report_exception(ctx, exception);
        goto done;
    }
    if (!ptr)
        goto done;
    // The pointer is temporary: do not call JSC, grow memory, or execute guest
    // callbacks between acquiring it and finishing this synchronous write.
    const unsigned char value_le[4] = {0x40, 0xe2, 0x01, 0x00}; // 123456
    memcpy(ptr, value_le, sizeof(value_le));
    ptr = NULL;
    result = evaluate(ctx, "instance.exports.read(0)");
    if (!result)
        goto done;
    double readback = JSValueToNumber(ctx, result, NULL);
    printf("Native write -> Wasm i32.load: %.0f\n", readback);
    status = readback == 123456 ? 0 : 1;
done:
    JSGlobalContextRelease(ctx);
    return status;
}
