#!/usr/bin/env python3
"""Compile original fixtures and run the separately built no-JIT Mono runtime.

The installed Unity SDK is a read-only compiler/bootstrap input. Assemblies,
native libraries, AOT support and logs remain in ignored wire-out output.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
FIXTURES = REPO / "tests/mono"
DEFAULT_SDK = Path("/Applications/Unity/Hub/Editor/2022.3.62f3c1/Unity.app/Contents/MonoBleedingEdge")


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def execute(args, env, output, label, expected=0, timeout=120):
    start = time.monotonic()
    result = subprocess.run(list(map(str, args)), env=env, cwd=output,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                            timeout=timeout)
    (output / (label + ".log")).write_bytes(result.stdout)
    record = {"arguments": list(map(str, args)), "exit_code": result.returncode,
              "expected_exit": expected, "seconds": time.monotonic() - start,
              "log": label + ".log"}
    print(f"{label}: exit={result.returncode}, {record['seconds']:.3f}s", flush=True)
    if result.returncode != expected:
        print(result.stdout.decode(errors="replace")[-10000:])
        raise RuntimeError(f"{label} failed; see {output / record['log']}")
    return record


def compile_fixtures(root, sdk, support_builder=None, classlib_input=None, fixture_output=None):
    # Game probes retain a frozen corelib/support pair in root/probe. SDK
    # fixtures must never overwrite it, even when both share the same runtime.
    output = fixture_output or root / "fixtures"
    output.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    # Bootstrap/compiler invocations never inherit the runtime noexec guard.
    for name in ("DYLD_INSERT_LIBRARIES", "MONO_ENV_OPTIONS", "MONO_PATH"):
        env.pop(name, None)
    classlib = output / "classlib/mono/4.5"
    classlib.mkdir(parents=True, exist_ok=True)
    classlib_input = classlib_input or sdk / "lib/mono/4.5"
    for name in ("mscorlib.dll", "System.dll", "System.Core.dll"):
        shutil.copy2(classlib_input / name, classlib / name)
    records = []
    records.append(execute([sdk / "bin/mono", sdk / "lib/mono/4.5/mcs.exe", "-unsafe",
                            "-optimize+", "-nostdlib",
                            *["-r:" + str(classlib / name) for name in ("mscorlib.dll", "System.dll", "System.Core.dll")],
                            "-out:" + str(output / "NativeProbe.exe"),
                            FIXTURES / "NativeProbe.cs"], env, output, "compile-il"))
    source_lock = json.loads((HERE / "source-lock.json").read_text())
    source = root / "sources" / source_lock["sources"][0]["directory"]
    runtime_lib = root / "build-runtime/mono/mini/.libs"
    records.append(execute(["clang", "-std=c11", "-O2", "-g", "-I" + str(source),
                            FIXTURES / "embed_probe.c", "-L" + str(runtime_lib),
                            "-lmonoboehm-2.0", "-pthread", "-o", output / "embed_probe"],
                           env, output, "compile-embedding"))
    for name, target in (("native_probe", "libwire_mono_probe.dylib"), ("noexec_guard", "noexec_guard.dylib")):
        records.append(execute(["clang", "-std=c11", "-O2", "-g", "-dynamiclib",
                                FIXTURES / (name + ".c"), "-o", output / target],
                               env, output, "compile-" + name))
    records.append(execute(["clang", "-std=c11", "-O2", FIXTURES / "guard_selftest.c",
                            "-o", output / "guard_selftest"], env, output, "compile-guard-test"))
    # Only support wrappers/trampolines are emitted with interp. Managed method
    # bodies remain IL; the game is not AOT-compiled or executed by this tool.
    aot_env = dict(env, MONO_PATH=str(classlib))
    support_builder = support_builder or root / "build-aot-tool/mono/mini/mono-boehm"
    header = output / "engine-header-probe"
    header.mkdir(exist_ok=True)
    records.append(execute([sdk / "bin/mono", sdk / "lib/mono/4.5/mcs.exe", "-unsafe",
                            "-optimize+", "-target:library", "-nostdlib",
                            "-r:" + str(classlib / "mscorlib.dll"),
                            "-out:" + str(header / "EngineHeaderProbe.dll"),
                            FIXTURES / "EngineHeaderProbe.cs"], env, output, "compile-engine-header-il"))
    records.append(execute(["clang", "-std=c11", "-O2", "-g", "-I" + str(source),
                            FIXTURES / "engine_header_probe.c", "-L" + str(runtime_lib),
                            "-lmonoboehm-2.0", "-o", header / "engine_header_probe"],
                           env, output, "compile-engine-header-embedding"))
    for assembly in (classlib / "mscorlib.dll", output / "NativeProbe.exe", header / "EngineHeaderProbe.dll"):
        records.append(execute([support_builder,
                                "--aot=interp", assembly], aot_env, output,
                               "aot-support-" + assembly.stem, timeout=300))
    manifest = {"sdk": str(sdk), "classlib_input": str(classlib_input), "runtime_is_sdk": False,
                "class_libraries": {p.name: sha256(p) for p in classlib.glob("*.dll")},
                "fixture_sources": {p.name: sha256(p) for p in FIXTURES.iterdir() if p.is_file()},
                "build": records}
    (output / "build-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")


def run_probes(root, cases, incremental=False, fixture_output=None):
    output = fixture_output or root / "fixtures"
    env = os.environ.copy()
    env.pop("MONO_ENV_OPTIONS", None)
    env.update(DYLD_LIBRARY_PATH=str(root / "build-runtime/mono/mini/.libs") + os.pathsep + str(output),
               DYLD_INSERT_LIBRARIES=str(output / "noexec_guard.dylib"),
               MONO_PATH=str(output / "classlib/mono/4.5"),
               MONO_THREADS_SUSPEND="preemptive")
    if incremental:
        env["WIRE_MONO_UNITY_GC_PROFILE"] = "1"
    else:
        env.pop("WIRE_MONO_UNITY_GC_PROFILE", None)
    records = []
    suffix = "-incremental" if incremental else ""
    for route in ("mmap", "jit", "mprotect", "vm_protect", "mach_vm_protect"):
        records.append(execute([output / "guard_selftest", route], env, output,
                               "guard-" + route, expected=86))
    for case in cases:
        if case == "engine-header":
            header = output / "engine-header-probe"
            command = [header / "engine_header_probe", output / "classlib", header / "EngineHeaderProbe.dll"]
        else:
            command = [output / "embed_probe", output / "classlib", output / "NativeProbe.exe", case, "interp"]
        records.append(execute(command,
                               env, output, "run-" + case + suffix))
        content = (output / ("run-" + case + suffix + ".log")).read_text()
        if f"PASS {case}" not in content or "WIRE_NOEXEC_GUARD checks=" not in content:
            raise RuntimeError(f"Missing runtime/guard evidence: {case}")
    (output / ("results" + suffix + ".json")).write_text(json.dumps(records, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("stage", choices=("compile", "run", "all"))
    parser.add_argument("--output", type=Path, default=REPO / "wire-out/native-mono-m1")
    parser.add_argument("--sdk", type=Path, default=DEFAULT_SDK)
    parser.add_argument("--cases", nargs="+", choices=("core", "interop", "threads", "gc", "layout", "remoting", "collation", "static-roots", "attrs", "objects", "reference-locals", "delegate-invoke", "engine-header"), default=["core", "interop", "threads", "gc", "layout", "remoting", "collation", "static-roots", "attrs", "objects", "reference-locals", "engine-header"])
    parser.add_argument("--incremental", action="store_true", help="initialize with the real Unity --gc-params=incremental=3 profile")
    parser.add_argument("--support-builder", type=Path, help="existing read-only development AOT tool input")
    parser.add_argument("--classlib-input", type=Path, help="existing read-only managed class libraries (copied to ignored output)")
    parser.add_argument("--fixture-output", type=Path, help="separate fixture directory under wire-out; defaults to runtime-output/fixtures, never game probe support")
    args = parser.parse_args()
    root = args.output.resolve()
    if not root.is_relative_to(REPO / "wire-out"):
        parser.error("Output must be below this repository's ignored wire-out directory")
    fixture_output = args.fixture_output.resolve() if args.fixture_output else root / "fixtures"
    if not fixture_output.is_relative_to(REPO / "wire-out") or fixture_output == root / "probe":
        parser.error("Fixture output must be below wire-out and separate from frozen game root/probe support")
    if args.stage in ("compile", "all"):
        compile_fixtures(root, args.sdk.resolve(), args.support_builder.resolve() if args.support_builder else None, args.classlib_input.resolve() if args.classlib_input else None, fixture_output)
    if args.stage in ("run", "all"):
        run_probes(root, args.cases, args.incremental, fixture_output)


if __name__ == "__main__":
    main()
