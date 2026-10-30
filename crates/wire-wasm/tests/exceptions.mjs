import assert from 'node:assert/strict';
import {WasmMachine,Exit} from '../web/runtime.mjs';
import {core,stateLayout,encode,fp} from './x87-harness.mjs';
import {fixture} from './pe-fixture.mjs';
import {dynamicFixture,trace} from './dynamic-fixture.mjs';
let checks=0;const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
const snapshot=t=>new Uint8Array(core.memory.buffer,core.wire_state(t.handle),stateLayout[0]).slice();
async function machine(mode){
 let handle;const vm=new WasmMachine({...core,wire_create(...a){handle=core.wire_create(...a);return handle;}},{mode,clock(){throw new Error('unexpected clock');},wallClock(){throw new Error('unexpected wall clock');}});
 const f=fixture(mode,{tls:false});f.put(f.dirs+8,0);f.put(f.dirs+12,0);f.bytes.fill(0x90,f.offset(0x1000),f.offset(0x1100));f.bytes[f.offset(0x1000)]=0xc3;f.relocations.splice(0,f.relocations.length,...f.relocations.filter(r=>r>=0x1100));f.rebuildRelocations();const image=vm.load(f.bytes);equal((await vm.run(100n)).reason,Exit.Returned);vm.map(0x20000n,0x30000n,7);
 return {vm,handle,mode,image,sp:image.sp-2048n,error:image.teb+BigInt(mode===32?0x34:0x68)};
}
function frame(t,name,args=[]){const {vm,sp,mode}=t;vm.ip=vm.bind('kernel32.dll',name);vm.register(4,sp);vm.write(sp,encode(vm.entryPoints.returned,mode/8));for(let n=0;n<args.length;n++)if(mode===32)vm.write(sp+4n+BigInt(n*4),encode(BigInt.asUintN(32,args[n]),4));else if(n<4)vm.register([1,2,8,9][n],args[n]);else vm.write(sp+40n+BigInt(n-4)*8n,encode(args[n],8));}
function call(t,name,args=[]){frame(t,name,args);const w=t.vm.work,r=t.vm.retired;equal(t.vm.service().reason,Exit.Dispatch);equal(t.vm.work,w+1n);equal(t.vm.retired,r);return t.vm.register(0);}
async function finish(t,budget=1000n){for(let n=0;n<10000;n++){const result=await t.vm.run(budget);if(result.reason===Exit.Returned)return result;equal(result.reason,Exit.Budget);}throw new Error('exception did not resume');}
function asm(t,base=0x30000n){const {mode,vm}=t,bytes=[];const emit=(...v)=>bytes.push(...v),value=(v,n)=>emit(...encode(BigInt.asUintN(n*8,BigInt(v)),n));const store=(p,width=mode/8)=>{if(mode===32){emit(0xa3);value(p,4);}else{emit(0x49,0xbb);value(p,8);emit(width===8?0x49:0x41,0x89,0x03);}};const api=(name,args=[])=>{const pc=vm.bind('kernel32.dll',name);if(mode===32){for(const p of args.toReversed()){emit(0x68);value(p,4);}emit(0xb8);value(pc,4);emit(0xff,0xd0);}else{emit(0x48,0x83,0xec,0x38);for(let n=0;n<args.length;n++){if(n<4){emit(...[[0x48,0xb9],[0x48,0xba],[0x49,0xb8],[0x49,0xb9]][n]);value(args[n],8);}else{emit(0x48,0xb8);value(args[n],8);emit(0x48,0x89,0x44,0x24,32+(n-4)*8);}}emit(0x48,0xb8);value(pc,8);emit(0xff,0xd0,0x48,0x83,0xc4,0x38);}};
 return{emit,value,store,api,context(){if(mode===32)emit(0x8b,0x44,0x24,4,0x8b,0x40,4);else emit(0x48,0x8b,0x41,8);},field(offset,v,width=4){if(width===8)emit(0x48);emit(0xc7,0x80);value(offset,4);value(v,4);},inc(p){if(mode===32){emit(0xff,0x05);value(p,4);}else{emit(0x49,0xbb);value(p,8);emit(0x41,0xff,0x03);}},finish(result=-1,cleanup=4){emit(0xb8);value(result,4);emit(...(mode===32?[0xc2,cleanup,0]:[0xc3]));vm.write(base,bytes);},bytes};}
const add=(t,p=0x30000n,first=0n)=>call(t,'AddVectoredExceptionHandler',[first,p]);
const raise=(t,flags=0n,code=0xe0420042n,args=0n,count=0n)=>{frame(t,'RaiseException',[code,flags,count,args]);equal(t.vm.service().reason,Exit.Dispatch);};

// An explicit filter call returns the LONG and restores its caller, without interpreting CONTEXT.
for(const mode of [32,64])for(const result of [0,1,-1,73]){
 const t=await machine(mode),{vm}=t,a=asm(t),argument=mode===32?0xdeadn:0x12345678deadbeefn;equal(call(t,'UnhandledExceptionFilter',[argument]),0n);
 a.emit(...(mode===32?[0x8b,0x44,0x24,4]:[0x48,0x89,0xc8]));a.store(0x20200n);a.inc(0x20210n);a.emit(0x66,0x0f,0xef,0xc9);a.api('Sleep',[0n]);a.finish(result);call(t,'SetUnhandledExceptionFilter',[0x30000n]);
 vm.flags=0x8d5n;vm.xmm(1,new Uint8Array(16).fill(0x7b));frame(t,'UnhandledExceptionFilter',[argument]);const work=vm.work,retired=vm.retired;equal(vm.service().reason,Exit.Dispatch);await finish(t,1n);
 equal(vm.register(0),BigInt.asUintN(32,BigInt(result)));equal(vm.readInt(0x20200n,mode/8),argument);equal(vm.readInt(0x20210n),1n);equal(vm.register(4),t.sp+8n);equal(vm.flags,0x8d5n);equal(vm.xmm(1),new Uint8Array(16).fill(0x7b));equal(vm.work-work,vm.retired-retired+2n);vm.dispose();
}

// Cleanup shares the legacy registration plan; the target stays installed and TargetIp is unused.
for(const external of [false,true])for(const exit of [false,true]){
 const t=await machine(32),{vm}=t,a=asm(t),first=t.sp+256n,target=first+32n;
 for(let n=0;n<3;n++){const at=first+BigInt(n*16);vm.write(at,encode(n===2?0xffffffffn:at+16n,4));vm.write(at+4n,encode(0x30000n,4));}
 vm.write(t.image.teb,encode(first,4));
 a.inc(0x20200n);a.emit(0x8b,0x44,0x24,4);a.store(0x20210n);a.emit(0x8b,0x00);a.store(0x20214n);a.emit(0x8b,0x44,0x24,12);a.store(0x20218n);a.field(176,73);a.api('Sleep',[0n]);a.finish(1,0);
 if(external){vm.write(0x21000n,new Uint8Array(80));vm.write(0x21000n,encode(0xe0420042n,4));vm.write(0x21004n,encode(1n,4));}
 frame(t,'RtlUnwind',[exit?0n:target,0xdeadn,external?0x21000n:0n,0xfacecafen]);
 equal(vm.service().reason,Exit.Dispatch);await finish(t,1n);equal(vm.register(0),73n);equal(vm.register(4),t.sp+20n);equal(vm.readInt(t.image.teb),exit?0xffffffffn:target);equal(vm.readInt(0x20200n),exit?3n:2n);equal(vm.readInt(0x20214n),external?0xe0420042n:0xc0000027n);const record=vm.readInt(0x20210n);equal(vm.readInt(record+4n),BigInt((external?1:0)|2|(exit?4:0)));vm.dispose();
}

