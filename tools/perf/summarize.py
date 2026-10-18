#!/usr/bin/env python3
"""Combine frozen timing evidence without merging different workloads/protocols."""
import argparse
import json
import math
from pathlib import Path
import statistics


METRICS = ('cpu_s', 'wall_s', 'peak_rss_bytes')


def summarize(directories, before='before', after='after'):
    sources = []
    binaries = {}
    groups = {}
    seen = set()
    for directory in directories:
        source = Path(directory).resolve(strict=True)
        if source in seen:
            raise ValueError(f'duplicate evidence directory: {source}')
        seen.add(source)
        manifest = json.loads((source / 'manifest.json').read_text())
        if manifest.get('mode', 'timing') != 'timing':
            raise ValueError(f'non-timing evidence: {source}')
        for label, binary in manifest['binaries'].items():
            if label in binaries and binaries[label] != binary['sha256']:
                raise ValueError(f'binary hash changed for {label}: {source}')
            binaries[label] = binary['sha256']
        if not {before, after} <= manifest['binaries'].keys():
            raise ValueError(f'missing comparison labels: {source}')
        sources.append(dict(directory=str(source), manifest=manifest))
        results = json.loads((source / 'compare.json').read_text())
        if not results:
            raise ValueError(f'empty comparison: {source}')
        for case, record in results.items():
            pe32 = '/' in case
            name, path = case.rsplit('/', 1) if pe32 else (case, 'default')
            protocol = dict(architecture='x86' if pe32 else 'x64', case=name, path=path,
                            input=manifest['inputs'][name], qos=manifest.get('qos_clamp'),
                            launch=manifest.get('launch', 'legacy'),
                            api_log=manifest.get('api_log'), caches=manifest.get('caches', {}),
                            script_sha256=manifest.get('script_sha256'),
                            replay_sha256=manifest.get('replay_sha256'),
                            fixtures_sha256=manifest.get('fixtures_sha256'),
                            measurement_sha256=manifest['measurement_sha256'],
                            taskpolicy_sha256=manifest.get('taskpolicy_sha256'))
            key = json.dumps(protocol, sort_keys=True)
            group = groups.setdefault(key, dict(protocol=protocol, runs=[]))
            pairs = {}
            for run in record['runs']:
                label = run.get('binary', run.get('version'))
                if (label not in manifest['binaries'] or run.get('validated') is not True
                        or run.get('timed_out', False)
                        or ('binary' in run and 'version' in run
                            and run['binary'] != run['version'])):
                    raise ValueError(f'invalid/unvalidated run: {source}/{case}')
                if (isinstance(manifest.get('launch'), dict)
                        and manifest['launch'].get('mode') == 'fixed-executable-slot-v1'
                        and run.get('executable_slot_sha256') != manifest['binaries'][label]['sha256']):
                    raise ValueError(f'executable slot hash mismatch: {source}/{case}')
                if any(not isinstance(run.get(metric), (int, float))
                       or not math.isfinite(run[metric]) or run[metric] <= 0
                       for metric in METRICS):
                    raise ValueError(f'invalid measurement: {source}/{case}')
                pair = run['pair']
                if pair not in range(manifest['pairs']):
                    raise ValueError(f'invalid pair index: {source}/{case}')
                members = pairs.setdefault(pair, {})
                if label in members:
                    raise ValueError(f'duplicate run: {source}/{case}/{pair}/{label}')
                members[label] = run
                group['runs'].append(dict(run, label=label, source=str(source)))
            if (len(pairs) != manifest['pairs']
                    or any(set(p) != set(manifest['binaries']) for p in pairs.values())):
                raise ValueError(f'incomplete pairs: {source}/{case}')

    for group in groups.values():
        runs = group['runs']
        labels = list(dict.fromkeys(run['label'] for run in runs))
        group['measurements'] = {label: dict(
            samples=sum(run['label'] == label for run in runs),
            **{metric: dict(median=statistics.median(values), minimum=min(values), maximum=max(values))
               for metric in METRICS
               for values in [[run[metric] for run in runs if run['label'] == label]]})
            for label in labels}
        med = lambda label, metric: group['measurements'][label][metric]['median']
        group['after_before_cpu_percent'] = 100 * (med(after, 'cpu_s') / med(before, 'cpu_s') - 1)
        group['after_before_rss_bytes'] = med(after, 'peak_rss_bytes') - med(before, 'peak_rss_bytes')
        group['paired_cpu_percent'] = []
        for source, pair in dict.fromkeys((run['source'], run['pair']) for run in runs):
            members = {r['label']: r for r in runs if r['source'] == source and r['pair'] == pair}
            group['paired_cpu_percent'].append(dict(source=source, pair=pair,
                change=100 * (members[after]['cpu_s'] / members[before]['cpu_s'] - 1)))
        group['same_binary_controls'] = {
            label: dict(before_cpu_percent=100 * (med(label, 'cpu_s') / med(before, 'cpu_s') - 1),
                        after_cpu_percent=100 * (med(after, 'cpu_s') / med(label, 'cpu_s') - 1))
            for label in labels if label not in (before, after) and binaries[label] == binaries[before]}
    return dict(schema_version=1, before=before, after=after, binaries=binaries,
                sources=sources, comparisons=list(groups.values()))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directories', nargs='+', type=Path)
    parser.add_argument('--out', required=True, type=Path)
    parser.add_argument('--before', default='before')
    parser.add_argument('--after', default='after')
    args = parser.parse_args()
    if args.before == args.after:
        parser.error('before and after labels must differ')
    try:
        report = summarize(args.directories, args.before, args.after)
        with args.out.open('x') as output:
            output.write(json.dumps(report, indent=2) + '\n')
    except (ValueError, KeyError, OSError) as error:
        parser.exit(1, f'{error}\n')
    for group in report['comparisons']:
        p = group['protocol']
        print(f"{p['architecture']}/{p['case']}/{p['path']} "
              f"retired={p['input']['retired']} CPU={group['after_before_cpu_percent']:+.3f}% "
              f"samples={group['measurements'][args.after]['samples']}")


if __name__ == '__main__':
    main()
