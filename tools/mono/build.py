#!/usr/bin/env python3
"""Build the locked Unity Mono prototype without modifying an installed SDK.

This tool is independent of Cargo and the guest CPU implementation. The AOT
tool is a development-only compiler; it must never become the game runtime.
"""
import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import subprocess
import tarfile
import tempfile

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
LOCK = json.loads((HERE / "source-lock.json").read_text())


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def run(args, cwd, env, log):
    print(f"{log.name}: {' '.join(map(str, args))}", flush=True)
    with log.open("a") as stream:
        subprocess.run(list(map(str, args)), cwd=cwd, env=env, stdout=stream,
                       stderr=subprocess.STDOUT, check=True)


def environment(root):
    env = os.environ.copy()
    env.update(PATH=str(root / "tools/bin") + os.pathsep + env["PATH"],
               GIT_CEILING_DIRECTORIES=str(root), MONO_PATH="",
               MACOSX_DEPLOYMENT_TARGET="11.0", CC="clang", CXX="clang++",
               CFLAGS="-O2 -g", CXXFLAGS="-O2 -g",
               MONO_BRANCH="wire-unity-prototype",
               MONO_BUILD_REVISION=LOCK["sources"][0]["revision"])
    return env


def patch_files(patches):
    paths = set()
    for patch in patches:
        for name in re.findall(r"^(?:--- a/|\+\+\+ b/)([^\t\n]+)", patch.read_text(), re.MULTILINE):
            path = Path(name)
            if path.is_absolute() or ".." in path.parts:
                raise RuntimeError(f"Invalid patch path: {name}")
            paths.add(path)
    return sorted(paths)


def patch_state(root, paths):
    return {str(path): digest(root / path) if (root / path).is_file() else None for path in paths}


def apply_patch_stack(mono, archive, directory, patches):
    """Recognize an exact prefix of the locked patch stack, then advance it.

    Later patches may edit earlier hunks, so reverse-checking each patch
    independently is unsound. Reconstruct only affected source files from the
    locked archive in a temporary tree; unknown edits fail before mutation.
    """
    paths = patch_files(patches)
    current = patch_state(mono, paths)
    installed = None
    with tempfile.TemporaryDirectory(prefix="wire-mono-patches-") as temporary:
        reference = Path(temporary)
        with tarfile.open(archive) as package:
            for path in paths:
                try:
                    member = package.getmember(str(Path(directory) / path))
                except KeyError:
                    continue  # A patch may add an original new file.
                if not member.isfile():
                    raise RuntimeError(f"Patch target is not an archive file: {path}")
                target = reference / path
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(package.extractfile(member).read())
        if patch_state(reference, paths) == current:
            installed = 0
        for index, patch in enumerate(patches, 1):
            with patch.open("rb") as stream:
                subprocess.run(["patch", "-p1", "--batch", "--forward"], cwd=reference,
                               stdin=stream, check=True, stdout=subprocess.DEVNULL)
            if patch_state(reference, paths) == current:
                installed = index
        expected = patch_state(reference, paths)
        if installed is None:
            raise RuntimeError("Existing patched source does not match any exact locked patch prefix; preserve and inspect local edits")
    for patch in patches[installed:]:
        with patch.open("rb") as stream:
            subprocess.run(["patch", "-p1", "--batch", "--forward"], cwd=mono, stdin=stream, check=True)
    if patch_state(mono, paths) != expected:
        raise RuntimeError("Patch application did not reproduce the locked source state")
    (mono / ".wire-patches.json").write_text(json.dumps({
        "archive_sha256": digest(archive),
        "patches": {patch.name: digest(patch) for patch in patches},
        "files": expected,
    }, indent=2) + "\n")
    print(f"Validated patch stack: {installed} existing, {len(patches) - installed} applied", flush=True)


def require_owned_output(root, path):
    if not path.resolve().is_relative_to(root.resolve()):
        raise RuntimeError(f"Writable Mono output aliases an input outside its output root: {path}; create a physical clone first")


def prepare(root, env):
    archives = root / "archives"
    sources = root / "sources"
    for path in [archives, sources, *[sources / item["directory"] for item in LOCK["sources"] + LOCK["build_tools"]]]:
        require_owned_output(root, path)
    archives.mkdir(parents=True, exist_ok=True)
    sources.mkdir(exist_ok=True)
    for item in LOCK["sources"] + LOCK["build_tools"]:
        archive = archives / item["archive"]
        if not archive.exists():
            temporary = archive.with_suffix(archive.suffix + ".download")
            run(["curl", "--fail", "--location", "--retry", "3", "--output",
                 temporary, item["url"]], root, env, root / "downloads.log")
            if digest(temporary) != item["sha256"]:
                raise RuntimeError(f"Archive hash mismatch: {item['name']}")
            temporary.replace(archive)
        if digest(archive) != item["sha256"]:
            raise RuntimeError(f"Archive hash mismatch: {item['name']}")
        destination = sources / item["directory"]
        marker = destination / ".wire-source.json"
        provenance = {"archive_sha256": item["sha256"], "url": item["url"]}
        if destination.exists():
            if not marker.exists() or json.loads(marker.read_text()) != provenance:
                raise RuntimeError(f"Unidentified existing source directory: {destination}")
            continue
        with tarfile.open(archive) as package:
            members = package.getmembers()
            tops = {Path(member.name).parts[0] for member in members}
            if tops != {item["directory"]}:
                raise RuntimeError(f"Unexpected archive layout: {item['name']}")
            package.extractall(sources, filter="data")
        marker.write_text(json.dumps(provenance, indent=2) + "\n")
    mono, gc, atomic = [sources / item["directory"] for item in LOCK["sources"]]
    for link, target in ((mono / "external/bdwgc", gc), (gc / "libatomic_ops", atomic)):
        if link.is_symlink():
            if link.resolve() != target.resolve():
                raise RuntimeError(f"Unexpected dependency symlink: {link}")
        else:
            if link.exists():
                link.rmdir()  # Only an empty submodule placeholder may be removed.
            link.symlink_to(target, target_is_directory=True)
    item = LOCK["sources"][0]
    apply_patch_stack(mono, archives / item["archive"], item["directory"],
                      sorted((HERE / "patches").glob("*.patch")))