// Corrupt plans stop before cleanup; completed cleanup survives registration/context faults.
{
 const t=await machine(32);t.sp-=4096n;const {vm}=t,a=asm(t),first=t.sp+4096n+256n;
 a.inc(0x20200n);a.emit(0x8b,0x44,0x24,12);a.store(0x20218n);a.finish(1,0);
 vm.write(first,encode(first,4));vm.write(first+4n,encode(0x30000n,4));vm.write(t.image.teb,encode(first,4));frame(t,'RtlUnwind',[0n,0n,0n,7n]);const before=snapshot(t);assert.throws(()=>vm.service(),/cyclic/);equal(snapshot(t),before);equal(vm.readInt(0x20200n),0n);
 vm.write(first,encode(0xffffffffn,4));equal(vm.service().reason,Exit.Dispatch);
 for(let n=0;vm.ip!==vm.entryPoints.callback;n++){assert.ok(n<100);equal((await vm.run(1n)).reason,Exit.Budget);}
 const context=vm.readInt(0x20218n);equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);equal((await vm.run(10n)).reason,Exit.Fault);equal(vm.readInt(0x20200n),1n);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);
 equal(core.wire_protect(t.handle,context&~4095n,4096n,1),0);equal((await vm.run(10n)).reason,Exit.Fault);equal(vm.readInt(t.image.teb),0xffffffffn);equal(core.wire_protect(t.handle,context&~4095n,4096n,4),0);await finish(t);equal(vm.register(0),7n);equal(vm.readInt(0x20200n),1n);vm.dispose();
}

// Catch transfers must release escaped callbacks before later exceptions reuse their bounded stack.
{
 const t=await machine(32),{vm}=t,first=t.sp+256n,target=first+32n,a=asm(t),h=asm(t,0x31000n),caught=asm(t,0x32000n);
 a.finish(1,0);h.emit(0x8b,0x44,0x24,4,0x6a,0,0x50,0x6a,0,0x68);h.value(target,4);h.emit(0xb8);h.value(vm.bind('ntdll.dll','RtlUnwind'),4);h.emit(0xff,0xd0,0xbc);h.value(target-16n,4);h.emit(0xb8);h.value(0x32000n,4);h.emit(0xff,0xe0);vm.write(0x31000n,h.bytes);
 caught.api('GetLastError');caught.inc(0x20200n);caught.emit(0xb8);caught.value(vm.entryPoints.returned,4);caught.emit(0xff,0xe0);vm.write(0x32000n,caught.bytes);
 for(let n=0;n<33;n++){vm.write(first,encode(target,4));vm.write(first+4n,encode(0x30000n,4));vm.write(target,encode(0xffffffffn,4));vm.write(target+4n,encode(0x31000n,4));vm.write(t.image.teb,encode(first,4));raise(t,1n);await finish(t);equal(vm.readInt(t.image.teb),target);}
 equal(vm.readInt(0x20200n),33n);vm.dispose();
}
// Independent Windows offsets plus full state comparisons detect loss across the callback boundary.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,a=asm(t);a.context();a.store(0x20080n);a.field(mode===32?176:120,0x1234,mode/8);a.field(mode===32?164:144,0x5678,mode/8);a.field(mode===32?192:68,0x247);a.field(mode===32?364:416,0x11223344);if(mode===64)a.field(52,0x3f80);a.field(mode===32?228:280,0x3f80);a.emit(0x66,0x0f,0xef,0xc9);a.inc(0x20090n);a.finish();assert.ok(add(t));
 vm.flags=0x202n;for(let n=0;n<(mode===32?8:16);n++)vm.xmm(n,new Uint8Array(16).fill(n+1));const initial=fp(t);initial.sw=3<<11;initial.tags=0;initial.ip=0x1234n;initial.dp=0x5678n;initial.opcode=0x321;initial.regs=Array.from({length:8},(_,n)=>(0x3fffn<<64n)|0x8000000000000000n|(BigInt(n)<<57n));fp(t,initial);const oldFp=fp(t);raise(t);const w=vm.work,r=vm.retired,before=snapshot(t);equal((await vm.run(0n)).reason,Exit.Budget);equal(snapshot(t),before);await finish(t,1n);equal(vm.register(0),0x1234n);equal(vm.register(3),0x5678n);equal(vm.flags,0x247n);equal(vm.mxcsr,0x3f80);const x=vm.xmm(0);equal([...x.slice(0,4)],[0x44,0x33,0x22,0x11]);equal([...vm.xmm(1)],new Array(16).fill(2));equal(fp(t),oldFp);equal(vm.readInt(0x20090n),1n);equal(vm.work-w,vm.retired-r);equal(vm.register(4),t.sp+BigInt(mode===32?20:8));vm.dispose();
}

// A context-read fault at return resumes the completed handler without replaying it.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,a=asm(t);a.context();a.store(0x20080n);a.inc(0x20090n);a.finish();add(t);raise(t);for(let n=0;vm.ip!==vm.entryPoints.callback;n++){assert.ok(n<100);equal((await vm.run(1n)).reason,Exit.Budget);}const p=vm.readInt(0x20080n,mode/8),page=p&~4095n;equal(core.wire_protect(t.handle,page,4096n,1),0);const before=snapshot(t);equal((await vm.run(10n)).reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,page,4096n,4),0);await finish(t);equal(vm.readInt(0x20090n),1n);vm.dispose();
}

// Record/frame preflight and input faults leave the original service call uncharged.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,a=asm(t);a.finish();add(t);frame(t,'RaiseException',[0xe0420042n,0n,1n,0x24fffn]);equal(core.wire_protect(t.handle,0x25000n,4096n,1),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);await finish(t);vm.dispose();
 const q=await machine(mode),b=asm(q);b.finish();add(q);q.sp=(q.image.sp&~4095n)+256n;frame(q,'RaiseException',[0xe0420042n,0n,0n,0n]);const guard=(q.sp&~4095n)-4096n;equal(core.wire_protect(q.handle,guard,4096n,0x104),0);const saved=snapshot(q);equal(q.vm.service().reason,Exit.Fault);equal(snapshot(q),saved);equal(q.vm.service().reason,Exit.Dispatch);await finish(q);q.vm.dispose();
}

// Nested RaiseException restores the outer handler before restoring the original caller.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,a=asm(t);if(mode===32)a.emit(0x8b,0x44,0x24,4,0x8b,0x00);else a.emit(0x48,0x8b,0x01);a.emit(0x81,0x38);a.value(0xe0420043,4);a.emit(0x74,0);const jump=a.bytes.length-1;a.api('RaiseException',[0xe0420043n,0n,0n,0n]);a.inc(0x20090n);const inner=a.bytes.length;a.bytes[jump]=inner-jump-1;a.inc(0x20094n);a.finish();add(t);raise(t);await finish(t,1n);equal(vm.readInt(0x20090n),1n);equal(vm.readInt(0x20094n),2n);vm.dispose();
}

// Thread-local callback stacks survive yields by both the initial thread and a worker.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=asm(t);h.context();h.emit(...(mode===32?[0x89,0xc3]:[0x48,0x89,0xc3]));h.api('Sleep',[0n]);h.api('GetCurrentThreadId');h.emit(...(mode===32?[]:[0x48]),0x89,0x83);h.value(mode===32?176:120,4);h.finish();add(t);const worker=asm(t,0x31000n);worker.api('RaiseException',[0xe0420042n,0n,0n,0n]);worker.store(0x20210n,4);worker.finish(73);const thread=call(t,'CreateThread',[0n,65536n,0x31000n,0n,0n,0n]);raise(t);await finish(t);equal(vm.register(0),1n);frame(t,'WaitForSingleObject',[thread,0xffffffffn]);await finish(t);equal(vm.readInt(0x20210n),2n);vm.dispose();
}

// A handler can load and unload DLLs without confusing the two callback return owners.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.supplyModule('leaf.dll',dynamicFixture(mode,{id:1,seed:73,tls:true}).bytes);vm.write(0x20000n,new TextEncoder().encode('leaf.dll\0'));const a=asm(t);a.api('LoadLibraryA',[0x20000n]);a.store(0x20200n);a.finish();add(t);raise(t);await finish(t,1n);const base=vm.readInt(0x20200n,mode/8);assert.ok(base);equal(vm.moduleBase('leaf.dll'),base);equal(vm.readInt(trace+16n),1n);frame(t,'FreeLibrary',[base]);await finish(t,1n);equal(vm.moduleBase('leaf.dll'),0n);vm.dispose();
}

