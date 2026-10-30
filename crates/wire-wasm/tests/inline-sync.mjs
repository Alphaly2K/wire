import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';
const api=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));

// SRW ownership is committed only after its complete guest token is writable.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,p=mode/8,lock=0x22000n;
 vm.write(lock-1n,new Uint8Array(p+2).fill(0xa5));vm.write(t.error,encode(123n,4));vm.flags=0x8d5n;vm.xmm(2,new Uint8Array(16).fill(0xa5));const fpu=fp(t);
 equal(api(t,'InitializeSRWLock',[lock]),0n);equal(vm.readInt(lock,p),0n);equal(vm.readInt(lock-1n,1),0xa5n);equal(vm.readInt(lock+BigInt(p),1),0xa5n);
 equal(api(t,'AcquireSRWLockShared',[lock]),0n);equal(api(t,'AcquireSRWLockShared',[lock]),0n);equal(vm.readInt(lock,p),10n);equal(api(t,'TryAcquireSRWLockExclusive',[lock]),0n);equal(api(t,'TryAcquireSRWLockShared',[lock]),1n);equal(vm.readInt(lock,p),14n);
 for(let n=0;n<3;n++)equal(api(t,'ReleaseSRWLockShared',[lock]),0n);equal(vm.readInt(lock,p),0n);equal(api(t,'TryAcquireSRWLockExclusive',[lock]),1n);equal(api(t,'TryAcquireSRWLockExclusive',[lock]),0n);equal(api(t,'TryAcquireSRWLockShared',[lock]),0n);
 frame(t,'ReleaseSRWLockExclusive',[lock],'kernel32.dll');equal(core.wire_protect(t.handle,lock,4096n,2),0);let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(lock,p),1n);equal(core.wire_protect(t.handle,lock,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(lock,p),0n);
 frame(t,'AcquireSRWLockShared',[lock],'kernel32.dll');equal(core.wire_protect(t.handle,lock,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,lock,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(lock,p),6n);equal(api(t,'ReleaseSRWLockShared',[lock]),0n);
 // Split-token failure cannot leave a partly initialized object.
 const split=0x24ffen;vm.write(split,new Uint8Array(p).fill(0xa5));frame(t,'InitializeSRWLock',[split],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(vm,split,2),new Uint8Array(2).fill(0xa5));equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(split,p),0n);
 if(mode===64){const high=api(t,'VirtualAlloc',[0n,4096n,0x103000n,4n]);assert.ok(high>0xffffffffn);equal(api(t,'InitializeSRWLock',[high]),0n);equal(api(t,'AcquireSRWLockExclusive',[high]),0n);equal(api(t,'ReleaseSRWLockExclusive',[high]),0n);equal(api(t,'VirtualFree',[high,0n,0x8000n]),1n);}
 equal(vm.readInt(t.error),123n);equal(vm.flags,0x8d5n);equal(vm.xmm(2),new Uint8Array(16).fill(0xa5));equal(fp(t),fpu);equal(t.time.calls,0);vm.dispose();
}

