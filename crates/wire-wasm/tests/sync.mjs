import assert from 'node:assert/strict';
import {WasmMachine,Exit} from '../web/runtime.mjs';
import {core,stateLayout,encode} from './x87-harness.mjs';
import {fixture} from './pe-fixture.mjs';
let checks=0;const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
const wide=s=>Uint8Array.from([...s.split('').flatMap(c=>[c.charCodeAt(0)&255,c.charCodeAt(0)>>8]),0,0]);
const snapshot=t=>new Uint8Array(core.memory.buffer,core.wire_state(t.handle),stateLayout[0]).slice();
async function machine(mode){
 let handle;const time={now:0n,wall:1000000n,elapsed:0,walls:0,failElapsed:false,failWall:false};
 const vm=new WasmMachine({...core,wire_create(...a){handle=core.wire_create(...a);return handle;}},{mode,clock(){time.elapsed++;if(time.failElapsed)throw new Error('elapsed failure');return time.now;},wallClock(){time.walls++;if(time.failWall)throw new Error('wall failure');return time.wall;}});
 const f=fixture(mode,{tls:false});f.put(f.dirs+8,0);f.put(f.dirs+12,0);f.bytes.fill(0x90,f.offset(0x1000),f.offset(0x1100));f.bytes[f.offset(0x1000)]=0xc3;f.relocations.splice(0,f.relocations.length,...f.relocations.filter(r=>r>=0x1100));f.rebuildRelocations();const image=vm.load(f.bytes);equal((await vm.run(100n)).reason,Exit.Returned);vm.map(0x20000n,0x20000n,7);
 return {vm,handle,mode,image,time,sp:image.sp-2048n,error:image.teb+BigInt(mode===32?0x34:0x68),invalid:(1n<<BigInt(mode))-1n};
}
function frame(t,name,args=[]){const {vm,sp,mode}=t;vm.ip=vm.bind('kernel32.dll',name);vm.register(4,sp);vm.write(sp,encode(vm.entryPoints.returned,mode/8));for(let n=0;n<args.length;n++)if(mode===32)vm.write(sp+4n+BigInt(n*4),encode(BigInt.asUintN(32,args[n]),4));else if(n<4)vm.register([1,2,8,9][n],BigInt.asUintN(64,args[n]));else vm.write(sp+40n+BigInt(n-4)*8n,encode(BigInt.asUintN(64,args[n]),8));}
function call(t,name,args=[]){const w=t.vm.work,r=t.vm.retired;frame(t,name,args);equal(t.vm.service().reason,Exit.Dispatch);equal(t.vm.work,w+1n);equal(t.vm.retired,r);return t.vm.register(0);}
const event=(t,manual=false,signal=false,p=0n)=>call(t,'CreateEventW',[0n,BigInt(manual),BigInt(signal),p]);
const sem=(t,count=0n,max=2n,p=0n)=>call(t,'CreateSemaphoreW',[0n,count,max,p]);
const wait=(t,h,ms=0n)=>call(t,'WaitForSingleObject',[h,ms]);
function multi(t,handles,all=false){for(let n=0;n<handles.length;n++)t.vm.write(0x21000n+BigInt(n*t.mode/8),encode(handles[n],t.mode/8));return call(t,'WaitForMultipleObjects',[BigInt(handles.length),0x21000n,BigInt(all),0n]);}
const timer=(t,manual=false,p=0n)=>call(t,'CreateWaitableTimerW',[0n,BigInt(manual),p]);
function set(t,h,due,period=0n,name='SetWaitableTimer'){t.vm.write(0x22000n,encode(BigInt.asUintN(64,due),8));return call(t,name,[h,0x22000n,period,0n,0n,0n,0n]);}
function asm(t,base=0x30000n){const {mode,vm}=t,bytes=[];const emit=(...v)=>bytes.push(...v),value=(v,n)=>emit(...encode(v,n));const store=(p,width=mode/8)=>{if(mode===32){emit(0xa3);value(p,4);}else{emit(0x49,0xbb);value(p,8);emit(width===8?0x49:0x41,0x89,0x03);}};const api=(name,args=[])=>{const pc=vm.bind('kernel32.dll',name);if(mode===32){for(const p of args.toReversed()){emit(0x68);value(p,4);}emit(0xb8);value(pc,4);emit(0xff,0xd0);}else{emit(0x48,0x83,0xec,0x38);for(let n=0;n<args.length;n++){if(n<4){emit(...[[0x48,0xb9],[0x48,0xba],[0x49,0xb8],[0x49,0xb9]][n]);value(args[n],8);}else{emit(0x48,0xb8);value(args[n],8);emit(0x48,0x89,0x44,0x24,32+(n-4)*8);}}emit(0x48,0xb8);value(pc,8);emit(0xff,0xd0,0x48,0x83,0xc4,0x38);}};return{emit,value,store,api,finish(code=73){emit(0xb8,code,0,0,0,...(mode===32?[0xc2,4,0]:[0xc3]));vm.write(base,bytes);}};}
const create=(t,base=0x30000n)=>call(t,'CreateThread',[0n,65536n,base,0n,0n,0n]);

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,e=event(t),m=event(t,true,true),s=sem(t);equal(wait(t,e),258n);equal(wait(t,m),0n);equal(wait(t,m),0n);equal(call(t,'ResetEvent',[m]),1n);equal(wait(t,m),258n);equal(call(t,'SetEvent',[e]),1n);equal(multi(t,[e,s],true),258n);equal(wait(t,e),0n);equal(wait(t,e),258n);equal(call(t,'SetEvent',[e]),1n);equal(multi(t,[e,0n]),0xffffffffn);equal(vm.readInt(t.error),6n);equal(wait(t,e),0n);
 equal(call(t,'ReleaseSemaphore',[s,1n,0x20100n]),1n);equal(vm.readInt(0x20100n),0n);equal(call(t,'SetEvent',[e]),1n);equal(multi(t,[e,s],true),0n);equal(wait(t,s),258n);equal(wait(t,e),258n);equal(call(t,'ReleaseSemaphore',[s,2n,0n]),1n);equal(call(t,'ReleaseSemaphore',[s,1n,0n]),0n);equal(vm.readInt(t.error),298n);equal(multi(t,[e,s]),1n);equal(wait(t,s),0n);equal(wait(t,s),258n);
 equal(call(t,'SetEvent',[e]),1n);equal(call(t,'DuplicateHandle',[t.invalid,e,t.invalid,0x20110n,0n,0n,2n]),1n);const alias=vm.readInt(0x20110n,mode/8);equal(multi(t,[e,alias],true),0xffffffffn);equal(vm.readInt(t.error),87n);equal(multi(t,[e,alias]),0n);equal(wait(t,alias),258n);equal(call(t,'CloseHandle',[e]),1n);equal(call(t,'SetEvent',[alias]),1n);equal(wait(t,alias),0n);equal(t.time.elapsed,0);equal(t.time.walls,0);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.write(0x20000n,wide('Local\\shared'));const e=event(t,false,false,0x20000n);equal(vm.readInt(t.error),0n);equal(event(t,true,true,0x20000n)!==e,true);equal(vm.readInt(t.error),183n);equal(wait(t,e),258n);equal(sem(t,0n,2n,0x20000n),0n);equal(vm.readInt(t.error),6n);equal(call(t,'CreateMutexW',[0n,0n,0x20000n]),0n);equal(timer(t,false,0x20000n),0n);
 vm.write(0x20200n,wide('shared'));vm.write(t.error,encode(99n,4));equal(core.wire_protect(t.handle,t.error&~4095n,4096n,0x104),0);const opened=call(t,'OpenEventW',[0x100000n,1n,0x20200n]);assert.ok(opened);equal(core.wire_protect(t.handle,t.error&~4095n,4096n,4),0);equal(vm.readInt(t.error),99n);equal(call(t,'SetEvent',[opened]),0n);equal(vm.readInt(t.error),5n);equal(call(t,'GetHandleInformation',[opened,0x20100n]),1n);equal(vm.readInt(0x20100n),1n);
 equal(call(t,'DuplicateHandle',[t.invalid,e,t.invalid,0x20110n,2n,0n,0n]),1n);const modify=vm.readInt(0x20110n,mode/8);equal(wait(t,modify),0xffffffffn);equal(vm.readInt(t.error),5n);equal(call(t,'SetEvent',[modify]),1n);equal(wait(t,opened),0n);vm.write(0x20300n,wide('x'.repeat(261)));equal(event(t,false,false,0x20300n),0n);equal(vm.readInt(t.error),206n);vm.dispose();
}

