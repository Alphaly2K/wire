"""Migration checks against hashes recorded from the original V2 audit."""
import hashlib
import json
from pathlib import Path
import unittest

from perf.fixtures.amd64_audit import fixture
from perf.fixtures import kernel_gateway
from interpreter32_perf import pe32
from api_inventory import PE


class AuditFixtureMigration(unittest.TestCase):
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