// InitOnce preserves full contexts, ordered aliases and captured blocked calls.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,p=mode/8,once=0x22000n,out=0x23000n,ctx=0x23010n,value=mode===64?0xfedcba9876543000n:0x76543000n;
 vm.write(out,encode(99n,4));vm.write(ctx,encode(0x7777n,p));equal(api(t,'InitOnceBeginInitialize',[once,1n,out,ctx]),0n);equal(vm.readInt(t.error),31n);equal(vm.readInt(out),99n);equal(vm.readInt(ctx,p),0x7777n);
 equal(api(t,'InitOnceBeginInitialize',[once,0n,out,ctx]),1n);equal(vm.readInt(once,p),1n);equal(vm.readInt(out),1n);equal(vm.readInt(ctx,p),0n);equal(api(t,'InitOnceComplete',[once,0n,3n]),0n);equal(vm.readInt(t.error),87n);equal(vm.readInt(once,p),1n);equal(api(t,'InitOnceComplete',[once,4n,0n]),1n);equal(vm.readInt(once,p),0n);
 equal(api(t,'InitOnceBeginInitialize',[once,0n,out,ctx]),1n);frame(t,'InitOnceBeginInitialize',[once,0n,out,ctx],'kernel32.dll');const before=snapshot(t);equal(vm.service().reason,Exit.Waiting);equal(snapshot(t),before);vm.write(t.sp,new Uint8Array(mode===32?8:16));vm.write(once,encode(value|2n,p));equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);equal(vm.register(0),1n);equal(vm.readInt(out),0n);equal(vm.readInt(ctx,p),value);
 vm.write(t.error,encode(123n,4));equal(api(t,'InitOnceBeginInitialize',[once,1n,out,ctx]),1n);equal(vm.readInt(t.error),123n);equal(vm.readInt(ctx,p),value);equal(api(t,'InitOnceBeginInitialize',[0xdead0000n,2n,out,ctx]),0n);equal(vm.readInt(t.error),50n);equal(api(t,'InitOnceComplete',[0xdead0000n,2n,0n]),0n);equal(vm.readInt(t.error),50n);equal(api(t,'InitOnceComplete',[0xdead0000n,0n,3n]),0n);equal(vm.readInt(t.error),87n);
 vm.write(once,encode(0n,p));vm.write(out,encode(99n,4));vm.write(0x24ffen,[0xa5,0xa5]);frame(t,'InitOnceBeginInitialize',[once,0n,out,0x24ffen],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const fault=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),fault);equal(vm.readInt(once,p),0n);equal(vm.readInt(out),99n);equal(bytes(vm,0x24ffen,2),Uint8Array.from([0xa5,0xa5]));equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(once,p),1n);
 equal(api(t,'InitOnceComplete',[once,4n,0n]),1n);equal(api(t,'InitOnceBeginInitialize',[once,0n,t.sp,ctx]),1n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp),1n);equal(api(t,'InitOnceComplete',[once,0n,value]),1n);
 // Aliased outputs retain the public state -> pending -> context write order.
 vm.write(once,encode(0n,p));equal(api(t,'InitOnceBeginInitialize',[once,0n,out,once]),1n);equal(vm.readInt(out),1n);equal(vm.readInt(once,p),0n);
 equal(t.time.calls,0);vm.dispose();
}

// Finite waits retain their first deadline and reacquire the original lock on timeout.
for(const mode of [32,64])for(const kind of ['shared','exclusive','critical']){
 let tick=1000000n;const t=await machine(mode,{clock:()=>tick}),{vm}=t,p=mode/8,condition=0x22000n,lock=0x23000n,critical=kind==='critical',acquire=critical?'EnterCriticalSection':kind==='shared'?'AcquireSRWLockShared':'AcquireSRWLockExclusive',release=critical?'LeaveCriticalSection':kind==='shared'?'ReleaseSRWLockShared':'ReleaseSRWLockExclusive';
 api(t,critical?'InitializeCriticalSection':'InitializeSRWLock',[lock]);api(t,'InitializeConditionVariable',[condition]);api(t,acquire,[lock]);const name=critical?'SleepConditionVariableCS':'SleepConditionVariableSRW',args=critical?[condition,lock,5n]:[condition,lock,5n,kind==='shared'?1n:0n];frame(t,name,args,'kernel32.dll');const w=vm.work,r=vm.retired;equal(vm.service().reason,Exit.Waiting);equal(vm.work,w);equal(vm.retired,r);equal(t.time.calls,1);equal(vm.readInt(lock+BigInt(critical?p+8:0),p),0n);
 vm.write(t.sp,new Uint8Array(mode===32?8:16));tick+=10000n;equal(vm.service().reason,Exit.Waiting);equal(t.time.calls,2);tick+=100000n;equal(core.wire_protect(t.handle,lock,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(t.time.calls,3);equal(vm.readInt(lock+BigInt(critical?p+8:0),p),0n);equal(core.wire_protect(t.handle,lock,4096n,4),0);
 equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);const error=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),error);equal(vm.readInt(lock+BigInt(critical?p+8:0),p),0n);equal(t.time.calls,3);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);equal(vm.register(0),0n);equal(vm.readInt(t.error),1460n);equal(vm.work,w+1n);equal(vm.retired,r);equal(t.time.calls,3);
 if(critical){equal(vm.readInt(lock+BigInt(p+4)),1n);equal(vm.readInt(lock+BigInt(p+8),p),1n);}else equal(vm.readInt(lock,p),kind==='shared'?6n:1n);
 equal(api(t,release,[lock]),0n);vm.dispose();
}

