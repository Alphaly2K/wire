using System;
using System.IO;
using System.Runtime.InteropServices;
public static class NativeServicesProbe {
    [StructLayout(LayoutKind.Sequential)]
    public struct AttributeData {
        public uint Attributes, CreationLow, CreationHigh, AccessLow, AccessHigh, WriteLow, WriteHigh, SizeHigh, SizeLow;
    }
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, ExactSpelling=true, SetLastError=true)]
    private static extern bool GetFileAttributesExW(string name, int level, ref AttributeData data);
    [DllImport("kernel32.dll", EntryPoint="GetFileAttributesExW", CharSet=CharSet.Unicode, ExactSpelling=true)]
    private static extern long BadSignature(long value);
    private static void Check(bool value, string message) { if (!value) throw new Exception(message); }
    public static string Inventory() {
        System.Text.StringBuilder output = new System.Text.StringBuilder("class\tmethod\tlibrary\tentry\treturn\tparameters\tcharset\tcalling_convention\tlast_error\texact_spelling\n");
        foreach (Type type in typeof(File).Assembly.GetTypes()) {
            foreach (System.Reflection.MethodInfo method in type.GetMethods(System.Reflection.BindingFlags.Public | System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Static | System.Reflection.BindingFlags.DeclaredOnly)) {
                if ((method.Attributes & System.Reflection.MethodAttributes.PinvokeImpl) == 0) continue;
                object[] attributes = method.GetCustomAttributes(typeof(DllImportAttribute), false);
                if (attributes.Length != 1) continue;
                DllImportAttribute import = (DllImportAttribute)attributes[0];
                System.Text.StringBuilder parameters = new System.Text.StringBuilder();
                foreach (System.Reflection.ParameterInfo parameter in method.GetParameters()) {
                    if (parameters.Length != 0) parameters.Append(";");
                    parameters.Append(parameter.ParameterType.FullName);
                    object[] marshal = parameter.GetCustomAttributes(typeof(MarshalAsAttribute), false);
                    if (marshal.Length == 1) parameters.Append("[" + ((MarshalAsAttribute)marshal[0]).Value + "]");
                }
                output.Append(type.FullName + "\t" + method.Name + "\t" + import.Value + "\t" + import.EntryPoint + "\t" + method.ReturnType.FullName + "\t" + parameters + "\t" + import.CharSet + "\t" + import.CallingConvention + "\t" + import.SetLastError + "\t" + import.ExactSpelling + "\n");
            }
        }
        return output.ToString();
    }
    [System.Runtime.CompilerServices.MethodImpl(System.Runtime.CompilerServices.MethodImplOptions.InternalCall)]
    private static extern int CaptureInterpreterWeak(object value);
    public static int InterpreterWeakFactory() {
        byte[] value = new byte[12345];
        Check(CaptureInterpreterWeak(value) == 1, "weak capture");
        GC.Collect();
        return value.Length;
    }
    public static int InterpreterRoots() {
        byte[][] values = new byte[128][];
        for (int i = 0; i < values.Length; ++i) { values[i] = new byte[4096]; values[i][0] = (byte)i; }
        GC.Collect(); GC.WaitForPendingFinalizers(); GC.Collect();
        int sum = 0;
        for (int i = 0; i < values.Length; ++i) sum += values[i][0];
        Check(sum == 8128, "active interpreter reference array was collected");
        System.Text.StringBuilder builder = new System.Text.StringBuilder();
        for (int i = 0; i < 10000; ++i) { builder.Append("abc"); if ((i & 127) == 0) GC.Collect(); }
        string result = builder.ToString();
        Check(result.Length == 30000 && result[29999] == 'c', "interpreter StringBuilder chunks were collected");
        return 1;
    }
    public static int TypedQueries() {
        Check(Marshal.SizeOf(typeof(AttributeData)) == 36, "attribute ABI size");
        AttributeData data = new AttributeData();
        Check(GetFileAttributesExW("C:\\PRESENT.TXT", 0, ref data), "case insensitive file");
        Check(data.SizeHigh == 0 && data.SizeLow == 26 && (data.Attributes & 0x10) == 0, "attributes/size");
        Check(GetFileAttributesExW("C:\\日本語.txt", 0, ref data) && data.SizeLow == 6, "Unicode path");
        data.SizeLow = 0x12345678;
        Check(!GetFileAttributesExW("C:\\missing.txt", 0, ref data), "missing file failed");
        Check(Marshal.GetLastWin32Error() == 2 && data.SizeLow == 0x12345678, "LastError/no partial output");
        Check(!GetFileAttributesExW("C:\\..\\outside", 0, ref data), "escape denied");
        Check(Marshal.GetLastWin32Error() == 5 && data.SizeLow == 0x12345678, "escape LastError/output");
        Check(!GetFileAttributesExW("C:\\present.txt", 7, ref data), "level rejected");
        Check(Marshal.GetLastWin32Error() == 87, "invalid parameter");
        bool rejected = false;
        try { BadSignature(0); } catch (EntryPointNotFoundException) { rejected = true; }
        Check(rejected, "mismatched method signature was admitted");
        return 1;
    }
    public static int ManagedQueries() {
        Check(Environment.OSVersion.Platform == PlatformID.Win32NT, "Windows platform identity");
        Check(Environment.OSVersion.Version == new Version(10, 0, 19045, 0), "wire OS version profile");
        Check(Environment.NewLine == "\r\n", "Windows newline");
        Check(Path.DirectorySeparatorChar == '\\' && Path.AltDirectorySeparatorChar == '/' && Path.VolumeSeparatorChar == ':', "Windows path separators");
        Check(Directory.GetCurrentDirectory() == "C:\\", "wire cwd leaked host path");
        Check(File.Exists("C:\\present.txt"), "real classlib present file");
        Check(File.Exists("present.txt"), "relative path uses wire cwd");
        Check(File.Exists("C:\\日本語.txt"), "real classlib Unicode file");
        Check(!File.Exists("C:\\missing.txt"), "real classlib missing file");
        Check(!File.Exists("/etc/hosts"), "host filesystem leaked");
        Check(Directory.Exists("C:\\sub"), "real classlib directory");
        return 1;
    }
    public static int ManagedStreams() {
        using (FileStream input = new FileStream("present.txt", FileMode.Open, FileAccess.Read, FileShare.Read)) {
            Check(input.Length == 26, "stream length");
            byte[] bytes = new byte[40];
            Check(input.Read(bytes, 3, 30) == 26 && bytes[3] == (byte)'t' && bytes[28] == (byte)'\n', "stream read/offset/EOF");
            Check(input.Read(bytes, 0, 1) == 0, "stream EOF");
            Check(input.Seek(-8, SeekOrigin.End) == 18 && input.ReadByte() == (byte)'s', "stream seek");
        }
        byte[] payload = new byte[180123];
        for (int i = 0; i < payload.Length; i++) payload[i] = (byte)(i * 13 + 7);
        using (FileStream output = new FileStream("C:\\日本語-stream.bin", FileMode.Create, FileAccess.Write, FileShare.None)) {
            output.Write(payload, 0, payload.Length); output.Flush();
            Check(output.Length == payload.Length, "large write lost a partial chunk");
        }
        GC.Collect();
        using (FileStream input = new FileStream("C:\\日本語-stream.bin", FileMode.Open, FileAccess.Read, FileShare.Read)) {
            int offset = 0;
            byte[] actual = new byte[payload.Length];
            while (offset < actual.Length) { int count = input.Read(actual, offset, actual.Length - offset); Check(count > 0, "premature EOF"); offset += count; GC.Collect(); }
            for (int i = 0; i < actual.Length; i++) Check(actual[i] == payload[i], "large read mismatch");
        }
        using (FileStream append = new FileStream("C:\\日本語-stream.bin", FileMode.Append, FileAccess.Write, FileShare.None)) {
            append.WriteByte(99);
        }
        byte[] all = File.ReadAllBytes("C:\\日本語-stream.bin");
        Check(all.Length == payload.Length + 1 && all[all.Length - 1] == 99, "append/File.ReadAllBytes");
        Type monoIO = typeof(File).Assembly.GetType("System.IO.MonoIO", true);
        System.Reflection.MethodInfo read = monoIO.GetMethod("Read", System.Reflection.BindingFlags.Static | System.Reflection.BindingFlags.NonPublic, null,
            new Type[] { typeof(IntPtr), typeof(byte[]), typeof(int), typeof(int), typeof(File).Assembly.GetType("System.IO.MonoIOError").MakeByRefType() }, null);
        Check(read != null, "MonoIO Read signature");
        byte[] sentinel = new byte[] { 31, 32, 33, 34 };
        object[] arguments = new object[] { new IntPtr(0xdead), sentinel, 1, 2, null };
        Check((int)read.Invoke(null, arguments) == -1 && Convert.ToInt32(arguments[4]) == 6, "invalid handle error");
        Check(sentinel[0] == 31 && sentinel[1] == 32 && sentinel[2] == 33 && sentinel[3] == 34, "invalid handle partially wrote buffer");
        arguments = new object[] { new IntPtr(0xdead), sentinel, 3, 2, null };
        Check((int)read.Invoke(null, arguments) == -1 && Convert.ToInt32(arguments[4]) == 87, "invalid array range");
        bool missing = false; try { using (FileStream file = File.OpenRead("C:\\missing.bin")) {} } catch (FileNotFoundException) { missing = true; }
        Check(missing, "missing stream error");
        bool host = false; try { using (FileStream file = File.OpenRead("/etc/hosts")) {} } catch (IOException) { host = true; }
        Check(host, "host stream leaked");
        return 1;
    }
    [System.Runtime.CompilerServices.MethodImpl(System.Runtime.CompilerServices.MethodImplOptions.InternalCall)]
    private static extern int ThreadBarrier();
    public static int ThreadWorker(int id) {
        AttributeData data = new AttributeData();
        Check(!GetFileAttributesExW(id == 1 ? "C:\\missing.txt" : "C:\\present.txt", id == 1 ? 0 : 7, ref data), "worker error");
        Check(ThreadBarrier() == 1, "worker barrier timeout");
        Check(Marshal.GetLastWin32Error() == (id == 1 ? 2 : 87), "LastError leaked between native Mono threads");
        return 1;
    }
    public static int ManagedThreadStreams() {
        Check(Directory.GetCurrentDirectory() == "C:\\", "queued cwd");
        byte[] expected = File.ReadAllBytes("C:\\present.txt");
        Check(expected.Length == 26, "queued expected bytes");
        Exception failure = null;
        int[] totals = new int[2];
        int[] identities = new int[2];
        object errorLock = new object();
        using (FileStream shared = File.OpenRead("C:\\present.txt")) {
            System.Threading.Thread[] workers = new System.Threading.Thread[2];
            for (int worker = 0; worker < 2; worker++) {
                int id = worker;
                workers[worker] = new System.Threading.Thread(() => {
                    try {
                        identities[id] = System.Threading.Thread.CurrentThread.ManagedThreadId;
                        Check(Directory.GetCurrentDirectory() == "C:\\" && File.Exists("C:\\present.txt"), "queued worker VFS");
                        AttributeData data = new AttributeData();
                        data.SizeLow = 0x12345678;
                        Check(!GetFileAttributesExW(id == 0 ? "C:\\missing.txt" : "C:\\present.txt", id == 0 ? 0 : 7, ref data), "queued error query");
                        Check(Marshal.GetLastWin32Error() == (id == 0 ? 2 : 87) && data.SizeLow == 0x12345678, "queued LastError/no partial output");
                        byte[] actual = new byte[13];
                        lock (shared) {
                            shared.Seek(id * 13, SeekOrigin.Begin);
                            Check(shared.Read(actual, 0, 13) == 13, "cross-thread shared handle read");
                        }
                        for (int i = 0; i < 13; i++) {
                            Check(actual[i] == expected[id * 13 + i], "queued byte mismatch");
                            totals[id] += actual[i];
                        }
                        byte[] sentinel = new byte[] { 31, 32, 33, 34 };
                        Type monoIO = typeof(File).Assembly.GetType("System.IO.MonoIO", true);
                        System.Reflection.MethodInfo read = monoIO.GetMethod("Read", System.Reflection.BindingFlags.Static | System.Reflection.BindingFlags.NonPublic, null,
                            new Type[] { typeof(IntPtr), typeof(byte[]), typeof(int), typeof(int), typeof(File).Assembly.GetType("System.IO.MonoIOError").MakeByRefType() }, null);
                        object[] arguments = new object[] { new IntPtr(0xdead), sentinel, 1, 2, null };
                        Check((int)read.Invoke(null, arguments) == -1 && Convert.ToInt32(arguments[4]) == 6, "queued invalid handle error");
                        Check(sentinel[0] == 31 && sentinel[1] == 32 && sentinel[2] == 33 && sentinel[3] == 34, "queued invalid handle partial write");
                        GC.Collect();
                    } catch (Exception error) { lock (errorLock) { failure = error; } }
                });
                workers[worker].Start();
            }
            foreach (System.Threading.Thread worker in workers) Check(worker.Join(10000), "queued worker join timeout");
        }
        if (failure != null) throw new Exception("managed-created service thread: " + failure.Message);
        Check(identities[0] != identities[1], "queued worker identities alias");
        int first = 0, second = 0;
        for (int i = 0; i < 13; i++) { first += expected[i]; second += expected[13 + i]; }
        Check(totals[0] == first && totals[1] == second, "queued checksums");
        return 1;
    }
    public static int NoOwner() {
        AttributeData data = new AttributeData();
        Check(!GetFileAttributesExW("C:\\present.txt", 0, ref data), "ownerless entry used filesystem");
        Check(Marshal.GetLastWin32Error() == 50, "ownerless error");
        bool failed = false; try { using (FileStream input = File.OpenRead("C:\\present.txt")) {} } catch (IOException) { failed = true; }
        Check(failed, "ownerless cached MonoIO used filesystem");
        return 1;
    }
}
