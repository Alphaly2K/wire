import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,finish} from './gdi-harness.mjs';
const api=(t,n,a=[])=>call(t,n,a,'kernel32.dll');
const event=(t,on=false)=>api(t,'CreateEventW',[0n,0n,BigInt(on),0n]);
const semaphore=t=>api(t,'CreateSemaphoreW',[0n,0n,3n,0n]);
const consume=(t,h,result)=>equal(api(t,'WaitForSingleObject',[h,0n]),result);
const signal=(t,a)=>api(t,'SignalObjectAndWait',a);

for(const mode of [32,64]){
 let now=0n;const t=await machine(mode,{clock:()=>now}),{vm}=t;
 const own=event(t);vm.write(t.error,encode(123n,4));equal(signal(t,[own,own,0n,0n]),0n);consume(t,own,258n);equal(vm.readInt(t.error),123n);
 const source=semaphore(t),target=event(t);
 equal(signal(t,[source,1n,0n,0n]),0xffffffffn);equal(vm.readInt(t.error),6n);consume(t,source,258n);
 const full=api(t,'CreateSemaphoreW',[0n,1n,1n,0n]);equal(signal(t,[full,target,0n,0n]),0xffffffffn);equal(vm.readInt(t.error),298n);consume(t,full,0n);consume(t,full,258n);
 const mutex=api(t,'CreateMutexW',[0n,1n,0n]);const ready=event(t,true);equal(signal(t,[mutex,ready,0n,0n]),0n);equal(api(t,'ReleaseMutex',[mutex]),0n);equal(vm.readInt(t.error),288n);consume(t,mutex,0n);
 // A blocked call retains the original target, timeout, caller and single signal.
 frame(t,'SignalObjectAndWait',[source,target,50n,0n],'kernel32.dll');const work=vm.work,retired=vm.retired;
 equal(vm.service().reason,Exit.Waiting);equal(vm.work,work);equal(vm.retired,retired);
 equal(vm.service().reason,Exit.Waiting);now=500000n;
 equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),258n);equal(vm.work,work+1n);
 consume(t,source,0n);consume(t,source,258n);
 // The return frame is validated before any signal can become visible.
 frame(t,'SignalObjectAndWait',[source,ready,0n,0n],'kernel32.dll');const page=t.sp&~4095n;
 equal(core.wire_protect(t.handle,page,4096n,1),0);const before=snapshot(t);
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,page,4096n,4),0);consume(t,source,258n);
 for(const h of [own,source,target,full,mutex,ready])equal(api(t,'CloseHandle',[h]),1n);vm.dispose();
}

// APC setup faults must not signal the semaphore again on repair and retry.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,source=semaphore(t),target=event(t);
 vm.write(0x30000n,mode===32?[0x31,0xc0,0xc2,4,0]:[0x31,0xc0,0xc3]);
 equal(api(t,'QueueUserAPC',[0x30000n,BigInt.asUintN(mode,-2n),0n]),1n);
 t.sp=mode===32?0x25004n:0x25008n;frame(t,'SignalObjectAndWait',[source,target,0xffffffffn,1n],'kernel32.dll');
 equal(core.wire_protect(t.handle,0x24000n,4096n,1),0);const before=snapshot(t),work=vm.work;
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.work,work);
 equal(core.wire_protect(t.handle,0x24000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);
 equal(await finish(t),0xc0n);consume(t,source,0n);consume(t,source,258n);
 equal(api(t,'CloseHandle',[source]),1n);equal(api(t,'CloseHandle',[target]),1n);vm.dispose();
}
console.log(`${checks} SignalObjectAndWait ownership, single-signal, deadline, frame and APC-retry checks passed`);
