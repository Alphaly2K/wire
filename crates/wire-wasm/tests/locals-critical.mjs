import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,window} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';
const api=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
async function finish(t,budget=1000n){for(let n=0;n<10000;n++){const r=await t.vm.run(budget);if(r.reason===Exit.Returned||r.reason===Exit.ThreadReturned)return r;equal(r.reason,Exit.Budget);}throw new Error('local callback did not finish');}
function callback(t,at=0x30000n){
 const {vm,mode}=t,p=0x23000n;
 vm.write(at,mode===32?[0x8b,0x44,0x24,4,0xa3,...encode(p,4),0xff,0x05,...encode(p+8n,4),0xc2,4,0]:[0x49,0xbb,...encode(p,8),0x49,0x89,0x0b,0x41,0xff,0x43,8,0xc3]);return at;
}

// Slot allocation is bounded and reusable, with exact pointer/error conventions.
for(const mode of [32,64])for(const prefix of ['Tls','Fls']){
 const t=await machine(mode),{vm}=t,capacity=prefix==='Tls'?64:128,indices=[];
 vm.write(t.error,encode(123n,4));vm.flags=0x8d5n;vm.xmm(3,new Uint8Array(16).fill(0xa5));const fpu=fp(t);
 for(let n=0;n<capacity;n++){const index=api(t,prefix+'Alloc',prefix==='Fls'?[0n]:[]);equal(index,BigInt(n));indices.push(index);}
 equal(vm.readInt(t.error),123n);equal(api(t,prefix+'Alloc',prefix==='Fls'?[0n]:[]),0xffffffffn);equal(vm.readInt(t.error),8n);
 const value=mode===64?0xfedcba9876543210n:0x76543210n;
 equal(api(t,prefix+'SetValue',[7n,value]),1n);vm.write(t.error,encode(123n,4));equal(api(t,prefix+'GetValue',[7n]),value);equal(vm.readInt(t.error),prefix==='Tls'?0n:123n);
 equal(api(t,prefix+'Free',[7n]),1n);equal(api(t,prefix+'GetValue',[7n]),0n);equal(vm.readInt(t.error),87n);equal(api(t,prefix+'Free',[7n]),0n);equal(api(t,prefix+'Alloc',prefix==='Fls'?[0n]:[]),7n);vm.write(t.error,encode(123n,4));equal(api(t,prefix+'GetValue',[7n]),0n);equal(vm.readInt(t.error),prefix==='Tls'?0n:123n);
 for(const index of indices)equal(api(t,prefix+'Free',[index]),1n);
 for(const name of ['GetValue','SetValue','Free']){equal(api(t,prefix+name,[0x10000n,value]),0n);equal(vm.readInt(t.error),87n);}
 if(mode===64){equal(api(t,prefix+'Alloc',prefix==='Fls'?[0n]:[]),0n);equal(api(t,prefix+'SetValue',[0xabcdef0000000000n,value]),1n);equal(api(t,prefix+'GetValue',[0x1234567800000000n]),value);equal(api(t,prefix+'Free',[0n]),1n);}
 equal(vm.flags,0x8d5n);equal(vm.xmm(3),new Uint8Array(16).fill(0xa5));equal(fp(t),fpu);equal(t.time.calls,0);vm.dispose();
}

// Slot output and error faults leave allocation markers and saved returns intact.
for(const mode of [32,64])for(const prefix of ['Tls','Fls']){
 const t=await machine(mode),{vm}=t,offset=prefix==='Fls'?0x1800:mode===32?0xe10:0x1480,page=(t.image.teb+BigInt(offset))&~4095n;
 frame(t,prefix+'Alloc',prefix==='Fls'?[0n]:[],'kernel32.dll');equal(core.wire_protect(t.handle,page,4096n,2),0);let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0n);
 frame(t,prefix+'SetValue',[0n,123n],'kernel32.dll');equal(core.wire_protect(t.handle,page,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(api(t,prefix+'GetValue',[0n]),123n);
 frame(t,prefix+'Free',[0n],'kernel32.dll');equal(core.wire_protect(t.handle,page,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(api(t,prefix+'Alloc',prefix==='Fls'?[0n]:[]),0n);
 frame(t,prefix+'GetValue',[0x10000n],'kernel32.dll');equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(t.error),87n);
 if(prefix==='Tls'){equal(api(t,'TlsSetValue',[0n,456n]),1n);frame(t,'TlsGetValue',[0n],'kernel32.dll');equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),456n);}
 frame(t,prefix+'Free',[0n],'kernel32.dll');const stack=t.sp&~4095n;equal(core.wire_protect(t.handle,stack,4096n,1),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,stack,4096n,4),0);equal(api(t,prefix+'SetValue',[0n,789n]),1n);equal(api(t,prefix+'GetValue',[0n]),789n);equal(api(t,prefix+'Free',[0n]),1n);equal(t.time.calls,0);vm.dispose();
}