// A release output fault preserves semaphore credits, including when the output straddles pages.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,s=sem(t),cross=0x24fffn;frame(t,'ReleaseSemaphore',[s,1n,cross]);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(cross),0n);equal(wait(t,s),0n);equal(wait(t,s),258n);
 frame(t,'CreateEventW',[0n,0n,0n,0n]);equal(core.wire_protect(t.handle,t.error&~4095n,4096n,0x104),0);const creating=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),creating);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),s+1n);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;const h=call(t,'CreateMutexW',[0n,1n,0n]);equal(wait(t,h),0n);const a=asm(t);a.api('ReleaseMutex',[h]);a.store(0x20200n,4);a.api('GetLastError');a.store(0x20204n,4);a.finish();const worker=create(t);frame(t,'WaitForSingleObject',[worker,0xffffffffn]);equal((await vm.run(1000n)).reason,Exit.Returned);equal(vm.readInt(0x20200n),0n);equal(vm.readInt(0x20204n),288n);equal(call(t,'ReleaseMutex',[h]),1n);equal(call(t,'ReleaseMutex',[h]),1n);equal(call(t,'ReleaseMutex',[h]),0n);equal(vm.readInt(t.error),288n);
 const b=asm(t);b.api('CreateMutexW',[0n,1n,0n]);b.store(0x20210n);b.finish();const dead=create(t);frame(t,'WaitForSingleObject',[dead,0xffffffffn]);equal((await vm.run(1000n)).reason,Exit.Returned);const abandoned=vm.readInt(0x20210n,mode/8);equal(wait(t,abandoned),128n);equal(wait(t,abandoned),0n);equal(call(t,'ReleaseMutex',[abandoned]),1n);equal(call(t,'ReleaseMutex',[abandoned]),1n);equal(t.time.elapsed,0);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm,time}=t,h=timer(t);equal(set(t,h,-100n,10n),1n);equal(time.elapsed,1);equal(time.walls,0);time.now=99n;equal(wait(t,h),258n);time.now=100n;equal(wait(t,h),0n);equal(wait(t,h),258n);time.now=500500n;equal(wait(t,h),0n);equal(wait(t,h),258n);time.now=600099n;equal(wait(t,h),258n);time.now=600100n;equal(wait(t,h),0n);
 equal(set(t,h,-50n),1n);time.now+=51n;equal(call(t,'CancelWaitableTimer',[h]),1n);const clocks=time.elapsed;equal(wait(t,h),0n);equal(wait(t,h),258n);equal(time.elapsed,clocks);const manual=timer(t,true);equal(set(t,manual,-10n),1n);time.now+=10n;equal(wait(t,manual),0n);equal(wait(t,manual),0n);equal(set(t,manual,-1000n),1n);equal(wait(t,manual),258n);equal(call(t,'CancelWaitableTimer',[manual]),1n);equal(wait(t,manual),258n);vm.dispose();
}

