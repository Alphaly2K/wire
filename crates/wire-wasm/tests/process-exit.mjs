import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,finish,window} from './gdi-harness.mjs';
import {dynamicFixture,trace} from './dynamic-fixture.mjs';

const api=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const current=t=>(1n<<BigInt(t.mode))-1n;
async function stopped(t,budget=1n){
 for(let n=0;n<500;n++){
  const result=await t.vm.run(budget);
  if(result.reason!==Exit.Budget)return result;
 }
 throw new Error('process shutdown did not finish');
}
function destructor(t,tag=14,at=0x30000n){
 const p=trace+0x400n;
 t.vm.write(at,t.mode===32?
  [0xa1,...encode(p,4),0xc1,0xe0,4,0x83,0xc8,tag,0xa3,...encode(p,4),0xc2,4,0]:
  [0x49,0xbb,...encode(p,8),0x41,0x8b,0x03,0xc1,0xe0,4,0x83,0xc8,tag,0x41,0x89,0x03,0xc3]);
 return at;
}
async function library(t){
 const f=dynamicFixture(t.mode,{tls:true});
 t.vm.supplyModule('shutdown.dll',f.bytes);
 t.vm.write(0x20000n,new TextEncoder().encode('shutdown.dll\0'));
 frame(t,'LoadLibraryA',[0x20000n],'kernel32.dll');await finish(t);
 return t.vm.register(0);
}

// Process termination is sticky, including exit codes equal to STILL_ACTIVE.
for(const mode of [32,64])for(const kind of ['normal','pseudo','handle'])for(const code of [0n,259n,0xfedcba98n]){
 const t=await machine(mode),{vm}=t;
 equal(vm.processExitCode,null);
 const h=kind==='handle'?api(t,'OpenProcess',[1n,0n,1n]):current(t);
 frame(t,kind==='normal'?'ExitProcess':'TerminateProcess',kind==='normal'?[code]:[h,code],'kernel32.dll');
 equal(vm.bind('kernelbase.dll',kind==='normal'?'ExitProcess':'TerminateProcess'),vm.ip);
 const work=vm.work,retired=vm.retired;
 const result=vm.service();equal(result.reason,kind==='normal'?Exit.Dispatch:Exit.ProcessExited);
 equal(vm.work,work+1n);equal(vm.retired,retired);
 equal((await stopped(t)).exitCode,Number(code));equal(vm.processExitCode,Number(code));
 equal(vm.stats(),{modules:0,chargedBytes:0,instances:0});
 vm.ip=0x20000n;vm.register(0,123n);vm.register(4,0n);
 const before=snapshot(t);
 equal((await vm.run(0n)).reason,Exit.Budget);
 for(let n=0;n<2;n++){
  const again=await vm.run(100n);equal(again.reason,Exit.ProcessExited);equal(again.work,0n);equal(again.exitCode,Number(code));
  const service=vm.service();equal(service.reason,Exit.ProcessExited);equal(service.work,0n);equal(service.exitCode,Number(code));
 }
 equal(snapshot(t),before);equal(core.wire_compile(t.handle),4);equal(core.wire_ticket(t.handle),0n);
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Nonreturning calls never read a return slot, even when it is inaccessible.
for(const mode of [32,64])for(const force of [false,true]){
 const t=await machine(mode),{vm}=t;
 frame(t,force?'TerminateProcess':'ExitProcess',force?[current(t),0xabcdef0012345678n]:[0xabcdef0012345678n],'kernel32.dll');
 if(mode===32){
  vm.register(4,0x24ffcn);vm.write(0x25000n,force?[...encode(current(t),4),...encode(0x12345678n,4)]:encode(0x12345678n,4));
  equal(core.wire_protect(t.handle,0x24000n,4096n,1),0);
 }else vm.register(4,0x60000n);
 equal(vm.service().reason,force?Exit.ProcessExited:Exit.Dispatch);
 equal((await stopped(t)).exitCode,0x12345678);vm.dispose();
}

// Failed TerminateProcess obeys ordinary return/error faults and full handle rights.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,denied=api(t,'OpenProcess',[0x400n,0n,1n]);
 const good=api(t,'OpenProcess',[1n,0n,1n]);
 equal(api(t,'DuplicateHandle',[current(t),good,current(t),0x22000n,0n,0n,0n]),1n);
 const reduced=vm.readInt(0x22000n,mode/8);
 for(const [h,error] of [[0n,6n],[denied,5n],[reduced,5n],...(mode===64?[[current(t)^(1n<<40n),6n],[good+(1n<<32n),6n]]:[])]){
  frame(t,'TerminateProcess',[h,77n],'kernel32.dll');
  const work=vm.work;equal(vm.service().reason,Exit.Dispatch);equal(vm.work,work+1n);equal(vm.register(0),0n);equal(vm.readInt(t.error),error);equal(vm.processExitCode,null);
 }
 frame(t,'TerminateProcess',[denied,99n],'kernel32.dll');
 const page=t.sp&~4095n;equal(core.wire_protect(t.handle,page,4096n,1),0);
 let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);
 equal(core.wire_protect(t.handle,page,4096n,4),0);
 equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);
 before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);
 equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0n);
 frame(t,'TerminateProcess',[good,88n],'kernel32.dll');equal(vm.service().exitCode,88);vm.dispose();
}