// A DLL initializer can raise through the handler without releasing or consuming its loader frame.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,a=asm(t);a.inc(0x20250n);a.finish();add(t);const f=dynamicFixture(mode,{id:1,seed:73,entry:false}),init=asm(t,0x34000n);f.put(f.opt+16,0x1000);init.api('RaiseException',[0xe0420042n,0n,0n,0n]);init.finish(1,12);f.bytes.fill(0x90,f.offset(0x1000),f.offset(0x1100));f.bytes.set(init.bytes,f.offset(0x1000));f.relocations.splice(0,f.relocations.length,...f.relocations.filter(r=>r>=0x1100));f.rebuildRelocations();vm.supplyModule('raising.dll',f.bytes);vm.write(0x20000n,new TextEncoder().encode('raising.dll\0'));frame(t,'LoadLibraryA',[0x20000n]);await finish(t,1n);assert.ok(vm.register(0));equal(vm.readInt(0x20250n),1n);vm.dispose();
}

// ExitThread abandons a worker's callback continuation before its stack is reclaimed.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,a=asm(t);a.api('ExitThread',[73n]);a.finish();add(t);const worker=asm(t,0x31000n);worker.api('RaiseException',[0xe0420042n,0n,0n,0n]);worker.inc(0x20250n);worker.finish(1);const thread=call(t,'CreateThread',[0n,65536n,0x31000n,0n,0n,0n]);frame(t,'WaitForSingleObject',[thread,0xffffffffn]);await finish(t);equal(call(t,'GetExitCodeThread',[thread,0x20240n]),1n);equal(vm.readInt(0x20240n),73n);equal(vm.readInt(0x20250n),0n);vm.dispose();
}

// Exhausted searches stop once with the Windows code, including zero; CPU edits allow resumption.
for(const mode of [32,64])for(const code of [0n,0xe0420042n]){
 const t=await machine(mode),{vm}=t,a=asm(t);a.inc(0x20090n);a.finish(0);add(t);raise(t,0n,code);const result=await vm.run(100n);equal(result.reason,Exit.Exception);equal(result.exception,{code:Number(code)});equal(vm.readInt(0x20090n),1n);const before=snapshot(t);equal((await vm.run(100n)).reason,Exit.Exception);equal(snapshot(t),before);equal(vm.readInt(0x20090n),1n);vm.ip=vm.entryPoints.returned;equal((await vm.run(10n)).reason,Exit.Returned);vm.dispose();
}

// PE32 registration handlers use four arguments and EXCEPTION_DISPOSITION, then the top-level filter.
for(const filter of [false,true]){
 const t=await machine(32),{vm}=t,a=asm(t);if(filter){a.context();a.field(176,73);a.finish();call(t,'SetUnhandledExceptionFilter',[0x30000n]);}else{a.emit(0x8b,0x44,0x24,12);a.field(176,73);a.finish(0,0);const registration=t.sp+256n;vm.write(registration,encode(0xffffffffn,4));vm.write(registration+4n,encode(0x30000n,4));vm.write(t.image.teb,encode(registration,4));}raise(t);await finish(t,1n);equal(vm.register(0),73n);vm.dispose();
}

// Noncontinuable disposition and invalid CONTEXT changes remain explicit, retryable errors.
for(const mode of [32,64]){
 const t=await machine(mode),a=asm(t);a.finish();add(t);raise(t,1n);await assert.rejects(()=>t.vm.run(100n),/noncontinuable/);t.vm.dispose();
 const q=await machine(mode),b=asm(q);b.context();b.store(0x20080n);b.field(mode===32?228:280,0xffffffff);if(mode===64)b.field(52,0xffffffff);b.finish();add(q);raise(q);await assert.rejects(()=>q.vm.run(100n),/MXCSR/);const p=q.vm.readInt(0x20080n,mode/8);q.vm.write(p+BigInt(mode===32?228:280),encode(0x1f80n,4));if(mode===64)q.vm.write(p+52n,encode(0x1f80n,4));await finish(q);q.vm.dispose();
}

// Registration capacity, token lifetime and error-slot faults do not invent registrations.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;equal(add(t,0n),0n);equal(vm.readInt(t.error),87n);const tokens=[];for(let n=0;n<64;n++)tokens.push(add(t));equal(add(t),0n);equal(vm.readInt(t.error),8n);for(const token of tokens)equal(call(t,'RemoveVectoredExceptionHandler',[token]),1n);equal(call(t,'RemoveVectoredExceptionHandler',[tokens[0]]),0n);for(let n=64;n<256;n++){const token=add(t);equal(call(t,'RemoveVectoredExceptionHandler',[token]),1n);}equal(add(t),0n);equal(vm.readInt(t.error),8n);vm.dispose();
 const q=await machine(mode);frame(q,'AddVectoredExceptionHandler',[0n,0n]);equal(core.wire_protect(q.handle,q.error&~4095n,4096n,0x104),0);const before=snapshot(q);equal(q.vm.service().reason,Exit.Fault);equal(snapshot(q),before);equal(q.vm.service().reason,Exit.Dispatch);equal(add(q),0x7fff8000n);q.vm.dispose();
}
// CPU traps and read/execute faults reach the same handlers with Windows records.
for(const mode of [32,64])for(const kind of ['divide','ud','read','fetch']){
 const t=await machine(mode),{vm}=t,base=0x32000n,a=asm(t);if(mode===32)a.emit(0x8b,0x44,0x24,4,0x8b,0);else a.emit(0x48,0x8b,0x01);a.store(0x20240n);a.context();const faulting=kind==='divide'?[0xb8,10,0,0,0,0x31,0xd2,0x31,0xc9,0xf7,0xf1]:kind==='ud'?[0x0f,0x01,0xd0]:kind==='read'?[...(mode===32?[]:[0x48]),0xa1,...encode(0xdead000n,mode/8)]:[];const resume=base+BigInt(faulting.length);a.field(mode===32?184:248,resume,mode/8);a.finish();add(t);vm.write(base,[...faulting,0xb8,73,0,0,0,0xc3]);vm.ip=kind==='fetch'?0xdead000n:base;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,mode/8));await finish(t,1n);equal(vm.register(0),73n);const record=vm.readInt(0x20240n,mode/8);equal(vm.readInt(record),kind==='divide'?0xc0000094n:kind==='ud'?0xc000001dn:0xc0000005n);equal(vm.readInt(record+BigInt(mode===32?12:16),mode/8),kind==='fetch'?0xdead000n:kind==='divide'?base+9n:base);if(kind==='read'||kind==='fetch'){const at=record+BigInt(mode===32?20:32);equal(vm.readInt(at,mode/8),kind==='fetch'?8n:0n);equal(vm.readInt(at+BigInt(mode/8),mode/8),0xdead000n);}vm.dispose();
}

// A delivery-frame guard fault retains the original guard exception after both guards clear.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,a=asm(t);if(mode===32)a.emit(0x8b,0x44,0x24,4,0x8b,0);else a.emit(0x48,0x8b,0x01);a.store(0x20240n);a.inc(0x20250n);a.finish();add(t);vm.write(0x24000n,encode(73n,mode/8));vm.write(0x32000n,[...(mode===32?[]:[0x48]),0xa1,...encode(0x24000n,mode/8),0xc3]);t.sp=(t.image.sp&~4095n)+256n;vm.ip=0x32000n;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,mode/8));equal(core.wire_protect(t.handle,0x24000n,4096n,0x104),0);const guard=(t.sp&~4095n)-4096n;equal(core.wire_protect(t.handle,guard,4096n,0x104),0);const before=snapshot(t);const fault=await vm.run(100n);equal(fault.reason,Exit.Fault);equal(fault.fault.address&~4095n,guard);equal(snapshot(t),before);equal(vm.readInt(0x20250n),0n);await finish(t,1n);equal(vm.register(0),73n);const record=vm.readInt(0x20240n,mode/8);equal(vm.readInt(record),0x80000001n);equal(vm.readInt(record+BigInt(mode===32?24:40),mode/8),0x24000n);equal(vm.readInt(0x20250n),1n);vm.dispose();
}

