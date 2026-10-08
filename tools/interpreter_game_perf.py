#!/usr/bin/env python3
"""Serial AB/BA startup-prefix comparisons with an identical owned Users seed."""
import argparse, hashlib, json, os, re, shutil, statistics, subprocess, sys, threading, time
from pathlib import Path

def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def main():
 p=argparse.ArgumentParser(description=__doc__)
 p.add_argument('--binary',action='append',required=True,help='NAME=FROZEN_PATH')
 p.add_argument('--runtime',type=Path,required=True)
 p.add_argument('--users-seed',type=Path,required=True)
 p.add_argument('--out',type=Path,required=True)
 p.add_argument('--graphics-backend',type=Path,required=True)
 p.add_argument('--moltenvk',type=Path,required=True)
 p.add_argument('--pairs',type=int,default=3)
 p.add_argument('--mode',choices=['timing','diagnostics','stats','profile'],default='timing')
 p.add_argument('--timeout',type=float,default=600)
 p.add_argument('--sample-seconds',type=int,default=60)
 p.add_argument('--sample-delay',type=float,default=10)
 args=p.parse_args()
 runtime=args.runtime.resolve(strict=True); seed=args.users_seed.resolve(strict=True); out=args.out.resolve()
 if not (runtime/'.wire-interpreter-replay').is_file(): p.error('runtime must be an explicitly marked isolated replay clone')
 users=runtime/'Users'
 if users.is_symlink() or seed==users or seed.is_relative_to(users): p.error('seed must be separate from the reset target')
 if args.pairs<1 or args.timeout<=0: p.error('invalid run count or timeout')
 binaries={}
 for item in args.binary:
  name,path=item.split('=',1)
  if not re.fullmatch(r'[A-Za-z0-9_-]+',name) or name in binaries: p.error('invalid or duplicate binary name')
  binaries[name]=Path(path).resolve(strict=True)
 out.mkdir(parents=True,exist_ok=False)
 inputs=[runtime/'AliceToPapaADV_1-0-0.exe',runtime/'UnityPlayer.dll',runtime/'MonoBleedingEdge/EmbedRuntime/mono-2.0-bdwgc.dll',args.graphics_backend.resolve(strict=True),args.moltenvk.resolve(strict=True)]
 game_inputs={str(f.relative_to(runtime)):sha(f) for f in runtime.rglob('*') if f.is_file() and 'Users' not in f.relative_to(runtime).parts}
 manifest=dict(script_sha256=sha(Path(__file__)),game_inputs=game_inputs,binaries={n:dict(path=str(b),sha256=sha(b)) for n,b in binaries.items()},inputs={str(f):sha(f) for f in inputs},users_seed={str(f.relative_to(seed)):sha(f) for f in seed.rglob('*') if f.is_file()},mode=args.mode)
 (out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
 expected='probe64: kernel32.dll!SleepConditionVariableSRW: unimplemented Win64 import: kernel32.dll!SleepConditionVariableSRW'
 runs=[]
 for pair in range(args.pairs):
  order=list(binaries) if pair%2==0 else list(reversed(binaries))
  for name in order:
   if users.exists(): shutil.rmtree(users)
   shutil.copytree(seed,users)
   folder=out/f'pair-{pair}-{name}'; folder.mkdir()
   command=[str(binaries[name]),'probe64','AliceToPapaADV_1-0-0.exe','--startup','--window','--max-instr','10000000000','--graphics-backend',str(args.graphics_backend.resolve()),'--moltenvk',str(args.moltenvk.resolve())]
   if args.mode in ('stats','diagnostics'): command.append('--cpu-stats' if args.mode=='stats' else '--cpu-diagnostics')
   with (folder/'report.txt').open('w') as report:
    start=time.perf_counter(); proc=subprocess.Popen(command,cwd=runtime,stdout=report,stderr=subprocess.STDOUT)
    timer=threading.Timer(args.timeout,proc.kill); timer.start()
    sampler=None
    if args.mode=='profile':
     def sample():
      nonlocal sampler
      sampler=subprocess.run(['/usr/bin/sample',str(proc.pid),str(args.sample_seconds),'1','-mayDie','-fullPaths','-file',str(folder/'sample.txt')],capture_output=True,text=True,timeout=args.sample_seconds+30)
      (folder/'sampler.txt').write_text(sampler.stdout+sampler.stderr)
     sampling=threading.Timer(args.sample_delay,sample);sampling.start()
    try:
     _,status,usage=os.wait4(proc.pid,0); proc.returncode=os.waitstatus_to_exitcode(status)
    finally: timer.cancel()
    wall=time.perf_counter()-start
    if args.mode=='profile': sampling.cancel(); sampling.join()
   report=(folder/'report.txt').read_text()
   state=re.search(r'^phase=(.*); retired=(\d+); budget work=(\d+); RIP=(0x[0-9a-f]+); API calls=(\d+)',report,re.M)
   result=dict(pair=pair,binary=name,command=command,cwd=str(runtime),wall_s=wall,cpu_s=usage.ru_utime+usage.ru_stime,peak_rss_bytes=usage.ru_maxrss*(1 if sys.platform=='darwin' else 1024),exit_code=proc.returncode,validated=proc.returncode==1 and expected in report.splitlines() and state is not None)
   changed={str(f.relative_to(runtime)):sha(f) for f in runtime.rglob('*') if f.is_file() and 'Users' not in f.relative_to(runtime).parts}
   result['non_user_input_changes']=[f for f in sorted(set(game_inputs)|set(changed)) if game_inputs.get(f)!=changed.get(f)]
   result['validated']=result['validated'] and not result['non_user_input_changes']
   if state: result.update(phase=state[1],retired=int(state[2]),work=int(state[3]),rip=state[4],api_calls=int(state[5]))
   counters=re.search(r'^AMD64 CPU stats: IR=(\d+) fallback=(\d+) translations=(\d+) invalidations=(\d+)$',report,re.M)
   if counters: result['cpu_stats']=dict(zip(['ir','fallback','translations','invalidations'],map(int,counters.groups())))
   if args.mode=='profile': result['sampler_status']=None if sampler is None else sampler.returncode
   player=users/'wire/AppData/LocalLow/TukiMahiru/AliceToPapaADV_1-0-0/Player.log'
   if player.exists(): shutil.copy2(player,folder/'Player.log')
   (folder/'run.json').write_text(json.dumps(result,indent=2)+'\n');runs.append(result)
   summary=dict(runs=runs,medians={n:{k:statistics.median(r[k] for r in runs if r['binary']==n) for k in ['wall_s','cpu_s','peak_rss_bytes','retired','work','api_calls']} for n in binaries if any(r['binary']==n for r in runs)})
   (out/'compare.json').write_text(json.dumps(summary,indent=2)+'\n')
   print(f'{name} pair={pair} wall={wall:.3f} cpu={result["cpu_s"]:.3f} RSS={result["peak_rss_bytes"]} endpoint={result["validated"]}',flush=True)
   if not result['validated']: raise RuntimeError(f'startup boundary differs: {folder}')
if __name__=='__main__': main()