// Zero timeout and invalid inputs publish no release or queue entry and read no clock.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,p=mode/8,condition=0x22000n,lock=0x23000n,cs=0x24000n;
 api(t,'InitializeSRWLock',[lock]);api(t,'InitializeConditionVariable',[condition]);api(t,'AcquireSRWLockShared',[lock]);equal(api(t,'SleepConditionVariableSRW',[condition,lock,0n,1n]),0n);equal(vm.readInt(t.error),1460n);equal(vm.readInt(lock,p),6n);equal(api(t,'SleepConditionVariableSRW',[0xdead0000n,lock,0n,2n]),0n);equal(vm.readInt(t.error),87n);equal(vm.readInt(lock,p),6n);api(t,'ReleaseSRWLockShared',[lock]);
 api(t,'InitializeCriticalSection',[cs]);api(t,'EnterCriticalSection',[cs]);equal(api(t,'SleepConditionVariableCS',[condition,cs,0n]),0n);equal(vm.readInt(t.error),1460n);equal(vm.readInt(cs+BigInt(p+4)),1n);equal(vm.readInt(cs+BigInt(p+8),p),1n);api(t,'EnterCriticalSection',[cs]);frame(t,'SleepConditionVariableCS',[condition,cs,0n],'kernel32.dll');const before=snapshot(t);assert.throws(()=>vm.service(),/one owned/);equal(snapshot(t),before);equal(vm.readInt(cs+BigInt(p+4)),2n);api(t,'LeaveCriticalSection',[cs]);api(t,'LeaveCriticalSection',[cs]);
 equal(api(t,'WakeConditionVariable',[condition]),0n);equal(api(t,'WakeAllConditionVariable',[condition]),0n);equal(t.time.calls,0);vm.dispose();
}

// A nested handler's Sleep preserves the released lock wait and its timeout result.
for(const mode of [32,64]){
 let tick=1000000n;const t=await machine(mode,{clock:()=>tick}),{vm}=t,p=mode/8,condition=0x22000n,lock=0x23000n,sleep=vm.bind('kernel32.dll','Sleep');vm.write(0x40000n,mode===32?[0x6a,0,0xb8,...encode(sleep,4),0xff,0xd0,0xb8,0xff,0xff,0xff,0xff,0xc2,4,0]:[0x48,0x83,0xec,40,0x31,0xc9,0x48,0xb8,...encode(sleep,8),0xff,0xd0,0x48,0x83,0xc4,40,0xb8,0xff,0xff,0xff,0xff,0xc3]);api(t,'AddVectoredExceptionHandler',[1n,0x40000n]);api(t,'InitializeSRWLock',[lock]);api(t,'AcquireSRWLockExclusive',[lock]);frame(t,'SleepConditionVariableSRW',[condition,lock,5n,0n],'kernel32.dll');equal(vm.service().reason,Exit.Waiting);vm.write(t.sp,new Uint8Array(mode===32?8:16));tick+=100000n;equal(core.wire_protect(t.handle,lock,4096n,0x104),0);equal(vm.service().reason,Exit.Fault);equal(core.wire_deliver_exception(t.handle,Exit.Fault),1);equal((await vm.run(1000n)).reason,Exit.Returned);equal(vm.register(0),0n);equal(vm.readInt(t.error),1460n);equal(vm.readInt(lock,p),1n);api(t,'ReleaseSRWLockExclusive',[lock]);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 for(const name of ['InitializeSRWLock','AcquireSRWLockExclusive','AcquireSRWLockShared','ReleaseSRWLockExclusive','ReleaseSRWLockShared','TryAcquireSRWLockExclusive','TryAcquireSRWLockShared','InitializeConditionVariable','SleepConditionVariableSRW','SleepConditionVariableCS','WakeConditionVariable','WakeAllConditionVariable','InitOnceBeginInitialize','InitOnceComplete']){const address=vm.bind('kernel32.dll',name);if(mode===64)equal(vm.bind('kernelbase.dll',name),address);else assert.throws(()=>vm.bind('kernelbase.dll',name),/unknown/);assert.throws(()=>vm.bind('user32.dll',name),/unknown/);}
 vm.dispose();
}
console.log(`${checks} SRW/condition/InitOnce ABI/token/deadline/reacquire/fault/continuation checks passed`);
