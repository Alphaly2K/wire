import assert from 'node:assert/strict';
import {WasmMachine,Exit} from '../web/runtime.mjs';
import {core,stateLayout,encode,fp} from './x87-harness.mjs';
import {fixture} from './pe-fixture.mjs';
import {moduleFixture} from './module-fixture.mjs';
let checks=0;
const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
const snapshot=t=>new Uint8Array(core.memory.buffer,core.wire_state(t.handle),stateLayout[0]).slice();
const cstring=s=>new TextEncoder().encode(s+'\0');
async function machine(mode){
 let handle;const time={now:0n,calls:0};
 const vm=new WasmMachine({...core,wire_create(...a){handle=core.wire_create(...a);return handle;}},{mode,clock(){time.calls++;return time.now;},wallClock(){throw new Error('unexpected wall clock');}});
 const f=fixture(mode);f.put(f.dirs+8,0);f.put(f.dirs+12,0);f.bytes.fill(0x90,f.offset(0x1000),f.offset(0x1100));f.bytes[f.offset(0x1000)]=0xc3;f.relocations.splice(0,f.relocations.length,...f.relocations.filter(r=>r>=0x1100));f.rebuildRelocations();
 const image=vm.load(f.bytes);equal((await vm.run(100n)).reason,Exit.Returned);vm.map(0x20000n,0x30000n,7);
 return {vm,handle,mode,image,time,sp:image.sp-2048n,error:image.teb+BigInt(mode===32?0x34:0x68),invalid:(1n<<BigInt(mode))-1n};
}
function frame(t,name,args=[]){const {vm,sp,mode}=t;vm.ip=vm.bind('kernel32.dll',name);vm.register(4,sp);vm.write(sp,encode(vm.entryPoints.returned,mode/8));for(let n=0;n<args.length;n++)if(mode===32)vm.write(sp+4n+BigInt(n*4),encode(BigInt.asUintN(32,args[n]),4));else if(n<4)vm.register([1,2,8,9][n],BigInt.asUintN(64,args[n]));else vm.write(sp+40n+BigInt(n-4)*8n,encode(BigInt.asUintN(64,args[n]),8));}
function call(t,name,args=[]){const work=t.vm.work,retired=t.vm.retired;frame(t,name,args);equal(t.vm.service().reason,Exit.Dispatch);equal(t.vm.work,work+1n);equal(t.vm.retired,retired);return t.vm.register(0);}
function create(t,entry=0x30000n,param=0n,flags=0n,out=0x20100n){return call(t,'CreateThread',[0n,65536n,entry,param,flags,out]);}
function assembler(mode,base=0x30000n){
 const bytes=[];const emit=(...v)=>bytes.push(...v);const value=(v,n)=>emit(...encode(BigInt(v),n));
 const store=(address,reg=0,width=4)=>{if(mode===32){emit(0x89,0x05|(reg<<3));value(address,4);}else{emit(0x49,0xbb);value(address,8);emit(width===8?0x49:0x41,0x89,0x03|(reg<<3));}};
 const load=address=>{if(mode===32){emit(0xa1);value(address,4);}else{emit(0x49,0xbb);value(address,8);emit(0x41,0x8b,0x03);}};
 const service=(pc,args=[])=>{if(mode===32){for(const a of args.toReversed()){emit(0x68);value(a,4);}emit(0xb8);value(pc,4);emit(0xff,0xd0);}else{emit(0x48,0x83,0xec,0x38);for(let n=0;n<args.length;n++){if(n<4){emit(...[[0x48,0xb9],[0x48,0xba],[0x49,0xb8],[0x49,0xb9]][n]);value(args[n],8);}else{emit(0x48,0xb8);value(args[n],8);emit(0x48,0x89,0x44,0x24,32+(n-4)*8);}}emit(0x48,0xb8);value(pc,8);emit(0xff,0xd0,0x48,0x83,0xc4,0x38);}};
 return {bytes,emit,value,store,load,service,get pc(){return base+BigInt(bytes.length);}};
}

