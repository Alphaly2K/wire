using System;
using System.Runtime.CompilerServices;
using System.Threading;
public static class GuestBridge {
    [MethodImpl(MethodImplOptions.InternalCall)]
    private static extern int GuestCallback(int value);
    [MethodImpl(MethodImplOptions.InternalCall)]
    private static extern int GuestSharedCallback(IntPtr data, int value);
    public static unsafe int SharedRoundTrip(int[] values) {
        fixed (int* data = values) {
            int result = GuestSharedCallback((IntPtr)data, 5);
            values[1] = values[0] + 7;
            return result + values[1];
        }
    }
    [MethodImpl(MethodImplOptions.InternalCall)]
    private static extern void GuestThreadReady(int id);
    [MethodImpl(MethodImplOptions.InternalCall)]
    private static extern int GuestConcurrentCallback(int id);
    private static readonly ManualResetEvent WorkerStart = new ManualResetEvent(false);
    public static int ConcurrentWorker(int id) {
        GuestThreadReady(id);
        if (!WorkerStart.WaitOne(10000)) throw new TimeoutException("worker did not unblock");
        int sum = 0;
        for (int i = 0; i < 100; ++i) {
            sum += GuestConcurrentCallback(id);
            if ((i % 20) == 0) GC.Collect();
        }
        return sum;
    }
    public static int ReleaseWorkers() { WorkerStart.Set(); return 1; }
    [MethodImpl(MethodImplOptions.InternalCall)]
    private static extern int QueuedCallback(int value, int depth);
    public static int QueuedRoundTrip(int value, int depth) {
        int result = QueuedCallback(value, depth);
        if (result == int.MinValue) throw new InvalidOperationException("queued execution owner revoked");
        return result + 3;
    }
    public static int QueuedWorker(int id) {
        int[] keptAcrossNativeWait = new int[] { id, 37 };
        int sum = 0;
        for (int i = 0; i < 50; ++i) sum += QueuedRoundTrip(id * 100 + i, 2);
        if (keptAcrossNativeWait[0] != id || keptAcrossNativeWait[1] != 37)
            throw new InvalidOperationException("queued wait lost interpreter roots");
        return sum;
    }

