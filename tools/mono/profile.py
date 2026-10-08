#!/usr/bin/env python3
"""Freeze current Mono evidence and queried export gaps without enabling a VM."""
import argparse
import csv
import json
from pathlib import Path
import re
import subprocess
import sys
from probe import HERE, REPO, sha256
sys.path.insert(0, str(REPO / "tools"))
from api_inventory import PE
KNOWN_SAMPLE = "86a5a4de6bcd74a637d63c96d822dc989d4ffaed6ef1c15c0e6e1f528ae6708f"
LEAF_EXPORTS = {"mono_unity_object_header_size", "mono_unity_array_object_header_size",
    "mono_unity_offset_of_array_length_in_array_object_header", "mono_unity_offset_of_array_bounds_in_array_object_header",
    "mono_unity_allocation_granularity", "mono_array_length", "mono_array_addr_with_size", "mono_object_unbox",
    "mono_class_get_name", "mono_class_get_namespace", "mono_class_get_image", "mono_class_get_parent", "mono_class_get_type",
    "mono_type_get_type", "mono_method_get_name", "mono_method_get_class", "mono_method_get_flags", "mono_method_get_token",
    "mono_method_signature", "mono_signature_get_param_count", "mono_signature_get_params", "mono_signature_get_return_type",
    "mono_signature_is_instance", "mono_type_is_byref"}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bridge-results", type=Path, required=True)
    parser.add_argument("--queried-exports", type=Path, required=True, help="observed lookups, not a mandatory-call list")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    output = args.output.resolve()
    if not output.is_relative_to(REPO / "wire-out"):
        parser.error("Output must be below the worktree's ignored wire-out directory")
    results_path = args.bridge_results.resolve()
    report = json.loads(results_path.read_text())
    record = report["records"][-1]
    if record["exit_code"] != 0:
        raise RuntimeError("Bridge evidence is not a successful run")
    runtime = Path(record["arguments"][1])
    sample = Path(record["arguments"][6])
    for path, expected in ((runtime, report["runtime_sha256"]), (sample, report["sample_mono_sha256"]),
                           (Path(record["arguments"][3]), report["fixture_sha256"])):
        if sha256(path) != expected:
            raise RuntimeError(f"Evidence input changed: {path}")
    for name, expected in report["sources"].items():
        if sha256(REPO / name) != expected:
            raise RuntimeError(f"Bridge source changed after evidence: {name}")
    for name, expected in report["runtime_configuration"]["patches"].items():
        if sha256(HERE / "patches" / name) != expected:
            raise RuntimeError(f"Runtime patch changed after evidence: {name}")
    log = (results_path.parent / record["log"]).read_text()
    guard = re.search(r"WIRE_NOEXEC_GUARD checks=(\d+) denied=(\d+)", log)
    if not guard or int(guard[1]) == 0 or int(guard[2]) != 0:
        raise RuntimeError("Bridge has no clean noexec guard evidence")
    required_leaf = (report["real_unity_leaf_layout_verified"], report["immutable_class_type_leaf_layout_verified"],
                     report["original_method_signature_type_leaves_verified"])
    leaf_verified = all(required_leaf)
    original = PE(sample.read_bytes()).exports()
    native = {line.split()[-1].removeprefix("_") for line in subprocess.check_output(["nm", "-gU", runtime], text=True).splitlines() if line.split()}
    queried = {line.strip() for line in args.queried_exports.read_text().splitlines()
               if line.strip().startswith(("mono_", "unity_mono_", "burst_mono_"))}
    if not queried:
        raise RuntimeError("No Mono queries in export input")
    output.mkdir(parents=True, exist_ok=True)
    rows = [{"name": name, "original_export": int(name in original), "native_symbol": int(name in native),
             "leaf_fixture_abi_verified": int(leaf_verified and name in LEAF_EXPORTS), "typed_game_gateway_admitted": 0,
             "mandatory_call_verified": 0} for name in sorted(queried)]
    with (output / "queried-mono-exports.tsv").open("w", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=list(rows[0]), delimiter="\t", lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)
    # These fields match the production Session admission input. Fixture proofs
    # cannot promote memory/service/helper/Unity callbacks to production-ready.
    admission = {"requested": True, "known_binary_fingerprint": report["sample_mono_sha256"] == KNOWN_SAMPLE,
                 "full_native_abi": False, "checked_memory_and_roots": False,
                 "windows_services_and_scheduler": False, "unity_initialization_callbacks": False,
                 "required_exports_covered": False, "no_dynamic_code": True}
    reasons = [key for key, value in admission.items() if not value]
    frozen = {"schema": 1, "evidence_scope": "independent_native_fixtures_and_read_only_original_leaf_abi",
              "selected_game_backend": "original_x64", "native_game_backend_enabled": False, "m3_admission": False,
              "admission": admission, "admission_failures": reasons,
              "original_mono_sha256": sha256(sample), "native_runtime_sha256": sha256(runtime),
              "bridge_results_sha256": sha256(results_path), "bridge_log_sha256": sha256(results_path.parent / record["log"]),
              "queried_input_sha256": sha256(args.queried_exports), "export_report_sha256": sha256(output / "queried-mono-exports.tsv"),
              "runtime_configuration": report["runtime_configuration"], "noexec_guard": {"checks": int(guard[1]), "denied": int(guard[2])},
              "queried_exports": len(rows), "native_symbols_present": sum(row["native_symbol"] for row in rows),
              "missing_native_symbols": [row["name"] for row in rows if not row["native_symbol"]],
              "missing_original_exports": [row["name"] for row in rows if not row["original_export"]],
              "leaf_abi_queries_verified": sum(row["leaf_fixture_abi_verified"] for row in rows),
              "typed_game_gateways_admitted": 0, "mandatory_query_classification_complete": False,
              "managed_bindings": {"abi_admitted": 0, "diagnostics": report.get("managed_inventory_diagnostics")},
              "notes": ["Queried names include optional capability lookups and are not call frequency or mandatory exports",
                        "Native symbol presence is not ABI compatibility or a typed Win64 gateway",
                        "Initialization failure must stop this VM and request isolated process restart, never per-function fallback"]}
    (output / "profile.json").write_text(json.dumps(frozen, indent=2) + "\n")
    print(f"Frozen Mono preflight: {len(rows)} queries, {frozen['native_symbols_present']} native symbols, {frozen['leaf_abi_queries_verified']} verified leaf queries, typed game gateways=0; game backend=original_x64")
    print("Missing native symbols: " + ", ".join(frozen["missing_native_symbols"]))

if __name__ == "__main__":
    main()
