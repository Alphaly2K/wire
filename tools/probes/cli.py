"""Compare frozen CLI behavior using bounded, original PE fixtures."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

from perf.fixtures.amd64_basic import fixtures
from perf.fixtures.pe32 import fixture
from perf.measurement import digest, stable_command


def normalized_output(text):
    text = re.sub(r'(instructions: \d+ in )[0-9.]+(s \()[0-9.]+( Minstr/s\))',
                  r'\1<TIME>\2<RATE>\3', text)
    text = re.sub(r'(seconds=)[0-9.e+-]+( MIPS=)[0-9.e+-]+',
                  r'\1<TIME>\2<RATE>', text)
    return re.sub(r'( bytes in )[0-9.]+(s;)', r'\1<TIME>\2', text)


def canonical_trace(trace):
    # Calls remain byte-for-byte ordered. Equal-frequency entries have the
    # original process-random HashMap order, so compare footer contents.
    all_header = b'; ---- API call frequency (all) ----\n'
    missing_header = b'; ---- UNIMPLEMENTED API frequency ----\n'
    calls, footer = trace.split(all_header, 1)
    all_rows, missing_rows = footer.split(missing_header, 1)
    return (calls + all_header + b''.join(sorted(all_rows.splitlines(keepends=True)))
            + missing_header + b''.join(sorted(missing_rows.splitlines(keepends=True))))


def cases(integer32, integer64, controls, integer_ir, controls_ir):
    run = ['--headless', '--max-instr', '1000', '--log-dir', 'logs']
    return [
        ('usage', []),
        ('short', ['run']),
        ('unknown', ['unknown', 'program']),
        ('prepare-option', ['prepare', 'missing.exe', '--bad']),
        ('prepare-missing', ['prepare', 'missing.exe']),
        ('run-option', ['run', 'missing.exe', '--bad']),
        ('run-budget', ['run', 'missing.exe', '--max-instr', 'bad']),
        ('run-missing', ['run', 'missing.exe']),
        ('probe-option', ['probe64', 'missing.exe', '--bad']),
        ('probe-budget', ['probe64', 'missing.exe', '--max-instr', 'bad']),
        ('probe-missing', ['probe64', 'missing.exe']),
        ('imports-missing', ['imports', 'missing.exe']),
        ('imports32', ['imports', str(integer32)]),
        ('imports64', ['imports', str(integer64)]),
        ('prepare32', ['prepare', str(integer32), '--out', str(integer_ir)]),
        ('run-raw', ['run', str(integer32), *run]),
        ('run-ir', ['run', str(integer32), *run, '--ir', str(integer_ir)]),
        ('probe64', ['probe64', str(integer64), '--max-instr', '1000']),
        ('probe64-stats', ['probe64', str(integer64), '--max-instr', '1000', '--cpu-stats']),
        ('common-controls-raw', ['run', str(controls), *run]),
        ('common-controls-ir', ['run', str(controls), *run, '--ir', str(controls_ir)]),
    ]


def compare(before, after, out):
    bins = {'before': Path(before).resolve(strict=True),
            'after': Path(after).resolve(strict=True)}
    out = Path(out).resolve()
    out.mkdir(parents=True, exist_ok=False)
    workspace = out / 'workspace'
    workspace.mkdir()
    inputs = out / 'inputs'
    inputs.mkdir()
    integer32, integer64, controls = (inputs / name for name in
                                    ['integer32.exe', 'integer64.exe', 'controls.exe'])
    integer32.write_bytes(fixture('integer', 12000)[0])
    integer64.write_bytes(next(data for name, data in fixtures(12000) if name == 'integer'))
    controls.write_bytes(fixture('gateway-common-controls', 700)[0])
    controls_ir = inputs / 'controls.wir'
    integer_ir = workspace / 'integer.wir'
    checks = cases(integer32, integer64, controls, integer_ir, controls_ir)
    manifest = dict(
        purpose='functional CLI compatibility; no timing admission',
        binaries={name: dict(path=str(p), sha256=digest(p)) for name, p in bins.items()},
        script_sha256=digest(Path(__file__)), cwd=str(workspace), argv0='wire',
        inputs={str(p): digest(p) for p in [integer32, integer64, controls]},
        cases=checks,
        normalization='prepare time; PE32/AMD64 seconds and rate; frequency footer row order')
    manifest_path = out / 'manifest.json'
    manifest_path.write_text(json.dumps(manifest, indent=2) + '\n')

    def execute(label, arguments):
        slot = out / 'wire'
        slot.symlink_to(bins[label])
        try:
            if digest(slot) != manifest['binaries'][label]['sha256']:
                raise ValueError('staged executable differs from the manifest')
            command = stable_command([str(slot), *arguments])
            run = subprocess.run(command, cwd=workspace, capture_output=True,
                                 text=True, timeout=30)
            return run, command
        finally:
            slot.unlink()

    preparation, command = execute('before', ['prepare', str(controls), '--out', str(controls_ir)])
    (out / 'prepare-controls.stdout').write_text(preparation.stdout)
    (out / 'prepare-controls.stderr').write_text(preparation.stderr)
    manifest['preparation'] = dict(command=command, exit_code=preparation.returncode)
    manifest_path.write_text(json.dumps(manifest, indent=2) + '\n')
    if preparation.returncode != 0:
        raise RuntimeError('baseline could not prepare the control fixture')
    manifest['inputs'][str(controls_ir)] = digest(controls_ir)
    manifest_path.write_text(json.dumps(manifest, indent=2) + '\n')

    results = []
    trace_cases = {'run-raw', 'run-ir', 'common-controls-raw', 'common-controls-ir'}
    for case, arguments in checks:
        row = dict(case=case, args=arguments, results={})
        for label in bins:
            run, command = execute(label, arguments)
            (out / f'{case}-{label}.stdout').write_text(run.stdout)
            (out / f'{case}-{label}.stderr').write_text(run.stderr)
            value = dict(exit_code=run.returncode, stdout=normalized_output(run.stdout),
                         stderr=run.stderr, command=command)
            if case in trace_cases:
                trace = (workspace / 'logs/api.log').read_bytes()
                (out / f'{case}-{label}.api.log').write_bytes(trace)
                row.setdefault('raw_api_trace_sha256', {})[label] = hashlib.sha256(trace).hexdigest()
                value['api_trace_sha256'] = hashlib.sha256(canonical_trace(trace)).hexdigest()
            if case == 'prepare32':
                prepared = integer_ir.read_bytes()
                (out / f'{case}-{label}.wir').write_bytes(prepared)
                value['prepared_sha256'] = hashlib.sha256(prepared).hexdigest()
            row['results'][label] = value
        row['matched'] = row['results']['before'] == row['results']['after']
        results.append(row)
        (out / 'compare.json').write_text(json.dumps(results, indent=2) + '\n')
        print(case, row['matched'], flush=True)
        if not row['matched']:
            raise RuntimeError(f'CLI mismatch: {case}')
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--before', required=True, type=Path)
    parser.add_argument('--after', required=True, type=Path)
    parser.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    compare(args.before, args.after, args.out)


if __name__ == '__main__':
    main()
