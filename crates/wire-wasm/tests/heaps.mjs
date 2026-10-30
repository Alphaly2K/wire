import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {moduleFixture} from './module-fixture.mjs';
const api=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
function status(t){t.vm.write(0x21000n,encode(64n,4));equal(api(t,'GlobalMemoryStatusEx',[0x21000n]),1n);return[t.vm.readInt(0x21010n,8),t.vm.readInt(0x21030n,8)];}

// A single owner packs small blocks, preserves exact sizes and keeps heaps distinct.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=api(t,'GetProcessHeap');equal(vm.readInt(t.image.peb+BigInt(mode===32?0x18:0x30),mode/8),h);if(mode===64)assert.ok(h>0xffffffffn);
 equal(api(t,'VirtualFree',[h,0n,0x8000n]),0n);equal(vm.readInt(t.error),487n);
 equal(api(t,'GetProcessHeaps',[0n,0xdead0000n]),1n);equal(api(t,'GetProcessHeaps',[4n,0x21400n]),1n);equal(vm.readInt(0x21400n,mode/8),h);
 vm.write(t.error,encode(123n,4));vm.flags=0x8d5n;vm.xmm(7,new Uint8Array(16).fill(0xa5));const fpu=fp(t);
 const p=api(t,'HeapAlloc',[h,8n,33n]);assert.ok(p);equal(p%16n,0n);equal(bytes(vm,p,33),new Uint8Array(33));equal(api(t,'HeapSize',[h,0n,p]),33n);vm.write(p,new Uint8Array(33).fill(0x73));
 equal(api(t,'HeapReAlloc',[h,8n,p,48n]),p);equal(bytes(vm,p,48),Uint8Array.from([...new Uint8Array(33).fill(0x73),...new Uint8Array(15)]));const barrier=api(t,'HeapAlloc',[h,0n,16n]);equal(barrier,p+48n);
 equal(api(t,'HeapReAlloc',[h,16n,p,80n]),0n);equal(vm.readInt(t.error),123n);equal(api(t,'HeapSize',[h,0n,p]),48n);
 const moved=api(t,'HeapReAlloc',[h,8n,p,80n]);assert.ok(moved&&moved!==p);equal(bytes(vm,moved,80),Uint8Array.from([...new Uint8Array(33).fill(0x73),...new Uint8Array(47)]));equal(api(t,'HeapValidate',[h,0n,p]),0n);equal(api(t,'HeapSize',[h,0n,moved]),80n);
 equal(api(t,'HeapReAlloc',[h,0n,moved,0n]),moved);equal(api(t,'HeapSize',[h,0n,moved]),0n);equal(api(t,'HeapFree',[h,0n,moved]),1n);equal(api(t,'HeapFree',[h,0n,barrier]),1n);equal(api(t,'HeapFree',[h,0n,0n]),1n);equal(api(t,'HeapValidate',[h,0n,0n]),1n);
 const other=api(t,'HeapCreate',[1n,4096n,4096n]);assert.ok(other&&other!==h);const fixed=api(t,'HeapAlloc',[other,0n,4096n]);assert.ok(fixed);equal(api(t,'HeapAlloc',[other,0n,1n]),0n);equal(vm.readInt(t.error),8n);equal(api(t,'HeapSize',[h,0n,fixed]),BigInt.asUintN(mode,-1n));equal(vm.readInt(t.error),87n);equal(api(t,'HeapFree',[h,0n,fixed]),0n);equal(api(t,'HeapSize',[other,0n,fixed]),4096n);
 vm.write(0x21400n,new Uint8Array(32).fill(0xa5));equal(api(t,'GetProcessHeaps',[1n,0x21400n]),2n);equal(bytes(vm,0x21400n,32),new Uint8Array(32).fill(0xa5));equal(api(t,'GetProcessHeaps',[2n,t.sp]),2n);equal(vm.ip,vm.entryPoints.returned);equal(new Set([vm.readInt(t.sp,mode/8),vm.readInt(t.sp+BigInt(mode/8),mode/8)]),new Set([h,other]));
 equal(api(t,'HeapDestroy',[other]),1n);assert.throws(()=>vm.readInt(fixed),/memory access/);assert.throws(()=>vm.readInt(other),/memory access/);equal(api(t,'HeapDestroy',[h]),0n);equal(vm.readInt(t.error),87n);equal(api(t,'GetProcessHeaps',[0n,0n]),1n);
 equal(api(t,'HeapAlloc',[h,4n,16n]),0n);equal(vm.readInt(t.error),50n);equal(api(t,'HeapAlloc',[h,0n,0x10000001n]),0n);equal(vm.readInt(t.error),8n);if(mode===64){equal(api(t,'HeapAlloc',[h,0n,0x100000010n]),0n);equal(vm.readInt(t.error),8n);equal(api(t,'HeapFree',[h&0xffffffffn,0n,0n]),0n);equal(vm.readInt(t.error),6n);const q=api(t,'HeapAlloc',[h,0xabcdef0000000008n,16n]);assert.ok(q);equal(api(t,'HeapFree',[h,0n,q]),1n);const wide=api(t,'HeapCreate',[0n,0n,0x100002000n]);assert.ok(wide);equal(api(t,'HeapDestroy',[wide]),1n);equal(api(t,'HeapCreate',[0n,0n,0xffffffffffffffffn]),0n);equal(vm.readInt(t.error),8n);}
 equal(vm.flags,0x8d5n);equal(vm.xmm(7),new Uint8Array(16).fill(0xa5));equal(fp(t),fpu);equal(t.time.calls,0);vm.dispose();
}

