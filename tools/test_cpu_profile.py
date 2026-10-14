"""Runtime samples retain legacy and architecture paths while excluding loading."""
import contextlib
import io
import json
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch
from cpu_profile import summarize


class ProfileTests(unittest.TestCase):
    def test_architecture_and_legacy_frames_preserve_runtime_weights_and_callback_partition(self):
        for root, cpu in [('wire::x86::process::Process32', 'wire::x86::cpu'),
                          ('wire::cpu::Cpu', 'wire::cpu')]:
            method = 'run' if 'process' in root else 'run_loop'
            tree = ('Call graph:\n'
                    '100 wire::main\n'
                    '  20 wire::loader::load\n'
                    f'  80 <{root}>::{method}\n'
                    f'    50 <{cpu}::Cpu>::steps_cached\n'
                    f'      20 {cpu}::fpu::arithmetic\n'
                    '    30 wire::api::dispatch\n'
                    f'      10 <{root}>::{method}\n'
                    f'        10 <{cpu}::Cpu>::step_x86\n'
                    'Total number in stack\n')
            with tempfile.TemporaryDirectory() as directory:
                source = Path(directory) / 'sample.txt'
                source.write_text(tree)
                out = Path(directory) / 'summary'
                with patch('cpu_profile.demangle', side_effect=lambda symbols: {s: s for s in symbols}), contextlib.redirect_stdout(io.StringIO()):
                    summarize(SimpleNamespace(samples=[source], out=out))
                report = json.loads((out / 'summary.json').read_text())[0]
                self.assertEqual(report['samples'], {'runtime': 80,
                                 'IR execution and cache': 30,
                                 'x87 software execution': 20,
                                 'API outside guest execution': 20,
                                 'other raw fallback': 10})
                roots = [item for item in report['inclusive_hotspots'] if item['symbol'] == f'<{root}>::{method}']
                self.assertEqual(roots[0]['samples'], 80)