// Absolute FILETIME is converted using a fresh pair of samples, then remains a monotonic deadline.
for(const mode of [32,64]){
 const t=await machine(mode),{vm,time}=t,h=timer(t);time.now=1000n;equal(set(t,h,time.wall+50n,0n,'SetWaitableTimerEx'),1n);equal(time.elapsed,1);equal(time.walls,1);time.wall=1n;time.now=1049n;frame(t,'WaitForSingleObject',[h,0xffffffffn]);const before=snapshot(t);equal((await vm.run(0n)).reason,Exit.Budget);equal(snapshot(t),before);equal((await vm.run(100n)).reason,Exit.Waiting);time.now=1050n;equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.register(0),0n);equal(vm.ip,vm.entryPoints.returned);equal(time.walls,1);vm.dispose();
}

// A pending timer wait retains the object and its name after the last guest handle closes.
for(const mode of [32,64]){
 const t=await machine(mode),{vm,time}=t;vm.write(0x20000n,wide('pending'));const h=timer(t,false,0x20000n);equal(set(t,h,-100n),1n);const a=asm(t);a.api('WaitForSingleObject',[h,0xffffffffn]);a.store(0x20200n,4);a.finish();const worker=create(t);frame(t,'Sleep',[0n]);equal((await vm.run(1000n)).reason,Exit.Returned);equal(call(t,'CloseHandle',[h]),1n);const alias=call(t,'OpenWaitableTimerW',[0x100000n,0n,0x20000n]);assert.ok(alias);equal(call(t,'CloseHandle',[alias]),1n);time.now=100n;frame(t,'WaitForSingleObject',[worker,0xffffffffn]);equal((await vm.run(1000n)).reason,Exit.Returned);equal(vm.readInt(0x20200n),0n);equal(call(t,'OpenWaitableTimerW',[0x100000n,0n,0x20000n]),0n);equal(vm.readInt(t.error),2n);vm.dispose();
}