// A worker gets fresh TLS and its own TEB while the main architectural context survives notifications and execution.
for(const mode of [32,64]){
 const t=await machine(mode),{vm,image}=t;vm.write(image.tls,encode(0xdeadbeefn,4));
 const a=assembler(mode);if(mode===32)a.emit(0x8b,0x44,0x24,4);else a.emit(0x48,0x89,0xc8);a.store(0x20200n,0,mode/8);
 if(mode===32)a.emit(0x64,0xa1,0x2c,0,0,0,0x8b,0);else a.emit(0x65,0x48,0x8b,0x04,0x25,0x58,0,0,0,0x48,0x8b,0);
 a.store(0x20210n,0,mode/8);a.emit(0x8b,0);a.store(0x20220n);a.service(vm.bind('kernel32.dll','SetLastError'),[123n]);a.emit(0xb8,73,0,0,0,...(mode===32?[0xc2,4,0]:[0xc3]));vm.write(0x30000n,a.bytes);
 const param=mode===32?0x12345678n:0xabcdef0123456789n,h=create(t,0x30000n,param,4n);equal(vm.readInt(0x20100n),2n);equal(call(t,'GetThreadId',[h]),2n);equal(call(t,'GetExitCodeThread',[h,0x20108n]),1n);equal(vm.readInt(0x20108n),259n);equal(call(t,'ResumeThread',[h]),1n);
 vm.flags=0xad7n;for(let r=0;r<(mode===32?8:16);r++)if(r!==0&&r!==4)vm.register(r,0x12345000n+BigInt(r));vm.xmm(0,new Uint8Array(16).fill(91));vm.mxcsr=0x5f80;const seeded=fp(t);seeded.cw=0x077f;seeded.tags=0xfffc;seeded.regs[0]=(0x3fffn<<64n)|0xc000000000000000n;seeded.ip=0x1234n;seeded.dp=0x5678n;fp(t,seeded);const beforeFp=fp(t),beforeXmm=vm.xmm(0);vm.write(t.error,encode(67n,4));
 frame(t,'WaitForSingleObject',[h,0xffffffffn]);const saved=Array.from({length:mode===32?8:16},(_,r)=>vm.register(r));const result=await vm.run(10000n);equal(result.reason,Exit.Returned);equal(vm.register(0),0n);equal(vm.readInt(0x20200n,mode/8),param);equal(vm.readInt(0x20220n),0x11223344n);equal(vm.readInt(image.tls),0xdeadbeefn);equal(vm.readInt(t.error),67n);equal(vm.flags,0xad7n);equal(fp(t),beforeFp);equal(vm.xmm(0),beforeXmm);equal(vm.mxcsr,0x5f80);for(let r=0;r<saved.length;r++)if(r!==0&&r!==4)equal(vm.register(r),saved[r]);
 const tls=vm.readInt(0x20210n,mode/8);assert.throws(()=>vm.readInt(tls),/memory access/);equal(call(t,'GetExitCodeThread',[h,0x20108n]),1n);equal(vm.readInt(0x20108n),73n);equal(call(t,'WaitForSingleObject',[h,0n]),0n);equal(call(t,'CloseHandle',[h]),1n);equal(t.time.calls,0);vm.dispose();
}

// Two CPU-bound workers need quantum preemption to observe each other's progress.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 for(const [base,own,other,code] of [[0x30000n,0x20200n,0x20204n,11],[0x31000n,0x20204n,0x20200n,22]]){
  const a=assembler(mode,base);a.emit(0xb8,1,0,0,0);a.store(own);const loop=a.pc;a.load(other);a.emit(0x85,0xc0,0x74);a.value(loop-a.pc-1n,1);a.emit(0xb8,code,0,0,0,...(mode===32?[0xc2,4,0]:[0xc3]));vm.write(base,a.bytes);
 }
 const h1=create(t),h2=create(t,0x31000n);frame(t,'WaitForSingleObject',[h1,0xffffffffn]);const work=vm.work;const result=await vm.run(100000n);equal(result.reason,Exit.Returned);assert.ok(vm.work>work+10000n);equal(vm.readInt(0x20200n),1n);equal(vm.readInt(0x20204n),1n);frame(t,'WaitForSingleObject',[h2,0xffffffffn]);equal((await vm.run(10000n)).reason,Exit.Returned);for(const [h,v] of [[h1,11n],[h2,22n]]){equal(call(t,'GetExitCodeThread',[h,0x20108n]),1n);equal(vm.readInt(0x20108n),v);equal(call(t,'CloseHandle',[h]),1n);}vm.dispose();
}

