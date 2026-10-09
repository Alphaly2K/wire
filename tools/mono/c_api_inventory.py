#!/usr/bin/env python3
"""Extract compiler-checked candidate C contracts, never game ABI admission."""
import argparse
import csv
import json
from pathlib import Path
import re
import shlex
import subprocess
from probe import HERE, REPO, sha256

HEADERS = ["mono/mini/jit.h", "mono/metadata/assembly.h", "mono/metadata/mono-config.h",
           "mono/metadata/mono-gc.h", "mono/metadata/object.h", "mono/metadata/threads.h",
           "mono/metadata/appdomain.h", "mono/metadata/class.h", "mono/metadata/debug-helpers.h",
           "mono/metadata/image.h", "mono/metadata/reflection.h", "mono/metadata/attrdefs.h",
           "mono/metadata/unity-utils.h", "mono/metadata/mono-debug.h", "mono/metadata/profiler.h",
           "mono/metadata/profiler-legacy.h", "mono/metadata/exception.h", "mono/utils/mono-logger.h",
           "mono/utils/mono-publib.h", "mono/mini/mini.h", "mono/mini/mini-runtime.h",
           "mono/metadata/unity-memory-info.h", "mono/metadata/verify-internals.h"]
DEFINITIONS = ["mono/metadata/unity-utils.c", "mono/metadata/unity-liveness.c",
               "mono/utils/mono-publib.c", "mono/utils/mono-logger.c",
               "mono/metadata/oop.c", "mono/mini/mini-unity.c"]
FOCUS = {"MonoAllocatorVTable", "MonoMemoryCallbacks", "BurstMonoDebuggerCallbacks",
         "UnityFindPluginCallback", "MonoLogCallback", "vprintf_func", "unitytls_interface_struct"}


def walk(node):
    yield node
    for child in node.get("inner", []):
        yield from walk(child)


def canonical(spelling, aliases):
    seen = set()
    while spelling in aliases and spelling not in seen:
        seen.add(spelling)
        spelling = aliases[spelling]
    return spelling


def shape(spelling, aliases):
    resolved = canonical(spelling, aliases)
    if re.search(r"\(\s*\*", resolved):
        kind = "callback_pointer"
    elif "*" in resolved:
        kind = "data_pointer"
    elif resolved == "void":
        kind = "void"
    elif resolved in {"float", "double"}:
        kind = "floating_scalar"
    elif resolved in {"_Bool", "bool", "char", "signed char", "unsigned char", "short", "unsigned short",
                      "int", "unsigned int", "long", "unsigned long", "long long", "unsigned long long"}:
        kind = "integer_scalar"
    elif resolved.startswith("enum "):
        kind = "enum_requires_width_check"
    else:
        kind = "aggregate_or_unresolved"
    return {"spelling": spelling, "canonical": resolved, "kind": kind,
            "needs_win64_width_check": bool(re.search(r"\blong\b|\bwchar_t\b", resolved)),
            "ownership_and_guest_marshaling_verified": False}


