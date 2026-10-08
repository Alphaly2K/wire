#!/usr/bin/env python3
"""Preserve a built interpreter and all tracked/new source inputs for review."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--binary', type=Path, required=True)
p.add_argument('--out', type=Path, required=True)
p.add_argument('--build-config', default='default: direct128K, 4–16 register operations, links, bounded REP, checked RMW')
args = p.parse_args()
root = Path.cwd()
args.out.mkdir(parents=True, exist_ok=False)
binary = args.out / 'wire'
shutil.copy2(args.binary, binary)
files = subprocess.check_output(['git', 'ls-files', '-z']).split(b'\0')
files += subprocess.check_output(['git', 'ls-files', '--others', '--exclude-standard', '-z']).split(b'\0')
hashes = {}
for raw in sorted(set(files) - {b''}):
    relative = Path(raw.decode())
    source = root / relative
    if not source.is_file():
        continue
    destination = args.out / 'source' / relative
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)
    hashes[str(relative)] = hashlib.sha256(source.read_bytes()).hexdigest()
diff = subprocess.check_output(['git', 'diff', 'HEAD'])
(args.out / 'source.diff').write_bytes(diff)
manifest = dict(head=subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(),
                build_config=args.build_config,
                binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                diff_sha256=hashlib.sha256(diff).hexdigest(), files=hashes)
(args.out / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
print(binary)