// Unmasked x87 and SIMD exceptions preserve the status in CONTEXT and can be masked by a handler.
for(const mode of [32,64])for(const kind of ['x87','simd']){
 const t=await machine(mode),{vm}=t,a=asm(t);if(mode===32)a.emit(0x8b,0x44,0x24,4,0x8b,0);else a.emit(0x48,0x8b,0x01);a.store(0x20240n);a.context();if(kind==='x87')a.field(mode===32?204:256,0x37f);else{a.field(mode===32?228:280,0x1f80);if(mode===64)a.field(52,0x1f80);}a.finish();add(t);if(kind==='x87'){const f=fp(t);f.cw=0x37b;f.sw=0x8084;fp(t,f);}else{vm.xmm(0,encode(0x3f800000n));vm.xmm(1,new Uint8Array(16));vm.mxcsr=0x1d80;}vm.write(0x32000n,[...(kind==='x87'?[0x9b]:[0xf3,0x0f,0x5e,0xc1]),0xb8,73,0,0,0,0xc3]);vm.ip=0x32000n;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,mode/8));await finish(t,1n);equal(vm.register(0),73n);const record=vm.readInt(0x20240n,mode/8);equal(vm.readInt(record),0xc000008en);if(kind==='simd')equal([...vm.xmm(0).slice(0,4)],[0,0,128,127]);vm.dispose();
}
// Function lookup preflights aliased/cross-page outputs and retains caller-owned dynamic tables.
{
 const t=await machine(64),{vm}=t,table=0x28000n;vm.write(t.error,encode(99n,4));vm.write(table,[...encode(0x100n,4),...encode(0x120n,4),...encode(0x400n,4)]);equal(call(t,'RtlAddFunctionTable',[table,0n,0x40000n]),0n);equal(call(t,'RtlAddFunctionTable',[table,1n,0x40000n]),1n);equal(call(t,'RtlAddFunctionTable',[table,1n,0x40000n]),0n);equal(call(t,'RtlLookupFunctionEntry',[0x40110n,t.sp,0n]),table);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp,8),0x40000n);equal(call(t,'RtlLookupFunctionEntry',[0x40120n,0x20200n,0n]),0n);equal(vm.readInt(0x20200n,8),0n);equal(vm.readInt(t.error),99n);
 frame(t,'RtlLookupFunctionEntry',[0x40110n,0x24fffn,0n]);equal(core.wire_protect(t.handle,0x25000n,4096n,1),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),table);equal(vm.readInt(0x24fffn,8),0x40000n);equal(call(t,'RtlDeleteFunctionTable',[table]),1n);equal(call(t,'RtlDeleteFunctionTable',[table]),0n);equal(call(t,'RtlLookupFunctionEntry',[0x40110n,0x20200n,0n]),0n);
 const tokens=[];for(let n=0;n<128;n++){const at=table+BigInt(n*16);vm.write(at,[...encode(0x100n,4),...encode(0x120n,4),...encode(0x400n,4)]);equal(call(t,'RtlAddFunctionTable',[at,1n,0x40000n]),1n);tokens.push(at);}vm.write(table+2048n,[...encode(0x100n,4),...encode(0x120n,4),...encode(0x400n,4)]);equal(call(t,'RtlAddFunctionTable',[table+2048n,1n,0x40000n]),0n);for(const token of tokens)equal(call(t,'RtlDeleteFunctionTable',[token]),1n);vm.dispose();
}

// History is advisory: stale cache entries never authorize deleted tables or modify outputs.
{
 const t=await machine(64),{vm}=t,table=0x28000n,history=0x27000n;vm.write(table,[...encode(0x100n,4),...encode(0x120n,4),...encode(0x400n,4)]);vm.write(history,new Uint8Array(216).fill(0xa5));equal(call(t,'RtlAddFunctionTable',[table,1n,0x40000n]),1n);
 for(const hint of [history,1n,0x123400000000n]){equal(call(t,'RtlLookupFunctionEntry',[0x40110n,0x20200n,hint]),table);equal(vm.readInt(0x20200n,8),0x40000n);}
 equal(Uint8Array.from({length:216},(_,n)=>Number(vm.readInt(history+BigInt(n),1))),new Uint8Array(216).fill(0xa5));equal(call(t,'RtlDeleteFunctionTable',[table]),1n);equal(call(t,'RtlLookupFunctionEntry',[0x40110n,0x20200n,history]),0n);equal(vm.readInt(0x20200n,8),0n);vm.dispose();
}

// Static tables follow DLL image lifetime; malformed PE tables never publish an image.
{
 const t=await machine(64),{vm}=t,f=dynamicFixture(64,{entry:false});f.put(f.dirs+3*8,0x2480);f.put(f.dirs+3*8+4,12);f.rput(0x2480,0x1000);f.rput(0x2484,0x1090);f.rput(0x2488,0x24a0);f.rput(0x24a0,1);vm.supplyModule('tables.dll',f.bytes);vm.write(0x20000n,new TextEncoder().encode('tables.dll\0'));frame(t,'LoadLibraryA',[0x20000n]);await finish(t);const base=vm.register(0);assert.ok(base);equal(call(t,'RtlLookupFunctionEntry',[base+0x1080n,0x20200n,0n]),base+0x2480n);equal(vm.readInt(0x20200n,8),base);equal(call(t,'RtlDeleteFunctionTable',[base+0x2480n]),0n);frame(t,'FreeLibrary',[base]);await finish(t);equal(call(t,'RtlLookupFunctionEntry',[base+0x1080n,0x20200n,0n]),0n);equal(vm.readInt(0x20200n,8),0n);vm.dispose();
 for(const size of [11,12]){const f=fixture(64,{tls:false});f.put(f.dirs+3*8,0x2480);f.put(f.dirs+3*8+4,size);f.rput(0x2480,0x1100);f.rput(0x2484,0x1000);f.rput(0x2488,0x24a0);const vm=new WasmMachine(core,{mode:64});assert.throws(()=>vm.load(f.bytes),/runtime function/);equal(vm.ip,0n);vm.dispose();}
}

// A metadata guard after RaiseException retries the language search without re-raising or replaying it.
{
 const t=await machine(64),{vm}=t,handler=asm(t);handler.emit(0x4c,0x89,0xc0);handler.field(120,73,8);handler.inc(0x20250n);handler.finish(0);const table=0x28000n;vm.write(table,[...encode(0x32000n,4),...encode(0x32100n,4),...encode(0x28100n,4)]);vm.write(0x28100n,[9,4,1,0,4,0x42,0,0,...encode(0x30000n,4)]);equal(call(t,'RtlAddFunctionTable',[table,1n,0n]),1n);const gateway=vm.bind('kernel32.dll','RaiseException');vm.write(0x32000n,[0x48,0x83,0xec,40,0xb9,...encode(0xe0420042n,4),0x31,0xd2,0x45,0x31,0xc0,0x45,0x31,0xc9,0x48,0xb8,...encode(gateway,8),0xff,0xd0,0x48,0x83,0xc4,40,0xc3]);vm.ip=0x32000n;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,8));const work=vm.work,retired=vm.retired;equal(core.wire_protect(t.handle,table,4096n,0x104),0);const result=await vm.run(1000n);equal(result.reason,Exit.Fault);equal(vm.ip,vm.entryPoints.callback);equal(vm.readInt(0x20250n),0n);await finish(t,1n);equal(vm.register(0),73n);equal(vm.readInt(0x20250n),1n);equal(vm.work-work,vm.retired-retired+1n);vm.dispose();
}
// Callback lookup preserves full-width inputs, caller state and a saved aliased return.
{
 const t=await machine(64),{vm}=t,base=0x123400000000n,pc=base+0x110n,entry=0x100028000n,cookie=0xfedcba9876543210n;
 vm.map(entry,4096n,3);vm.write(entry,[...encode(0x100n,4),...encode(0x120n,4),...encode(0x400n,4)]);
 const a=asm(t);a.emit(0x48,0x89,0xc8);a.store(0x20200n);a.emit(0x48,0x89,0xd0);a.store(0x20208n);a.inc(0x20210n);a.emit(0x48,0xb8);a.value(entry,8);a.emit(0xc3);vm.write(0x30000n,a.bytes);
 equal(call(t,'RtlInstallFunctionTableCallback',[0x28003n,base,0x1000n,0x30000n,cookie,0n]),1n);
 vm.flags=0x247n;vm.mxcsr=0x3f80;vm.xmm(7,new Uint8Array(16).fill(77));const beforeFp=fp(t),work=vm.work,retired=vm.retired;
 frame(t,'RtlLookupFunctionEntry',[pc,t.sp,1n]);equal(vm.service().reason,Exit.Dispatch);equal(vm.work,work+1n);await finish(t,1n);
 equal(vm.register(0),entry);equal(vm.readInt(t.sp,8),base);equal(vm.register(4),t.sp+8n);equal(vm.readInt(0x20200n,8),pc);equal(vm.readInt(0x20208n,8),cookie);equal(vm.readInt(0x20210n),1n);equal(vm.flags,0x247n);equal(vm.mxcsr,0x3f80);equal([...vm.xmm(7)],new Array(16).fill(77));equal(fp(t),beforeFp);equal(vm.work-work,vm.retired-retired+1n);
 equal(call(t,'RtlLookupFunctionEntry',[base-1n,0x20220n,0n]),0n);equal(call(t,'RtlLookupFunctionEntry',[base+0x1000n,0x20220n,0n]),0n);equal(vm.readInt(0x20210n),1n);
 equal(call(t,'RtlDeleteFunctionTable',[0x28003n]),1n);const b=asm(t);b.inc(0x20210n);b.finish(0);equal(call(t,'RtlInstallFunctionTableCallback',[0x28003n,base,0x1000n,0x30000n,0n,0n]),1n);call(t,'RtlLookupFunctionEntry',[pc,0x20220n,0n]);await finish(t);equal(vm.register(0),0n);equal(vm.readInt(0x20220n,8),0n);equal(vm.readInt(0x20210n),2n);vm.dispose();
}

