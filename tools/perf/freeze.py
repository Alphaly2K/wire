"""Freeze a built binary and source inputs without collecting unrelated new files."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

NEW_SOURCE_ROOTS = {'src', 'tests', 'tools', 'examples', 'native'}
SOURCE_SUFFIXES = {'.rs', '.py', '.c', '.cpp', '.cc', '.m', '.mm', '.h', '.metal',
                   '.sh', '.json', '.toml', '.cs', '.il', '.s', '.asm', '.spec'}
DEFAULT_BUILD_CONFIG = 'default: direct128K, 4–16 register operations, links, bounded REP, checked RMW'


def git(root, *arguments):
    return subprocess.check_output(['git', '-C', str(root), *arguments])


def source_inputs(root, includes=()):
    tracked = git(root, 'ls-files', '-z').split(b'\0')
    new = git(root, 'ls-files', '--others', '--exclude-standard', '-z').split(b'\0')
    paths = {Path(os.fsdecode(raw)) for raw in tracked if raw}
    paths.update(Path(os.fsdecode(raw)) for raw in new if raw
                 and Path(os.fsdecode(raw)).parts[0] in NEW_SOURCE_ROOTS
                 and Path(os.fsdecode(raw)).suffix in SOURCE_SUFFIXES)
    for value in includes:
        relative = Path(value)
        if relative.is_absolute() or '..' in relative.parts:
            raise ValueError(f'--include requires a repository-relative file: {value}')
        if not (root / relative).is_file() and not (root / relative).is_symlink():
            raise ValueError(f'--include file does not exist: {value}')
        paths.add(relative)
    return sorted(path for path in paths
                  if (root / path).is_file() or (root / path).is_symlink())


def freeze(root, binary_input, out, build_config=DEFAULT_BUILD_CONFIG, includes=()):
    root = Path(root).resolve()
    # Validate before creating a partial output directory.
    paths = source_inputs(root, includes)
    out = Path(out)
    out.mkdir(parents=True, exist_ok=False)
    binary = out / 'wire'
    shutil.copy2(binary_input, binary)
    hashes, symlinks = {}, {}
    for relative in paths:
        source = root / relative
        destination = out / 'source' / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        # A link records its own target text; it never copies an external game.
        shutil.copy2(source, destination, follow_symlinks=False)
        if source.is_symlink():
            target = os.readlink(source)
            symlinks[str(relative)] = target
            data = os.fsencode(target)
        else:
            data = source.read_bytes()
        hashes[str(relative)] = hashlib.sha256(data).hexdigest()
    diff = git(root, 'diff', 'HEAD')
    (out / 'source.diff').write_bytes(diff)
    manifest = dict(head=git(root, 'rev-parse', 'HEAD').decode().strip(),
                    build_config=build_config,
                    binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                    diff_sha256=hashlib.sha256(diff).hexdigest(), files=hashes,
                    symlinks=symlinks,
                    source_policy='tracked + new source files + explicit --include',
                    toolchain=dict(rustc=subprocess.check_output(['rustc', '-vV'], text=True).strip(),
                                   cargo=subprocess.check_output(['cargo', '--version'], text=True).strip()))
    (out / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    return binary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--build-config', default=DEFAULT_BUILD_CONFIG)
    parser.add_argument('--include', action='append', default=[],
                        help='additional repository-relative source input file')
    args = parser.parse_args()
    print(freeze(Path.cwd(), args.binary, args.out, args.build_config, args.include))


if __name__ == '__main__':
    main()
