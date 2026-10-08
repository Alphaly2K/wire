#!/usr/bin/env python3
"""Record actual linked objects/dependencies; do not declare a release audited."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def command(*args):
    return subprocess.check_output(list(map(str, args)), text=True)


def audit(root):
    build = root / "build-runtime"
    library = build / "mono/mini/.libs/libmonoboehm-2.0.1.dylib"
    archive = library.with_name("libmonoboehm-2.0.a")
    objects = [name for name in command("ar", "-t", archive).splitlines() if name.endswith(".o")]
    dependencies = {}
    for dependency in build.rglob(".deps/*.Plo"):
        dependencies.setdefault(dependency.stem + ".o", []).append(dependency)
    source_files = set()
    unmatched = []
    for obj in objects:
        matches = dependencies.get(obj, [])
        if not matches:
            unmatched.append(obj)
        for dep in matches:
            text = dep.read_text().split("\n\n", 1)[0].replace("\\\n", " ")
            # Paths below the source root contain no whitespace by construction.
            for word in text.split():
                path = Path(word.rstrip(":"))
                path = path if path.is_absolute() else dep.parent.parent / path
                path = path.resolve()
                if path.is_file() and path.is_relative_to(root / "sources"):
                    source_files.add(path)
    forbidden = {"atomic_ops_stack.h", "atomic_ops_malloc.h", "atomic_ops_malloc.c", "atomic_ops_stack.c"}
    selected_forbidden = [str(p) for p in source_files if p.name in forbidden]
    if selected_forbidden:
        raise RuntimeError(f"GPL atomic facilities entered linked dependencies: {selected_forbidden}")
    config = (build / "config.h").read_text()
    gc_config = (build / "external/bdwgc/include/config.h").read_text()
    for define in ("DISABLE_JIT", "TARGET_ARM64", "TARGET_OSX", "UNITY"):
        if not re.search(rf"^#define {define} 1$", config, re.MULTILINE):
            raise RuntimeError(f"Missing runtime configuration: {define}")
    for define in ("GC_NO_THREADS_DISCOVERY", "IGNORE_DYNAMIC_LOADING", "GC_DONT_REGISTER_MAIN_STATIC_DATA", "NO_EXECUTE_PERMISSION"):
        if not re.search(rf"^#define {define} 1$", gc_config, re.MULTILINE):
            raise RuntimeError(f"Missing BDWGC configuration: {define}")
    selected = [{"path": str(p.relative_to(root / "sources")), "sha256": digest(p)}
                for p in sorted(source_files)]
    report = {"source_lock_sha256": digest(HERE / "source-lock.json"),
              "library_sha256": digest(library), "archive_sha256": digest(archive),
              "file": command("file", library).strip(),
              "dynamic_dependencies": command("otool", "-L", library).splitlines()[1:],
              "linked_objects": objects, "compiler_dependencies": selected,
              "unmatched_objects": unmatched,
              "atomic_gpl_facilities_selected": False,
              "runtime_configuration_sha256": digest(root / "runtime-configuration.json"),
              "release_license_audit_complete": False,
              "exact_unity_binary_match": False}
    (root / "build-audit.json").write_text(json.dumps(report, indent=2) + "\n")
    (root / "native-exports.txt").write_text(command("nm", "-gU", library))
    print(f"Audited inventory: {len(objects)} linked objects, {len(selected)} compiler dependencies, {len(unmatched)} unmatched objects")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=REPO / "wire-out/native-mono-m1")
    args = parser.parse_args()
    root = args.output.resolve()
    if not root.is_relative_to(REPO / "wire-out"):
        parser.error("Output must be below the ignored wire-out directory")
    audit(root)