// FlsFree executes the guest destructor once and restores its original caller.
for(const mode of [32,64])for(const budget of [1n,100n]){
 const t=await machine(mode),{vm}=t,target=callback(t),slot=api(t,'FlsAlloc',[target]),value=mode===64?0xfedcba9876543210n:0x76543210n;equal(api(t,'FlsSetValue',[slot,value]),1n);vm.flags=0x8d5n;const fpu=fp(t);
 frame(t,'FlsFree',[slot],'kernel32.dll');const work=vm.work,retired=vm.retired;equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,target);equal(vm.work,work+1n);equal(vm.retired,retired);vm.write(t.sp,encode(0n,mode/8));equal((await finish(t,budget)).reason,Exit.Returned);equal(vm.register(0),1n);equal(vm.readInt(0x23000n,mode/8),value);equal(vm.readInt(0x23008n),1n);equal(vm.flags,0x8d5n);equal(fp(t),fpu);equal(vm.work-work,vm.retired-retired+1n);equal(api(t,'FlsGetValue',[slot]),0n);equal(vm.readInt(t.error),87n);equal(t.time.calls,0);vm.dispose();
}

// An inaccessible first destructor frame cannot clear the slot or retire its index.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,target=callback(t),slot=api(t,'FlsAlloc',[target]);equal(api(t,'FlsSetValue',[slot,123n]),1n);t.sp=mode===32?0x25000n:0x25008n;frame(t,'FlsFree',[slot],'kernel32.dll');equal(core.wire_protect(t.handle,0x24000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(0x23008n),0n);equal(core.wire_protect(t.handle,0x24000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal((await finish(t,1n)).reason,Exit.Returned);equal(vm.register(0),1n);equal(vm.readInt(0x23008n),1n);equal(vm.readInt(0x23000n,mode/8),123n);vm.dispose();
}

// Failure to prepare a later destructor never repeats an earlier completed call.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,target=callback(t),first=api(t,'FlsAlloc',[target]),second=api(t,'FlsAlloc',[target]);equal(api(t,'FlsSetValue',[first,111n]),1n);equal(api(t,'FlsSetValue',[second,222n]),1n);frame(t,'ExitThread',[37n],'kernel32.dll');equal(vm.service().reason,Exit.Dispatch);
 for(let n=0;n<1000;n++){if(vm.ip===vm.entryPoints.callback&&vm.readInt(0x23008n)===1n)break;equal((await vm.run(1n)).reason,Exit.Budget);assert.ok(n<999);}
 const page=(t.sp-64n)&~4095n;equal(core.wire_protect(t.handle,page,4096n,2),0);const before=snapshot(t);equal((await vm.run(1n)).reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(0x23008n),1n);equal(core.wire_protect(t.handle,page,4096n,4),0);equal((await finish(t,1n)).reason,Exit.ThreadReturned);equal(vm.register(0),37n);equal(vm.readInt(0x23008n),2n);equal(vm.readInt(0x23000n,mode/8),222n);vm.dispose();
}

// Critical sections use the ABI layout, recursive ownership and narrow field writes.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,p=mode/8,cs=0x22000n,length=mode===32?24:40,spin=p*3+8;
 vm.write(t.error,encode(123n,4));vm.write(cs-1n,new Uint8Array(length+2).fill(0xa5));equal(api(t,'InitializeCriticalSectionAndSpinCount',[cs,1234n]),1n);equal(vm.readInt(cs-1n,1),0xa5n);equal(vm.readInt(cs+BigInt(length),1),0xa5n);equal(vm.readInt(cs+BigInt(p)),0xffffffffn);equal(vm.readInt(cs+BigInt(spin),p),1234n);equal(api(t,'SetCriticalSectionSpinCount',[cs,5678n]),1234n);equal(vm.readInt(cs+BigInt(spin),p),5678n);
 equal(api(t,'EnterCriticalSection',[cs]),0n);equal(api(t,'TryEnterCriticalSection',[cs]),1n);equal(vm.readInt(cs+BigInt(p+4)),2n);equal(vm.readInt(cs+BigInt(p+8),p),1n);equal(api(t,'LeaveCriticalSection',[cs]),0n);equal(vm.readInt(cs+BigInt(p+4)),1n);equal(api(t,'LeaveCriticalSection',[cs]),0n);equal(vm.readInt(cs+BigInt(p+4)),0n);equal(vm.readInt(cs+BigInt(p+8),p),0n);equal(vm.readInt(cs+BigInt(p)),0xffffffffn);equal(api(t,'DeleteCriticalSection',[cs]),0n);equal(vm.readInt(t.error),123n);
 // Spin is on a separate read-only page; Enter/Leave need only the ownership fields.
 const split=0x25000n-BigInt(spin);equal(api(t,'InitializeCriticalSectionAndSpinCount',[split,1234n]),1n);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);equal(api(t,'EnterCriticalSection',[split]),0n);equal(api(t,'LeaveCriticalSection',[split]),0n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);
 equal(api(t,'InitializeCriticalSectionEx',[cs,0n,0x02000000n]),0n);equal(vm.readInt(t.error),87n);vm.write(cs+BigInt(p+4),encode(1n,4));vm.write(cs+BigInt(p+8),encode(2n,p));equal(core.wire_protect(t.handle,cs,4096n,2),0);equal(api(t,'TryEnterCriticalSection',[cs]),0n);equal(core.wire_protect(t.handle,cs,4096n,4),0);
 for(const name of ['EnterCriticalSection','SetCriticalSectionSpinCount']){frame(t,name,name.startsWith('Set')?[cs,3n]:[cs],'kernel32.dll');equal(core.wire_protect(t.handle,cs,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,cs,4096n,4),0);}
 const crossing=0x24ff0n;vm.write(crossing,new Uint8Array(length).fill(0xa5));frame(t,'InitializeCriticalSectionAndSpinCount',[crossing,7n],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(vm,crossing,16),new Uint8Array(16).fill(0xa5));equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),1n);
 // A contended call owns its original arguments and return even after stack writes.
 frame(t,'EnterCriticalSection',[cs],'kernel32.dll');before=snapshot(t);equal(vm.service().reason,Exit.Waiting);equal(snapshot(t),before);vm.write(t.sp,new Uint8Array(mode===32?8:16));vm.write(cs+BigInt(p),[...encode(0xffffffffn,4),...encode(0n,4),...encode(0n,p)]);equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(cs+BigInt(p+8),p),1n);equal(api(t,'LeaveCriticalSection',[cs]),0n);
 equal(api(t,'InitializeCriticalSection',[t.sp]),0n);equal(vm.ip,vm.entryPoints.returned);
 if(mode===64){equal(api(t,'InitializeCriticalSectionAndSpinCount',[cs,0xabcdef0012345678n]),1n);equal(vm.readInt(cs+BigInt(spin),8),0x12345678n);equal(api(t,'SetCriticalSectionSpinCount',[cs,0xdeadbeef76543210n]),0x12345678n);equal(vm.readInt(cs+BigInt(spin),8),0x76543210n);}
 equal(t.time.calls,0);vm.dispose();
}

