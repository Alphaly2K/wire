"""Independent process/status/timeout checks for the common measurement layer."""
from pathlib import Path
import sys
import tempfile
import unittest
from perf.measurement import digest, execution, launch_protocol, paired_order, run_command, stable_command


class MeasurementTests(unittest.TestCase):
    @unittest.skipUnless(Path('/bin/zsh').is_file(), 'stable launch requires zsh')
    def test_executable_slot_keeps_actual_path_cwd_and_frozen_source(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            commands, workspaces = [], []
            for label in ['before', 'different-length-after']:
                source = root / label / 'frozen'
                source.parent.mkdir()
                source.write_text('#!/bin/sh\nprintf "%s" "$0" > marker\npwd > cwd\nexit 7\n')
                source.chmod(0o755)
                expected = digest(source)
                out = root / 'case' / f'pair-{label}'; out.mkdir(parents=True)
                with execution(source, out, True, True, expected) as (program, cwd):
                    commands.append((str(program), str(cwd)))
                    self.assertEqual(digest(program), expected)
                    result = run_command(stable_command([str(program)]), out / 'report.txt', cwd=cwd)
                    self.assertEqual(result['exit_code'], 7)
                    workspaces.append(cwd)
                self.assertEqual((out / 'workspace/marker').read_text(), str(program))
                self.assertEqual((out / 'workspace/cwd').read_text().strip(), str(cwd))
                self.assertFalse((out / 'workspace/wire').exists())
                self.assertFalse(cwd.exists())
                self.assertEqual(digest(source), expected)
                self.assertEqual(source.stat().st_nlink, 1)
            self.assertEqual(commands[0], commands[1])
            self.assertEqual(workspaces[0], workspaces[1])
            self.assertNotEqual(launch_protocol(True, True), launch_protocol(True))

    def test_executable_slot_rejects_hash_mismatch_and_missing_stable_launch(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); source = root / 'frozen'
            source.write_bytes(b'original')
            out = root / 'run'; out.mkdir()
            with self.assertRaisesRegex(ValueError, 'stable launch'):
                with execution(source, out, False, True, digest(source)):
                    self.fail('must reject before entering')
            with self.assertRaisesRegex(ValueError, 'frozen manifest'):
                with execution(source, out, True, True, 'bad hash'):
                    self.fail('must reject before entering')
            self.assertEqual(source.read_bytes(), b'original')
            self.assertEqual(source.stat().st_nlink, 1)
            self.assertFalse((root / 'execution').exists())

    @unittest.skipUnless(Path('/bin/zsh').is_file(), 'stable launch requires zsh')
    def test_stable_launch_fixes_real_argv0_and_forwards_literal_arguments(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / 'report.txt'
            run_command(stable_command(['/bin/sh', '-c', 'printf "%s" "$0"']), report,
                        cwd=directory)
            self.assertEqual(report.read_text(), 'wire')
            literal = 'spaces $(touch forbidden) `exit 9`; * "quotes"'
            result = run_command(stable_command([
                '/bin/sh', '-c', 'printf "%s" "$1"', 'fixture', literal]), report,
                cwd=directory)
            self.assertEqual(result['exit_code'], 0)
            self.assertEqual(report.read_text(), literal)
            self.assertFalse((Path(directory) / 'forbidden').exists())
            self.assertEqual(launch_protocol(True)['shell_sha256'], digest('/bin/zsh'))

    def test_child_cwd_is_recorded_without_changing_parent_or_report_path(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            child = root / 'child'; child.mkdir()
            (child / 'fixture.txt').write_text('cwd fixture')
            parent = Path.cwd()
            report = root / 'report.txt'
            result = run_command([sys.executable, '-c',
                'from pathlib import Path; print(Path("fixture.txt").read_text())'],
                report, cwd=child)
            self.assertEqual(result['exit_code'], 0)
            self.assertEqual(report.read_text(), 'cwd fixture\n')
            self.assertEqual(result['cwd'], str(child.resolve()))
            self.assertEqual(Path.cwd(), parent)

    def test_failed_process_retains_report_status_and_resource_units(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / 'report.txt'
            command = [sys.executable, '-c', 'print("fixture failure"); raise SystemExit(7)']
            result = run_command(command, report)
            self.assertEqual(result['exit_code'], 7)
            self.assertIn('fixture failure', report.read_text())
            self.assertFalse(result['timed_out'])
            self.assertGreater(result['wall_s'], 0)
            self.assertGreaterEqual(result['cpu_s'], 0)
            self.assertGreater(result['peak_rss_bytes'], 1024)

    def test_timeout_preserves_partial_report_and_negative_signal_status(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / 'report.txt'
            command = [sys.executable, '-c', 'import time; print("started", flush=True); time.sleep(10)']
            result = run_command(command, report, timeout=0.5)
            self.assertTrue(result['timed_out'])
            self.assertLess(result['exit_code'], 0)
            self.assertIn('started', report.read_text())

    def test_aba_order_and_hash_have_independent_known_values(self):
        self.assertEqual(paired_order({'first': 0, 'second': 0}, 0), ['first', 'second'])
        self.assertEqual(paired_order({'first': 0, 'second': 0}, 1), ['second', 'first'])
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'input'
            path.write_bytes(b'abc')
            self.assertEqual(digest(path), 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad')