// Mixed wait-all does not consume a semaphore while the thread target is still alive.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,s=sem(t,1n),e=event(t,true,true);vm.write(0x30000n,[0xb8,41,0,0,0,...(mode===32?[0xc2,4,0]:[0xc3])]);const thread=call(t,'CreateThread',[0n,65536n,0x30000n,0n,4n,0n]);equal(multi(t,[s,thread,e],true),258n);equal(wait(t,s),0n);equal(call(t,'ReleaseSemaphore',[s,1n,0n]),1n);equal(call(t,'ResumeThread',[thread]),1n);for(const [n,h] of [s,thread,e].entries())vm.write(0x21000n+BigInt(n*mode/8),encode(h,mode/8));frame(t,'WaitForMultipleObjectsEx',[3n,0x21000n,1n,0xffffffffn,0n]);const w=vm.work;equal((await vm.run(0n)).reason,Exit.Budget);equal(vm.work,w);equal((await vm.run(1000n)).reason,Exit.Returned);equal(vm.register(0),0n);equal(wait(t,s),258n);equal(wait(t,e),0n);equal(t.time.elapsed,0);vm.dispose();
}

// A failed array read or LastError write never consumes an otherwise ready auto-reset event.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,e=event(t,false,true),p=0x24ffdn;vm.write(p,encode(e,mode/8));vm.write(p+BigInt(mode/8),encode(0n,mode/8));frame(t,'WaitForMultipleObjects',[2n,p,0n,0n]);equal(core.wire_protect(t.handle,0x25000n,4096n,1),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(core.wire_protect(t.handle,t.error&~4095n,4096n,0x104),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0xffffffffn);equal(wait(t,e),0n);
 equal(call(t,'WaitForMultipleObjects',[0n,t.invalid,1n,0n]),0xffffffffn);equal(vm.readInt(t.error),87n);equal(call(t,'WaitForMultipleObjects',[65n,t.invalid,0n,0n]),0xffffffffn);equal(vm.readInt(t.error),87n);equal(call(t,'WaitForSingleObjectEx',[e,0n,1n]),258n);equal(vm.readInt(t.error),87n);vm.dispose();
}

// Editing a suspended CPU cancels its pending wait and releases the retained object reference.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.write(0x20000n,wide('cancelled'));const h=timer(t,false,0x20000n);equal(set(t,h,-100n),1n);frame(t,'WaitForSingleObject',[h,0xffffffffn]);equal((await vm.run(100n)).reason,Exit.Waiting);equal(call(t,'CloseHandle',[h]),1n);equal(call(t,'OpenWaitableTimerW',[0x100000n,0n,0x20000n]),0n);equal(vm.readInt(t.error),2n);vm.dispose();
}

// Clock failures occur before timer mutation, and the due-time classification observes guards without consuming them.
for(const mode of [32,64]){
 const t=await machine(mode),{vm,time}=t,h=timer(t);equal(set(t,h,-100n),1n);time.now=50n;vm.write(0x22000n,encode(time.wall+1000n,8));frame(t,'SetWaitableTimer',[h,0x22000n,0n,0n,0n,0n]);const before=snapshot(t);time.failWall=true;assert.throws(()=>vm.service(),/wall failure/);equal(snapshot(t),before);time.failWall=false;time.now=100n;equal(wait(t,h),0n);
 vm.write(0x22000n,encode(BigInt.asUintN(64,-50n),8));frame(t,'SetWaitableTimer',[h,0x22000n,0n,0n,0n,0n]);equal(core.wire_protect(t.handle,0x22000n,4096n,0x104),0);equal(core.wire_service_clock(t.handle),1);const guarded=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),guarded);equal(core.wire_service_clock(t.handle),2);equal(vm.service().reason,Exit.Dispatch);time.now=150n;equal(wait(t,h),0n);equal(set(t,h,-100n,0xffffffffn),0n);equal(vm.readInt(t.error),87n);
 frame(t,'SetWaitableTimerEx',[h,0x22000n,0n,0n,0n,1n,0n]);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0n);equal(vm.readInt(t.error),50n);vm.dispose();
}