// A handler's service cannot discard an interrupted lock or message wait.
for(const mode of [32,64])for(const kind of ['critical','message']){
 const t=await machine(mode),{vm}=t,p=mode/8,cs=0x22000n,out=0x24000n,hwnd=kind==='message'?await window(t):0n,sleep=vm.bind('kernel32.dll','Sleep');
 vm.write(0x40000n,mode===32?[0x6a,0,0xb8,...encode(sleep,4),0xff,0xd0,0xb8,0xff,0xff,0xff,0xff,0xc2,4,0]:[0x48,0x83,0xec,40,0x31,0xc9,0x48,0xb8,...encode(sleep,8),0xff,0xd0,0x48,0x83,0xc4,40,0xb8,0xff,0xff,0xff,0xff,0xc3]);
 assert.ok(api(t,'AddVectoredExceptionHandler',[1n,0x40000n]));
 if(kind==='critical'){api(t,'InitializeCriticalSection',[cs]);vm.write(cs+BigInt(p+4),encode(1n,4));vm.write(cs+BigInt(p+8),encode(2n,p));frame(t,'EnterCriticalSection',[cs],'kernel32.dll');}
 else frame(t,'GetMessageW',[out,0n,0x100n,0x100n],'user32.dll');equal(vm.service().reason,Exit.Waiting);
 vm.write(t.sp,new Uint8Array(mode===32?8:16));
 if(kind==='critical')vm.write(cs+BigInt(p),[...encode(0xffffffffn,4),...encode(0n,4),...encode(0n,p)]);
 else vm.queueWindowEvents([{window:hwnd,message:0x100,wparam:65n,lparam:0x1e0001n,time:57}]);
 equal(core.wire_protect(t.handle,kind==='critical'?cs:out,4096n,0x104),0);equal(vm.service().reason,Exit.Fault);equal(core.wire_deliver_exception(t.handle,Exit.Fault),1);equal((await vm.run(1000n)).reason,Exit.Returned);
 if(kind==='critical')equal(vm.readInt(cs+BigInt(p+8),p),1n);else {equal(vm.readInt(out+BigInt(p)),0x100n);equal(vm.readInt(out+BigInt(mode===32?8:16),p),65n);}vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 for(const name of ['TlsAlloc','TlsGetValue','TlsSetValue','TlsFree','FlsAlloc','FlsGetValue','FlsSetValue','FlsFree','InitializeCriticalSection','InitializeCriticalSectionAndSpinCount','InitializeCriticalSectionEx','EnterCriticalSection','TryEnterCriticalSection','LeaveCriticalSection','DeleteCriticalSection','SetCriticalSectionSpinCount']){const address=vm.bind('kernel32.dll',name);if(mode===64)equal(vm.bind('kernelbase.dll',name),address);else assert.throws(()=>vm.bind('kernelbase.dll',name),/unknown/);assert.throws(()=>vm.bind('user32.dll',name),/unknown/);}
 vm.dispose();
}
console.log(`${checks} TLS/FLS/destructor/critical-section ABI/ownership/fault/continuation checks passed`);
