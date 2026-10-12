// Read metadata and IL references only. Never load or execute game assemblies
// with the CLR; a declaration or scalar shape does not admit a native ABI.
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using Mono.Cecil;

class BridgeInventory {
    static IEnumerable<TypeDefinition> Types(IEnumerable<TypeDefinition> types) {
        foreach (var type in types) {
            yield return type;
            foreach (var nested in Types(type.NestedTypes)) yield return nested;
        }
    }
    static string Safe(string value) {
        return (value ?? "").Replace("\t", "\\t").Replace("\r", "\\r").Replace("\n", "\\n");
    }
    static string Hash(string path) {
        using (var input = File.OpenRead(path)) using (var sha = SHA256.Create())
            return BitConverter.ToString(sha.ComputeHash(input)).Replace("-", "").ToLowerInvariant();
    }
    static string Shape(TypeReference type, HashSet<string> owned, HashSet<string> seen, int depth) {
        if (type is ByReferenceType) return "byref:" + Shape(((ByReferenceType)type).ElementType, owned, seen, depth);
        if (type is PointerType || type.MetadataType == MetadataType.FunctionPointer) return "raw-pointer";
        if (type is ArrayType) return "array:" + Shape(((ArrayType)type).ElementType, owned, seen, depth);
        if (type.ContainsGenericParameter) return "open-generic";
        switch (type.MetadataType) {
            case MetadataType.Void: return "void";
            case MetadataType.Boolean: return "bool";
            case MetadataType.Char: return "u16";
            case MetadataType.SByte: return "i8";
            case MetadataType.Byte: return "u8";
            case MetadataType.Int16: return "i16";
            case MetadataType.UInt16: return "u16";
            case MetadataType.Int32: return "i32";
            case MetadataType.UInt32: return "u32";
            case MetadataType.Int64: return "i64";
            case MetadataType.UInt64: return "u64";
            case MetadataType.Single: return "f32";
            case MetadataType.Double: return "f64";
            case MetadataType.IntPtr: case MetadataType.UIntPtr: return "native-int";
            case MetadataType.String: return "string";
        }
        if (depth > 12 || !seen.Add(type.FullName)) return "recursive-or-deep";
        try {
            var definition = type.Resolve();
            if (definition == null || !owned.Contains(definition.Module.FileName)) return "unresolved";
            if (definition.IsEnum) {
                var storage = definition.Fields.Single(field => field.Name == "value__");
                return "enum:" + Shape(storage.FieldType, owned, seen, depth + 1);
            }
            if (!definition.IsValueType) return "managed-reference";
            var fields = definition.Fields.Where(field => !field.IsStatic).Select(field =>
                field.Name + "=" + Shape(field.FieldType, owned, new HashSet<string>(seen), depth + 1));
            return "value{" + String.Join(",", fields.ToArray()) + "}";
        } catch { return "unresolved"; }
    }
    static bool Scalar(string shape) {
        if (shape.StartsWith("enum:")) shape = shape.Substring(5);
        return new[] {"void", "bool", "i8", "u8", "i16", "u16", "i32", "u32", "i64", "u64", "f32", "f64"}.Contains(shape);
    }
    static void Main(string[] args) {
        if (args.Length != 2) throw new ArgumentException("Managed directory and output directory required");
        var input = Path.GetFullPath(args[0]);
        var output = Path.GetFullPath(args[1]);
        Directory.CreateDirectory(output);
        var resolver = new DefaultAssemblyResolver();
        foreach (var search in resolver.GetSearchDirectories()) resolver.RemoveSearchDirectory(search);
        resolver.AddSearchDirectory(input);
        var files = Directory.GetFiles(input, "*.dll").OrderBy(path => path, StringComparer.Ordinal).ToArray();
        var owned = new HashSet<string>(files);
        var assemblies = new List<AssemblyDefinition>();
        var references = new Dictionary<string, int>();
        using (var failures = new StreamWriter(Path.Combine(output, "metadata-errors.tsv")))
        using (var hashes = new StreamWriter(Path.Combine(output, "assembly-inputs.tsv"))) {
            failures.WriteLine("assembly\treason");
            hashes.WriteLine("assembly\tsha256");
            foreach (var file in files) {
                hashes.WriteLine(Safe(Path.GetFileName(file)) + "\t" + Hash(file));
                try {
                    assemblies.Add(AssemblyDefinition.ReadAssembly(file, new ReaderParameters {
                        AssemblyResolver = resolver, ReadingMode = ReadingMode.Deferred, ReadSymbols = false
                    }));
                } catch (Exception error) {
                    failures.WriteLine(Safe(Path.GetFileName(file)) + "\t" + Safe(error.Message));
                }
            }
            foreach (var assembly in assemblies) foreach (var type in Types(assembly.MainModule.Types))
                foreach (var method in type.Methods) if (method.HasBody) foreach (var instruction in method.Body.Instructions) {
                    var reference = instruction.Operand as MethodReference;
                    if (reference == null) continue;
                    int count;
                    references.TryGetValue(reference.FullName, out count);
                    references[reference.FullName] = count + 1;
                }
        }
        int icalls = 0, pinvokes = 0;
        using (var declarations = new StreamWriter(Path.Combine(output, "managed-declarations.tsv"))) {
            declarations.WriteLine("assembly\tclass\tmethod\ttoken\tkind\treturn\tparameters\treturn_shape\tparameter_shapes\tstatic\tgeneric\tlibrary\tentry\tpinvoke_flags\til_references\tscalar_candidate\tfull_signature\tparameter_modes\tparameter_names");
            foreach (var assembly in assemblies) foreach (var type in Types(assembly.MainModule.Types))
                foreach (var method in type.Methods) {
                    if (!method.IsInternalCall && !method.IsPInvokeImpl) continue;
                    var result = Shape(method.ReturnType, owned, new HashSet<string>(), 0);
                    var parameters = method.Parameters.Select(parameter => Shape(parameter.ParameterType, owned, new HashSet<string>(), 0)).ToArray();
                    var pinvoke = method.PInvokeInfo;
                    int refs;
                    references.TryGetValue(method.FullName, out refs);
                    if (method.IsInternalCall) icalls++; else pinvokes++;
                    var row = new[] {
                        Path.GetFileName(assembly.MainModule.FileName), type.FullName, method.Name,
                        "0x" + method.MetadataToken.ToUInt32().ToString("x8"), method.IsInternalCall ? "InternalCall" : "PInvoke",
                        method.ReturnType.FullName, String.Join(";", method.Parameters.Select(parameter => parameter.ParameterType.FullName).ToArray()),
                        result, String.Join(";", parameters), method.IsStatic ? "1" : "0",
                        method.HasGenericParameters || type.HasGenericParameters ? "1" : "0",
                        pinvoke == null ? "" : pinvoke.Module.Name, pinvoke == null ? "" : pinvoke.EntryPoint,
                        pinvoke == null ? "" : "0x" + ((int)pinvoke.Attributes).ToString("x4"), refs.ToString(),
                        method.IsStatic && !method.HasGenericParameters && !type.HasGenericParameters && parameters.Length <= 4 && Scalar(result) && parameters.All(Scalar) ? "1" : "0",
                        method.FullName,
                        String.Join(";", method.Parameters.Select(parameter => parameter.IsOut && !parameter.IsIn ? "out" : "inout").ToArray()),
                        String.Join(";", method.Parameters.Select(parameter => parameter.Name).ToArray())
                    };
                    declarations.WriteLine(String.Join("\t", row.Select(Safe).ToArray()));
                }
        }
        foreach (var assembly in assemblies) assembly.Dispose();
        resolver.Dispose();
        Console.WriteLine("BRIDGE_METADATA assemblies=" + assemblies.Count + " icalls=" + icalls + " pinvokes=" + pinvokes + " game_methods_executed=0");
    }
}