// SECURITY_ATTRIBUTES and extended APIs share the same alias rights and namespace in both ABIs.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,sa=0x20200n;vm.write(sa,new Uint8Array(24));vm.write(sa,encode(BigInt(mode===32?12:24),4));vm.write(sa+BigInt(mode===32?8:16),encode(1n,4));const e=call(t,'CreateEventExA',[sa,0n,3n,0x1f0003n]);equal(call(t,'GetHandleInformation',[e,0x20100n]),1n);equal(vm.readInt(0x20100n),1n);equal(wait(t,e),0n);equal(wait(t,e),0n);equal(call(t,'CreateEventExW',[0n,0n,4n,0x1f0003n]),0n);equal(vm.readInt(t.error),87n);equal(call(t,'CreateSemaphoreExW',[sa,1n,2n,0n,0n,0x1f0003n])!==0n,true);equal(call(t,'CreateMutexExA',[sa,0n,1n,0x1f0001n])!==0n,true);equal(call(t,'CreateWaitableTimerExA',[sa,0n,2n,0x1f0003n])!==0n,true);
 vm.write(sa+BigInt(mode===32?4:8),encode(1n,mode/8));equal(call(t,'CreateEventW',[sa,0n,0n,0n]),0n);equal(vm.readInt(t.error),50n);if(mode===64)equal(vm.bind('kernelbase.dll','CreateMutexW'),vm.bind('kernel32.dll','CreateMutexW'));vm.dispose();
}