// Compatibility queries describe the actual allocator and preflight both SIZE_T and DWORD outputs.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=api(t,'GetProcessHeap'),p=mode===32?0x22000n:0x100022000n,width=mode/8;if(mode===64)vm.map(p,8192n,3);
 const query=(heap=h,kind=0n,buffer=p,length=4n,returned=p+16n)=>api(t,'HeapQueryInformation',[heap,kind,buffer,length,returned]);
 vm.write(t.error,encode(77n,4));vm.write(p,new Uint8Array(32).fill(0xa5));equal(query(),1n);equal(vm.readInt(p),0n);equal(vm.readInt(p+4n),0xa5a5a5a5n);equal(vm.readInt(p+16n,width),4n);equal(vm.readInt(p+16n+BigInt(width)),0xa5a5a5a5n);equal(vm.readInt(t.error),77n);
 const other=api(t,'HeapCreate',[0n,4096n,0n]);equal(query(other),1n);equal(api(t,'HeapDestroy',[other]),1n);equal(query(other),0n);equal(vm.readInt(t.error),6n);
 for(const length of [0n,1n,3n]){vm.write(p,new Uint8Array(32).fill(0xa5));equal(query(h,0n,0xdead0000n,length),0n);equal(vm.readInt(t.error),122n);equal(vm.readInt(p),0xa5a5a5a5n);equal(vm.readInt(p+16n,width),4n);}
 equal(query(h,0n,p,mode===32?4n:0x100000004n,0n),1n);equal(query(h,0x1234000000000000n),1n);equal(query(0n,0n,0xdead0000n,4n,0xdead0000n),0n);equal(vm.readInt(t.error),6n);
 for(const kind of [1n,2n,0xffffffffn]){equal(query(h,kind,0xdead0000n,4n,0xdead0000n),0n);equal(vm.readInt(t.error),50n);}
 const faultPointer=p+4096n-BigInt(width)+1n;for(const kind of ['length','value']){
  vm.write(p,new Uint8Array(32).fill(0xa5));vm.write(faultPointer,new Uint8Array(width).fill(0xa5));frame(t,'HeapQueryInformation',[h,0n,kind==='value'?p+4094n:p,4n,kind==='length'?faultPointer:p+16n],'kernel32.dll');equal(core.wire_protect(t.handle,p+4096n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(p),0xa5a5a5a5n);equal(vm.readInt(p+16n,width),BigInt('0x'+'a5'.repeat(width)));equal(core.wire_protect(t.handle,p+4096n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),1n);
 }
 vm.write(p+16n,new Uint8Array(width).fill(0xa5));frame(t,'HeapQueryInformation',[h,0n,0xdead0000n,0n,p+16n],'kernel32.dll');equal(core.wire_protect(t.handle,t.error&~4095n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(p+16n,width),BigInt('0x'+'a5'.repeat(width)));equal(core.wire_protect(t.handle,t.error&~4095n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(p+16n,width),4n);equal(vm.register(0),0n);
 equal(query(h,0n,p,4n,p),1n);equal(vm.readInt(p),0n);if(mode===64)equal(vm.readInt(p+4n),0n);equal(query(h,0n,p,4n,t.sp),1n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp,width),4n);equal(query(h,0n,t.sp,4n,0n),1n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp),0n);equal(t.time.calls,0);vm.dispose();
}

