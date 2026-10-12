#!/usr/bin/env python3
"""Join read-only Cecil declarations, actual native lookup and original targets.

Lookup/declaration coverage is not execution coverage or proof of semantics.
No game code runs here; only the installed development SDK runs the scanner.
"""
import argparse
from collections import Counter, defaultdict
import csv
import json
import os
from pathlib import Path
import re
import subprocess
import sys

from probe import DEFAULT_SDK, HERE, REPO, sha256
sys.path.insert(0, str(REPO / 'tools'))
from api_inventory import PE


def rows(path):
    with path.open() as stream:
        return list(csv.DictReader(stream, delimiter='\t'))


def write_rows(path, fields, values):
    with path.open('w', newline='') as stream:
        writer = csv.DictWriter(stream, fieldnames=fields, delimiter='\t', lineterminator='\n')
        writer.writeheader()
        writer.writerows(values)


def binding_key(name):
    return name.split('(', 1)[0].replace('/', '.')


def abi_group(row):
    """Classify representation, without admitting pointer/lifetime semantics."""
    if row['kind'] == 'PInvoke':
        return 'external-native-library'
    if row.get('generic') == '1':
        return 'generic-signature'
    shapes = [row['return_shape']] + (row['parameter_shapes'].split(';') if row['parameter_shapes'] else [])
    if any('raw-pointer' in shape or 'native-int' in shape for shape in shapes):
        return 'pointer-or-capability-contract'
    if any('unresolved' in shape or 'open-generic' in shape or 'recursive-or-deep' in shape for shape in shapes):
        return 'unresolved-or-complex-layout'
    if any('managed-reference' in shape or 'array:' in shape or 'string' in shape for shape in shapes):
        return 'reference-string-array'
    if any('value{' in shape or 'byref:' in shape for shape in shapes):
        return 'value-or-byref'
    return 'scalar'


def join_declarations(declarations, lookups, registrations):
    lookup = {}
    for row in lookups:
        key = (row['assembly'], row['token'].lower())
        if key in lookup:
            raise ValueError(f'duplicate runtime method identity: {key}')
        lookup[key] = row
    targets = defaultdict(set)
    for row in registrations:
        targets[binding_key(row['name'])].add(row['guest_target'])
    result = []
    for declaration in declarations:
        row = dict(declaration)
        actual = lookup.get((row['assembly'], row['token'].lower()))
        if actual and actual['kind'] != row['kind']:
            raise ValueError('runtime/offline method kind changed')
        row['lookup'] = actual['lookup'] if actual else 'not_in_runtime_snapshot'
        binding = binding_key(row['class'] + '::' + row['method'])
        row['binding'] = binding
        row['guest_targets'] = ';'.join(sorted(targets.get(binding, ())))
        row['semantics_verified_by_inventory'] = '0'
        result.append(row)
    return result