// Registration/name and initial call-frame faults publish no callback or service charge.
{
 const t=await machine(64),{vm}=t,id=0x28003n,args=[id,0x40000n,0x100n,0x30000n,0n,0n];
 for(const change of [[0,id-1n],[2,0n],[3,0n],[1,(1n<<47n)-128n]]){const invalid=args.slice();invalid[change[0]]=change[1];equal(call(t,'RtlInstallFunctionTableCallback',invalid),0n);}
 vm.write(0x24ffen,[0x41,0]);equal(core.wire_protect(t.handle,0x25000n,4096n,1),0);frame(t,'RtlInstallFunctionTableCallback',[...args.slice(0,5),0x24ffen]);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(call(t,'RtlDeleteFunctionTable',[id]),0n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);
 equal(call(t,'RtlInstallFunctionTableCallback',[...args.slice(0,5),0x24ffen]),1n);equal(call(t,'RtlInstallFunctionTableCallback',args),0n);
 const a=asm(t);a.finish(0);t.sp=(t.image.sp&~4095n)+16n;frame(t,'RtlLookupFunctionEntry',[0x40010n,0x20200n,0n]);const guard=(t.sp&~4095n)-4096n;equal(core.wire_protect(t.handle,guard,4096n,0x104),0);const saved=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),saved);equal(vm.service().reason,Exit.Dispatch);await finish(t);equal(vm.register(0),0n);vm.dispose();
}

// Returned-entry and output faults retain the callback result, including invalid-range repair.
for(const failure of ['entry','range','output']){
 const t=await machine(64),{vm}=t,entry=0x28000n,output=0x24fffn;
 vm.write(entry,[...encode(failure==='range'?0x200n:0x100n,4),...encode(0x120n,4),...encode(0x400n,4)]);
 const a=asm(t);a.inc(0x20250n);a.emit(0x48,0xb8);a.value(entry,8);a.emit(0xc3);vm.write(0x30000n,a.bytes);equal(call(t,'RtlInstallFunctionTableCallback',[0x28003n,0x40000n,0x1000n,0x30000n,0n,0n]),1n);
 const work=vm.work,retired=vm.retired;call(t,'RtlLookupFunctionEntry',[0x40110n,output,0n]);for(let n=0;vm.ip!==vm.entryPoints.callback;n++){assert.ok(n<20);equal((await vm.run(1n)).reason,Exit.Budget);}
 if(failure!=='range')equal(core.wire_protect(t.handle,failure==='entry'?entry:0x25000n,4096n,1),0);
 const saved=snapshot(t);if(failure==='range')await assert.rejects(()=>vm.run(10n),/invalid range/);else equal((await vm.run(10n)).reason,Exit.Fault);equal(snapshot(t),saved);equal(vm.readInt(0x20250n),1n);
 if(failure==='range')vm.write(entry,encode(0x100n,4));else equal(core.wire_protect(t.handle,failure==='entry'?entry:0x25000n,4096n,4),0);
 await finish(t);equal(vm.register(0),entry);equal(vm.readInt(output,8),0x40000n);equal(vm.readInt(0x20250n),1n);equal(vm.work-work,vm.retired-retired+1n);vm.dispose();
}

// Arrays and callbacks share one registration capacity; deletion releases either kind.
{
 const t=await machine(64),{vm}=t,ids=[];
 for(let n=0;n<128;n++){const entry=0x28000n+BigInt(n*16);if(n&1){const id=entry|3n;equal(call(t,'RtlInstallFunctionTableCallback',[id,0x40000n,0x100n,0x30000n,0n,0n]),1n);ids.push(id);}else{vm.write(entry,[...encode(0n,4),...encode(0x100n,4),...encode(0x400n,4)]);equal(call(t,'RtlAddFunctionTable',[entry,1n,0x40000n]),1n);ids.push(entry);}}
 equal(call(t,'RtlInstallFunctionTableCallback',[0x29003n,0x40000n,0x100n,0x30000n,0n,0n]),0n);equal(call(t,'RtlDeleteFunctionTable',[ids[0]]),1n);equal(call(t,'RtlInstallFunctionTableCallback',[0x29003n,0x40000n,0x100n,0x30000n,0n,0n]),1n);for(const id of [...ids.slice(1),0x29003n])equal(call(t,'RtlDeleteFunctionTable',[id]),1n);vm.dispose();
}

// Lookup callbacks can block, enter a loader frame and abandon their continuation at thread exit.
{
 const t=await machine(64),{vm}=t,entry=0x28000n;vm.write(entry,[...encode(0n,4),...encode(0x100n,4),...encode(0x400n,4)]);const input=call(t,'GetStdHandle',[0xfffffff6n]);
 vm.supplyModule('leaf.dll',dynamicFixture(64,{id:1,seed:73,tls:true}).bytes);vm.write(0x20000n,new TextEncoder().encode('leaf.dll\0'));
 const a=asm(t);a.api('ReadFile',[input,0x20240n,1n,0x20244n,0n]);a.api('LoadLibraryA',[0x20000n]);a.store(0x20248n);a.inc(0x20250n);a.emit(0x48,0xb8);a.value(entry,8);a.emit(0xc3);vm.write(0x30000n,a.bytes);equal(call(t,'RtlInstallFunctionTableCallback',[0x28003n,0x40000n,0x100n,0x30000n,0n,0n]),1n);
 call(t,'RtlLookupFunctionEntry',[0x40010n,0x20220n,0n]);equal((await vm.run(1000n)).reason,Exit.Waiting);equal(vm.readInt(0x20250n),0n);vm.writeInput([73]);await finish(t,1n);equal(vm.register(0),entry);equal(vm.readInt(0x20240n,1),73n);equal(vm.readInt(0x20250n),1n);equal(vm.moduleBase('leaf.dll'),vm.readInt(0x20248n,8));equal(vm.readInt(trace+16n),1n);
 const b=asm(t);b.api('ExitThread',[83n]);b.inc(0x20250n);b.finish(0);const worker=asm(t,0x31000n);worker.api('RtlLookupFunctionEntry',[0x40010n,0x20220n,0n]);worker.inc(0x20250n);worker.finish(1);const thread=call(t,'CreateThread',[0n,65536n,0x31000n,0n,0n,0n]);frame(t,'WaitForSingleObject',[thread,0xffffffffn]);await finish(t);equal(call(t,'GetExitCodeThread',[thread,0x20260n]),1n);equal(vm.readInt(0x20260n),83n);equal(vm.readInt(0x20250n),1n);vm.dispose();
}