// Capacity is returned by closing objects, while identities keep increasing across reuse.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,list=[];for(let n=0;n<1024;n++)list.push(event(t));equal(event(t),0n);equal(vm.readInt(t.error),8n);for(const h of list)equal(call(t,'CloseHandle',[h]),1n);const fresh=event(t);assert.ok(!list.includes(fresh));equal(call(t,'SetHandleInformation',[fresh,2n,2n]),1n);equal(call(t,'CloseHandle',[fresh]),0n);equal(vm.readInt(t.error),6n);equal(call(t,'SetHandleInformation',[fresh,2n,0n]),1n);equal(call(t,'CloseHandle',[fresh]),1n);vm.dispose();
}
// Pulse completes only existing waits, atomically acquiring wait-all peers before resetting.
for(const manual of [false,true]){
 const t=await machine(32),{vm}=t,e=event(t,manual),workers=[];
 for(let n=0;n<3;n++){const p=0x20200n+BigInt(n*4),base=0x30000n+BigInt(n*256);vm.write(p,encode(0xffffffffn,4));const a=asm(t,base);a.api('WaitForSingleObject',[e,0xffffffffn]);a.store(p);a.finish();workers.push(create(t,base));}
 frame(t,'Sleep',[0n]);equal((await vm.run(1000n)).reason,Exit.Returned);equal(call(t,'PulseEvent',[e]),1n);equal(wait(t,e),258n);
 // A later Set/Reset or handle close cannot revoke the completed pulse.
 equal(call(t,'SetEvent',[e]),1n);equal(call(t,'ResetEvent',[e]),1n);frame(t,'Sleep',[0n]);equal((await vm.run(1000n)).reason,Exit.Returned);
 const values=()=>workers.map((_,n)=>vm.readInt(0x20200n+BigInt(n*4)));equal(values().filter(v=>v===0n).length,manual?3:1);
 if(!manual){equal(call(t,'PulseEvent',[e]),1n);equal(call(t,'PulseEvent',[e]),1n);equal(call(t,'CloseHandle',[e]),1n);frame(t,'Sleep',[0n]);equal((await vm.run(1000n)).reason,Exit.Returned);equal(values(),[0n,0n,0n]);}
 equal(t.time.elapsed,0);vm.dispose();
}
for(const ready of [false,true]){
 const t=await machine(32),{vm}=t,e=event(t),s=sem(t,ready?1n:0n);vm.write(0x21000n,[...encode(e,4),...encode(s,4)]);vm.write(0x20200n,encode(0xffffffffn,4));
 const a=asm(t);a.api('WaitForMultipleObjects',[2n,0x21000n,1n,0xffffffffn]);a.store(0x20200n);a.finish();const worker=create(t);frame(t,'Sleep',[0n]);equal((await vm.run(1000n)).reason,Exit.Returned);
 equal(call(t,'PulseEvent',[e]),1n);equal(wait(t,e),258n);equal(wait(t,s),258n);frame(t,'Sleep',[0n]);equal((await vm.run(1000n)).reason,Exit.Returned);equal(vm.readInt(0x20200n),ready?0n:0xffffffffn);
 if(!ready){equal(call(t,'ReleaseSemaphore',[s,1n,0n]),1n);equal(call(t,'PulseEvent',[e]),1n);frame(t,'WaitForSingleObject',[worker,0xffffffffn]);equal((await vm.run(1000n)).reason,Exit.Returned);equal(vm.readInt(0x20200n),0n);}
 vm.dispose();
}
// A pending timeout requires one sample at pulse time; a clock failure commits no wake.
{
 const t=await machine(32),{vm,time}=t,e=event(t);vm.write(0x20200n,encode(0xffffffffn,4));const a=asm(t);a.api('WaitForSingleObject',[e,1n]);a.store(0x20200n);a.finish();const worker=create(t);frame(t,'Sleep',[0n]);equal((await vm.run(1000n)).reason,Exit.Returned);
 frame(t,'PulseEvent',[e]);time.failElapsed=true;const before=snapshot(t);assert.throws(()=>vm.service(),/elapsed failure/);equal(snapshot(t),before);time.failElapsed=false;time.now=10001n;equal(vm.service().reason,Exit.Dispatch);frame(t,'WaitForSingleObject',[worker,0xffffffffn]);equal((await vm.run(1000n)).reason,Exit.Returned);equal(vm.readInt(0x20200n),258n);equal(wait(t,e),258n);vm.dispose();
}
{
 const t=await machine(32),{vm}=t,e=event(t),s=sem(t);for(const[h,code]of [[0n,6n],[s,6n]]){equal(call(t,'PulseEvent',[h]),0n);equal(vm.readInt(t.error),code);}
 equal(call(t,'DuplicateHandle',[t.invalid,e,t.invalid,0x20110n,0x100000n,0n,0n]),1n);equal(call(t,'PulseEvent',[vm.readInt(0x20110n)]),0n);equal(vm.readInt(t.error),5n);vm.dispose();
 const q=await machine(64);assert.throws(()=>q.vm.bind('kernel32.dll','PulseEvent'),/unknown/);q.vm.dispose();
}
// APC interruption wins before the pulse, and timer peers use the pulse-time sample.
{
 const t=await machine(32),{vm}=t,e=event(t),a=asm(t),apc=asm(t,0x31000n);apc.emit(0xff,0x05);apc.value(0x20204n,4);apc.finish(0);a.api('WaitForSingleObjectEx',[e,0xffffffffn,1n]);a.store(0x20200n);a.finish();const worker=create(t);frame(t,'Sleep',[0n]);equal((await vm.run(1000n)).reason,Exit.Returned);
 equal(call(t,'QueueUserAPC',[0x31000n,worker,0n]),1n);equal(call(t,'PulseEvent',[e]),1n);frame(t,'WaitForSingleObject',[worker,0xffffffffn]);equal((await vm.run(1000n)).reason,Exit.Returned);equal(vm.readInt(0x20200n),0xc0n);equal(vm.readInt(0x20204n),1n);equal(wait(t,e),258n);vm.dispose();
}
{
 const t=await machine(32),{vm,time}=t,e=event(t),h=timer(t);equal(set(t,h,-100n),1n);vm.write(0x21000n,[...encode(e,4),...encode(h,4)]);vm.write(0x20200n,encode(0xffffffffn,4));const a=asm(t);a.api('WaitForMultipleObjects',[2n,0x21000n,1n,0xffffffffn]);a.store(0x20200n);a.finish();const worker=create(t);frame(t,'Sleep',[0n]);equal((await vm.run(1000n)).reason,Exit.Returned);
 time.now=99n;equal(call(t,'PulseEvent',[e]),1n);equal(vm.readInt(0x20200n),0xffffffffn);time.now=100n;equal(call(t,'PulseEvent',[e]),1n);time.failElapsed=true;frame(t,'WaitForSingleObject',[worker,0xffffffffn]);equal((await vm.run(1000n)).reason,Exit.Returned);equal(vm.readInt(0x20200n),0n);equal(wait(t,e),258n);equal(wait(t,h),258n);vm.dispose();
}
console.log(`${checks} event/semaphore/mutex/timer/pulse/mixed-wait checks passed`);