// A blocked service and a zero budget preserve the CPU until another thread or host action can make progress.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.write(0x30000n,[0xb8,9,0,0,0,...(mode===32?[0xc2,4,0]:[0xc3])]);const h=create(t,0x30000n,0n,4n);
 frame(t,'WaitForSingleObject',[h,0xffffffffn]);const before=snapshot(t);equal((await vm.run(0n)).reason,Exit.Budget);equal(snapshot(t),before);equal((await vm.run(100n)).reason,Exit.Waiting);equal(snapshot(t),before);equal(call(t,'ResumeThread',[h]),1n);frame(t,'WaitForSingleObject',[h,0xffffffffn]);equal((await vm.run(1000n)).reason,Exit.Returned);vm.dispose();
}

// A finite wait retains its initial deadline and saved return even when guest call-frame bytes change.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;const h=create(t,0x30000n,0n,4n);frame(t,'WaitForSingleObject',[h,5n]);const work=vm.work;equal((await vm.run(100n)).reason,Exit.Waiting);equal(vm.work,work);vm.write(t.sp,encode(0xdeadbeefn,mode/8));t.time.now=49999n;equal((await vm.run(100n)).reason,Exit.Waiting);t.time.now=50000n;equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.ip,vm.entryPoints.returned);equal(vm.register(0),258n);equal(vm.work,work+1n);
 frame(t,'Sleep',[3n]);equal((await vm.run(100n)).reason,Exit.Waiting);t.time.now=80000n;equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.ip,vm.entryPoints.returned);vm.dispose();
}

// Dynamic module TLS is provisioned and reclaimed for all live threads, including suspended ones.
for(const mode of [32,64]){
 const t=await machine(mode),{vm,image}=t;vm.supplyModule('tls.dll',moduleFixture(mode,{dll:true,tls:true}).bytes);vm.write(0x30000n,[0xb8,1,0,0,0,...(mode===32?[0xc2,4,0]:[0xc3])]);const h=create(t,0x30000n,0n,4n);
 const teb=mode===32?0x7f000000n:0x7ffc00000000n,array=teb+4096n;equal(vm.readInt(teb+BigInt(mode===32?0x24:0x48)),2n);const childRootTls=vm.readInt(array,mode/8);assert.notEqual(childRootTls,image.tls);equal(vm.readInt(childRootTls),0x11223344n);
 vm.write(0x20300n,cstring('tls.dll'));frame(t,'LoadLibraryA',[0x20300n]);equal((await vm.run(1000n)).reason,Exit.Returned);const module=vm.register(0),slot=Number(vm.readInt(module+0x300cn)),mainTls=vm.readInt(image.tlsArray+BigInt(slot*mode/8),mode/8),childTls=vm.readInt(array+BigInt(slot*mode/8),mode/8);assert.notEqual(mainTls,childTls);equal(vm.readInt(childTls),0x11110007n);
 frame(t,'FreeLibrary',[module]);equal(core.wire_protect(t.handle,array,4096n,2),0);equal((await vm.run(1000n)).reason,Exit.Fault);equal(vm.readInt(mainTls),0x11110007n);equal(vm.readInt(childTls),0x11110007n);equal(core.wire_protect(t.handle,array,4096n,4),0);equal((await vm.run(1000n)).reason,Exit.Returned);equal(vm.readInt(array+BigInt(slot*mode/8),mode/8),0n);for(const p of [mainTls,childTls])assert.throws(()=>vm.readInt(p),/memory access/);equal(call(t,'ResumeThread',[h]),1n);frame(t,'WaitForSingleObject',[h,0xffffffffn]);equal((await vm.run(1000n)).reason,Exit.Returned);assert.throws(()=>vm.readInt(teb),/memory access/);vm.dispose();
}