def export_candidates(sample, observed):
    # Code references to dispatch conditions are candidates only; a guarded
    # handler can still reject a signature, thread, lifetime or pointer shape.
    source = '\n'.join(path.read_text() for path in sorted((REPO / 'src/x64/process').rglob('*.rs')))
    source += (REPO / 'src/x64/process/mono_startup.rs').read_text()
    candidates = set(re.findall(r'function\s*==\s*"([^"]+)"', source))
    for group in re.findall(r'matches!\(\s*function\s*,([^)]*)\)', source, re.S):
        candidates.update(re.findall(r'"([^"]+)"', group))
    for group in re.findall(r'((?:"(?:mono_|unity_mono_|burst_mono_)[^"]+"\s*\|?\s*)+)\s*=>', source):
        candidates.update(re.findall(r'"([^"]+)"', group))
    hits = Counter()
    for row in observed:
        if row['dll'].lower() == 'mono-2.0-bdwgc.dll' and row['kind'] == 'completed':
            hits[row['name']] += int(row['count'])
    return [{'export': name, 'source_dispatch_candidate': int(name in candidates),
             'observed_completed': hits[name], 'semantics_verified_by_inventory': 0}
            for name in sorted(PE(sample.read_bytes()).exports()) if not name.startswith('#')]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--managed', type=Path, required=True)
    parser.add_argument('--runtime-audit', type=Path, required=True)
    parser.add_argument('--sample-mono', type=Path, required=True)
    parser.add_argument('--observed-api', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--sdk', type=Path, default=DEFAULT_SDK)
    args = parser.parse_args()
    output = args.output.resolve()
    if not output.is_relative_to(REPO / 'wire-out'):
        parser.error('output must be below wire-out')
    output.mkdir(parents=True, exist_ok=True)
    (output / 'summary.json').unlink(missing_ok=True)
    sdk = args.sdk.resolve()
    env = {key: value for key, value in os.environ.items() if not key.startswith('MONO_')}
    env.pop('DYLD_INSERT_LIBRARIES', None)
    env['MONO_PATH'] = str(sdk.parent / 'Managed')
    executable = output / 'BridgeInventory.exe'
    subprocess.run([str(sdk / 'bin/mono'), str(sdk / 'lib/mono/4.5/mcs.exe'),
                    '-r:' + str(sdk.parent / 'Managed/Unity.Cecil.dll'), '-out:' + str(executable),
                    str(HERE / 'BridgeInventory.cs')], env=env, check=True, timeout=60)
    subprocess.run([str(sdk / 'bin/mono'), str(executable), str(args.managed.resolve()), str(output)],
                   env=env, check=True, timeout=60)
    inputs = {row['assembly']: row['sha256'] for row in rows(output / 'assembly-inputs.tsv')}
    runtime = args.runtime_audit.resolve()
    for row in rows(runtime / 'runtime-images.tsv'):
        filename = Path(row['name']).name
        if filename in inputs and inputs[filename] != row['sha256']:
            raise ValueError(f'runtime/static image fingerprint differs: {filename}')
    declarations = rows(output / 'managed-declarations.tsv')
    lookups = rows(runtime / 'runtime-lookup.tsv')
    runtime_hashes = {row['sha256'] for row in rows(runtime / 'runtime-images.tsv')}
    for row in lookups:
        if row['assembly'] not in inputs or inputs[row['assembly']] not in runtime_hashes:
            raise ValueError('runtime lookup method lacks matching input fingerprint: ' + row['assembly'])
    registrations = rows(runtime / 'engine-registrations.tsv')
    joined = join_declarations(declarations, lookups, registrations)
    fields = list(joined[0])
    write_rows(output / 'bridge-gaps.tsv', fields, joined)
    families = defaultdict(Counter)
    for row in joined:
        family = families[(row['assembly'], row['class'])]
        family['declarations'] += 1
        family[row['lookup']] += 1
        if row['lookup'] == 'missing':
            family['missing_il_references'] += int(row['il_references'])
            family['scalar_candidates'] += row['scalar_candidate'] == '1'
            family['referenced_missing'] += int(row['il_references']) > 0
    family_rows = [{'assembly': assembly, 'class': owner, **{key: count[key] for key in
                   ('declarations', 'resolved', 'missing', 'referenced_missing', 'missing_il_references',
                    'scalar_candidates', 'not_in_runtime_snapshot', 'not_resolved_by_audit', 'unresolved_metadata')}}
                   for (assembly, owner), count in families.items()]
    family_rows.sort(key=lambda row: (-row['referenced_missing'], -row['missing_il_references'], row['class']))
    write_rows(output / 'families.tsv', list(family_rows[0]), family_rows)
    groups = defaultdict(Counter)
    for row in joined:
        group = groups[abi_group(row)]
        group['declarations'] += 1
        group[row['lookup']] += 1
        group['referenced_missing'] += row['lookup'] == 'missing' and int(row['il_references']) > 0
    abi_rows = [{'abi': name, **{key: count[key] for key in
                ('declarations', 'resolved', 'missing', 'referenced_missing', 'not_in_runtime_snapshot', 'not_resolved_by_audit')}}
                for name, count in sorted(groups.items())]
    write_rows(output / 'abi-groups.tsv', list(abi_rows[0]), abi_rows)
    original_bindings = Counter(row['name'] for row in registrations)
    metadata_bindings = Counter(row['binding'] for row in joined if row['kind'] == 'InternalCall')
    engine_rows = [{'binding': row['name'], 'guest_target': row['guest_target'],
                    'declaration_candidates': metadata_bindings[binding_key(row['name'])]}
                   for row in registrations]
    write_rows(output / 'engine-bindings.tsv', list(engine_rows[0]), engine_rows)
    exports = export_candidates(args.sample_mono, rows(args.observed_api) if args.observed_api else [])
    write_rows(output / 'mono-exports.tsv', list(exports[0]), exports)
    status = Counter(row['lookup'] for row in joined if row['kind'] == 'InternalCall')
    errors = rows(output / 'metadata-errors.tsv')
    summary = {'schema': 1, 'assemblies': len(inputs), 'metadata_errors': errors,
               'internal_calls': sum(row['kind'] == 'InternalCall' for row in joined),
               'pinvokes': sum(row['kind'] == 'PInvoke' for row in joined),
               'internal_call_lookup': dict(status), 'engine_bindings': len(original_bindings),
               'engine_bindings_without_declaration': sum(not row['declaration_candidates'] for row in engine_rows),
               'referenced_missing_icalls': sum(row['lookup'] == 'missing' and int(row['il_references']) > 0 for row in joined),
               'missing_scalar_candidates': sum(row['lookup'] == 'missing' and row['scalar_candidate'] == '1' for row in joined),
               'mono_exports': len(exports), 'game_methods_executed_by_inventory': 0,
               'semantics_verified_by_inventory': False,
               'input_hashes': {str(path): sha256(path) for path in
                   [runtime / 'runtime-lookup.tsv', runtime / 'engine-registrations.tsv',
                    runtime / 'runtime-images.tsv', args.sample_mono.resolve(), HERE / 'BridgeInventory.cs']},
               'limits': ['Native lookup resolution does not verify adapter semantics or execution coverage',
                          'Static IL references count call sites, not execution frequency or reachability',
                          'PInvoke declarations are never resolved or executed by this audit',
                          'Source dispatch matches are candidates, not complete Mono export contracts',
                          'Not-in-snapshot methods remain unknown; they are not classified as implemented']}
    (output / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(json.dumps({key: summary[key] for key in ('assemblies', 'internal_calls', 'pinvokes',
          'internal_call_lookup', 'referenced_missing_icalls', 'missing_scalar_candidates', 'mono_exports')}, indent=2))


if __name__ == '__main__':
    main()
