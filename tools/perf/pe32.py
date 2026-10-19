#!/usr/bin/env python3
"""Frozen PE32 raw/static-IR comparisons with algebraic state and budget checks."""
import argparse,json,os,re,shutil,statistics,struct,subprocess,sys
from pathlib import Path
sys.dont_write_bytecode=True
from .measurement import digest, execution, launch_protocol, paired_order, run_command, stable_command

from .fixtures.pe32 import (pe32, guid_gateway_fixture, gdi_gateway_fixture,
                            version_gateway_fixture, nls_conversion_fixture, fixture)

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--binary',action='append',required=True);p.add_argument('--out',type=Path,required=True);p.add_argument('--pairs',type=int,default=3);p.add_argument('--work',type=int,default=30000000);p.add_argument('--cases',nargs='+',default=['sse-register','fp-scalar','bit-test','integer']);p.add_argument('--paths',nargs='+',default=['ir','raw']);p.add_argument('--discard-api-log',action='store_true',help='send API trace writes to the null device, retaining formatting and counters')
    p.add_argument('--qos-clamp',choices=['utility','background','maintenance'],help='optional macOS taskpolicy clamp applied equally to every binary')
    p.add_argument('--stable-launch',action='store_true',help='fix guest argv0/log argument and use each run directory as host cwd')
    p.add_argument('--fixed-executable-slot',action='store_true',help='also fix executable path/cwd across labels; requires --stable-launch')
    a=p.parse_args()
    if a.fixed_executable_slot and not a.stable_launch:p.error('--fixed-executable-slot requires --stable-launch')
    if a.qos_clamp and sys.platform != 'darwin':
        p.error('--qos-clamp requires macOS taskpolicy')
    if a.qos_clamp and shutil.which('taskpolicy') is None:
        p.error('taskpolicy is unavailable')
    bins={n:Path(v).resolve(strict=True) for n,v in (s.split('=',1) for s in a.binary)};a.out.mkdir(parents=True,exist_ok=False)
    launch=launch_protocol(a.stable_launch,a.fixed_executable_slot)
    if a.stable_launch:a.out=a.out.resolve()
    manifest=dict(binaries={n:dict(path=str(v),sha256=digest(v)) for n,v in bins.items()},script_sha256=digest(Path(__file__)),measurement_sha256=digest(Path(__file__).with_name('measurement.py')),pairs=a.pairs,api_log='discard' if a.discard_api_log else 'file',qos_clamp=a.qos_clamp,taskpolicy_sha256=digest(Path(shutil.which('taskpolicy'))) if a.qos_clamp else None,inputs={});results={}
    manifest['launch']=launch
    for case in a.cases:
        data,expected=fixture(case,a.work);exe=a.out/f'{case}.exe';exe.write_bytes(data);ir=a.out/f'{case}.wir'
        subprocess.run([str(next(iter(bins.values()))),'prepare',str(exe),'--out',str(ir)],check=True,stdout=subprocess.DEVNULL)
        manifest['inputs'][case]=dict(exe_sha256=digest(exe),ir_sha256=digest(ir),**expected);(a.out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
        for path in a.paths:
            runs=[]
            for pair in range(a.pairs):
                for name in paired_order(bins, pair):
                    out=a.out/case/path/f'pair-{pair}-{name}';out.mkdir(parents=True)
                    with execution(bins[name],out,a.stable_launch,a.fixed_executable_slot,manifest['binaries'][name]['sha256']) as (program,cwd):
                        if a.discard_api_log:
                            logs=(cwd or out)/'logs';logs.mkdir()
                            (logs/'api.log').symlink_to(os.devnull)
                        cmd=[str(program),'run',str(exe),'--headless','--max-instr',str(expected['retired']+10),'--log-dir','logs' if a.stable_launch else str(out/'logs')]
                        if path=='ir':cmd+=['--ir',str(ir)]
                        if a.stable_launch:cmd=stable_command(cmd)
                        if a.qos_clamp:cmd=[shutil.which('taskpolicy'),'-c',a.qos_clamp]+cmd
                        result = run_command(cmd,out/'report.txt',cwd=cwd)
                        if a.fixed_executable_slot:result['executable_slot_sha256']=manifest['binaries'][name]['sha256']
                    text=(out/'report.txt').read_text();regs=re.search(r'final regs: eax=(0x[0-9a-f]+)',text);count=re.search(r'instructions: (\d+)',text)
                    result.update(pair=pair,binary=name,path=path,eax=None if regs is None else int(regs[1],16),retired=None if count is None else int(count[1]))
                    result['validated']=bool('breakpoint' in text and result['eax']==expected['eax'] and result['retired']==expected['retired'])
                    if 'api_calls' in expected:
                        api_count=re.search(r'^api calls: (\d+)',text,re.MULTILINE)
                        result['api_calls']=None if api_count is None else int(api_count[1])
                        result['validated'] &= result['api_calls']==expected['api_calls']
                    (out/'run.json').write_text(json.dumps(result,indent=2)+'\n');runs.append(result);print(f'{case}/{path} pair={pair} {name} wall={result["wall_s"]:.4f} cpu={result["cpu_s"]:.4f} validated={result["validated"]}',flush=True)
                    if not result['validated']:raise RuntimeError(str(out)+'\n'+text)
            results[f'{case}/{path}']=dict(runs=runs,medians={n:{k:statistics.median(r[k] for r in runs if r['binary']==n) for k in ['wall_s','cpu_s','peak_rss_bytes']} for n in bins});(a.out/'compare.json').write_text(json.dumps(results,indent=2)+'\n')
if __name__=='__main__':main()