// Output faults cannot publish a new thread or consume its ID; aliases retain exit status independently of resources.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,cross=0x24fffn;vm.write(0x30000n,[0xb8,7,0,0,0,...(mode===32?[0xc2,4,0]:[0xc3])]);
 frame(t,'CreateThread',[0n,65536n,0x30000n,0n,4n,cross]);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);const h=vm.register(0);equal(vm.readInt(cross),2n);
 equal(call(t,'DuplicateHandle',[t.invalid,h,t.invalid,0x20110n,0x100000n,1n,0n]),1n);const waitOnly=vm.readInt(0x20110n,mode/8);equal(call(t,'GetThreadId',[waitOnly]),0n);equal(vm.readInt(t.error),5n);equal(call(t,'GetHandleInformation',[waitOnly,0x20120n]),1n);equal(vm.readInt(0x20120n),1n);equal(call(t,'OpenThread',[0x840n,0n,2n])!==0n,true);
 equal(call(t,'SetThreadErrorMode',[0x8003n,t.sp]),1n);equal(vm.ip,vm.entryPoints.returned);equal(call(t,'GetThreadErrorMode'),0x8003n);equal(call(t,'SetThreadPriority',[h,0xffffffffn]),1n);equal(call(t,'GetThreadPriority',[h]),0xffffffffn);equal(call(t,'SuspendThread',[h]),1n);equal(call(t,'ResumeThread',[h]),2n);equal(call(t,'ResumeThread',[h]),1n);equal(call(t,'ResumeThread',[h]),0n);
 frame(t,'WaitForSingleObject',[waitOnly,0xffffffffn]);equal((await vm.run(1000n)).reason,Exit.Returned);equal(call(t,'CloseHandle',[h]),1n);equal(call(t,'WaitForSingleObject',[waitOnly,0n]),0n);equal(call(t,'CloseHandle',[waitOnly]),1n);vm.dispose();
}

// Thread notifications run once per live DLL, obey DisableThreadLibraryCalls, and preserve the original exit code.
for(const mode of [32,64])for(const disabled of [false,true]){
 const t=await machine(mode),{vm}=t;vm.supplyModule('notify.dll',moduleFixture(mode,{dll:true}).bytes);vm.write(0x20300n,cstring('notify.dll'));frame(t,'LoadLibraryA',[0x20300n]);equal((await vm.run(1000n)).reason,Exit.Returned);const module=vm.register(0);equal(vm.readInt(module+0x3030n),1n);if(disabled)equal(call(t,'DisableThreadLibraryCalls',[module]),1n);
 const a=assembler(mode);a.service(vm.bind('kernel32.dll','ExitThread'),[0xfedcba98n]);a.emit(0xcc);vm.write(0x30000n,a.bytes);const h=create(t);frame(t,'WaitForSingleObject',[h,0xffffffffn]);equal((await vm.run(1000n)).reason,Exit.Returned);equal(vm.readInt(module+0x3030n),disabled?1n:3n);equal(vm.readInt(module+0x3048n),disabled?1n:3n);equal(call(t,'GetExitCodeThread',[h,0x20108n]),1n);equal(vm.readInt(0x20108n),0xfedcba98n);vm.dispose();
}

// Live resource slots are bounded and are reusable after full exit cleanup, with fresh monotonic identities.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.write(0x30000n,[0xb8,9,0,0,0,...(mode===32?[0xc2,4,0]:[0xc3])]);const handles=[];for(let n=0;n<64;n++)handles.push(create(t,0x30000n,0n,4n));equal(create(t),0n);equal(vm.readInt(t.error),8n);
 for(const h of handles)equal(call(t,'ResumeThread',[h]),1n);frame(t,'WaitForSingleObject',[handles.at(-1),0xffffffffn]);equal((await vm.run(100000n)).reason,Exit.Returned);for(const h of handles)equal(call(t,'CloseHandle',[h]),1n);const fresh=create(t);equal(vm.readInt(0x20100n),66n);assert.ok(!handles.includes(fresh));frame(t,'WaitForSingleObject',[fresh,0xffffffffn]);equal((await vm.run(1000n)).reason,Exit.Returned);equal(t.time.calls,0);vm.dispose();
}


