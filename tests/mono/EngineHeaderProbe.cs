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
    public unsafe int Unscoped(IntPtr pointer) { return *(int*)(void*)pointer; }
    public int AlreadyRun() { return 7; }
}
public sealed class DerivedEngineHeaderProbe : EngineHeaderProbe {}

[StructLayout(LayoutKind.Sequential)]
public unsafe struct FixedNumericProbe {
    public int Tag;
    public fixed float Planes[24];
    public bool Visible;
    public int Tail;
}
public unsafe struct LargeFixedNumericProbe { public fixed byte Bytes[1024]; }
