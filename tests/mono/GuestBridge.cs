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
    public static int MarshalHelperWrite(long p, int value) { System.Runtime.InteropServices.Marshal.WriteInt32(new IntPtr(p), value); return value; }
    public struct Pair { public int Left; public int Right; }
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
    public static object NewArray() { return new int[] { 17, 29, 43, 71 }; }
}
