"""Snapshot input isolation, compatibility fields and explicit file selection."""
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from perf.freeze import freeze, source_inputs


class FreezeTests(unittest.TestCase):
    def test_snapshot_preserves_inputs_without_copying_unrelated_files_or_link_targets(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / 'repo'
            root.mkdir()
            subprocess.run(['git', 'init', '-q', str(root)], check=True)
            (root / 'Cargo.toml').write_text('tracked fixture\n')
            (root / 'deleted.rs').write_text('deleted fixture\n')
            subprocess.run(['git', '-C', str(root), 'add', '.'], check=True)
            subprocess.run(['git', '-C', str(root), '-c', 'user.name=Fixture',
                            '-c', 'user.email=fixture@example.invalid', '-c', 'commit.gpgsign=false',
                            'commit', '-qm', 'fixture'], check=True)
            (root / 'deleted.rs').unlink()
            for name in ['src/new.rs', 'docs/private.pdf', 'docs/Project design.md',
                         '.idea/session.xml', 'tests/.DS_Store']:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(name)
            external = Path(directory) / 'external-game'
            external.write_bytes(b'do not copy this content')
            (root / 'src/link.rs').symlink_to(external)
            executable = Path(directory) / 'binary'
            executable.write_bytes(b'frozen binary fixture')
            executable.chmod(0o755)
            out = Path(directory) / 'snapshot'
            binary = freeze(root, executable, out, 'fixture default')
            manifest = json.loads((out / 'manifest.json').read_text())
            self.assertEqual(set(manifest['files']), {'Cargo.toml', 'src/new.rs', 'src/link.rs'})
            self.assertEqual(manifest['build_config'], 'fixture default')
            self.assertEqual(manifest['binary_sha256'], hashlib.sha256(executable.read_bytes()).hexdigest())
            self.assertEqual(binary.read_bytes(), executable.read_bytes())
            self.assertEqual(binary.stat().st_mode & 0o777, 0o755)
            self.assertEqual((out / 'source/src/new.rs').read_text(), 'src/new.rs')
            self.assertTrue((out / 'source/src/link.rs').is_symlink())
            self.assertEqual(manifest['symlinks'], {'src/link.rs': str(external)})
            self.assertIn('deleted fixture', (out / 'source.diff').read_text())
            self.assertIn('rustc', manifest['toolchain']['rustc'])
            self.assertFalse((out / 'source/docs').exists())

    def test_explicit_extra_inputs_are_confined_to_repository_relative_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(['git', 'init', '-q', str(root)], check=True)
            (root / 'profile.bin').write_bytes(b'original input')
            self.assertEqual(source_inputs(root), [])
            self.assertEqual(source_inputs(root, ['profile.bin']), [Path('profile.bin')])
            for value in ['../profile.bin', str(root / 'profile.bin'), '.', 'missing']:
                with self.assertRaises(ValueError):
                    source_inputs(root, [value])
