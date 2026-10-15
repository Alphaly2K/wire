"""Prevent acceptance reports from silently mixing incompatible evidence."""
import json
from pathlib import Path
import tempfile
import unittest

from perf.summarize import summarize


class SummaryEvidence(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def evidence(self, name, work=100, qos=None, version_key='version', after_cpu=2,
                 launch=None):
        folder = self.root / name
        folder.mkdir()
        manifest = dict(pairs=1, mode='timing', measurement_sha256='measurement', qos_clamp=qos,
                        binaries={label: dict(sha256=sha) for label, sha in
                                  [('before', 'old'), ('after', 'new'), ('control', 'old')]},
                        inputs={'integer': dict(sha256=f'input-{work}', retired=work, work=work)})
        runs = [dict(pair=0, **{version_key: label}, validated=True, timed_out=False,
                     cpu_s=cpu, wall_s=cpu + 1, peak_rss_bytes=100)
                for label, cpu in [('before', 1), ('after', after_cpu), ('control', 1.5)]]
        if launch is not None:
            manifest['launch'] = launch
        (folder / 'manifest.json').write_text(json.dumps(manifest))
        (folder / 'compare.json').write_text(json.dumps({'integer': dict(runs=runs)}))
        return folder

    def test_all_rounds_and_source_identity_survive_aggregation(self):
        first = self.evidence('first', after_cpu=9)
        second = self.evidence('second', after_cpu=1, version_key='binary')
        row = summarize([first, second])['comparisons'][0]
        self.assertEqual(row['measurements']['after']['cpu_s']['median'], 5)
        self.assertEqual(row['measurements']['after']['cpu_s']['maximum'], 9)
        self.assertEqual(len(row['runs']), 6)
        self.assertEqual([p['source'] for p in row['paired_cpu_percent']],
                         [str(first.resolve()), str(second.resolve())])
        self.assertAlmostEqual(row['same_binary_controls']['control']['before_cpu_percent'], 50)

    def test_different_work_and_qos_remain_separate_comparisons(self):
        report = summarize([self.evidence('short'), self.evidence('long', work=700),
                            self.evidence('utility', qos='utility')])
        self.assertEqual(len(report['comparisons']), 3)
        self.assertTrue(all(row['measurements']['after']['samples'] == 1
                            for row in report['comparisons']))

    def test_duplicate_incomplete_unvalidated_and_diagnostic_inputs_are_rejected(self):
        original = self.evidence('original')
        with self.assertRaisesRegex(ValueError, 'duplicate evidence'):
            summarize([original, original])
        for mode in ['incomplete', 'duplicate', 'unvalidated', 'diagnostics', 'different-binary']:
            with self.subTest(mode=mode):
                folder = self.evidence(mode)
                manifest = json.loads((folder / 'manifest.json').read_text())
                comparison = json.loads((folder / 'compare.json').read_text())
                runs = comparison['integer']['runs']
                if mode == 'incomplete':
                    runs.pop()
                elif mode == 'duplicate':
                    runs.append(runs[0])
                elif mode == 'unvalidated':
                    runs[0]['validated'] = False
                elif mode == 'diagnostics':
                    manifest['mode'] = 'diagnostics'
                else:
                    manifest['binaries']['after']['sha256'] = 'other-candidate'
                (folder / 'manifest.json').write_text(json.dumps(manifest))
                (folder / 'compare.json').write_text(json.dumps(comparison))
                with self.assertRaises(ValueError):
                    summarize([original, folder])

    def test_launch_protocols_are_never_silently_merged(self):
        legacy = self.evidence('legacy')
        explicit = self.evidence('explicit', launch='legacy')
        stable = self.evidence('stable', launch=dict(mode='stable-argv-v1', shell_sha256='shell'))
        other_shell = self.evidence('other-shell', launch=dict(mode='stable-argv-v1', shell_sha256='other'))
        rows = summarize([legacy, explicit, stable, other_shell])['comparisons']
        self.assertEqual(sorted(r['measurements']['after']['samples'] for r in rows), [1, 1, 2])

    def test_pe32_raw_ir_and_amd64_keep_distinct_workloads(self):
        amd64 = self.evidence('amd64')
        pe32 = self.evidence('pe32', version_key='binary')
        record = json.loads((pe32 / 'compare.json').read_text())['integer']
        (pe32 / 'compare.json').write_text(json.dumps(
            {'integer/ir': record, 'integer/raw': record}))
        report = summarize([amd64, pe32])
        self.assertEqual({(row['protocol']['architecture'], row['protocol']['path'])
                          for row in report['comparisons']},
                         {('x64', 'default'), ('x86', 'ir'), ('x86', 'raw')})


if __name__ == '__main__':
    unittest.main()