// Free ranges coalesce, and empty chunks are reclaimed while one 64 KiB spare remains.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=api(t,'GetProcessHeap'),baseline=status(t),pointers=[];
 for(let n=0;n<4097;n++){const p=api(t,'HeapAlloc',[h,0n,1n]);assert.ok(p);equal(p%16n,0n);pointers.push(p);vm.write(p,[n&255]);}
 const full=status(t);equal(full[0],baseline[0]-131072n);equal(new Set(pointers).size,4097);
 for(let n=0;n<4097;n+=2){equal(vm.readInt(pointers[n],1),BigInt(n&255));equal(api(t,'HeapFree',[h,0n,pointers[n]]),1n);}
 for(let n=1;n<4097;n+=2){equal(vm.readInt(pointers[n],1),BigInt(n&255));equal(api(t,'HeapFree',[h,0n,pointers[n]]),1n);}
 equal(status(t)[0],baseline[0]-65536n);const merged=api(t,'HeapAlloc',[h,8n,65536n]);equal(merged,pointers[4096]);equal(bytes(vm,merged,65536),new Uint8Array(65536));equal(status(t)[0],baseline[0]-65536n);equal(api(t,'HeapFree',[h,0n,merged]),1n);
 const large=api(t,'HeapAlloc',[h,0n,65537n]);assert.ok(large);equal(status(t)[0],baseline[0]-65536n-69632n);equal(api(t,'HeapFree',[h,0n,large]),1n);assert.throws(()=>vm.readInt(large),/memory access/);equal(status(t)[0],baseline[0]-65536n);vm.dispose();
}

// Reused zero-fill and moving copies fault before consuming a free span or old allocation.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=api(t,'GetProcessHeap'),p=api(t,'HeapAlloc',[h,0n,8192n]),blocker=api(t,'HeapAlloc',[h,0n,16n]);vm.write(p,new Uint8Array(8192).fill(0x73));equal(api(t,'HeapFree',[h,0n,p]),1n);
 frame(t,'HeapAlloc',[h,8n,8192n],'kernel32.dll');equal(core.wire_protect(t.handle,p+4096n,4096n,2),0);let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(vm,p,16),new Uint8Array(16).fill(0x73));equal(core.wire_protect(t.handle,p+4096n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),p);equal(bytes(vm,p,8192),new Uint8Array(8192));
 // In-place expansion checks the whole tail before changing either size or bytes.
 equal(api(t,'HeapReAlloc',[h,0n,p,1n]),p);vm.write(p+1n,new Uint8Array(8191).fill(0x73));frame(t,'HeapReAlloc',[h,8n,p,8192n],'kernel32.dll');equal(core.wire_protect(t.handle,p+4096n,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(vm,p+1n,15),new Uint8Array(15).fill(0x73));equal(core.wire_protect(t.handle,p+4096n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),p);
 vm.write(p,[0x37]);const baseline=status(t);frame(t,'HeapReAlloc',[h,8n,p,100000n],'kernel32.dll');equal(core.wire_protect(t.handle,p+4096n,4096n,1),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,p+4096n,4096n,4),0);equal(status(t),baseline);equal(api(t,'HeapSize',[h,0n,p]),8192n);frame(t,'HeapReAlloc',[h,8n,p,100000n],'kernel32.dll');equal(vm.service().reason,Exit.Dispatch);const q=vm.register(0);assert.ok(q!==p);equal(vm.readInt(q,1),0x37n);equal(bytes(vm,q+8192n,32),new Uint8Array(32));equal(api(t,'HeapFree',[h,0n,q]),1n);equal(api(t,'HeapFree',[h,0n,blocker]),1n);
 // Failed error/first-return slots retain the block and the original CPU.
 frame(t,'HeapFree',[h,0n,0xdead0000n],'kernel32.dll');equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(t.error),87n);
 const live=api(t,'HeapAlloc',[h,0n,16n]);frame(t,'HeapFree',[h,0n,live],'kernel32.dll');const stack=t.sp&~4095n;equal(core.wire_protect(t.handle,stack,4096n,1),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,stack,4096n,4),0);equal(api(t,'HeapSize',[h,0n,live]),16n);equal(api(t,'HeapFree',[h,0n,live]),1n);
 equal(t.time.calls,0);vm.dispose();
}