// Both language search and target planning retain resolved callbacks across metadata faults.
{
 const t=await machine(64),{vm}=t,entries=[0x28000n,0x2800cn],metadata=[0x29000n,0x2a000n],counts=0x20280n,value=0xfedcba9876543210n;
 for(let n=0;n<2;n++){vm.write(entries[n],[...encode(0x32000n+BigInt(n)*0x1000n,4),...encode(0x32100n+BigInt(n)*0x1000n,4),...encode(metadata[n],4)]);vm.write(metadata[n],[25,4,1,0,4,0x42,0,0,...encode(0x30000n+BigInt(n)*0x200n,4)]);}
 for(let n=0;n<2;n++){
  const h=asm(t,0x30000n+BigInt(n)*0x200n);if(n)h.emit(0x48,0x83,0xec,56);h.emit(0xf6,0x41,4,2,0x75,0);const branch=h.bytes.length-1;h.inc(counts+4n+BigInt(n)*4n);
  if(n){h.emit(0x4c,0x89,0x44,0x24,32,0x48,0xc7,0x44,0x24,40,0,0,0,0,0x49,0x89,0xc8,0x48,0x89,0xd1,0x48,0xba);h.value(0x33080n,8);h.emit(0x49,0xb9);h.value(value,8);h.emit(0x48,0xb8);h.value(vm.bind('kernel32.dll','RtlUnwindEx'),8);h.emit(0xff,0xd0,0x48,0x83,0xc4,56);}
  h.emit(0xb8,1,0,0,0,0xc3);h.bytes[branch]=h.bytes.length-branch-1;h.inc(counts+12n);if(n)h.emit(0x48,0x83,0xc4,56);h.finish(1);
 }
 vm.write(0x27000n,[...encode(0x30200n,4),...encode(0x30300n,4),...encode(0x2b000n,4)]);vm.write(0x2b000n,[1,4,1,0,4,0x62,0,0]);equal(call(t,'RtlAddFunctionTable',[0x27000n,1n,0n]),1n);
 const lookup=asm(t,0x31000n);lookup.inc(counts);lookup.emit(0x48,0x81,0xf9);lookup.value(0x33000n,4);lookup.emit(0x73,11,0x48,0xb8);lookup.value(entries[0],8);lookup.emit(0xc3,0x48,0xb8);lookup.value(entries[1],8);lookup.emit(0xc3);vm.write(0x31000n,lookup.bytes);
 equal(call(t,'RtlInstallFunctionTableCallback',[0x28003n,0n,0x40000n,0x31000n,0n,0n]),1n);
 const inner=asm(t,0x32000n);inner.emit(0x48,0x83,0xec,40,0xb9);inner.value(0xe0420042n,4);inner.emit(0x31,0xd2,0x45,0x31,0xc0,0x45,0x31,0xc9,0x48,0xb8);inner.value(vm.bind('kernel32.dll','RaiseException'),8);inner.emit(0xff,0xd0,0x48,0x83,0xc4,40,0xc3);vm.write(0x32000n,inner.bytes);
 vm.write(0x33000n,[0x48,0x83,0xec,40,0x48,0xb8,...encode(0x32000n,8),0xff,0xd0,0x48,0x83,0xc4,40,0xc3]);vm.write(0x33080n,[0x48,0x83,0xc4,40,0xc3]);vm.ip=0x33000n;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,8));const work=vm.work,retired=vm.retired;
 for(let stage=1;stage<=4;stage++){
  for(let n=0;!(vm.ip===vm.entryPoints.callback&&vm.readInt(counts)===BigInt(stage));n++){assert.ok(n<300,`lookup stage ${stage}`);equal((await vm.run(1n)).reason,Exit.Budget);}
  equal(core.wire_protect(t.handle,metadata[(stage-1)%2],4096n,0x104),0);const fault=await vm.run(1000n);equal(fault.reason,Exit.Fault);equal(fault.fault.address&~4095n,metadata[(stage-1)%2]);equal(vm.readInt(counts),BigInt(stage));equal(vm.readInt(counts+12n),0n);
 }
 await finish(t,1n);equal(vm.register(0),value);equal(vm.readInt(counts),4n);equal(vm.readInt(counts+4n),1n);equal(vm.readInt(counts+8n),1n);equal(vm.readInt(counts+12n),2n);equal(vm.work-work,vm.retired-retired+2n);vm.dispose();
}
function fixedApi(t,a,name,args=[]){for(let n=0;n<args.length;n++){if(n<4){a.emit(...[[0x48,0xb9],[0x48,0xba],[0x49,0xb8],[0x49,0xb9]][n]);a.value(args[n],8);}else{a.emit(0x48,0xb8);a.value(args[n],8);a.emit(0x48,0x89,0x44,0x24,32+(n-4)*8);}}a.emit(0x48,0xb8);a.value(t.vm.bind('kernel32.dll',name),8);a.emit(0xff,0xd0);}

// A direct unwind captures its caller and treats ContextRecord as output storage.
async function directUnwind({provided=true,target,context=0x26000n,callback=false,history=0n,name='RtlUnwindEx',dll='kernel32.dll',stackOffset}={}){
 const t=await machine(64),{vm}=t,record=0x25000n,value=0xfedcba9876543210n,counts=0x20280n;if(stackOffset!==undefined)t.sp=(t.image.sp&~4095n)+stackOffset;
 const original=new Uint8Array(152);original.set(encode(0xe0420042n,4));original.set(encode(1n,4),4);original.set(encode(0x1122334455667788n,8),8);original.set(encode(0x123456789abcn,8),16);original.set(encode(2n,4),24);original.set(encode(0xaabbccddeeff0011n,8),32);original.set(encode(0x9988776655443322n,8),40);vm.write(record,original);vm.write(context,new Uint8Array(1232).fill(255));
 for(let n=0;n<2;n++){
  const entry=0x28000n+BigInt(n)*12n;vm.write(entry,[...encode(0x32000n+BigInt(n)*0x1000n,4),...encode(0x32100n+BigInt(n)*0x1000n,4),...encode(0x29000n+BigInt(n)*0x1000n,4)]);vm.write(0x29000n+BigInt(n)*0x1000n,[17,4,1,0,4,n?0x42:0x62,0,0,...encode(0x30000n+BigInt(n)*0x200n,4)]);
  const h=asm(t,0x30000n+BigInt(n)*0x200n);h.emit(0x48,0x89,0xc8);h.store(0x20200n);h.emit(0x4c,0x89,0xc0);h.store(0x20208n);h.emit(0x49,0x8b,0x41,64);h.store(counts+32n+BigInt(n)*8n);h.emit(0x8b,0x41,4);h.store(counts+4n+BigInt(n)*4n,4);h.emit(0x49,0xbb);h.value(counts,8);h.emit(0x41,0x8b,0x03,0x6b,0xc0,16,0x83,0xc8,n+1,0x41,0x89,0x03);h.api('Sleep',[0n]);h.finish(1);
 }
 if(callback){const a=asm(t,0x31000n);a.inc(counts+16n);a.emit(0x48,0x81,0xf9);a.value(0x33000n,4);a.emit(0x73,11,0x48,0xb8);a.value(0x28000n,8);a.emit(0xc3,0x48,0xb8);a.value(0x2800cn,8);a.emit(0xc3);vm.write(0x31000n,a.bytes);equal(call(t,'RtlInstallFunctionTableCallback',[0x28003n,0n,0x40000n,0x31000n,0n,0n]),1n);}else equal(call(t,'RtlAddFunctionTable',[0x28000n,2n,0n]),1n);
 const inner=asm(t,0x32000n);inner.emit(0x48,0x83,0xec,56);const args=[target??t.sp-40n,0x33080n,provided?record:0n,value,...(name==='RtlUnwindEx'?[context,history]:[])];for(let n=0;n<args.length;n++){if(n<4){inner.emit(...[[0x48,0xb9],[0x48,0xba],[0x49,0xb8],[0x49,0xb9]][n]);inner.value(args[n],8);}else{inner.emit(0x48,0xb8);inner.value(args[n],8);inner.emit(0x48,0x89,0x44,0x24,32+(n-4)*8);}}inner.emit(0x48,0xb8);inner.value(vm.bind(dll,name),8);inner.emit(0xff,0xd0);const after=0x32000n+BigInt(inner.bytes.length);inner.inc(counts+12n);inner.emit(0x48,0x83,0xc4,56,0xc3);vm.write(0x32000n,inner.bytes);
 vm.write(0x33000n,[0x48,0x83,0xec,40,0x48,0xb8,...encode(0x32000n,8),0xff,0xd0,0x48,0x83,0xc4,40,0xc3]);vm.write(0x33080n,[0x48,0x83,0xc4,40,0xc3]);vm.ip=0x33000n;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,8));return {...t,record,context,counts,value,original,after};
}
for(const provided of [false,true]){
 const t=await directUnwind({provided,context:0x26ff0n,callback:provided,history:provided?1n:0n}),{vm}=t,work=vm.work,retired=vm.retired;
 for(let n=0;!(vm.ip===vm.entryPoints.callback&&vm.readInt(t.counts)===0x12n);n++){assert.ok(n<300);equal((await vm.run(1n)).reason,Exit.Budget);}
 equal(core.wire_protect(t.handle,0x27000n,4096n,0x104),0);const before=snapshot(t);equal((await vm.run(1000n)).reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(t.counts),0x12n);await finish(t,1n);
 equal(vm.register(0),t.value);equal(vm.register(4),t.sp+8n);equal(vm.readInt(t.counts),0x12n);equal(vm.readInt(t.counts+32n,8),provided?1n:0n);equal(vm.readInt(t.counts+40n,8),provided?1n:0n);equal(vm.readInt(t.counts+4n),provided?3n:2n);equal(vm.readInt(t.counts+8n),provided?35n:34n);equal(vm.readInt(t.counts+12n),0n);equal(vm.readInt(t.counts+16n),provided?2n:0n);equal(vm.readInt(0x20208n,8),t.context);equal(vm.readInt(t.context+248n,8),0x33080n);equal(vm.readInt(t.context+152n,8),t.sp-40n);equal(vm.readInt(t.context+120n,8),t.value);equal(vm.work-work,vm.retired-retired+3n);
 if(provided){equal(vm.readInt(0x20200n,8),t.record);t.original.set(encode(35n,4),4);equal(Uint8Array.from({length:152},(_,n)=>Number(vm.readInt(t.record+BigInt(n),1))),t.original);}else{const p=vm.readInt(0x20200n,8);equal(vm.readInt(p),0xc0000027n);equal(vm.readInt(p+16n,8),t.after);equal(vm.readInt(p+24n),0n);}
 vm.dispose();
}

