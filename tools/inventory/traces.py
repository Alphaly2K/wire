"""Parse observed API calls, dynamic lookups and failure evidence."""
from collections import Counter, defaultdict
import re


CALL32 = re.compile(r'^#\d+\s+([^\s!]+)!([^\s(]+)\(')
CALL64 = re.compile(r'^\s+([^\s!]+)!([^\s]+) args=')
FAULT = re.compile(r'probe64: ([^\s!]+)!([^\s:]+):.*unimplemented')
LOOKUP = re.compile(r'(?:export=Name\("([^"\n]+)"\)|GetProcAddress lookup module=\S+ symbol=([^\s]+))')


def unescape_tsv(value):
    return re.sub(r'\\([\\trn])', lambda m: {'\\': '\\', 't': '\t', 'r': '\r', 'n': '\n'}[m[1]], value)


def trace_records(path):
    observed = defaultdict(lambda: {'count': 0, 'unimplemented': 0, 'failed': 0})
    lookups = Counter()
    with path.open(errors='replace') as stream:
        for line in stream:
            parts = line.rstrip('\n').split('\t')
            if len(parts) == 4 and parts[0] in ('completed','lookup','failed'):
                kind, dll, name, count = parts
                dll, name = unescape_tsv(dll), unescape_tsv(name)
                count = int(count)
                if count < 0: raise ValueError('negative runtime API count')
                if kind == 'lookup':
                    # Keep the provider when the full runtime inventory knows it.
                    observed.setdefault((dll.lower(), name, 'lookup'), {'count':0,'unimplemented':0,'failed':0})['count'] += count
                else:
                    value = observed[(dll.lower(), name)]
                    value['count' if kind == 'completed' else 'failed'] += count
                continue
            match = CALL32.match(line) or CALL64.match(line)
            if match:
                key = (match[1].lower(), match[2])
                observed[key]['count'] += 1
                observed[key]['unimplemented'] += '[UNIMPLEMENTED]' in line
            match = FAULT.search(line)
            if match:
                observed[(match[1].lower(), match[2])]['unimplemented'] += 1
                observed[(match[1].lower(), match[2])]['failed'] += 1
            match = LOOKUP.search(line)
            if match:
                lookups[match[1] or match[2]] += 1
    out = [{'dll': key[0], 'name': key[1], 'evidence': 'runtime_lookup' if len(key) == 3 else 'runtime', **value}
           for key, value in observed.items()]
    out += [{'dll': '?', 'name': name, 'evidence': 'runtime_lookup', 'count': count} for name, count in lookups.items()]
    return out
