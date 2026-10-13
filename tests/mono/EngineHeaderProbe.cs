using System;
using System.Runtime.InteropServices;
// Original fixture for a checked read inside otherwise ordinary managed IL.
public class EngineHeaderProbe {
    public IntPtr Cached;
    public static int Offset = 20;
    public unsafe int Read() {
        if (Cached == IntPtr.Zero) return 0;
        return *(int*)((byte*)(void*)Cached + Offset);
    }
    public unsafe HeaderValue ReadValue() {
        if (Cached == IntPtr.Zero) return HeaderValue.None;
        return (HeaderValue)(*(int*)((byte*)(void*)Cached + Offset));
    }
    public HeaderValue AlreadyValue() { return (HeaderValue)7; }
    public long WideValue() { return 0; }
    public unsafe int Unscoped(IntPtr pointer) { return *(int*)(void*)pointer; }
    public int AlreadyRun() { return 7; }
}
public sealed class DerivedEngineHeaderProbe : EngineHeaderProbe {}

public struct HeaderValue {
    public int Data;
    public static HeaderValue None { get { return default(HeaderValue); } }
    public static implicit operator HeaderValue(int data) { return new HeaderValue { Data = data }; }
}

[StructLayout(LayoutKind.Sequential)]
public unsafe struct FixedNumericProbe {
    public int Tag;
    public fixed float Planes[24];
    public bool Visible;
    public int Tail;
}
public unsafe struct LargeFixedNumericProbe { public fixed byte Bytes[1024]; }