// Four-argument Win64 unwind shares the metadata walk and owns its CONTEXT on the callback stack.
for(const provided of [false,true])for(const dll of ['kernel32.dll','kernelbase.dll','ntdll.dll']){
 const t=await directUnwind({provided,callback:provided,name:'RtlUnwind',dll}),{vm}=t,work=vm.work,retired=vm.retired;vm.write(t.error,encode(77n,4));await finish(t,1n);
 equal(vm.register(0),t.value);equal(vm.register(4),t.sp+8n);equal(vm.readInt(t.counts),0x12n);equal(vm.readInt(t.counts+12n),0n);equal(vm.readInt(t.counts+16n),provided?2n:0n);equal(vm.readInt(t.counts+4n),provided?3n:2n);equal(vm.readInt(t.counts+8n),provided?35n:34n);
 const context=vm.readInt(0x20208n,8);assert.ok(context<t.sp);equal(context&15n,0n);equal(vm.readInt(context+248n,8),0x33080n);equal(vm.readInt(context+152n,8),t.sp-40n);equal(vm.readInt(context+120n,8),t.value);equal(vm.readInt(t.error),77n);equal(vm.work-work,vm.retired-retired+3n);
 if(provided){equal(vm.readInt(0x20200n,8),t.record);t.original.set(encode(35n,4),4);equal(Uint8Array.from({length:152},(_,n)=>Number(vm.readInt(t.record+BigInt(n),1))),t.original);}else{const p=vm.readInt(0x20200n,8);equal(vm.readInt(p),0xc0000027n);equal(vm.readInt(p+16n,8),t.after);}
 vm.dispose();
}
// The internal storage is preflighted before flags change; final faults do not replay cleanup.
for(const stage of ['initial','final']){
 const t=await directUnwind({name:'RtlUnwind',stackOffset:1024n}),{vm}=t;
 if(stage==='initial'){
  for(let n=0;vm.ip!==vm.bind('kernel32.dll','RtlUnwind');n++){assert.ok(n<50);equal((await vm.run(1n)).reason,Exit.Budget);}const block=(vm.register(4)&~15n)-1616n,page=block&~4095n,before=snapshot(t);equal(core.wire_protect(t.handle,page,4096n,0x104),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(t.record+4n),1n);equal(vm.readInt(t.counts),0n);equal(vm.service().reason,Exit.Dispatch);
 }else{
  for(let n=0;!(vm.ip===vm.entryPoints.callback&&vm.readInt(t.counts)===0x12n);n++){assert.ok(n<300);equal((await vm.run(1n)).reason,Exit.Budget);}const context=vm.readInt(0x20208n,8),page=context&~4095n;equal(core.wire_protect(t.handle,page,4096n,0x104),0);const before=snapshot(t);equal((await vm.run(1000n)).reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(t.counts),0x12n);
 }
 await finish(t,1n);equal(vm.register(0),t.value);equal(vm.readInt(t.counts),0x12n);vm.dispose();
}

// Output preflight leaves the original API uncharged; an unreachable target runs no cleanup.
{
 const t=await directUnwind(),{vm}=t;for(let n=0;vm.ip!==vm.bind('kernel32.dll','RtlUnwindEx');n++){assert.ok(n<50);equal((await vm.run(1n)).reason,Exit.Budget);}const before=snapshot(t),record=Uint8Array.from({length:152},(_,n)=>Number(vm.readInt(t.record+BigInt(n),1)));
 for(const page of [t.record,t.context]){equal(core.wire_protect(t.handle,page,4096n,1),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,page,4096n,4),0);equal(Uint8Array.from({length:152},(_,n)=>Number(vm.readInt(t.record+BigInt(n),1))),record);}
 equal(vm.service().reason,Exit.Dispatch);await finish(t);equal(vm.register(0),t.value);vm.dispose();
 const q=await directUnwind({target:0x12340n});await assert.rejects(()=>q.vm.run(1000n),/target frame/);equal(q.vm.readInt(q.counts),0n);q.vm.dispose();
}

// Unsupported restore protocols and invalid records remain explicit before any output is committed.
{
 const t=await machine(64),{vm}=t,record=0x25000n,context=0x26000n;
 for(const name of ['RtlUnwind','RtlUnwindEx'])for(const change of ['exit','alignment','count','longjmp','consolidation']){
  if(name==='RtlUnwind'&&change==='alignment')continue;
  vm.write(record,new Uint8Array(152));vm.write(context,new Uint8Array(1232).fill(255));const args=[t.sp,0x33000n,record,73n,context,0n];if(change==='exit')args[0]=0n;if(change==='alignment')args[4]+=8n;if(change==='count')vm.write(record+24n,encode(16n,4));if(change==='longjmp'||change==='consolidation')vm.write(record,encode(change==='longjmp'?0x80000026n:0x80000029n,4));frame(t,name,name==='RtlUnwind'?args.slice(0,4):args);const before=snapshot(t);assert.throws(()=>vm.service(),/AMD64|unwind/);equal(snapshot(t),before);equal(vm.readInt(record+4n),0n);equal(vm.readInt(context+48n),0xffffffffn);
 }
 vm.dispose();
}

