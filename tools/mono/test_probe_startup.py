"""The startup harness must reuse assets and retire temporary inputs on failure."""
from contextlib import redirect_stderr, redirect_stdout
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import probe_startup
import probe


class StorageTests(unittest.TestCase):
    def test_fixture_compilation_cannot_overwrite_frozen_game_corelib_support(self):
        argv = ['probe.py', 'compile', '--output', str(self.runtime), '--fixture-output', str(self.runtime / 'probe')]
        with patch.object(probe, 'REPO', self.repo), patch('sys.argv', argv), \
             patch.object(probe, 'compile_fixtures') as compile_fixtures, \
             redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as stopped:
            probe.main()
        self.assertEqual(stopped.exception.code, 2)
        compile_fixtures.assert_not_called()
        self.assertEqual(self.support.read_bytes(), b'support')

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='wire-startup-storage-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.repo = self.root / 'repo'
        self.sandbox = self.repo / 'wire-out/native-mono-probe-state/runtime'
        self.game = self.root / 'original/Alice.exe'
        self.runtime = self.repo / 'wire-out/mono'
        self.wire = self.repo / 'wire'
        self.support = self.runtime / 'probe/classlib/mono/4.5/mscorlib.dll.dylib'
        inputs = {
            self.game: b'exe', self.game.parent / 'UnityPlayer.dll': b'player',
            self.game.parent / 'MonoBleedingEdge/EmbedRuntime/mono-2.0-bdwgc.dll': b'mono',
            self.sandbox / self.game.name: b'exe', self.sandbox / 'UnityPlayer.dll': b'player',
            self.sandbox / 'AliceToPapaADV_1-0-0_Data/Managed/mscorlib.dll': b'corelib',
            self.runtime / 'probe/classlib/mono/4.5/mscorlib.dll': b'corelib',
            self.support: b'support', self.runtime / 'probe/noexec_guard.dylib': b'guard',
            self.runtime / 'build-runtime/mono/mini/.libs/libmonoboehm-2.0.1.dylib': b'native',
            self.wire: b'wire before rebuild',
        }
        for path, content in inputs.items():
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
        self.original_inputs = {path: content for path, content in inputs.items()
                                if path.is_relative_to(self.game.parent)}

    def invoke(self, output, fail=False, before_startup=False):
        argv = ['probe_startup.py', '--game', str(self.game), '--sandbox', str(self.sandbox),
                '--runtime-output', str(self.runtime), '--wire', str(self.wire),
                '--output', str(output)]

        def guest(command, **kwargs):
            self.assertEqual(Path(command[2]).resolve(), self.sandbox / self.game.name)
            self.assertTrue((output / 'runtime').is_symlink())
            self.assertTrue((output / 'wire').is_symlink())
            frozen = Path(command[0]).resolve()
            expected = frozen.read_bytes()
            self.wire.write_bytes(b'rebuilt wire')
            self.assertEqual(frozen.read_bytes(), expected)
            if before_startup:
                raise subprocess.TimeoutExpired(command, 180)
            log = self.sandbox / 'Users/wire/AppData/LocalLow/Game/Player.log'
            log.parent.mkdir(parents=True, exist_ok=True)
            log.write_text('actual guest log\n')
            if fail:
                raise subprocess.TimeoutExpired(command, 180)
            kwargs['stdout'].write('native-root-ready=true\nWIRE_NOEXEC_GUARD checks=1 denied=0\n')
            return subprocess.CompletedProcess(command, 0)

        with patch.object(probe_startup, 'REPO', self.repo), patch('sys.argv', argv), \
                patch.object(probe_startup.subprocess, 'run', side_effect=guest), \
                redirect_stdout(io.StringIO()):
            probe_startup.main()

    def assert_retired(self, output):
        for name in ('runtime', 'wire'):
            self.assertFalse((output / name).is_symlink())
            self.assertFalse((output / name).exists())
        self.assertEqual(list(self.sandbox.parent.glob('wire-snapshot-*')), [])
        self.assertFalse((self.sandbox / 'AliceToPapaADV_1-0-0_Data/Managed/mscorlib.dll.dylib').is_symlink())
        self.assertEqual((output / 'guest-logs/wire/AppData/LocalLow/Game/Player.log').read_text(),
                         'actual guest log\n')
        for path, content in self.original_inputs.items():
            self.assertEqual(path.read_bytes(), content)

    def test_repeated_probes_reuse_one_sandbox_and_keep_only_evidence(self):
        inode = self.sandbox.stat().st_ino
        for number in (1, 2):
            output = self.repo / f'wire-out/probe-{number}'
            self.invoke(output)
            self.assert_retired(output)
            self.assertEqual(self.sandbox.stat().st_ino, inode)
            self.assertEqual(json.loads((output / 'results.json').read_text())['returncode'], 0)

    def test_timeout_cleans_links_snapshot_and_preserves_guest_log(self):
        output = self.repo / 'wire-out/failed'
        with self.assertRaises(subprocess.TimeoutExpired):
            self.invoke(output, fail=True)
        self.assert_retired(output)

    def test_failure_before_startup_does_not_capture_previous_player_log(self):
        log = self.sandbox / 'Users/wire/AppData/LocalLow/Game/Player.log'
        log.parent.mkdir(parents=True)
        log.write_text('previous probe only\n')
        output = self.repo / 'wire-out/before-startup'
        with self.assertRaises(subprocess.TimeoutExpired):
            self.invoke(output, before_startup=True)
        self.assertFalse((output / 'guest-logs').exists())
        self.assertFalse(log.exists())
        self.assertEqual(list(self.sandbox.parent.glob('wire-snapshot-*')), [])

    def test_symlink_to_original_cannot_be_used_as_sandbox(self):
        forbidden = self.repo / 'wire-out/original-link'
        forbidden.symlink_to(self.game.parent, target_is_directory=True)
        argv = ['probe_startup.py', '--game', str(self.game), '--sandbox', str(forbidden),
                '--output', str(self.repo / 'wire-out/rejected')]
        with patch.object(probe_startup, 'REPO', self.repo), patch('sys.argv', argv), \
                patch.object(probe_startup.subprocess, 'run') as guest, \
                redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            probe_startup.main()
        guest.assert_not_called()


if __name__ == '__main__':
    unittest.main()
