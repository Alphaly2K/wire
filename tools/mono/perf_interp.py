#!/usr/bin/env python3
"""Compare frozen native Mono configurations with the same pure IL checksum.

No comparison with guest x64 Mono or real-game acceleration is implied.
"""
import argparse
import json
import os
from pathlib import Path
import re
import resource
import statistics
import subprocess
import time
from probe import REPO, sha256


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--driver-root", required=True, type=Path)
    parser.add_argument("--runtime-output", required=True, type=Path)
    parser.add_argument("--baseline-library", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    driver, runtime, output = [p.resolve() for p in (args.driver_root, args.runtime_output, args.output)]
    if not output.is_relative_to(REPO / "wire-out"):
        parser.error("Output must be inside this repository's ignored wire-out")
    output.mkdir(parents=True, exist_ok=True)
    libraries = {"baseline": args.baseline_library.resolve(),
                 "candidate": runtime / "build-runtime/mono/mini/.libs/libmonoboehm-2.0.1.dylib"}
    env = os.environ.copy()
    for name in ("MONO_ENV_OPTIONS", "WIRE_MONO_TRACE", "WIRE_MONO_REFLECTION_INVENTORY"):
        env.pop(name, None)
    env.update(WIRE_MONO_PERF_ONLY="1", MONO_THREADS_SUSPEND="preemptive",
               DYLD_INSERT_LIBRARIES=str(runtime / "probe/noexec_guard.dylib"),
               MONO_PATH=str(runtime / "probe/classlib/mono/4.5"))
    records, samples, checksum = [], {name: [] for name in libraries}, None
    for run, name in enumerate(("baseline", "candidate", "candidate", "baseline", "baseline", "candidate")):
        command = [str(driver / "cargo/release/wire-native-mono-m2-probe"), str(libraries[name]),
                   str(runtime / "probe/classlib"), str(driver / "GuestBridge.dll"),
                   str(driver / "unused.exe"), str(driver / "CheckedBlocks.dll")]
        before = resource.getrusage(resource.RUSAGE_CHILDREN)
        start = time.monotonic()
        result = subprocess.run(command, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=60)
        elapsed = time.monotonic() - start
        after = resource.getrusage(resource.RUSAGE_CHILDREN)
        log = output / f"run-{run}-{name}.log"
        log.write_text(result.stdout)
        if result.returncode != 0:
            raise RuntimeError(f"Failed {name}: {log}")
        times = re.findall(r"PURE_IL_TIMING trial=(\d+) iterations=(\d+) checksum=(-?\d+) wall_s=([\d.]+)", result.stdout)
        guard = re.search(r"WIRE_NOEXEC_GUARD checks=(\d+) denied=(\d+)", result.stdout)
        if len(times) != 7 or not guard or int(guard[1]) == 0 or int(guard[2]) != 0:
            raise RuntimeError(f"Incomplete checksum/noexec evidence: {log}")
        for _, count, value, duration in times:
            observed = (int(count), int(value))
            if checksum is None:
                checksum = observed
            if observed != checksum:
                raise RuntimeError("Workload/checksum mismatch")
            samples[name].append(float(duration))
        record = {"name": name, "command": command, "log": log.name, "process_wall_s": elapsed,
                  "process_cpu_s": after.ru_utime + after.ru_stime - before.ru_utime - before.ru_stime,
                  "method_wall_s": [float(x[3]) for x in times]}
        records.append(record)
        print(f"{run} {name}: median method {statistics.median(record['method_wall_s']):.6f}s", flush=True)
    report = {"scope": "frozen native Mono configurations, pure managed arithmetic/array checksum only",
              "sequence": [r["name"] for r in records], "records": records, "iterations": checksum[0], "checksum": checksum[1],
              "libraries_sha256": {name: sha256(path) for name, path in libraries.items()},
              "fixture_sha256": sha256(driver / "GuestBridge.dll"),
              "driver_sha256": sha256(driver / "cargo/release/wire-native-mono-m2-probe"),
              "classlib_sha256": sha256(runtime / "probe/classlib/mono/4.5/mscorlib.dll"),
              "median_method_wall_s": {name: statistics.median(values) for name, values in samples.items()},
              "samples_per_configuration": 21,
              "runtime_configuration": json.loads((libraries["candidate"].resolve().parents[4] / "runtime-configuration.json").read_text()), "native_game_backend_enabled": False}
    report["method_speed_ratio"] = report["median_method_wall_s"]["baseline"] / report["median_method_wall_s"]["candidate"]
    (output / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"pure IL configuration ratio={report['method_speed_ratio']:.3f}; no real-game speed claim")


if __name__ == "__main__":
    main()
