import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';
const api=(t,name,args=[])=>call(t,name,args,'kernel32.dll');

// Width validation precedes reads; comparison success preserves thread error and CPU state.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,current=0x22001n,compared=0x23003n;
 vm.flags=0x8d5n;vm.xmm(3,new Uint8Array(16).fill(0xa5));const fpu=fp(t);
 for(const width of [1,2,4,8]){
  const value=0xfedcba9876543210n;vm.write(current,encode(value,8));vm.write(compared,encode(value,8));vm.write(t.error,encode(123n,4));
  equal(api(t,'WaitOnAddress',[current,compared,BigInt(width),0n]),0n);equal(vm.readInt(t.error),1460n);
  vm.write(compared+BigInt(width-1),[0x55]);vm.write(t.error,encode(123n,4));
  equal(api(t,'WaitOnAddress',[current,compared,BigInt(width),0n]),1n);equal(vm.readInt(t.error),123n);
 }
 for(const width of [0n,3n,16n,...(mode===64?[0x100000001n]:[])]){
  equal(api(t,'WaitOnAddress',[0xdead0000n,0xbeef0000n,width,5n]),0n);equal(vm.readInt(t.error),87n);
 }
 equal(api(t,'WakeByAddressSingle',[0xdead0000n]),0n);equal(api(t,'WakeByAddressAll',[0n]),0n);
 equal(vm.flags,0x8d5n);equal(vm.xmm(3),new Uint8Array(16).fill(0xa5));equal(fp(t),fpu);equal(t.time.calls,0);
 if(mode===64){const high=api(t,'VirtualAlloc',[0n,4096n,0x103000n,4n]);assert.ok(high>0xffffffffn);vm.write(high,encode(7n,8));vm.write(compared,encode(6n,8));equal(api(t,'WaitOnAddress',[high,compared,8n,0n]),1n);equal(api(t,'VirtualFree',[high,0n,0x8000n]),1n);}
 vm.dispose();
}

// Retries keep their initial deadline and call frame, and never reread the compared bytes.
for(const mode of [32,64]){
 let tick=1000000n;const t=await machine(mode,{clock:()=>tick}),{vm}=t,current=0x22000n,compared=0x23000n;
 vm.write(current,encode(123n,8));vm.write(compared,encode(123n,8));frame(t,'WaitOnAddress',[current,compared,8n,5n],'kernel32.dll');const before=snapshot(t),work=vm.work,retired=vm.retired;
 equal(vm.service().reason,Exit.Waiting);equal(snapshot(t),before);equal(t.time.calls,1);
 vm.write(t.sp,new Uint8Array(mode===32?20:40));equal(core.wire_protect(t.handle,current,4096n,1),0);equal(core.wire_protect(t.handle,compared,4096n,1),0);
 tick+=10000n;equal(vm.service().reason,Exit.Waiting);equal(snapshot(t),before);equal(t.time.calls,2);
 tick+=100000n;equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.work,work);equal(vm.retired,retired);equal(t.time.calls,3);
 equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);equal(vm.register(4),t.sp+BigInt(mode===32?20:8));equal(vm.register(0),0n);equal(vm.readInt(t.error),1460n);equal(vm.work,work+1n);equal(vm.retired,retired);equal(t.time.calls,3);vm.dispose();
}

// Initial faults cannot register a waiter; error-output faults preserve the saved caller.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,current=0x22000n,compared=0x24ffen;
 vm.write(current,encode(7n,8));vm.write(compared,encode(7n,8));vm.write(t.error,encode(123n,4));frame(t,'WaitOnAddress',[current,compared,8n,0n],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,1),0);let before=snapshot(t);
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(t.error),123n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(t.error),1460n);
 frame(t,'WaitOnAddress',[0xdead0000n,0n,3n,0n],'kernel32.dll');equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(t.error),87n);equal(t.time.calls,0);vm.dispose();
}

// An exception handler's Sleep must leave the outer timed-out wait intact.
for(const mode of [32,64]){
 let tick=1000000n;const t=await machine(mode,{clock:()=>tick}),{vm}=t,sleep=vm.bind('kernel32.dll','Sleep');
 vm.write(0x40000n,mode===32?[0x6a,0,0xb8,...encode(sleep,4),0xff,0xd0,0xb8,0xff,0xff,0xff,0xff,0xc2,4,0]:[0x48,0x83,0xec,40,0x31,0xc9,0x48,0xb8,...encode(sleep,8),0xff,0xd0,0x48,0x83,0xc4,40,0xb8,0xff,0xff,0xff,0xff,0xc3]);
 api(t,'AddVectoredExceptionHandler',[1n,0x40000n]);vm.write(0x22000n,encode(7n,8));vm.write(0x23000n,encode(7n,8));frame(t,'WaitOnAddress',[0x22000n,0x23000n,8n,5n],'kernel32.dll');equal(vm.service().reason,Exit.Waiting);vm.write(t.sp,new Uint8Array(mode===32?20:40));tick+=100000n;
 equal(core.wire_protect(t.handle,t.image.teb,4096n,0x104),0);equal(vm.service().reason,Exit.Fault);equal(core.wire_deliver_exception(t.handle,Exit.Fault),1);equal((await vm.run(1000n)).reason,Exit.Returned);equal(vm.register(0),0n);equal(vm.readInt(t.error),1460n);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 for(const name of ['WaitOnAddress','WakeByAddressSingle','WakeByAddressAll','Sleep','SleepEx']){
  const address=vm.bind('kernel32.dll',name);for(const provider of ['kernelbase.dll','api-ms-win-core-synch-l1-2-0.dll'])equal(vm.bind(provider,name),address);assert.throws(()=>vm.bind('user32.dll',name),/unknown/);
 }
 vm.dispose();
}
console.log(`${checks} address-wait ABI/width/deadline/call-frame/fault/continuation checks passed`);