    public static int MarshalHelperWrite(long p, int value) { System.Runtime.InteropServices.Marshal.WriteInt32(new IntPtr(p), value); return value; }
    public struct Pair { public int Left; public int Right; }
    public static unsafe int BufferPattern(long address, int length) {
        byte* data = (byte*)address;
        int sum = 0;
        for (int i = 0; i < length; i++) { data[i] = (byte)(i * 13 + 7); sum += data[i]; }
        return sum;
    }
    public static unsafe int ReadI32(long p) { return *(int*)p; }
    public static unsafe int WriteI32(long p, int value) { *(int*)p = value; return value; }
    public static unsafe int ReadI8(long p) { return *(sbyte*)p; }
    public static unsafe int ReadU8(long p) { return *(byte*)p; }
    public static unsafe int ReadI16(long p) { return *(short*)p; }
    public static unsafe int ReadU16(long p) { return *(ushort*)p; }
    public static unsafe int WriteI8(long p, int value) { *(byte*)p = (byte)value; return value; }
    public static unsafe int WriteI16(long p, int value) { *(short*)p = (short)value; return value; }
    public static unsafe int WriteI64(long p, long value) { *(long*)p = value; return (int)*(long*)p; }
    public static unsafe int WriteFloat(long p, float value) { *(float*)p = value; return (int)*(float*)p; }
    public static unsafe int WriteDouble(long p, double value) { *(double*)p = value; return (int)*(double*)p; }
    public static unsafe int UpdatePair(long p) { Pair* pair = (Pair*)p; pair->Left += 7; pair->Right *= 2; return pair->Left + pair->Right; }
    public static unsafe int LoadPair(long p) { Pair pair = *(Pair*)p; return pair.Left + pair.Right; }
    public static unsafe int StorePair(long p, int left, int right) { Pair pair; pair.Left = left; pair.Right = right; *(Pair*)p = pair; return left + right; }
    [MethodImpl(MethodImplOptions.InternalCall)]
    private static extern double GuestFloatCallback(double value, float scale, long bias);
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct Wide { public double Value; public long Count; public uint Tag; }
    [MethodImpl(MethodImplOptions.InternalCall)]
    private static extern void GuestWideCallback(ref Wide input, double scale, long bias, out Wide output);
    public static double FloatRoundTrip(double value, float scale, long bias) { return GuestFloatCallback(value, scale, bias) + 0.25; }
    public static Wide WideRoundTrip(Wide input, double scale, long bias) {
        Wide output; GuestWideCallback(ref input, scale, bias, out output); output.Value += 0.5; return output;
    }
    public static double ThrowFloatRoundTrip(double value, float scale, long bias) {
        GuestFloatCallback(value, scale, bias); throw new ArgumentException("owned typed exception");
    }
    private static void CheckedIncrement(ref int value, int depth) {
        if (depth <= 0) return; value += depth; CheckedIncrement(ref value, depth - 1);
    }
    public static int CheckedNestedLocal(int seed) { CheckedIncrement(ref seed, 5); return seed; }
    public static int CheckedNestedWrite(long address, int value) { WriteI32(address, value); return ReadI32(address); }
    public static int CheckedNestedExternal(long address, int value) { return MarshalHelperWrite(address, value); }
    public static int CheckedNestedCached(int value) { return CachedPrimitive(value); }
    public static unsafe int CachedWrite(long address, int value) { *(int*)address = value; return value; }
    public static int CachedPrimitive(int value) { return value + 1; }
    public static int PureCompute(int count) {
        int[] values = new int[256]; for (int i = 0; i < values.Length; i++) values[i] = i * 3 + 1;
        int sum = 7; for (int i = 0; i < count; i++) { sum = unchecked(sum * 33) ^ i; sum = unchecked(sum + values[i & 255]); }
        return sum;
    }
    public static int RoundTrip(int value) { return GuestCallback(value) + 3; }
    public static int PlatformContract() { try { return (int)Environment.OSVersion.Platform; } catch { return -1; } }
    public static int HostFilesystemContract() { try { return System.IO.File.Exists("/etc/hosts") ? 1 : 0; } catch { return -1; } }
    public static string ServiceContractDiagnostics() {
        string text = "";
        try { text += "platform=" + Environment.OSVersion.Platform; } catch (Exception e) { text += "platform_error=" + e.GetType().FullName + ": " + e.Message; }
        try { text += "; file_exists=" + System.IO.File.Exists("/etc/hosts"); } catch (Exception e) { text += "; file_error=" + e.GetType().FullName + ": " + e.Message; }
        return text;
    }
    public static object BoxInt() { return (object)73; }
    public sealed class MetadataInstance {
        public double Echo(float value, ref long count) { return value + count; }
    }
    [MethodImpl(MethodImplOptions.InternalCall)]
    private static extern int GuestDelegateNested(int value);
    [MethodImpl(MethodImplOptions.InternalCall)]
    private static extern int GuestDelegateRetire(int slot);
    public static object NewScalarDelegate(int seed) {
        int counter = seed;
        return new Func<int, int>(value => {
            if (value < 0) throw new ArgumentOutOfRangeException("value");
            counter += value;
            if (seed == 300 && value == 9) return GuestDelegateNested(counter);
            if (seed == 400 && value == 11 && GuestDelegateRetire(3) != 1)
                throw new InvalidOperationException("delegate owner failed retirement");
            return counter;
        });
    }
    private static int domainCounter;
    public static int DomainCounter(int delta) { domainCounter += delta; return domainCounter; }
    public static int DomainRaise() { throw new ArgumentException("domain-owned exception"); }
    public static object NewArray() { return new int[] { 17, 29, 43, 71 }; }
    public static int CheckedArrayLength(int[] values) { return values.Length; }
    public static int CheckedArrayRead(int[] values, int index) { return values[index]; }
    public static int CheckedArrayWrite(int[] values, int index, int value) { values[index] = value; return values[index]; }
    public static unsafe int CheckedArrayByRef(int[] values, int index, int value) {
        fixed (int* cell = &values[index]) { *cell = value; return *cell; }
    }
    public static int CheckedArraySum(int[] values) {
        int sum = 0; for (int i = 0; i < values.Length; ++i) sum += values[i]; return sum;
    }
    public static long CheckedArrayWrongWidth(long[] values, int index) { return values[index]; }

}
