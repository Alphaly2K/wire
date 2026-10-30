import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,width=mode/8,base=8+8*width,ex=base+width,current=(1n<<BigInt(mode))-1n,out=0x22001n;
 const api=(name,args=[])=>call(t,name,args,'kernel32.dll');
 const query=(handle=current,size=ex,p=out)=>api('K32GetProcessMemoryInfo',[handle,p,BigInt(size)]);
 const counters=()=>Array.from({length:9},(_,n)=>vm.readInt(out+8n+BigInt(n*width),width));
 vm.write(t.error,encode(123n,4));
 for(const [dll,name] of [['psapi.dll','GetProcessMemoryInfo'],['kernel32.dll','K32GetProcessMemoryInfo'],['kernelbase.dll','K32GetProcessMemoryInfo']]){
  vm.write(out-1n,new Uint8Array(ex+2).fill(0xa5));
  equal(call(t,name,[current,out,BigInt(base)],dll),1n);equal(vm.readInt(out),BigInt(base));equal(vm.readInt(out+4n),0n);
  equal(vm.readInt(out-1n,1),0xa5n);equal(vm.readInt(out+BigInt(base),1),0xa5n);
  equal(call(t,name,[current,out,BigInt(ex+5)],dll),1n);equal(vm.readInt(out),BigInt(ex));equal(vm.readInt(out+BigInt(ex),1),0xa5n);
 }
 for(const [dll,name]of [['psapi.dll','K32GetProcessMemoryInfo'],['kernel32.dll','GetProcessMemoryInfo'],['user32.dll','GetProcessMemoryInfo']])assert.throws(()=>vm.bind(dll,name),/unknown/);
 equal(vm.readInt(t.error),123n);equal(query(),1n);const before=counters();
 for(const n of [2,3,4,5])equal(before[n],0n);equal(before[6],before[8]);assert.ok(before[0]>=before[1]&&before[7]>=before[6]&&before[1]>before[6]);
 const page=api('VirtualAlloc',[0x60000n,12288n,0x2000n,4n]);equal(page,0x60000n);equal(query(),1n);equal(counters(),before);
 for(let n=0;n<2;n++)equal(api('VirtualAlloc',[page,12288n,0x1000n,4n]),page);
 equal(query(),1n);const live=counters();equal(live[1],before[1]+12288n);equal(live[6],before[6]+12288n);equal(live[8],live[6]);equal(live[0],before[0]>live[1]?before[0]:live[1]);equal(live[7],before[7]>live[6]?before[7]:live[6]);
 equal(api('VirtualFree',[page,0n,0x8000n]),1n);equal(query(),1n);const freed=counters();equal(freed[1],before[1]);equal(freed[6],before[6]);equal(freed[8],before[8]);equal(freed[0],live[0]);equal(freed[7],live[7]);
 const handles=[];
 for(const access of [0x400n,0x1000n,0x10n,0n]){
  const h=api('OpenProcess',[access,0n,1n]);handles.push(h);equal(query(h),access===0x400n||access===0x1000n?1n:0n);
  if(access<0x400n)equal(vm.readInt(t.error),5n);
 }
 equal(query(0x123n,base-1,0xdead0000n),0n);equal(vm.readInt(t.error),122n);
 equal(query(0x123n,base,0xdead0000n),0n);equal(vm.readInt(t.error),6n);
 if(mode===64){equal(query(0xffffffffn),0n);equal(vm.readInt(t.error),6n);equal(query(handles[0]|(1n<<32n)),0n);equal(vm.readInt(t.error),6n);equal(query(current,0x100000000+ex),1n);}
 // A cross-page output failure must preserve the entire structure and saved call.
 const at=0x24ff0n;vm.write(at,new Uint8Array(ex).fill(0xa5));frame(t,'K32GetProcessMemoryInfo',[current,at,BigInt(ex)],'kernel32.dll');
 equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const saved=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),saved);equal(vm.readInt(at,8),0xa5a5a5a5a5a5a5a5n);
 equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),1n);
 equal(query(current,base,t.sp),1n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp),BigInt(base));
 for(const h of handles)equal(api('CloseHandle',[h]),1n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}
console.log(`${checks} process-memory-info layout/live/peak/rights/alias/fault checks passed`);