// Current-thread FLS precedes process detach; force termination skips both.
for(const mode of [32,64])for(const force of [false,true]){
 const t=await machine(mode),{vm}=t,base=await library(t),slot=api(t,'FlsAlloc',[destructor(t)]);
 equal(api(t,'FlsSetValue',[slot,123n]),1n);vm.write(trace+0x400n,encode(0n,4));
 frame(t,force?'TerminateProcess':'ExitProcess',force?[current(t),37n]:[37n],'kernel32.dll');
 equal(vm.service().reason,force?Exit.ProcessExited:Exit.Dispatch);
 const result=await stopped(t);equal(result.reason,Exit.ProcessExited);equal(result.exitCode,37);
 equal(vm.readInt(trace+0x400n),force?0n:0xe1n);
 equal(vm.readInt(base+0x3048n),force?1n:0n);equal(vm.readInt(base+0x3050n,mode/8),force?0n:1n);
 equal(vm.readInt(base+0x3068n),force?1n:0n);equal(vm.readInt(base+0x3070n,mode/8),0n);
 equal(vm.moduleBase('shutdown.dll'),base);equal(vm.stats().instances,0);vm.dispose();
}

// A fault at the first FLS frame preserves pending shutdown and its exit code.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,slot=api(t,'FlsAlloc',[destructor(t)]);
 equal(api(t,'FlsSetValue',[slot,123n]),1n);t.sp=mode===32?0x25000n:0x25008n;
 frame(t,'ExitProcess',[91n],'kernel32.dll');equal(vm.service().reason,Exit.Dispatch);
 equal(core.wire_protect(t.handle,0x24000n,4096n,2),0);
 const before=snapshot(t);equal((await vm.run(1n)).reason,Exit.Fault);equal(snapshot(t),before);equal(vm.processExitCode,null);equal(vm.readInt(trace+0x400n),0n);
 equal(core.wire_protect(t.handle,0x24000n,4096n,4),0);
 equal((await stopped(t)).exitCode,91);equal(vm.readInt(trace+0x400n),14n);vm.dispose();
}

// Completed destructors and DLL callbacks are not replayed after a later frame fault.
for(const mode of [32,64])for(const kind of ['fls','dll']){
 const t=await machine(mode),{vm}=t;
 if(kind==='fls'){
  for(const [tag,at] of [[10,0x30000n],[11,0x31000n]]){
   const slot=api(t,'FlsAlloc',[destructor(t,tag,at)]);equal(api(t,'FlsSetValue',[slot,123n]),1n);
  }
 }else await library(t);
 vm.write(trace+0x400n,encode(0n,4));frame(t,'ExitProcess',[93n],'kernel32.dll');equal(vm.service().reason,Exit.Dispatch);
 const marker=kind==='fls'?trace+0x400n:trace+28n,expected=kind==='fls'?10n:1n;
 for(let n=0;!(vm.ip===vm.entryPoints.callback&&vm.readInt(marker)===expected);n++){
  assert.ok(n<300);equal((await vm.run(1n)).reason,Exit.Budget);
 }
 const page=(t.sp-64n)&~4095n;equal(core.wire_protect(t.handle,page,4096n,2),0);
 const work=vm.work,retired=vm.retired;equal((await vm.run(1n)).reason,Exit.Fault);equal(vm.work,work);equal(vm.retired,retired);equal(vm.readInt(marker),expected);
 equal(core.wire_protect(t.handle,page,4096n,4),0);equal((await stopped(t)).exitCode,93);
 equal(vm.readInt(marker),kind==='fls'?0xabn:2n);vm.dispose();
}

