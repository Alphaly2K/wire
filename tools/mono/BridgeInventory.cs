// Read metadata and IL references only. Never load or execute game assemblies
// with the CLR; a declaration or scalar shape does not admit a native ABI.
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using Mono.Cecil;
using Mono.Cecil.Cil;

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
    // Substitute the actual closed class, including nested arrays/values. A
    // definition's T must never be mistaken for the instantiated field type.
    static TypeReference Close(TypeReference type, TypeReference owner) {
        var instance = owner as GenericInstanceType;
        var parameter = type as GenericParameter;
        if (parameter != null && parameter.Type == GenericParameterType.Type && instance != null
            && parameter.Position < instance.GenericArguments.Count
            && parameter.Owner is TypeReference
            && ((TypeReference)parameter.Owner).FullName == instance.ElementType.FullName)
            return instance.GenericArguments[parameter.Position];
        if (type is ArrayType) {
            var array = (ArrayType)type;
            return new ArrayType(Close(array.ElementType, owner), array.Rank);
        }
        if (type is ByReferenceType) return new ByReferenceType(Close(((ByReferenceType)type).ElementType, owner));
        if (type is PointerType) return new PointerType(Close(((PointerType)type).ElementType, owner));
        if (type is GenericInstanceType) {
            var generic = (GenericInstanceType)type;
            var closed = new GenericInstanceType(generic.ElementType);
            foreach (var argument in generic.GenericArguments) closed.GenericArguments.Add(Close(argument, owner));
            return closed;
        }
        return type;
    }
    static bool Delegate(TypeDefinition definition) {
        var parent = definition.BaseType;
        return parent != null && (parent.FullName == "System.MulticastDelegate" || parent.FullName == "System.Delegate");
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
            if (Delegate(definition)) return "delegate";
            if (!definition.IsValueType) return "managed-reference";
            var fields = new List<string>();
            int length = 0;
            foreach (var field in definition.Fields.Where(field => !field.IsStatic)) {
                var shape = field.Name + "=" + Shape(Close(field.FieldType, type), owned, new HashSet<string>(seen), depth + 1);
                length += shape.Length + 1;
                if (length > 8192) return "recursive-or-deep";
                fields.Add(shape);
            }
            return "value{" + String.Join(",", fields.ToArray()) + "}";
        } catch { return "unresolved"; }
    }
    static bool Scalar(string shape) {
        if (shape.StartsWith("enum:")) shape = shape.Substring(5);
        return new[] {"void", "bool", "i8", "u8", "i16", "u16", "i32", "u32", "i64", "u64", "f32", "f64"}.Contains(shape);
    }
    static void Row(StreamWriter stream, params string[] cells) {
        stream.WriteLine(String.Join("\t", cells.Select(Safe).ToArray()));
    }
    static string Identity(TypeReference type) {
        try {
            var definition = type.Resolve();
            if (definition != null) return definition.Module.FileName + ":" + type.FullName;
        } catch { }
        return (type.Scope == null ? "" : type.Scope.Name) + ":" + type.FullName;
    }
    static bool Attribute(FieldDefinition field, string name) {
        return field.CustomAttributes.Any(attribute => attribute.AttributeType.FullName == name);
    }
    static string GraphPolicy(TypeDefinition definition, HashSet<string> owned) {
        if (Delegate(definition)) return "opaque-delegate";
        var cursor = definition;
        var seen = new HashSet<string>();
        while (cursor != null && seen.Count < 64 && seen.Add(cursor.FullName)) {
            if (cursor.FullName == "UnityEngine.Object" && owned.Contains(cursor.Module.FileName)) return "engine-object";
            try { cursor = cursor.BaseType == null ? null : cursor.BaseType.Resolve(); } catch { cursor = null; }
        }
        return definition.IsSerializable ? "serializable-data" : "native-only";
    }
    static string Protocol(string shape) {
        if (shape.Contains("raw-pointer") || shape.Contains("native-int")) return "pointer-lifetime";
        if (shape.Contains("open-generic") || shape.Contains("unresolved") || shape.Contains("recursive-or-deep")) return "unresolved-layout";
        if (shape.Contains("delegate")) return "callback-lifetime";
        if (shape.Contains("managed-reference") || shape.Contains("array:") || shape.Contains("string")) return "reference-barrier";
        return "scalar-value";
    }
    static void ProtocolInventory(string output, List<AssemblyDefinition> assemblies, HashSet<string> owned) {
        var queue = new Queue<Tuple<TypeReference, int>>();
        var nativeNames = new HashSet<string>(assemblies.SelectMany(assembly => Types(assembly.MainModule.Types))
            .SelectMany(type => type.Methods).Where(method => method.IsInternalCall || method.IsPInvokeImpl)
            .Select(method => method.DeclaringType.FullName + "::" + method.Name));
        Action<TypeReference> seed = type => { if (type != null) queue.Enqueue(Tuple.Create(type, 0)); };
        using (var calls = new StreamWriter(Path.Combine(output, "native-wrappers.tsv")))
        using (var unsafeUses = new StreamWriter(Path.Combine(output, "unsafe-il.tsv"))) {
            Row(calls, "assembly", "caller", "offset", "opcode", "callee", "generic_arguments", "return_shape", "parameter_shapes", "converters");
            Row(unsafeUses, "assembly", "method", "offset", "opcode", "operand", "shape");
            foreach (var assembly in assemblies) foreach (var type in Types(assembly.MainModule.Types)) {
                var file = Path.GetFileName(assembly.MainModule.FileName);
                seed(type); seed(type.BaseType);
                foreach (var iface in type.Interfaces) seed(iface.InterfaceType);
                foreach (var field in type.Fields) seed(field.FieldType);
                foreach (var method in type.Methods) {
                    seed(method.ReturnType);
                    foreach (var parameter in method.Parameters) seed(parameter.ParameterType);
                    if (!method.HasBody) continue;
                    foreach (var local in method.Body.Variables) seed(local.VariableType);
                    var converters = String.Join(";", method.Body.Instructions.Select(instruction => instruction.Operand as MethodReference)
                        .Where(reference => reference != null && (reference.DeclaringType.FullName.Contains("BindingsMarshaller")
                            || reference.DeclaringType.FullName.Contains("MarshalledUnityObject")
                            || reference.Name.Contains("UnmarshalUnityObject") || reference.Name.Contains("FunctionPointerForDelegate")))
                        .Select(reference => reference.FullName).Distinct().ToArray());
                    foreach (var instruction in method.Body.Instructions) {
                        var reference = instruction.Operand as MethodReference;
                        var field = instruction.Operand as FieldReference;
                        var operandType = instruction.Operand as TypeReference;
                        string shape = "";
                        if (reference != null) {
                            seed(reference.DeclaringType); seed(reference.ReturnType);
                            foreach (var parameter in reference.Parameters) seed(parameter.ParameterType);
                            var generic = reference as GenericInstanceMethod;
                            if (generic != null) foreach (var argument in generic.GenericArguments) seed(argument);
                            if (nativeNames.Contains(reference.DeclaringType.FullName + "::" + reference.Name)) {
                                MethodDefinition target = null;
                                try { target = reference.Resolve(); } catch { }
                                if (target != null && owned.Contains(target.Module.FileName) && (target.IsInternalCall || target.IsPInvokeImpl))
                                    Row(calls, file, method.FullName, instruction.Offset.ToString(), instruction.OpCode.Name,
                                        reference.FullName, generic == null ? "" : String.Join(";", generic.GenericArguments.Select(argument => argument.FullName).ToArray()),
                                        Shape(method.ReturnType, owned, new HashSet<string>(), 0),
                                        String.Join(";", method.Parameters.Select(parameter => Shape(parameter.ParameterType, owned, new HashSet<string>(), 0)).ToArray()), converters);
                            }
                        } else if (field != null) {
                            seed(field.DeclaringType); seed(field.FieldType);
                            shape = Shape(Close(field.FieldType, field.DeclaringType), owned, new HashSet<string>(), 0);
                        } else if (operandType != null) {
                            seed(operandType);
                            shape = Shape(operandType, owned, new HashSet<string>(), 0);
                        }
                        var code = instruction.OpCode.Code;
                        if (shape.Contains("raw-pointer") || shape.Contains("native-int") || shape.Contains("delegate")
                            || instruction.OpCode.Name.StartsWith("ldind.") || instruction.OpCode.Name.StartsWith("stind.")
                            || code == Code.Cpblk || code == Code.Initblk || code == Code.Calli || code == Code.Localloc
                            || code == Code.Ldftn || code == Code.Ldvirtftn)
                            Row(unsafeUses, file, method.FullName, instruction.Offset.ToString(), instruction.OpCode.Name,
                                instruction.Operand == null ? "" : instruction.Operand.ToString(), shape);
                    }
                }
            }
        }
        var seen = new HashSet<string>();
        using (var types = new StreamWriter(Path.Combine(output, "managed-types.tsv")))
        using (var fields = new StreamWriter(Path.Combine(output, "managed-fields.tsv")))
        using (var callbacks = new StreamWriter(Path.Combine(output, "managed-callbacks.tsv"))) {
            Row(types, "assembly", "type", "definition", "value", "serializable", "delegate", "layout", "packing", "size", "base", "status", "graph_policy");
            Row(fields, "assembly", "owner", "definition", "field", "token", "type", "shape", "protocol", "static", "readonly", "nonserialized", "visibility", "serialization_candidate", "explicit_offset", "graph_policy");
            Row(callbacks, "assembly", "type", "signature", "return_shape", "parameter_shapes");
            while (queue.Count != 0) {
                var item = queue.Dequeue();
                var type = item.Item1;
                if (!seen.Add(Identity(type))) continue;
                if (seen.Count > 100000) throw new InvalidDataException("Type inventory exceeded bounded capacity");
                var generic = type as GenericInstanceType;
                if (generic != null) foreach (var argument in generic.GenericArguments) seed(argument);
                if (type is TypeSpecification && generic == null) {
                    seed(((TypeSpecification)type).ElementType);
                    continue;
                }
                TypeDefinition definition = null;
                string status = type.ContainsGenericParameter ? "open-generic" : "resolved";
                try { definition = type.Resolve(); } catch { status = "unresolved"; }
                if (definition == null || !owned.Contains(definition.Module.FileName)) status = "unresolved";
                if (item.Item2 > 12) status = "deep";
                if (status == "unresolved" || status == "deep") {
                    Row(types, type.Scope == null ? "" : type.Scope.Name, type.FullName, "", "", "", "", "", "", "", "", status, "unknown");
                    continue;
                }
                var file = Path.GetFileName(definition.Module.FileName);
                var policy = GraphPolicy(definition, owned);
                Row(types, file, type.FullName, definition.FullName, definition.IsValueType ? "1" : "0",
                    definition.IsSerializable ? "1" : "0", Delegate(definition) ? "1" : "0",
                    definition.IsExplicitLayout ? "explicit" : definition.IsSequentialLayout ? "sequential" : "auto",
                    definition.PackingSize.ToString(), definition.ClassSize.ToString(),
                    definition.BaseType == null ? "" : Close(definition.BaseType, type).FullName, status, policy);
                if (Delegate(definition)) foreach (var method in definition.Methods.Where(method => method.Name == "Invoke"))
                    Row(callbacks, file, type.FullName, method.FullName,
                        Shape(Close(method.ReturnType, type), owned, new HashSet<string>(), 0),
                        String.Join(";", method.Parameters.Select(parameter => Shape(Close(parameter.ParameterType, type), owned, new HashSet<string>(), 0)).ToArray()));
                if (definition.BaseType != null) queue.Enqueue(Tuple.Create(Close(definition.BaseType, type), item.Item2 + 1));
                foreach (var field in definition.Fields) {
                    var actual = Close(field.FieldType, type);
                    var shape = Shape(actual, owned, new HashSet<string>(), 0);
                    bool selected = !field.IsStatic && !field.IsLiteral && !field.IsInitOnly && !field.IsNotSerialized
                        && (field.IsPublic || Attribute(field, "UnityEngine.SerializeField") || Attribute(field, "UnityEngine.SerializeReference"));
                    Row(fields, file, type.FullName, definition.FullName, field.Name,
                        "0x" + field.MetadataToken.ToUInt32().ToString("x8"), actual.FullName, shape, Protocol(shape),
                        field.IsStatic ? "1" : "0", field.IsInitOnly ? "1" : "0", field.IsNotSerialized ? "1" : "0",
                        (field.Attributes & FieldAttributes.FieldAccessMask).ToString(), selected ? "1" : "0", field.Offset.ToString(), policy);
                    queue.Enqueue(Tuple.Create(actual, item.Item2 + 1));
                }
            }
        }
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
        ProtocolInventory(output, assemblies, owned);
        foreach (var assembly in assemblies) assembly.Dispose();
        resolver.Dispose();
        Console.WriteLine("BRIDGE_METADATA assemblies=" + assemblies.Count + " icalls=" + icalls + " pinvokes=" + pinvokes + " game_methods_executed=0");
    }
}
