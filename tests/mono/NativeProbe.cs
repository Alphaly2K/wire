// Original fixture; compiled locally with the installed Unity compiler.
using System;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Threading;

[StructLayout(LayoutKind.Sequential)]
public struct ProbePair {
    public double X;
    public long Y;
}

public static class NativeProbe {
    private static int finalizerCount;
    private sealed class Finalizable {
        ~Finalizable() { Interlocked.Increment(ref finalizerCount); }
    }
    [MethodImpl(MethodImplOptions.InternalCall)]
    private static extern double HostScale(double value, int factor);
    [MethodImpl(MethodImplOptions.InternalCall)]
    private static extern long HostRef(ref ProbePair value, out int result);
    [MethodImpl(MethodImplOptions.InternalCall)]
    private static extern int HostBounce(int value);

    [DllImport("wire_mono_probe", CallingConvention = CallingConvention.Cdecl)]
    private static extern ProbePair wire_probe_pair(ProbePair value, double increment);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate double Callback(double value, int factor);
    [DllImport("wire_mono_probe", CallingConvention = CallingConvention.Cdecl)]
    private static extern double wire_probe_callback(Callback callback, double value, int factor);

    private static T Identity<T>(T value) { return value; }
    public static int Reflected(int value) { return value * 7 + 3; }
    public static int Reenter(int value) {
        if (value < 0) throw new ArgumentException("owned boundary exception");
        return value + 11;
    }
    private static double ReverseCallback(double value, int factor) {
        return value * factor + 0.25;
    }

    public static int Core() {
        long sum = 0;
        int[] values = new int[257];
        for (int i = 0; i < values.Length; i++) values[i] = Identity(i * 3 + 1);
        foreach (int value in values) sum += value;
        if (sum != 98945 || Identity("generic") != "generic") return 1;
        Func<int, int> closure = value => value + values[17];
        if (closure(9) != 61) return 2;
        MethodInfo method = typeof(NativeProbe).GetMethod("Reflected");
        if ((int)method.Invoke(null, new object[] { 13 }) != 94) return 3;
        bool caught = false;
        try { Reenter(-1); } catch (ArgumentException) { caught = true; }
        if (!caught) return 4;
        for (int i = 0; i < 10000; i++) {
            object[] allocation = new object[] { i, new byte[128] };
            if ((int)allocation[0] != i) return 5;
        }
        GC.Collect();
        GC.WaitForPendingFinalizers();
        if (closure(9) != 61) return 6;
        return 0;
    }

    public static int Interop() {
        if (HostScale(1.5, 4) != 6.0) return 11;
        ProbePair pair = new ProbePair { X = 2.5, Y = 31 };
        int result;
        if (HostRef(ref pair, out result) != 39 || pair.X != 3.75 || pair.Y != 38 || result != 73) return 12;
        if (HostBounce(5) != 16 || HostBounce(-1) != -777) return 13;
        ProbePair returned = wire_probe_pair(pair, 0.5);
        if (returned.X != 4.25 || returned.Y != 40) return 14;
        Callback callback = ReverseCallback;
        if (wire_probe_callback(callback, 2.0, 3) != 6.25) return 15;
        GC.KeepAlive(callback);
        return 0;
    }

    public static int Worker(int seed) {
        for (int i = 0; i < 100; i++) {
            if (HostBounce(seed) != seed + 11) return 21;
            if (i % 10 == 0) GC.Collect();
            Thread.Sleep(1);
        }
        return 0;
    }

    public static object NewObject() { return new byte[65536]; }
    public static object NewFinalizable() { return new Finalizable(); }
    public static int Finalized() { GC.WaitForPendingFinalizers(); return finalizerCount; }
    public static int ThrowBoundary() { throw new InvalidOperationException("owned top-level exception"); }
    public static int Main() { return Core(); }
}