def inventory(ast):
    nodes = list(walk(ast))
    by_id = {node["id"]: node for node in nodes if "id" in node}
    aliases = {node["name"]: node["type"].get("desugaredQualType", node["type"]["qualType"])
               for node in nodes if node.get("kind") == "TypedefDecl" and "type" in node}
    functions = {}
    for node in nodes:
        if node.get("kind") != "FunctionDecl" or not node.get("name", "").startswith(("mono_", "unity_mono_", "burst_mono_")):
            continue
        signature = node["type"]["qualType"]
        params = [{"name": child.get("name", ""), **shape(child["type"].get("desugaredQualType", child["type"]["qualType"]), aliases)}
                  for child in node.get("inner", []) if child["kind"] == "ParmVarDecl"]
        defined = any(child["kind"] == "CompoundStmt" for child in node.get("inner", []))
        record = {"name": node["name"], "signature": signature, "parameters": params,
                  "return": shape(signature.split("(", 1)[0].strip(), aliases),
                  "definition_present": defined, "variadic": node.get("variadic", False),
                  "legacy_unspecified_prototype": signature.endswith("()"),
                  "typed_game_gateway_admitted": False}
        old = functions.get(node["name"])
        if old and old["definition_present"] and not defined:
            continue
        functions[node["name"]] = record
    callbacks = {}
    for node in nodes:
        name = node.get("name")
        if name not in FOCUS or node.get("kind") not in {"TypedefDecl", "RecordDecl"}:
            continue
        record = node
        for child in walk(node):
            ref = child.get("ownedTagDecl", {})
            if ref.get("id") in by_id:
                record = by_id[ref["id"]]
        fields = [{"name": child.get("name", ""), **shape(child["type"].get("desugaredQualType", child["type"]["qualType"]), aliases)}
                  for child in record.get("inner", []) if child["kind"] == "FieldDecl"]
        callbacks[name] = {"name": name, "type": node.get("type", {}).get("qualType", "struct " + name),
                           "complete_record": record.get("completeDefinition", False), "fields": fields,
                           "canonical": canonical(node.get("type", {}).get("qualType", "struct " + name), aliases),
                           "game_layout_verified": False}
    return functions, callbacks


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime-output", type=Path, required=True)
    parser.add_argument("--queried-exports", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    runtime = args.runtime_output.resolve()
    output = args.output.resolve()
    if not output.is_relative_to(REPO / "wire-out"):
        parser.error("Output must be below this repository's ignored wire-out directory")
    lock = json.loads((HERE / "source-lock.json").read_text())
    source = runtime / "sources" / lock["sources"][0]["directory"]
    configuration = json.loads((runtime / "runtime-configuration.json").read_text())
    for relative, expected in configuration["patched_source_files"].items():
        if sha256(source / relative) != expected:
            raise RuntimeError(f"Patched source changed after runtime build: {relative}")
    for relative, expected in configuration["patches"].items():
        if sha256(HERE / "patches" / relative) != expected:
            raise RuntimeError(f"Runtime patch changed: {relative}")
    output.mkdir(parents=True, exist_ok=True)
    for name in ("contracts.json", "queried-signatures.tsv"):
        (output / name).unlink(missing_ok=True)
    includes = [source, source / "mono", runtime / "build-runtime", source / "mono/eglib",
                runtime / "build-runtime/mono/eglib", source / "external/bdwgc/include"]
    command = ["clang", "-std=c11", "-fsyntax-only", "-DHAVE_CONFIG_H",
               *shlex.split(configuration["environment"]["CPPFLAGS"]),
               *["-I" + str(path) for path in includes], "-Xclang", "-ast-dump=json"]
    public = output / "public-api.c"
    public.write_text("".join(f"#include <{header}>\n" for header in HEADERS))
    records = []
    dependency_hashes = {}
    functions, callbacks = {}, {}
    input_paths = [source / header for header in HEADERS] + [source / name for name in DEFINITIONS]
    for unit in [public, *[source / name for name in DEFINITIONS]]:
        label = unit.stem
        ast_path = output / (label + ".ast.json")
        diagnostic = output / (label + ".diagnostics.log")
        with ast_path.open("w") as stdout, diagnostic.open("w") as stderr:
            run = subprocess.run([*command, str(unit)], stdout=stdout, stderr=stderr, timeout=60)
        if run.returncode:
            raise RuntimeError(f"C contract extraction failed; partial AST is not evidence: {diagnostic}")
        dependency = output / (label + ".dependencies.mk")
        dependency_command = [*command[:-2], "-M", "-MT", "contracts", "-MF", str(dependency), str(unit)]
        subprocess.run(dependency_command, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, timeout=60)
        dependency_paths = shlex.split(dependency.read_text().replace("\\\n", " ").split(":", 1)[1])
        for name in dependency_paths:
            path = Path(name)
            key = str(path.relative_to(source)) if path.is_relative_to(source) else str(path)
            dependency_hashes[key] = sha256(path)
        extracted, types = inventory(json.loads(ast_path.read_text()))
        for name, record in extracted.items():
            record["translation_unit"] = str(unit.relative_to(source)) if unit.is_relative_to(source) else unit.name
            old = functions.get(name)
            if old is None or record["definition_present"] or not old["definition_present"]:
                functions[name] = record
        callbacks.update(types)
        records.append({"arguments": [*command, str(unit)], "exit_code": run.returncode,
                        "ast_sha256": sha256(ast_path), "diagnostics_sha256": sha256(diagnostic)})
    queried = {line.strip() for line in args.queried_exports.read_text().splitlines()
               if line.strip().startswith(("mono_", "unity_mono_", "burst_mono_"))}
    queried_rows = [functions.get(name, {"name": name, "signature": None, "parameters": [],
                     "definition_present": False, "typed_game_gateway_admitted": False}) for name in sorted(queried)]
    for row in queried_rows:
        row["queried"] = True
    # Assert concrete contracts against the actual compiler AST. These names
    # are different exports; the stale header declaration is not the real API.
    allocator = functions["mono_unity_install_memory_callbacks"]
    if not allocator["definition_present"] or allocator["signature"] != "void (MonoAllocatorVTable *)":
        raise RuntimeError("Actual Unity allocator definition was not identified")
    expected_fields = ["version", "malloc", "realloc", "free", "calloc"]
    if [field["name"] for field in callbacks["MonoAllocatorVTable"]["fields"]] != expected_fields:
        raise RuntimeError("Actual allocator callback table changed")
    report = {"schema": 1, "evidence_scope": "candidate_native_c_compiler_contracts",
              "native_game_backend_enabled": False, "m3_admission": False,
              "queried_exports": len(queried), "queried_signatures_found": sum(bool(row["signature"]) for row in queried_rows),
              "queried_definitions_found": sum(row["definition_present"] for row in queried_rows),
              "unresolved_queries": [row["name"] for row in queried_rows if not row["signature"]],
              "callback_or_table_parameters": [row["name"] for row in queried_rows if any(
                  param["kind"] == "callback_pointer" or any(name in param["canonical"] for name in FOCUS)
                  for param in row["parameters"])],
              "queried_input_sha256": sha256(args.queried_exports),
              "runtime_configuration_sha256": sha256(runtime / "runtime-configuration.json"),
              "native_compiler": subprocess.check_output(["clang", "--version"], text=True),
              "transitive_contract_inputs_sha256": dependency_hashes,
              "contract_inputs_sha256": {str(path.relative_to(source)): sha256(path) for path in input_paths},
              "records": records, "functions": queried_rows, "callback_types": callbacks,
              "notes": ["Source declarations and candidate native definitions do not prove the original Win64 binary ABI",
                        "Pointer direction, extent, ownership, callback lifetime, TLS and reentry need separate verified contracts",
                        "Long/wchar_t widths and legacy unspecified prototypes require explicit Win64 validation",
                        "unity_mono_install_memory_callbacks(MonoMemoryCallbacks*) and mono_unity_install_memory_callbacks(MonoAllocatorVTable*) are different names",
                        "Opaque UnityTLS interface is not a complete callback-table layout", "No typed production gateway is admitted"]}
    (output / "contracts.json").write_text(json.dumps(report, indent=2) + "\n")
    with (output / "queried-signatures.tsv").open("w", newline="") as stream:
        writer = csv.writer(stream, delimiter="\t", lineterminator="\n")
        writer.writerow(["name", "signature", "definition_present", "variadic", "callback_parameters", "game_abi_verified"])
        for row in queried_rows:
            writer.writerow([row["name"], row["signature"] or "UNRESOLVED", int(row["definition_present"]),
                             int(row.get("variadic", False)), sum(param["kind"] == "callback_pointer" for param in row["parameters"]), 0])
    print(f"Candidate C contracts: {report['queried_signatures_found']}/{len(queried)} query signatures, {report['queried_definitions_found']} actual definitions; production ABI admission=0")
    print("Unresolved: " + ", ".join(report["unresolved_queries"]))


if __name__ == "__main__":
    main()