// Forced termination can interrupt shutdown callbacks; recursive normal exit rejects.
for(const mode of [32,64])for(const kind of ['fls','dll','detach']){
 const t=await machine(mode),{vm}=t,exit=vm.bind('kernel32.dll','ExitProcess');
 const code=mode===32?[0x6a,99,0xb8,...encode(exit,4),0xff,0xd0,0xcc]:[0x48,0x83,0xec,40,0xb9,99,0,0,0,0x48,0xb8,...encode(exit,8),0xff,0xd0,0xcc];
 if(kind==='fls'){
  await library(t);vm.write(0x30000n,code);const slot=api(t,'FlsAlloc',[0x30000n]);equal(api(t,'FlsSetValue',[slot,123n]),1n);
  frame(t,'ExitProcess',[37n],'kernel32.dll');equal(vm.service().reason,Exit.Dispatch);
 }else{
  const f=dynamicFixture(mode),prefix=mode===32?[0x83,0x7c,0x24,8,0,0x74,8,0xb8,1,0,0,0,0xc2,12,0]:[0x85,0xd2,0x74,6,0xb8,1,0,0,0,0xc3];
  f.bytes.set(kind==='detach'?[...prefix,...code]:code,f.offset(0x1180));
  f.relocations.splice(0,f.relocations.length,...f.relocations.filter(r=>r<0x1180||r>=0x1200));f.rebuildRelocations();
  vm.supplyModule('active-loader.dll',f.bytes);vm.write(0x20000n,new TextEncoder().encode('active-loader.dll\0'));
  frame(t,'LoadLibraryA',[0x20000n],'kernel32.dll');equal(vm.service().reason,Exit.Dispatch);
  if(kind==='detach'){await finish(t);frame(t,'ExitProcess',[37n],'kernel32.dll');equal(vm.service().reason,Exit.Dispatch);}
 }
 for(let n=0;vm.ip!==exit;n++){assert.ok(n<200);equal((await vm.run(1n)).reason,Exit.Budget);}
 const before=snapshot(t);assert.throws(()=>vm.service(),/active loader or shutdown callback/);equal(snapshot(t),before);equal(vm.processExitCode,null);
 frame(t,'TerminateProcess',[current(t),81n],'kernel32.dll');equal(vm.service().reason,Exit.ProcessExited);equal((await stopped(t)).exitCode,81);equal(vm.readInt(trace+20n),0n);vm.dispose();
}

// A worker can exit the entire process; killed threads do not receive FLS cleanup.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 const callback=mode===32?[0x8b,0x44,0x24,4,0xa3,...encode(0x23000n,4),0x64,0xa1,0x24,0,0,0,0xa3,...encode(0x23008n,4),0xc2,4,0]:
  [0x49,0xbb,...encode(0x23000n,8),0x49,0x89,0x0b,0x65,0x48,0x8b,0x04,0x25,0x48,0,0,0,0x49,0x89,0x43,8,0xc3];
 vm.write(0x30000n,callback);const slot=api(t,'FlsAlloc',[0x30000n]);equal(api(t,'FlsSetValue',[slot,111n]),1n);
 const set=vm.bind('kernel32.dll','FlsSetValue'),exit=vm.bind('kernel32.dll','ExitProcess');
 vm.write(0x32000n,mode===32?[0xff,0x05,...encode(0x23200n,4),0xeb,0xf8]:[0x48,0xb8,...encode(0x23200n,8),0xff,0x00,0xeb,0xfc]);
 api(t,'CreateThread',[0n,65536n,0x32000n,0n,0n,0n]);
 vm.write(0x34000n,mode===32?[0x68,...encode(222n,4),0x68,...encode(slot,4),0xb8,...encode(set,4),0xff,0xd0,0x6a,37,0xb8,...encode(exit,4),0xff,0xd0,0xcc]:
  [0x48,0x83,0xec,40,0xb9,...encode(slot,4),0xba,222,0,0,0,0x48,0xb8,...encode(set,8),0xff,0xd0,0xb9,37,0,0,0,0x48,0xb8,...encode(exit,8),0xff,0xd0,0xcc]);
 const killer=api(t,'CreateThread',[0n,65536n,0x34000n,0n,0n,0n]),id=api(t,'GetThreadId',[killer]);
 frame(t,'WaitForSingleObject',[killer,0xffffffffn],'kernel32.dll');
 equal((await stopped(t,10000n)).exitCode,37);equal(vm.readInt(0x23000n,mode/8),222n);equal(vm.readInt(0x23008n,mode/8),id);
 const count=vm.readInt(0x23200n);assert.ok(count>0n);equal((await vm.run(10000n)).reason,Exit.ProcessExited);equal(vm.readInt(0x23200n),count);vm.dispose();
}

// Closing execution preserves drained output and memory, and closes USER objects.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;await window(t);
 const output=api(t,'GetStdHandle',[0xfffffff5n]);vm.write(0x22000n,[65,66,67]);
 equal(api(t,'WriteFile',[output,0x22000n,3n,0x23000n,0n]),1n);
 frame(t,'TerminateProcess',[current(t),42n],'kernel32.dll');equal(vm.service().reason,Exit.ProcessExited);
 equal(vm.windows(),[]);equal(vm.readInt(0x22000n,1),65n);equal(vm.readOutput(1),new Uint8Array([65,66,67]));equal(vm.readOutput(1),new Uint8Array());
 assert.throws(()=>vm.writeInput([1]),/rejected/);vm.dispose();
}

console.log(`${checks} process exit/force/rights/ABI/FLS/detach/fault/sticky/resource checks passed`);