def build_tools(root, env, jobs):
    for item in LOCK["build_tools"]:
        if (root / "tools/bin" / item["name"]).exists():
            continue
        source = root / "sources" / item["directory"]
        build = root / ("build-" + item["directory"])
        build.mkdir(exist_ok=True)
        log = root / (item["directory"] + ".log")
        run([source / "configure", "--prefix=" + str(root / "tools")], build, env, log)
        run(["make", f"-j{jobs}"], build, env, log)
        run(["make", "install"], build, env, log)


def autogen(root, env):
    mono = root / "sources" / LOCK["sources"][0]["directory"]
    env = dict(env, NOCONFIGURE="1", ACLOCAL_FLAGS="-I " + str(root / "tools/share/aclocal"))
    run(["./autogen.sh"], mono, env, root / "autogen.log")
    # The tarball omits corefx. Only the C runtime is built; mcs/native class
    # libraries are deliberately not built. autogen prints a corefx warning.


def build_runtime(root, env, mode, jobs, msvc_class_abi=False):
    mono = root / "sources" / LOCK["sources"][0]["directory"]
    require_owned_output(root, mono)
    require_owned_output(root, root / ("build-" + mode))
    marker = mono / ".wire-patches.json"
    patches = {p.name: digest(p) for p in sorted((HERE / "patches").glob("*.patch"))}
    if not marker.exists():
        raise RuntimeError("Run prepare first to verify the complete source patch stack")
    source_state = json.loads(marker.read_text())
    if source_state["patches"] != patches or patch_state(mono, [Path(p) for p in source_state["files"]]) != source_state["files"]:
        raise RuntimeError("Source patch state changed; run prepare and inspect unexpected edits before building")
    build = root / ("build-" + mode)
    build.mkdir(exist_ok=True)
    options = [mono / "configure", "--prefix=" + str(root / ("install-" + mode)),
               "--host=aarch64-apple-darwin20", "--build=aarch64-apple-darwin20",
               "--disable-mcs-build", "--enable-boehm", "--with-sgen=no",
               "--disable-btls", "--disable-btls-lib", "--enable-unity-define",
               "--disable-maintainer-mode", "--enable-no-threads-discovery=yes",
               "--enable-gc-threads=yes", "--enable-thread-local-alloc=no",
               "--enable-ignore-dynamic-loading=yes",
               "--enable-dont-register-main-static-data=yes"]
    if mode == "runtime":
        options.append("--enable-minimal=jit")
        env = dict(env, CPPFLAGS="-DWIRE_MONO_NO_EXEC=1" +
                   (" -DWIRE_MONO_MSVC_CLASS_ABI=1" if msvc_class_abi else ""))
    manifest = {"arguments": list(map(str, options)),
                "environment": {key: env.get(key, "") for key in
                                ("CC", "CXX", "CFLAGS", "CXXFLAGS", "CPPFLAGS",
                                 "MACOSX_DEPLOYMENT_TARGET", "MONO_BRANCH", "MONO_BUILD_REVISION")},
                "patches": patches, "patched_source_files": source_state["files"]}
    (root / (mode + "-configuration.json")).write_text(json.dumps(manifest, indent=2) + "\n")
    run(options, build, env, root / ("configure-" + mode + ".log"))
    # Upstream's all-recursive includes SGen-only unit-test libraries even
    # when SGen is disabled. Build the actual embedding runtime components.
    for component in ("external/bdwgc", "mono/eglib", "mono/utils", "mono/culture", "mono/metadata", "mono/mini"):
        run(["make", f"-j{jobs}"], build / component, env, root / ("build-" + mode + ".log"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("stage", choices=("prepare", "tools", "autogen", "runtime", "aot-tool", "all"))
    parser.add_argument("--output", type=Path, default=REPO / "wire-out/native-mono-m1")
    parser.add_argument("--jobs", type=int, default=6)
    parser.add_argument("--msvc-class-abi", action="store_true", help="opt in to Windows MonoClass bit-field layout for diagnostic runtime")
    args = parser.parse_args()
    root = args.output.resolve()
    if not root.is_relative_to(REPO / "wire-out"):
        parser.error("Build output must be below this repository's ignored wire-out directory")
    root.mkdir(parents=True, exist_ok=True)
    env = environment(root)
    stages = ("prepare", "tools", "autogen", "aot-tool", "runtime") if args.stage == "all" else (args.stage,)
    for stage in stages:
        if stage == "prepare":
            prepare(root, env)
        elif stage == "tools":
            build_tools(root, env, args.jobs)
        elif stage == "autogen":
            autogen(root, env)
        else:
            build_runtime(root, env, stage, args.jobs, args.msvc_class_abi)


if __name__ == "__main__":
    main()
