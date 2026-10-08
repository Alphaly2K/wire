#!/usr/bin/env python3
"""Build the locked Unity Mono prototype without modifying an installed SDK.

This tool is independent of Cargo and the guest CPU implementation. The AOT
tool is a development-only compiler; it must never become the game runtime.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tarfile

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


def prepare(root, env):
    archives = root / "archives"
    sources = root / "sources"
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
    for patch in sorted((HERE / "patches").glob("*.patch")):
        command = ["patch", "-p1", "--batch"]
        with patch.open("rb") as stream:
            forward = subprocess.run(command + ["--dry-run", "--forward"], cwd=mono,
                                     stdin=stream, stdout=subprocess.DEVNULL,
                                     stderr=subprocess.DEVNULL)
        if forward.returncode == 0:
            with patch.open("rb") as stream:
                subprocess.run(command + ["--forward"], cwd=mono, stdin=stream, check=True)
        else:
            with patch.open("rb") as stream:
                subprocess.run(command + ["--dry-run", "--reverse"], cwd=mono,
                               stdin=stream, check=True, stdout=subprocess.DEVNULL)


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


def build_runtime(root, env, mode, jobs):
    mono = root / "sources" / LOCK["sources"][0]["directory"]
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
        env = dict(env, CPPFLAGS="-DWIRE_MONO_NO_EXEC=1")
    manifest = {"arguments": list(map(str, options)),
                "environment": {key: env.get(key, "") for key in
                                ("CC", "CXX", "CFLAGS", "CXXFLAGS", "CPPFLAGS",
                                 "MACOSX_DEPLOYMENT_TARGET", "MONO_BRANCH", "MONO_BUILD_REVISION")},
                "patches": {p.name: digest(p) for p in sorted((HERE / "patches").glob("*.patch"))}}
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
            build_runtime(root, env, stage, args.jobs)


if __name__ == "__main__":
    main()