// Existing workers can satisfy I/O while DllMain owns the loader lock; new threads wait for notifications.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;equal(call(t,'CreatePipe',[0x20140n,0x20148n,0n,0n]),1n);const r=vm.readInt(0x20140n,mode/8),w=vm.readInt(0x20148n,mode/8);vm.write(0x20400n,[7]);equal(call(t,'WriteFile',[w,0x22000n,4096n,0x20160n,0n]),1n);
 const a=assembler(mode),loop=a.pc;a.load(0x20200n);a.emit(0x85,0xc0,0x74);a.value(loop-a.pc-1n,1);a.service(vm.bind('kernel32.dll','WriteFile'),[w,0x20400n,1n,0x20160n,0n]);a.service(vm.bind('kernel32.dll','GetModuleHandleA'),[0n]);a.store(0x20240n,0,mode/8);a.service(vm.bind('kernel32.dll','Sleep'),[0xffffffffn]);a.emit(0xcc);vm.write(0x30000n,a.bytes);create(t);frame(t,'Sleep',[0n]);equal((await vm.run(20000n)).reason,Exit.Returned);
 const b=assembler(mode,0x31000n);b.emit(0xb8,1,0,0,0);b.store(0x20220n);b.emit(0xb8,8,0,0,0,...(mode===32?[0xc2,4,0]:[0xc3]));vm.write(0x31000n,b.bytes);const child=create(t,0x31000n);
 const f=moduleFixture(mode,{dll:true}),init=assembler(mode);init.emit(...(mode===32?[0x83,0x7c,0x24,8,1,0x0f,0x85,0,0,0,0]:[0x83,0xfa,1,0x0f,0x85,0,0,0,0]));const branch=init.bytes.length-4;init.emit(0xb8,1,0,0,0);init.store(0x20200n);init.service(vm.bind('kernel32.dll','Sleep'),[0n]);init.service(vm.bind('kernel32.dll','ReadFile'),[r,0x23000n,4096n,0x20168n,0n]);init.service(vm.bind('kernel32.dll','ReadFile'),[r,0x20410n,1n,0x20168n,0n]);init.load(0x20220n);init.store(0x20230n);init.load(0x20240n);init.store(0x20250n);init.bytes.splice(branch,4,...encode(BigInt(init.bytes.length-branch-4),4));init.emit(0xb8,1,0,0,0,...(mode===32?[0xc2,12,0]:[0xc3]));f.bytes.fill(0x90,f.offset(0x1000),f.offset(0x1400));f.bytes.set(init.bytes,f.offset(0x1000));f.relocations.splice(0,f.relocations.length,...f.relocations.filter(r=>r>=0x1400));f.rebuildRelocations();vm.supplyModule('lock.dll',f.bytes);vm.write(0x20300n,cstring('lock.dll'));frame(t,'LoadLibraryA',[0x20300n]);equal((await vm.run(100000n)).reason,Exit.Returned);assert.ok(vm.register(0));equal(vm.readInt(0x20410n,1),7n);equal(vm.readInt(0x20230n),0n);equal(vm.readInt(0x20220n),0n);equal(vm.readInt(0x20250n),0n);equal(vm.readInt(0x20240n,mode/8),0n);
 frame(t,'WaitForSingleObject',[child,0xffffffffn]);equal((await vm.run(20000n)).reason,Exit.Returned);equal(vm.readInt(0x20220n),1n);equal(vm.readInt(0x20240n,mode/8),t.image.base);vm.dispose();
}


// An invalid entry faults in its own context and resumes through the same JIT after repair.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.write(0x30000n,[0xb8,27,0,0,0,...(mode===32?[0xc2,4,0]:[0xc3])]);const h=create(t,0n);frame(t,'WaitForSingleObject',[h,0xffffffffn]);equal((await vm.run(1000n)).reason,Exit.Fault);equal(vm.ip,0n);vm.ip=0x30000n;equal((await vm.run(1000n)).reason,Exit.Returned);equal(call(t,'GetExitCodeThread',[h,0x20108n]),1n);equal(vm.readInt(0x20108n),27n);vm.dispose();
}

// Exiting the initial thread leaves its workers alive, and the last worker produces the terminal thread result.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.write(0x30000n,[0xb8,37,0,0,0,...(mode===32?[0xc2,4,0]:[0xc3])]);create(t);frame(t,'ExitThread',[83n]);equal((await vm.run(1000n)).reason,Exit.ThreadReturned);equal(vm.register(0),37n);equal((await vm.run(1000n)).reason,Exit.ThreadReturned);vm.dispose();
}