// Unwind callbacks traverse their own cleanup frames before crossing saved return boundaries.
for(const kind of ['vectored','nested','lookup','lookup-search','nested-local','filter-call']){
 const t=await directUnwind(),{vm}=t,local=kind==='nested-local',nested=kind==='nested'||local;
 vm.write(0x27000n,[...encode(0x34000n,4),...encode(0x34200n,4),...encode(0x2b000n,4)]);vm.write(0x2b000n,[17,4,1,0,4,0x62,0,0,...encode(0x36000n,4)]);equal(call(t,'RtlAddFunctionTable',[0x27000n,1n,0n]),1n);
 const cleanup=asm(t,0x36000n);cleanup.emit(0x49,0xbb);cleanup.value(t.counts,8);cleanup.emit(0x41,0x8b,0x03,0x6b,0xc0,16,0x83,0xc8,3,0x41,0x89,0x03);cleanup.inc(t.counts+20n);cleanup.finish(1);
 const callback=asm(t,0x34000n);callback.emit(0x48,0x83,0xec,56);callback.inc(t.counts+24n);
 if(nested){callback.emit(0x48,0x8b,0x01,0x81,0x38);callback.value(0xe0420043n,4);callback.emit(0x74,0);const branch=callback.bytes.length-1;callback.emit(0x48,0x89,0xe0);callback.store(0x202e0n);fixedApi(t,callback,'RaiseException',[0xe0420043n,0n,0n,0n]);callback.inc(t.counts+12n);callback.emit(0x48,0x83,0xc4,56,0xb8,0xff,0xff,0xff,0xff,0xc3);callback.bytes[branch]=callback.bytes.length-branch-1;}
 if(kind==='lookup-search'){fixedApi(t,callback,'RtlDeleteFunctionTable',[0x40003n]);fixedApi(t,callback,'RtlAddFunctionTable',[0x28000n,2n,0n]);}
 fixedApi(t,callback,'Sleep',[0n]);
 if(local){callback.emit(0x48,0xb8);callback.value(0x202e0n,8);callback.emit(0x48,0x8b,0x08,0x48,0xba);callback.value(0x341c0n,8);callback.emit(0x49,0xb8);callback.value(t.record,8);callback.emit(0x49,0xb9);callback.value(t.value,8);callback.emit(0x48,0xb8);callback.value(t.context,8);callback.emit(0x48,0x89,0x44,0x24,32,0x48,0xc7,0x44,0x24,40,0,0,0,0,0x48,0xb8);callback.value(vm.bind('kernel32.dll','RtlUnwindEx'),8);callback.emit(0xff,0xd0);}else fixedApi(t,callback,kind==='filter-call'?'RtlUnwind':'RtlUnwindEx',kind==='filter-call'?[t.sp-40n,0x33080n,t.record,t.value]:[t.sp-40n,0x33080n,t.record,t.value,t.context,0n]);
 callback.inc(t.counts+12n);callback.emit(0x48,0x83,0xc4,56,0xc3);assert.ok(callback.bytes.length<448);vm.write(0x34000n,callback.bytes);vm.write(0x341c0n,[0x48,0x83,0xc4,56,0xb8,0xff,0xff,0xff,0xff,0xc3]);
 if(kind.startsWith('lookup')){if(kind==='lookup-search'){equal(call(t,'RtlDeleteFunctionTable',[0x28000n]),1n);equal(call(t,'RtlAddFunctionTable',[0x2800cn,1n,0n]),1n);}equal(call(t,'RtlInstallFunctionTableCallback',[0x40003n,kind==='lookup'?0x40000n:0x32000n,0x100n,0x34000n,0n,0n]),1n);}else if(kind==='filter-call')call(t,'SetUnhandledExceptionFilter',[0x34000n]);else add(t,0x34000n);
 const source=asm(t,0x32000n);source.api(kind==='lookup'?'RtlLookupFunctionEntry':kind==='filter-call'?'UnhandledExceptionFilter':'RaiseException',kind==='lookup'?[0x40010n,0x202f0n,0n]:kind==='filter-call'?[t.record]:[0xe0420042n,0n,0n,0n]);source.inc(t.counts+12n);source.finish(73);vm.write(0x202f0n,encode(0x1122334455667788n,8));vm.ip=0x33000n;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,8));
 const work=vm.work,retired=vm.retired;
 if(!local){for(let n=0;!(vm.ip===vm.entryPoints.callback&&vm.readInt(t.counts+20n)===BigInt(nested?2:1)&&vm.readInt(t.counts)===BigInt(nested?0x3312:0x312));n++){assert.ok(n<500);equal((await vm.run(1n)).reason,Exit.Budget);}const context=kind==='filter-call'?vm.readInt(0x20208n,8)&~4095n:t.context;equal(core.wire_protect(t.handle,context,4096n,0x104),0);equal((await vm.run(1000n)).reason,Exit.Fault);}
 await finish(t,1n);equal(vm.register(0),local?73n:t.value);equal(vm.readInt(t.counts),BigInt(local?0x33:nested?0x3312:0x312));equal(vm.readInt(t.counts+20n),BigInt(nested?2:1));equal(vm.readInt(t.counts+24n),BigInt(nested?2:1));equal(vm.readInt(t.counts+12n),local?1n:0n);equal(vm.readInt(0x202f0n,8),0x1122334455667788n);equal(vm.work-work,vm.retired-retired+BigInt(local?4:nested?6:kind==='lookup-search'?7:5));
 const handler=asm(t,0x38000n);handler.context();handler.field(120,91,8);handler.finish();const token=add(t,0x38000n,1n);raise(t);await finish(t);equal(vm.register(0),91n);equal(call(t,'RemoveVectoredExceptionHandler',[token]),1n);vm.dispose();
}
// A callback from an active cleanup cannot be abandoned using an ordinary target unwind.
{
 const t=await directUnwind(),{vm}=t;vm.write(0x27000n,[...encode(0x30000n,4),...encode(0x30100n,4),...encode(0x2b000n,4)]);vm.write(0x2b000n,[1,4,1,0,4,0x62,0,0]);equal(call(t,'RtlAddFunctionTable',[0x27000n,1n,0n]),1n);const h=asm(t);h.emit(0x48,0x83,0xec,56);h.inc(t.counts+20n);fixedApi(t,h,'RtlUnwindEx',[t.sp-40n,0x33080n,t.record,t.value,t.context,0n]);h.inc(t.counts+12n);h.emit(0x48,0x83,0xc4,56,0xc3);vm.write(0x30000n,h.bytes);vm.ip=0x33000n;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,8));await assert.rejects(()=>vm.run(1000n),/collided unwind/);equal(vm.readInt(t.counts+20n),1n);equal(vm.readInt(t.counts),0n);equal(vm.readInt(t.counts+12n),0n);vm.dispose();
}

// Loader frames remain owned by the loader when a target would cross a DLL initializer.
{
 const t=await directUnwind(),{vm}=t;vm.write(0x27000n,[...encode(0x34000n,4),...encode(0x34100n,4),...encode(0x2b000n,4)]);vm.write(0x2b000n,[1,4,1,0,4,0x62,0,0]);equal(call(t,'RtlAddFunctionTable',[0x27000n,1n,0n]),1n);const h=asm(t,0x34000n);h.emit(0x48,0x83,0xec,56);fixedApi(t,h,'RtlUnwindEx',[t.sp-40n,0x33080n,t.record,t.value,t.context,0n]);h.inc(t.counts+12n);h.emit(0x48,0x83,0xc4,56,0xc3);vm.write(0x34000n,h.bytes);
 const f=dynamicFixture(64,{entry:false}),init=asm(t,0x37000n);init.emit(0x48,0x83,0xec,40,0x48,0xb8);init.value(0x34000n,8);init.emit(0xff,0xd0,0x48,0x83,0xc4,40,0xb8,1,0,0,0,0xc3);f.put(f.opt+16,0x1000);f.bytes.fill(0x90,f.offset(0x1000),f.offset(0x1100));f.bytes.set(init.bytes,f.offset(0x1000));f.relocations.splice(0,f.relocations.length,...f.relocations.filter(r=>r>=0x1100));f.rebuildRelocations();f.put(f.dirs+3*8,0x2480);f.put(f.dirs+3*8+4,12);f.rput(0x2480,0x1000);f.rput(0x2484,0x1100);f.rput(0x2488,0x24a0);f.bytes.set([1,4,1,0,4,0x42,0,0],f.offset(0x24a0));vm.supplyModule('unwinding.dll',f.bytes);vm.write(0x20000n,new TextEncoder().encode('unwinding.dll\0'));
 const source=asm(t,0x32000n);source.api('LoadLibraryA',[0x20000n]);source.inc(t.counts+12n);source.finish(73);vm.ip=0x33000n;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,8));await assert.rejects(()=>vm.run(1000n),/target frame/);equal(vm.readInt(t.counts),0n);equal(vm.readInt(t.counts+12n),0n);assert.ok(vm.moduleBase('unwinding.dll'));vm.dispose();
}
console.log(`${checks} exception/callback/CONTEXT/ordering/retry/thread/loader checks passed`);
