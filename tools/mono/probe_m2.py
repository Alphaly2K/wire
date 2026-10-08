#!/usr/bin/env python3
"""Run an original PE/IL bridge against wire without editing CPU/SSE code."""
import argparse
import json
import os
from pathlib import Path
import subprocess

from probe import DEFAULT_SDK, FIXTURES, HERE, REPO, execute, sha256


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime-output", type=Path, default=REPO / "wire-out/native-mono-m1")
    parser.add_argument("--output", type=Path, default=REPO / "wire-out/native-mono-m2")
    parser.add_argument("--sdk", type=Path, default=DEFAULT_SDK)
    args = parser.parse_args()
    root, runtime, sdk = args.output.resolve(), args.runtime_output.resolve(), args.sdk.resolve()
    if not root.is_relative_to(REPO / "wire-out") or not runtime.is_relative_to(REPO / "wire-out"):
        parser.error("Outputs must be below this repository's ignored wire-out directory")
    root.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    for name in ("DYLD_INSERT_LIBRARIES", "MONO_ENV_OPTIONS", "MONO_PATH"):
        env.pop(name, None)
    records = []
    records.append(execute([sdk / "bin/mono", sdk / "lib/mono/4.5/mcs.exe", "-optimize+",
                            "-target:library", "-out:" + str(root / "GuestBridge.dll"),
                            FIXTURES / "GuestBridge.cs"], env, root, "compile-guest-il"))
    records.append(execute(["cargo", "build", "--release", "--offline", "--locked",
                            "--manifest-path", HERE / "m2/Cargo.toml", "--target-dir", root / "cargo"],
                           env, root, "build-rust-bridge", timeout=300))
    guarded = dict(env, DYLD_INSERT_LIBRARIES=str(runtime / "probe/noexec_guard.dylib"),
                   MONO_PATH=str(runtime / "probe/classlib/mono/4.5"),
                   MONO_THREADS_SUSPEND="preemptive")
    library = runtime / "build-runtime/mono/mini/.libs/libmonoboehm-2.0.1.dylib"
    records.append(execute([root / "cargo/release/wire-native-mono-m2-probe", library,
                            runtime / "probe/classlib", root / "GuestBridge.dll",
                            root / "GuestBridge.exe"], guarded, root, "run-guest-bridge"))
    content = (root / "run-guest-bridge.log").read_text()
    for marker in ("PASS original PE", "PASS scoped guest", "WIRE_NOEXEC_GUARD checks="):
        if marker not in content:
            raise RuntimeError(f"Missing bridge evidence: {marker}")
    report = {"records": records, "runtime_sha256": sha256(library),
              "fixture_sha256": sha256(root / "GuestBridge.dll"),
              "original_pe_sha256": sha256(root / "GuestBridge.exe"),
              "wire_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip(),
              "cpu_source_sha256": sha256(REPO / "src/x64/cpu.rs"),
              "memory_source_sha256": sha256(REPO / "src/x64/memory.rs"),
              "native_game_backend_enabled": False,
              "shared_backing_verified": False,
              "unsafe_il_checked": False}
    (root / "results.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
