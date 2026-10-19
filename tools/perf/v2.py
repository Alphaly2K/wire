#!/usr/bin/env python3
"""Serial frozen V2 probes; extend the audit fixtures without overwriting evidence."""
import argparse, importlib.util, json, statistics, struct, sys
from pathlib import Path
sys.dont_write_bytecode=True
from .amd64 import replay
from .measurement import digest
from .measurement import launch_protocol, paired_order
from .fixtures.pe import pe64

def bit_fixture(work):
    n=max(1,work//6)
    c=bytearray.fromhex('48b80100000000000000 48b90300000000000000 41bf')+struct.pack('<I',n)
    loop=len(c);c+=bytes.fromhex('480fbbc8 480fb3c8 480fabc8 480fa3c8 49ffcf')
    at=len(c);c+=bytes.fromhex('0f85')+struct.pack('<i',loop-at-6)+bytes.fromhex('c3')
    return pe64(c),dict(rax='0x9',work=6*n+4,retired=6*n+4)

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--binary',action='append',required=True)
    p.add_argument('--audit-probes',type=Path,
                   default=Path(__file__).resolve().parent/'fixtures/amd64_audit.py',
                   help='fixture module; defaults to the versioned V2 acceptance inputs')
    p.add_argument('--out',type=Path,required=True)
    p.add_argument('--cases',nargs='+',default=['fp-scalar','bit-test'])
    p.add_argument('--work',type=int,default=100000000)
    p.add_argument('--pairs',type=int,default=3)
    p.add_argument('--mode',choices=['timing','stats','diagnostics'],default='timing')
    p.add_argument('--stable-launch',action='store_true',help='fix guest argv0 and use each run directory as host cwd')
    p.add_argument('--fixed-executable-slot',action='store_true',help='also fix executable path/cwd across labels; requires --stable-launch')
    args=p.parse_args();binaries={n:Path(v).resolve(strict=True) for n,v in (a.split('=',1) for a in args.binary)}
    if args.fixed_executable_slot and not args.stable_launch:p.error('--fixed-executable-slot requires --stable-launch')
    spec=importlib.util.spec_from_file_location('audit_probes',args.audit_probes.resolve(strict=True));audit=importlib.util.module_from_spec(spec)
    # The versioned fixture uses package imports; external audit modules keep
    # their original standalone loading behavior.
    if args.audit_probes.resolve() == Path(__file__).resolve().parent/'fixtures/amd64_audit.py':
        audit.__package__ = __package__ + '.fixtures'
    spec.loader.exec_module(audit)
    args.out.mkdir(parents=True,exist_ok=False)
    manifest=dict(binaries={n:dict(path=str(v),sha256=digest(v)) for n,v in binaries.items()},fixtures_sha256=digest(args.audit_probes),script_sha256=digest(Path(__file__)),replay_sha256=digest(Path(__file__).with_name('amd64.py')),measurement_sha256=digest(Path(__file__).with_name('measurement.py')),mode=args.mode,pairs=args.pairs,inputs={})
    results={}
    manifest['launch']=launch_protocol(args.stable_launch,args.fixed_executable_slot)
    for name in args.cases:
        data,expected=bit_fixture(args.work) if name=='bit-test' else audit.fixture(name,args.work)
        exe=args.out/f'{name}.exe';exe.write_bytes(data)
        manifest['inputs'][name]=dict(sha256=digest(exe),**expected)
        (args.out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
        runs=[]
        for pair in range(args.pairs):
            for version in paired_order(binaries, pair):
                r=replay(binaries[version],exe,expected,args.out/name/f'pair-{pair}-{version}',args.mode,180,stable_launch=args.stable_launch,fixed_executable=args.fixed_executable_slot,binary_sha256=manifest['binaries'][version]['sha256'])
                r.update(pair=pair,version=version);runs.append(r)
                print(f'{name} pair={pair} {version}: wall={r["wall_s"]:.4f} cpu={r["cpu_s"]:.4f} RSS={r["peak_rss_bytes"]}',flush=True)
        results[name]=dict(runs=runs,medians={v:{k:statistics.median(r[k] for r in runs if r['version']==v) for k in ['wall_s','cpu_s','peak_rss_bytes']} for v in binaries})
        (args.out/'compare.json').write_text(json.dumps(results,indent=2)+'\n')
if __name__=='__main__':main()
