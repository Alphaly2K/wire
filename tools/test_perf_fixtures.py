"""Migration checks against hashes recorded from the original V2 audit."""
import hashlib
import json
from pathlib import Path
import unittest

from perf.fixtures.amd64_audit import fixture


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


if __name__ == '__main__':
    unittest.main()
