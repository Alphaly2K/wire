"""Independent process/status/timeout checks for the common measurement layer."""
from pathlib import Path
import sys
import tempfile
import unittest
from perf.measurement import digest, paired_order, run_command


class MeasurementTests(unittest.TestCase):
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
