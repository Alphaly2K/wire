#!/usr/bin/env python3
"""Summarize raw original-VM entry observations; never admit a native ABI."""
import argparse
import csv
import json
from pathlib import Path
import re
import sys
from probe import REPO, sha256
sys.path.insert(0, str(REPO / "tools"))
from api_inventory import PE


def rows(path):
    with path.open() as stream:
        return list(csv.DictReader((line for line in stream if not line.startswith("#")), delimiter="\t"))


def image_size(pe):
    return pe.number(pe.number(0x3c, 4) + 24 + 56, 4)


def export_rvas(pe):
    directory, _ = pe.directory(0)
    if not directory:
        return {}
    count, functions, strings, ordinals = [pe.at(directory + offset, 4) for offset in (24, 28, 32, 36)]
    if count > 65536:
        raise ValueError("export table exceeds bound")
    return {pe.string(pe.at(strings + index * 4, 4)):
            pe.at(functions + pe.at(ordinals + index * 2, 2) * 4, 4)
            for index in range(count)}


def raw_u64(value):
    if not re.fullmatch(r"[0-9a-f]{16}", value):
        return None
    return int.from_bytes(bytes.fromhex(value), "little")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--entries", type=Path, required=True)
    parser.add_argument("--startup-log", type=Path, required=True)
    parser.add_argument("--sample-mono", type=Path, required=True)
    parser.add_argument("--unity-player", type=Path, required=True)
    parser.add_argument("--managed-contracts", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    out = args.output.resolve()
    if not out.is_relative_to(REPO / "wire-out"):
        parser.error("output must be inside the worktree's ignored wire-out directory")
    # Remove success markers first: failed/new inputs cannot leave old success.
    out.mkdir(parents=True, exist_ok=True)
    for name in ("summary.json", "unity-entry-samples.tsv", "registered-internal-calls.tsv"):
        (out / name).unlink(missing_ok=True)
    trace = rows(args.entries)
    counts = [row for row in trace if row["kind"] == "count"]
    samples = [row for row in trace if row["kind"] == "sample"]
    mono = PE(args.sample_mono.read_bytes())
    unity = PE(args.unity_player.read_bytes())
    if mono.arch != "x64" or unity.arch != "x64":
        raise ValueError("expected original AMD64 inputs")
    rvas = export_rvas(mono)
    bases = set()
    module_name = args.sample_mono.name.lower()
    for row in counts:
        for alias in row["aliases"].split("|"):
            prefix, _, name = alias.partition("!")
            if prefix == module_name and name in rvas:
                bases.add(int(row["pc"], 16) - rvas[name])
    if len(bases) != 1:
        raise ValueError(f"trace does not identify exactly one original Mono mapping: {bases}")
    mono_base = bases.pop()
    mapped = re.findall(r"^  mapped unityplayer\.dll at (0x[0-9a-f]+)$", args.startup_log.read_text(), re.M)
    if len(set(mapped)) != 1:
        raise ValueError("startup log does not identify exactly one UnityPlayer mapping")
    unity_base = int(mapped[0], 16)
    # A return PC inside Unity is evidence of a sampled engine boundary. Tail
    # calls can produce multiple entries for the same return; do not count this
    # as an exact number of external CALL instructions or infer unsampled hits.
    engine = [row for row in samples if (raw_u64(row["return_bytes"]) is not None
              and unity_base <= raw_u64(row["return_bytes"]) < unity_base + image_size(unity))]
    fields = list(trace[0])
    with (out / "unity-entry-samples.tsv").open("w", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=fields, delimiter="\t", lineterminator="\n")
        writer.writeheader()
        writer.writerows(engine)
    declarations = {}
    if args.managed_contracts:
        for row in rows(args.managed_contracts):
            if row["kind"] == "InternalCall":
                key = row["class"] + "::" + row["method"]
                declarations.setdefault(key, []).append(row)
    bindings = []
    for row in trace:
        if row["kind"] != "binding":
            continue
        name_bytes = bytes.fromhex(row["aliases"])
        name = name_bytes.decode("utf-8", errors="backslashreplace")
        key = name.split("(", 1)[0].replace("/", ".")
        bindings.append({"name": name, "name_bytes_hex": row["aliases"], "guest_target": row["pc"],
                         "metadata_declaration_candidates": len(declarations.get(key, [])),
                         "typed_game_gateway_admitted": 0})
    with (out / "registered-internal-calls.tsv").open("w", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=["name", "name_bytes_hex", "guest_target",
            "metadata_declaration_candidates", "typed_game_gateway_admitted"], delimiter="\t", lineterminator="\n")
        writer.writeheader()
        writer.writerows(bindings)
    callbacks = [{"pc": row["pc"], "aliases": row["aliases"], "hits": int(row["hits"])}
                 for row in counts if "observed-callback!" in row["aliases"]]
    first = {}
    for row in engine:
        for name in row["aliases"].split("|"):
            first.setdefault(name, {"name": name, "work": int(row["work"]), "thread": int(row["thread"]),
                "return_pc": hex(raw_u64(row["return_bytes"])),
                "raw_registers": {reg: row[reg] for reg in ("rcx", "rdx", "r8", "r9")},
                "raw_observations": row["observations"]})
    allocator_entry = rvas.get("mono_unity_install_memory_callbacks")
    allocator_code = mono.slice(mono.offset(allocator_entry, 3), 3) if allocator_entry is not None else b""
    known_sample = sha256(args.sample_mono) == "86a5a4de6bcd74a637d63c96d822dc989d4ffaed6ef1c15c0e6e1f528ae6708f"
    headers = "\n".join(line for line in args.entries.read_text().splitlines() if line.startswith("#"))
    counters = {key: int(value) for key, value in re.findall(r"([a-z_]+)=(\d+)", headers)}
    summary = {"schema": 1, "scope": "original_guest_VM_bounded_raw_entry_observations",
        "native_game_backend_enabled": False, "m3_admission": False, "typed_game_gateways_admitted": 0,
        "trace_counters": counters,
        "sample_allocator_installer": {"export": "mono_unity_install_memory_callbacks",
            "known_sample_hash": known_sample, "rva": hex(allocator_entry) if allocator_entry is not None else None,
            "entry_bytes": allocator_code.hex(),
            "returns_without_reading_arguments": known_sample and allocator_code in (b"\xc2\x00\x00", b"\xc3\xcc\xcc"),
            "scope": "this exact original DLL; not all Unity Mono versions or the native candidate"},
        "timing_is_performance_comparison": False,
        "inputs_sha256": {str(path.resolve()): sha256(path) for path in
            [args.entries, args.startup_log, args.sample_mono, args.unity_player]
            + ([args.managed_contracts] if args.managed_contracts else [])},
        "mono_mapping": {"base": hex(mono_base), "size": image_size(mono)},
        "unity_mapping": {"base": hex(unity_base), "size": image_size(unity)},
        "active_observed_pcs": sum(int(row["hits"]) != 0 for row in counts),
        "entry_hits_including_VM_internal_and_tail_entries": sum(int(row["hits"]) for row in counts),
        "bounded_samples": len(samples), "engine_return_pc_samples": len(engine),
        "sampled_engine_entry_names": len(first),
        "raw_callback_pc_counts": callbacks,
        "registered_internal_call_names": len({row["name_bytes_hex"] for row in bindings}),
        "registered_internal_call_targets": len(bindings),
        "registered_names_with_metadata_candidates": len({row["name_bytes_hex"] for row in bindings
            if row["metadata_declaration_candidates"]}),
        "first_sampled_engine_entries": list(first.values()),
        "limits": ["all counts include internal VM calls, tail transfers and entry fallthrough",
            "samples are bounded; absent sample does not establish absence of a call",
            "callback PCs are raw observed installer inputs, not validated native function pointers",
            "binding registration is not execution, return-value validation or ABI admission",
            "metadata matches normalize nested-type / to . and are name candidates; overload/signature/Injected ABI remain unverified"]}
    (out / "summary.json").write_text(json.dumps(summary, indent=2, ensure_ascii=False) + "\n")
    print(json.dumps({key: summary[key] for key in ["active_observed_pcs", "bounded_samples",
        "sampled_engine_entry_names", "registered_internal_call_names", "registered_names_with_metadata_candidates"]}))


if __name__ == "__main__":
    main()