// Fixed Local/Global operations share process-heap ownership and all paired aliases.
for(const mode of [32,64])for(const prefix of ['Local','Global']){
 const t=await machine(mode),{vm}=t,h=api(t,'GetProcessHeap');vm.write(t.error,encode(123n,4));const p=api(t,prefix+'Alloc',[0x40n,33n]);assert.ok(p);equal(bytes(vm,p,33),new Uint8Array(33));equal(api(t,prefix+'Size',[p]),33n);equal(api(t,'HeapSize',[h,0n,p]),33n);equal(api(t,prefix+'Lock',[p]),p);equal(api(t,prefix+'Flags',[p]),0n);equal(vm.readInt(t.error),123n);equal(api(t,prefix+'Unlock',[p]),prefix==='Global'?1n:0n);equal(vm.readInt(t.error),prefix==='Global'?123n:158n);
 vm.write(p,[0x73]);const barrier=api(t,'HeapAlloc',[h,0n,16n]);equal(api(t,prefix+'ReAlloc',[p,65536n,0n]),0n);equal(api(t,prefix+'Size',[p]),33n);const q=api(t,prefix+'ReAlloc',[p,65536n,0x42n]);assert.ok(q&&q!==p);equal(vm.readInt(q,1),0x73n);equal(bytes(vm,q+33n,64),new Uint8Array(64));equal(api(t,prefix+'Size',[q]),65536n);equal(api(t,prefix+'Free',[q]),0n);equal(api(t,prefix+'Free',[0n]),0n);equal(api(t,'HeapFree',[h,0n,barrier]),1n);
 equal(api(t,prefix+'Alloc',[2n,8n]),0n);equal(vm.readInt(t.error),50n);equal(api(t,prefix+'Flags',[0xdead0000n]),0x8000n);equal(vm.readInt(t.error),6n);equal(api(t,prefix+'Free',[0xdead0000n]),0xdead0000n);equal(api(t,prefix+'Lock',[0n]),0n);equal(vm.readInt(t.error),6n);equal(t.time.calls,0);vm.dispose();
}

// Guest-written heap code participates in the same source revocation as VMM code.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=api(t,'HeapCreate',[0x40000n,0n,0n]),p=api(t,'HeapAlloc',[h,0n,16n]);vm.write(p,[0xb8,17,0,0,0,0xc3]);const run=async()=>{vm.ip=p;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,mode/8));return vm.run(10n);};equal((await run()).reason,Exit.Returned);equal(vm.register(0),17n);const ticket=core.wire_ticket(t.handle);equal(api(t,'HeapFree',[h,0n,p]),1n);equal(api(t,'HeapAlloc',[h,8n,16n]),p);equal(core.wire_live(t.handle,ticket),0);vm.write(p,[0xb8,29,0,0,0,0xc3]);equal((await run()).reason,Exit.Returned);equal(vm.register(0),29n);equal(api(t,'HeapDestroy',[h]),1n);equal((await run()).reason,Exit.Fault);vm.dispose();
}

// Rejected startup also reclaims process-owned heap allocations and identities.
for(const mode of [32,64]){
 const vm=new WasmMachine(core,{mode}),bad=moduleFixture(mode,{dll:true,attach:false,tls:false});
 vm.supplyModule('bad.dll',bad.bytes);const image=vm.load(moduleFixture(mode,{dependency:'bad.dll',tls:false}).bytes),t={vm,mode,image,sp:image.sp-2048n};
 const ip=vm.ip,registers=Array.from({length:mode===32?8:16},(_,i)=>vm.register(i)),h=api(t,'GetProcessHeap'),p=api(t,'HeapAlloc',[h,8n,70000n]),other=api(t,'HeapCreate',[0n,4096n,0n]);assert.ok(other);vm.ip=ip;registers.forEach((v,i)=>vm.register(i,v));
 await assert.rejects(vm.run(10000n),/attach returned FALSE/);for(const address of [h,p,other])assert.throws(()=>vm.readInt(address),/memory access/);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 for(const name of ['GetProcessHeap','HeapCreate','HeapDestroy','HeapAlloc','HeapFree','HeapReAlloc','HeapSize','HeapValidate','GetProcessHeaps','HeapQueryInformation','LocalAlloc','GlobalAlloc','LocalReAlloc','GlobalReAlloc','LocalFree','GlobalFree','LocalSize','GlobalSize','LocalLock','GlobalLock','LocalUnlock','GlobalUnlock','LocalFlags','GlobalFlags']){const address=vm.bind('kernel32.dll',name);if(mode===64)equal(vm.bind('kernelbase.dll',name),address);else assert.throws(()=>vm.bind('kernelbase.dll',name),/unknown/);assert.throws(()=>vm.bind('user32.dll',name),/unknown/);}
 for(const name of ['HeapSetInformation','HeapCompact','HeapWalk'])assert.throws(()=>vm.bind('kernel32.dll',name),/not been migrated|unknown/);vm.dispose();
}
console.log(`${checks} heap/local/global ABI/packing/ownership/resize/fault/source checks passed`);