// Static rejection releases unstarted worker resources and handles along with all module TLS blocks.
for(const mode of [32,64]){
 let handle;const vm=new WasmMachine({...core,wire_create(...a){handle=core.wire_create(...a);return handle;}},{mode});vm.supplyModule('reject.dll',moduleFixture(mode,{dll:true,tls:true,attach:false}).bytes);const image=vm.load(moduleFixture(mode,{dependency:'reject.dll'}).bytes);vm.map(0x20000n,0x20000n,7);const t={vm,handle,mode,image,sp:image.sp-2048n,error:image.teb+BigInt(mode===32?0x34:0x68),invalid:(1n<<BigInt(mode))-1n},saved=snapshot(t);const h=create(t,0x30000n,0n,4n);new Uint8Array(core.memory.buffer,core.wire_state(handle),stateLayout[0]).set(saved);await assert.rejects(vm.run(1000n),/attach returned FALSE/);const teb=mode===32?0x7f000000n:0x7ffc00000000n;assert.throws(()=>vm.readInt(teb),/memory access/);equal(call(t,'GetThreadId',[h]),0n);equal(vm.readInt(t.error),6n);vm.dispose();
}


// Single-work resumes cross attach, entry, detach and wait completion without charging synthetic scheduling work.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.write(0x30000n,[0xb8,19,0,0,0,...(mode===32?[0xc2,4,0]:[0xc3])]);const h=create(t);frame(t,'WaitForSingleObject',[h,0xffffffffn]);let result;
 for(let n=0;n<1000;n++){const work=vm.work,retired=vm.retired;result=await vm.run(1n);equal(vm.work-work,result.work);assert.ok(vm.retired>=retired);if(result.reason!==Exit.Budget)break;equal(result.work,1n);}
 equal(result.reason,Exit.Returned);equal(call(t,'GetExitCodeThread',[h,0x20108n]),1n);equal(vm.readInt(0x20108n),19n);vm.dispose();
}

// A repeated comparison keeps its exact restart state across quantum switches and another thread's notifications.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,count=20001;vm.map(0x200000n,0x20000n,3);vm.write(0x200000n,new Uint8Array(count).fill(0x44));vm.write(0x210000n,new Uint8Array(count).fill(0x44));vm.write(0x210000n+BigInt(count-1),[0x45]);
 const a=assembler(mode);a.emit(0xfc,0xb9);a.value(count,4);a.emit(...(mode===32?[0xbe]:[0x48,0xbe]));a.value(0x200000n,mode/8);a.emit(...(mode===32?[0xbf]:[0x48,0xbf]));a.value(0x210000n,mode/8);a.emit(0xf3,0xa6,0x9c,0x58);a.store(0x20200n);a.store(0x20208n,1);a.emit(0xb8,29,0,0,0,...(mode===32?[0xc2,4,0]:[0xc3]));vm.write(0x30000n,a.bytes);vm.write(0x31000n,[0xb8,31,0,0,0,...(mode===32?[0xc2,4,0]:[0xc3])]);const h=create(t);create(t,0x31000n);frame(t,'WaitForSingleObject',[h,0xffffffffn]);equal((await vm.run(100000n)).reason,Exit.Returned);equal(vm.readInt(0x20208n),0n);equal(vm.readInt(0x20200n)&0x8d5n,0x95n);vm.dispose();
}


// Notification-frame and TLS cleanup faults resume the selected worker without replaying completed callbacks.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.write(0x30000n,[0xb8,43,0,0,0,...(mode===32?[0xc2,4,0]:[0xc3])]);const h=create(t),teb=mode===32?0x7f000000n:0x7ffc00000000n,array=teb+4096n,top=vm.readInt(teb+BigInt(mode===32?4:8),mode/8);frame(t,'WaitForSingleObject',[h,0xffffffffn]);equal(core.wire_protect(t.handle,top-4096n,4096n,2),0);equal((await vm.run(1000n)).reason,Exit.Fault);equal(vm.readInt(t.image.base+0x3008n),2n);equal(core.wire_protect(t.handle,top-4096n,4096n,4),0);equal(core.wire_protect(t.handle,array,4096n,2),0);equal((await vm.run(1000n)).reason,Exit.Fault);equal(vm.readInt(t.image.base+0x3008n),6n);equal(core.wire_protect(t.handle,array,4096n,4),0);equal((await vm.run(1000n)).reason,Exit.Returned);equal(vm.readInt(t.image.base+0x3008n),6n);equal(call(t,'GetExitCodeThread',[h,0x20108n]),1n);equal(vm.readInt(0x20108n),43n);vm.dispose();
}
console.log(`${checks} thread scheduling/TLS/waits/loader/context/resource checks passed`);
