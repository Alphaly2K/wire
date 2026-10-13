using System;
public enum OutputMode { Off = 0, On = 3 }
public sealed class OpaqueReferenceCell {
    public IntPtr Native;
    public string Cached;
}
// Original fixture. The bridge must derive pointer roles from actual metadata
// and IL rather than recognizing these names or their MethodDef tokens.
public static unsafe class ScalarOutputsProbe {
    public static void ThrowNested() {
        throw new TypeInitializationException("OriginalFixture", new MissingMethodException("missing original fixture entry"));
    }
    public static void ThrowDeep() {
        Exception error = new MissingMethodException("deep original fixture entry");
        for (int i = 0; i < 12; ++i) error = new Exception("level " + i, error);
        throw error;
    }
    public static void ThrowCycle() {
        var error = new Exception("cyclic original fixture entry");
        typeof(Exception).GetField("_innerException", System.Reflection.BindingFlags.Instance | System.Reflection.BindingFlags.NonPublic).SetValue(error, error);
        throw error;
    }
    public static void Boolean(IntPtr pointer) {
        bool* output = (bool*)(void*)pointer;
        *output = true;
    }
    public static void Mode(int choice, IntPtr pointer) {
        OutputMode* output = (OutputMode*)(void*)pointer;
        if (choice > 0) *output = OutputMode.On;
        else *output = OutputMode.Off;
    }
    public static void Wide(IntPtr pointer, long value) {
        long* output = (long*)(void*)pointer;
        *output = value;
    }
    public static void Pair(IntPtr first, IntPtr second) {
        int* a = (int*)(void*)first;
        int* b = (int*)(void*)second;
        *a = 41;
        *b = 42;
    }
    public static void ReadBeforeWrite(IntPtr pointer) {
        int* output = (int*)(void*)pointer;
        *output = *output + 1;
    }
    public static void Offset(IntPtr pointer) {
        int* output = (int*)(void*)pointer;
        output[1] = 7;
    }
    static void Escape(void* pointer) {}
    public static void Escaping(IntPtr pointer) {
        int* output = (int*)(void*)pointer;
        Escape(output);
        *output = 1;
    }
    public static void Partial(bool choose, IntPtr pointer) {
        int* output = (int*)(void*)pointer;
        if (choose) *output = 1;
    }
}
