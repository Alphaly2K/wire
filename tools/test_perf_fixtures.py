"""Migration checks against hashes recorded from the original V2 audit."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from perf.fixtures.amd64_audit import fixture
from perf.fixtures import kernel_gateway
from perf.fixtures.pe32 import fixture as pe32_fixture
from interpreter32_perf import pe32
from api_inventory import PE


class AuditFixtureMigration(unittest.TestCase):
    def test_snapshot_copyout_keeps_three_independent_kernel_imports(self):
        from perf.fixtures.snapshot_gateway import x64
        for work in [1, 12000, 100000000]:
            with self.subTest(work=work):
                data, _ = x64(work)
                self.assertEqual([(row['dll'], row['name'], row['iat_rva'])
                                  for row in PE(data).imports()],
                                 [('kernel32.dll', 'CreateToolhelp32Snapshot', 0x1180),
                                  ('kernel32.dll', 'Process32FirstW', 0x1188),
                                  ('kernel32.dll', 'CloseHandle', 0x1190)])

    def test_token_queries_keep_exact_provider_and_independent_import_tables(self):
        from perf.fixtures.token_gateway import x64
        for work in [1, 12000, 100000000]:
            with self.subTest(work=work):
                data, _ = x64(work)
                self.assertEqual([(row['dll'], row['name'], row['iat_rva'])
                                  for row in PE(data).imports()],
                                 [('advapi32.dll', 'OpenProcessToken', 0x1180),
                                  ('advapi32.dll', 'GetTokenInformation', 0x1188)])

    def test_etw_lookup_and_iat_keep_all_eight_imports_independent(self):
        from perf.fixtures.tracing_gateway import x64
        for work in [1, 12000, 100000000]:
            with self.subTest(work=work):
                data, _ = x64(work)
                self.assertEqual([(row['dll'], row['name'], row['iat_rva'])
                                  for row in PE(data).imports()],
                                 [('advapi32.dll', name, 0x1290 + 8 * index)
                                  for index, name in enumerate([
                                      'EventRegister', 'EventUnregister', 'EventEnabled',
                                      'EventProviderEnabled', 'EventWrite', 'EventWriteString',
                                      'EventWriteTransfer', 'EventSetInformation'])])

    def test_pe32_rejects_unknown_cases_instead_of_substituting_integer(self):
        for name in ['gdi-gateway', 'Integer', '', 'not-a-fixture']:
            with self.subTest(case=name), self.assertRaisesRegex(ValueError, 'unknown PE32 fixture'):
                pe32_fixture(name, 12000)
        image, expected = pe32_fixture('integer', 12000)
        self.assertEqual(hashlib.sha256(image).hexdigest(),
                         'b30ac7d6438224c178e4277057bdc0bb666bd9c2e50644264756118710d33f79')
        self.assertEqual(expected, dict(eax=8001, retired=12003))

    def test_pe32_cli_rejects_mixed_invalid_cases_before_io_or_binary_lookup(self):
        script = Path(__file__).parent / 'interpreter32_perf.py'
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            out = root / 'must-not-be-created'
            result = subprocess.run([
                sys.executable, '-B', str(script), '--binary', f'before={root / "missing"}',
                '--cases', 'integer', 'gdi-gateway', '--out', str(out)],
                capture_output=True, text=True, cwd=root, timeout=10)
            self.assertEqual(result.returncode, 2)
            self.assertIn('unknown PE32 fixture: gdi-gateway', result.stderr)
            self.assertNotIn('Traceback', result.stderr)
            self.assertEqual(result.stdout, '')
            self.assertFalse(out.exists())

    def test_pe_bytes_and_expected_retirement_match_original_audit(self):
        golden = json.loads((Path(__file__).parent /
                             'perf/fixtures/amd64_audit_golden.json').read_text())
        for work, cases in golden.items():
            for name, before in cases.items():
                with self.subTest(work=work, case=name):
                    image, expected = fixture(name, int(work))
                    self.assertEqual(hashlib.sha256(image).hexdigest(), before['sha256'])
                    self.assertEqual({key: expected[key] for key in ('rax', 'retired', 'work')},
                                     {key: before[key] for key in ('rax', 'retired', 'work')})

    def test_event_lookup_and_iat_have_exactly_three_independent_imports(self):
        for width, make in [(4, lambda work: kernel_gateway.x86(work, pe32)),
                            (8, kernel_gateway.x64)]:
            for work in [12000, 100000000]:
                with self.subTest(width=width, work=work):
                    data, _ = make(work)
                    imports = PE(data).imports()
                    self.assertEqual([(row['dll'], row['name'], row['iat_rva'])
                                      for row in imports],
                                     [('kernel32.dll', name, 0x1190 + i * width)
                                      for i, name in enumerate(
                                          ['SetEvent', 'ResetEvent', 'CreateEventW'])])
                    lookup = int.from_bytes(data[0x300:0x304], 'little')
                    self.assertLessEqual(lookup + 4 * width, 0x1190)


if __name__ == '__main__':
    unittest.main()
