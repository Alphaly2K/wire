using System.Globalization;
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

    private sealed class RemotingFields : MarshalByRefObject {
        public object Payload;
        public int Number;
        public event EventHandler Changed;
        public void Fire() { EventHandler callback = Changed; if (callback != null) callback(this, EventArgs.Empty); }
    }
    private sealed class RootPayload {
        public byte[] Bytes;
        public int Marker;
        public RootPayload(int marker) { Bytes = new byte[1024]; Bytes[511] = (byte)marker; Marker = marker; }
    }
    private struct RootPair { public RootPayload First, Last; }
    private static RootPair staticPair;
    private static RootPayload[] staticPayloads;
    [ThreadStatic] private static RootPayload threadPayload;
    private static class GenericRoots<T> { public static RootPair Pair; }
    public static int StaticRoots() {
        staticPayloads = new RootPayload[1024];
        for (int i = 0; i < staticPayloads.Length; i++) staticPayloads[i] = new RootPayload(i);
        staticPair = new RootPair { First = new RootPayload(1701), Last = new RootPayload(1702) };
        threadPayload = new RootPayload(1703);
        GenericRoots<string>.Pair = new RootPair { First = new RootPayload(1704), Last = new RootPayload(1705) };
        for (int cycle = 0; cycle < 32; cycle++) {
            // Allocation pressure starts incremental steps between stores/reads.
            for (int i = 0; i < 2048; i++) { byte[] garbage = new byte[1024]; garbage[511] = (byte)i; }
            GC.Collect(); GC.WaitForPendingFinalizers();
            for (int i = 0; i < staticPayloads.Length; i++)
                if (staticPayloads[i].Marker != i || staticPayloads[i].Bytes[511] != (byte)i) return 71;
            if (staticPair.First.Marker != 1701 || staticPair.Last.Marker != 1702) return 72;
            if (threadPayload.Marker != 1703) return 73;
            if (GenericRoots<string>.Pair.First.Marker != 1704 || GenericRoots<string>.Pair.Last.Marker != 1705) return 74;
        }
        staticPayloads = null; staticPair = default(RootPair); threadPayload = null;
        GenericRoots<string>.Pair = default(RootPair);
        return 0;
    }

    public static int Collation() {
        CultureInfo[] cultures = { CultureInfo.InvariantCulture, new CultureInfo("en-US"), new CultureInfo("ja-JP") };
        for (int cycle = 0; cycle < 8; cycle++) {
            foreach (CultureInfo culture in cultures) {
                if (culture.CompareInfo.Compare("abc", "abd") >= 0) return 61;
                if (!culture.CompareInfo.IsPrefix("prefix-value", "prefix")) return 62;
                if (culture.CompareInfo.Compare("same", "same") != 0) return 63;
            }
            GC.Collect(); GC.WaitForPendingFinalizers();
        }
        return 0;
    }

    public static int Remoting() {
        RemotingFields value = new RemotingFields();
        object first = new object(), second = new object();
        value.Payload = first;
        if (Interlocked.CompareExchange(ref value.Payload, second, first) != first || value.Payload != second) return 41;
        if (Interlocked.CompareExchange(ref value.Payload, first, first) != second || value.Payload != second) return 42;
        value.Number = 17;
        if (Interlocked.CompareExchange(ref value.Number, 23, 17) != 17 || value.Number != 23) return 43;
        int fired = 0;
        EventHandler handler = (sender, args) => fired++;
        value.Changed += handler; value.Changed += handler;
        value.Fire(); if (fired != 2) return 44;
        value.Changed -= handler; GC.Collect();
        value.Fire(); if (fired != 3 || value.Payload != second) return 45;
        value.Changed -= handler; value.Fire(); if (fired != 3) return 46;
        AppDomain domain = AppDomain.CurrentDomain;
        FieldInfo field = typeof(AppDomain).GetField("AssemblyResolve", BindingFlags.Instance | BindingFlags.NonPublic);
        if (field == null) return 47;
        Delegate original = (Delegate)field.GetValue(domain);
        int baseline = original == null ? 0 : original.GetInvocationList().Length;
        ResolveEventHandler resolve = (sender, args) => null;
        domain.AssemblyResolve += resolve; domain.AssemblyResolve += resolve;
        Delegate combined = (Delegate)field.GetValue(domain);
        if (combined == null || combined.GetInvocationList().Length != baseline + 2) return 48;
        domain.AssemblyResolve -= resolve; GC.Collect();
        combined = (Delegate)field.GetValue(domain);
        if (combined == null || combined.GetInvocationList().Length != baseline + 1) return 49;
        domain.AssemblyResolve -= resolve;
        if (!Object.ReferenceEquals(field.GetValue(domain), original)) return 50;
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


[AttributeUsage(AttributeTargets.Class | AttributeTargets.Field)]
public sealed class ProbeTagAttribute : Attribute {
    public int Number;
    public bool Toggle;
    public ProbeTagAttribute(int number) { Number = number; }
}
[ProbeTag(71, Toggle = true)]
public sealed class ProbeAttrTarget {
    [ProbeTag(89, Toggle = false)] public int Value;
}
public sealed class ProbeThrowAttribute : Attribute {
    public ProbeThrowAttribute() { throw new InvalidOperationException("checked attribute constructor failure"); }
}
[ProbeThrow]
public sealed class ProbeAttrThrowTarget { }

// Object allocation must leave instance constructor side effects untouched.
public sealed class ProbeAllocated {
    public int Number;
    public string Text;
    public ProbeAllocated() { Number = 37; Text = "rooted after constructor"; }
}
public abstract class ProbeAbstractAllocated { }
public struct ProbeValueAllocated { public int Number; }
