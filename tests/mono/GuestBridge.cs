using System;
using System.Runtime.CompilerServices;
public static class GuestBridge {
    [MethodImpl(MethodImplOptions.InternalCall)]
    private static extern int GuestCallback(int value);
    public static int RoundTrip(int value) { return GuestCallback(value) + 3; }
    public static object NewArray() { return new int[] { 17, 29, 43, 71 }; }
}
