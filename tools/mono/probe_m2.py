#!/usr/bin/env python3
"""Run an original PE/IL bridge against wire without editing CPU/SSE code."""
import argparse
import json
import os
import re
from pathlib import Path
import subprocess

from probe import DEFAULT_SDK, FIXTURES, HERE, REPO, execute, sha256


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime-output", type=Path, default=REPO / "wire-out/native-mono-m1")
    parser.add_argument("--output", type=Path, default=REPO / "wire-out/native-mono-m2")
    parser.add_argument("--sdk", type=Path, default=DEFAULT_SDK)
    parser.add_argument("--checked-memory", action="store_true", help="verify scoped unsafe IL using patch 0010")
    parser.add_argument("--sample-mono", type=Path, help="read-only original x64 Unity Mono layout probe")
    parser.add_argument("--native-services", action="store_true", help="trusted-classlib typed Windows service prototype")
    args = parser.parse_args()
    if args.native_services and (not args.sample_mono or not args.checked_memory):
        parser.error("--native-services requires --sample-mono and --checked-memory")
    if args.sample_mono and not args.checked_memory:
        parser.error("--sample-mono requires --checked-memory")
    root, runtime, sdk = args.output.resolve(), args.runtime_output.resolve(), args.sdk.resolve()
    if not root.is_relative_to(REPO / "wire-out"):
        parser.error("Outputs must be below this repository's ignored wire-out directory")
    # Runtime is a read-only input and may live in the primary checkout while
    # this driver is built in an isolated worktree. Never create/write it here.
    required = [runtime / "probe/noexec_guard.dylib",
                runtime / "probe/classlib/mono/4.5/mscorlib.dll.dylib",
                runtime / "build-runtime/mono/mini/.libs/libmonoboehm-2.0.1.dylib"]
    if not all(p.is_file() for p in required):
        parser.error("Runtime input must be an existing built noexec Mono probe tree")
    root.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    for name in ("DYLD_INSERT_LIBRARIES", "MONO_ENV_OPTIONS", "MONO_PATH"):
        env.pop(name, None)
    records = []
    records.append(execute([sdk / "bin/mono", sdk / "lib/mono/4.5/mcs.exe", "-optimize+", "-unsafe",
                            "-target:library", "-nostdlib",
                            *["-r:" + str(runtime / "probe/classlib/mono/4.5" / name) for name in ("mscorlib.dll", "System.dll", "System.Core.dll")],
                            "-out:" + str(root / "GuestBridge.dll"),
                            FIXTURES / "GuestBridge.cs"], env, root, "compile-guest-il"))
    records.append(execute(["cargo", "build", "--release", "--offline", "--locked",
                            "--manifest-path", HERE / "m2/Cargo.toml", "--target-dir", root / "cargo"],
                           env, root, "build-rust-bridge", timeout=300))
    if args.checked_memory:
        records.append(execute([sdk / "bin/mono", sdk / "lib/mono/4.5/ilasm.exe", "/dll",
                                "/output=" + str(root / "CheckedBlocks.dll"), FIXTURES / "CheckedBlocks.il"],
                               env, root, "compile-block-il"))
    if args.native_services:
        records.append(execute([sdk / "bin/mono", sdk / "lib/mono/4.5/mcs.exe", "-optimize+",
                                "-target:library", "-nostdlib",
                                *["-r:" + str(runtime / "probe/classlib/mono/4.5" / name) for name in ("mscorlib.dll", "System.dll", "System.Core.dll")],
                                "-out:" + str(root / "NativeServicesProbe.dll"), FIXTURES / "NativeServicesProbe.cs"],
                               env, root, "compile-services-il"))
    guarded = dict(env, DYLD_INSERT_LIBRARIES=str(runtime / "probe/noexec_guard.dylib"),
                   MONO_PATH=str(runtime / "probe/classlib/mono/4.5"),
                   MONO_THREADS_SUSPEND="preemptive")
    library = runtime / "build-runtime/mono/mini/.libs/libmonoboehm-2.0.1.dylib"
    command = [root / "cargo/release/wire-native-mono-m2-probe", library,
                            runtime / "probe/classlib", root / "GuestBridge.dll",
                            root / "GuestBridge.exe"]
    if args.checked_memory:
        command.append(root / "CheckedBlocks.dll")
    if args.sample_mono:
        command.append(args.sample_mono.resolve())
    if args.native_services:
        command.append(root / "NativeServicesProbe.dll")
    records.append(execute(command, guarded, root, "run-guest-bridge"))
    content = (root / "run-guest-bridge.log").read_text()
    for marker in ("PASS original PE", "PASS shared pinned Mono array", "PASS two attached Mono threads", "PASS scoped guest", "WIRE_NOEXEC_GUARD checks="):
        if marker not in content:
            raise RuntimeError(f"Missing bridge evidence: {marker}")
    if args.checked_memory and "PASS scoped checked IL" not in content:
        raise RuntimeError("Missing scoped checked IL evidence")
    if args.sample_mono and "PASS original Unity Mono leaf" not in content:
        raise RuntimeError("Missing original Unity Mono layout evidence")
    if args.native_services and "PASS typed native Windows services" not in content:
        raise RuntimeError("Missing native service evidence")
    if args.native_services:
        for marker in ("PASS BDWGC interpreter roots", "NATIVE_SERVICE_RESULT ManagedStreams=1", "NATIVE_SERVICE_RESULT ManagedQueries=1"):
            if marker not in content:
                raise RuntimeError(f"Missing native service regression evidence: {marker}")
    guard = re.search(r"WIRE_NOEXEC_GUARD checks=(\d+) denied=(\d+)", content)
    if not guard or int(guard[1]) == 0 or int(guard[2]) != 0:
        raise RuntimeError("Noexec guard did not verify a clean run")
    contract = re.search(r"PROFILE_CONTRACT platform=(-?\d+) expected_windows=2 host_filesystem_visible=(-?\d+)", content)
    platform_contract = {"observed_platform": int(contract[1]), "expected_platform": 2,
                         "host_filesystem_visible": bool(int(contract[2])) if int(contract[2]) >= 0 else None, "windows_vfs_bound": False,
                         "diagnostics": re.search(r"PROFILE_SERVICE_DIAGNOSTICS (.*)", content)[1]} if contract else None
    report = {"records": records, "runtime_sha256": sha256(library),
              "fixture_sha256": sha256(root / "GuestBridge.dll"),
              "original_pe_sha256": sha256(root / "GuestBridge.exe"),
              "wire_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip(),
              "cpu_source_sha256": sha256(REPO / "src/x64/cpu.rs"),
              "memory_source_sha256": sha256(REPO / "src/x64/memory.rs"),
              "sources": {str(path.relative_to(REPO)): sha256(path) for path in
                          [*sorted((HERE / "m2/src").rglob("*.rs")), REPO / "src/x64/memory/shared.rs",
                           REPO / "src/x64/memory/vm.rs", FIXTURES / "GuestBridge.cs", FIXTURES / "CheckedBlocks.il", FIXTURES / "NativeServicesProbe.cs"]},
              "class_libraries": {name: sha256(runtime / "probe/classlib/mono/4.5" / name)
                                  for name in ("mscorlib.dll", "System.dll", "System.Core.dll")},
              "checked_patch_sha256": sha256(HERE / "patches/0010-scoped-checked-interpreter-memory.patch"),
              "native_game_backend_enabled": False,
              "shared_backing_verified": True,
              "unsafe_il_checked": False,
              "scoped_primitive_il_checked": args.checked_memory,
              "complete_unsafe_il_sandbox": False,
              "sample_mono_sha256": sha256(args.sample_mono) if args.sample_mono else None,
              "real_unity_leaf_layout_verified": bool(args.sample_mono),
              "immutable_class_type_leaf_layout_verified": "CLASS_ABI_PROFILE compatible=1" in content,
              "immutable_class_profile_checked": bool(args.sample_mono),
              "immutable_class_profile_incompatible": "CLASS_ABI_PROFILE compatible=0" in content,
              "platform_contract": platform_contract,
              "trusted_classlib_native_services_verified": args.native_services,
              "trusted_classlib_filestream_verified": args.native_services,
              "bdwgc_interp_roots_patch_sha256": sha256(HERE / "patches/0012-bdwgc-interpreter-stack-roots.patch"),
              "class_profile_patch_sha256": sha256(HERE / "patches/0013-immutable-class-profile-fields.patch"),
              "msvc_class_abi_patch_sha256": sha256(HERE / "patches/0014-opt-in-msvc-class-bitfield-abi.patch"),
              "runtime_configuration": json.loads((runtime / "runtime-configuration.json").read_text()) if (runtime / "runtime-configuration.json").exists() else None,
              "active_and_popped_interp_roots_verified": args.native_services,
              "managed_reflection_inventory_verified": "PASS managed reflection P/Invoke inventory" in content,
              "method_qualified_patch_sha256": sha256(HERE / "patches/0011-method-qualified-pinvoke-adapter.patch"),
              "pinvoke_contract_inventory_sha256": sha256(root / "pinvoke-contracts.tsv") if args.native_services else None,
              "production_native_pointer_validation": False,
              "m3_admission": False,
              "admission_blockers": ["Complete checked unsafe IL and managed/native helper coverage",
                                     "Managed reference write barriers and production roots/scheduler",
                                     "Production Windows service ownership, complete binding and scheduler",
                                     "Full Unity ABI profile and typed callback coverage"]}
    (root / "results.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
