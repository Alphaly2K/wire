import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,gdi} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';

const ptr=t=>t.mode/8;
const pseudo=t=>BigInt.asUintN(t.mode,-1n);
function setup(t,name,args=[],ex=false,dll='kernel32.dll'){
 const all=ex?[pseudo(t),...args]:args;frame(t,name+(ex?'Ex':''),all,dll);return all.length;
}
function invoke(t,name,args=[],ex=false,dll='kernel32.dll'){
 const n=setup(t,name,args,ex,dll),work=t.vm.work,retired=t.vm.retired;
 equal(t.vm.service().reason,Exit.Dispatch);equal(t.vm.work,work+1n);equal(t.vm.retired,retired);equal(t.vm.ip,t.vm.entryPoints.returned);equal(t.vm.register(4),t.sp+BigInt(ptr(t)+(t.mode===32?n*4:0)));return t.vm.register(0);
}
function query(t,address,ex=false,output=0x21000n){
 const length=t.mode===32?28:48;equal(invoke(t,'VirtualQuery',[address,output,BigInt(length+16)],ex),BigInt(length));
 const read=(at,size=4)=>t.vm.readInt(output+BigInt(at),size);
 return t.mode===32?{base:read(0),allocation:read(4),initial:read(8),size:read(12),state:read(16),protect:read(20),kind:read(24)}:{base:read(0,8),allocation:read(8,8),initial:read(16),size:read(24,8),state:read(32),protect:read(36),kind:read(40)};
}
const read=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));

// All eight call signatures use one owner and one commit/decommit/protect/query protocol.
for(const mode of [32,64])for(const ex of [false,true]){
 const t=await machine(mode),{vm}=t,base=0x60000n,page=base+4096n;vm.write(t.error,encode(0xabcdef01n,4));vm.flags=0x8d5n;vm.xmm(0,new Uint8Array(16).fill(0xa5));const fpu=fp(t);
 equal(invoke(t,'VirtualAlloc',[base,0x3000n,0x2000n,4n],ex),base);equal(query(t,base+1n,ex),{base,allocation:base,initial:4n,size:0x3000n,state:0x2000n,protect:0n,kind:0x20000n});
 equal(invoke(t,'VirtualAlloc',[page+1n,4095n,0x1000n,4n],ex),page);equal(query(t,page+7n,ex),{base:page,allocation:base,initial:4n,size:4096n,state:0x1000n,protect:4n,kind:0x20000n});
 vm.write(page,encode(0x11223344n,4));equal(invoke(t,'VirtualProtect',[page,4n,2n,0x21200n],ex),1n);equal(vm.readInt(0x21200n),4n);assert.throws(()=>vm.write(page,[1]),/memory|permission|Protection/);equal(vm.readInt(page),0x11223344n);equal(invoke(t,'VirtualProtect',[page,4096n,4n,0x21200n],ex),1n);equal(vm.readInt(0x21200n),2n);
 equal(invoke(t,'VirtualFree',[page,4096n,0x4000n],ex),1n);equal(query(t,base,ex).state,0x2000n);assert.throws(()=>vm.readInt(page),/memory access/);equal(invoke(t,'VirtualAlloc',[page,4096n,0x1000n,4n],ex),page);equal(vm.readInt(page),0n);equal(invoke(t,'VirtualFree',[page,0n,0x8000n],ex),0n);equal(vm.readInt(t.error),487n);equal(invoke(t,'VirtualFree',[base,0n,0x8000n],ex),1n);equal(query(t,base,ex).state,0x10000n);equal(vm.readInt(t.error),487n);
 equal(vm.flags,0x8d5n);equal(vm.xmm(0),new Uint8Array(16).fill(0xa5));equal(fp(t),fpu);equal(t.time.calls,0);vm.dispose();
}

// Copy-outs preflight the whole range and capture returns even when outputs overwrite the call frame.
for(const mode of [32,64])for(const ex of [false,true]){
 const t=await machine(mode),{vm}=t,base=invoke(t,'VirtualAlloc',[0n,4096n,0x3000n,4n]);
 setup(t,'VirtualProtect',[base,4096n,2n,0x24ffen],ex);vm.write(0x24ffen,[0xcc,0xcc]);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(read(vm,0x24ffen,2),Uint8Array.from([0xcc,0xcc]));vm.write(base,[0x33]);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(0x24ffen),4n);assert.throws(()=>vm.write(base,[1]),/memory|permission|Protection/);
 equal(invoke(t,'VirtualProtect',[base,4096n,4n,t.sp],ex),1n);equal(vm.readInt(t.sp),2n);equal(query(t,base,ex,t.sp).allocation,base);
 const length=mode===32?28:48;vm.write(0x24ff0n,new Uint8Array(length).fill(0xa5));setup(t,'VirtualQuery',[base,0x24ff0n,BigInt(length)],ex);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const q=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),q);equal(read(vm,0x24ff0n,16),new Uint8Array(16).fill(0xa5));equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),BigInt(length));
 equal(invoke(t,'VirtualQuery',[base,0xdead0000n,BigInt(length-1)],ex),0n);equal(vm.readInt(t.error),mode===32?87n:24n);if(mode===32){equal(invoke(t,'VirtualQuery',[base,0n,28n],ex),0n);equal(vm.readInt(t.error),87n);}
 equal(invoke(t,'VirtualFree',[base,0n,0x8000n]),1n);vm.dispose();
}

// Ex validates full-width pseudo handles before output pointers; LastError and first-frame faults are retryable.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,bad=mode===64?0xffffffffn:0n;equal(core.wire_protect(t.handle,0x25000n,4096n,0x104),0);
 for(const [name,args]of [['VirtualAllocEx',[bad,0n,4096n,0x3000n,4n]],['VirtualFreeEx',[bad,0xdead0000n,0n,0x8000n]],['VirtualProtectEx',[bad,0xdead0000n,4096n,2n,0x25000n]],['VirtualQueryEx',[bad,0n,0x25000n,48n]]]){equal(call(t,name,args,'kernel32.dll'),0n);equal(vm.readInt(t.error),6n);}
 assert.throws(()=>vm.readInt(0x25000n),/memory access/);equal(vm.readInt(0x25000n),0n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);
 setup(t,'VirtualFree',[0x70000n,0n,0x8000n]);equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0n);equal(vm.readInt(t.error),487n);
 setup(t,'VirtualAlloc',[0x60000n,4096n,0x3000n,4n]);const stack=t.sp&~4095n;equal(core.wire_protect(t.handle,stack,4096n,1),0);const first=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),first);equal(core.wire_protect(t.handle,stack,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0x60000n);equal(invoke(t,'VirtualFree',[0x60000n,0n,0x8000n]),1n);
 assert.throws(()=>vm.bind('user32.dll','VirtualAlloc'),/unknown/);for(const name of ['VirtualAlloc','VirtualAllocEx','VirtualFree','VirtualFreeEx','VirtualProtect','VirtualProtectEx','VirtualQuery','VirtualQueryEx','FlushInstructionCache']){const address=vm.bind('kernel32.dll',name);if(mode===64||name==='FlushInstructionCache')equal(vm.bind('kernelbase.dll',name),address);else assert.throws(()=>vm.bind('kernelbase.dll',name),/unknown/);}
 equal(t.time.calls,0);vm.dispose();
}

// A newly admitted Win64 kernelbase export supports fresh lookup and canonical gateway ownership.
{
 const t=await machine(64),{vm}=t;vm.bind('kernelbase.dll','VirtualAlloc');vm.write(0x21000n,Uint8Array.from([...('kernelbase.dll\0')].flatMap(c=>[c.charCodeAt(0),0])));const owner=invoke(t,'GetModuleHandleW',[0x21000n],false,'kernelbase.dll');assert.ok(owner);vm.write(0x21000n,new TextEncoder().encode('FlushInstructionCache\0'));const address=invoke(t,'GetProcAddress',[owner,0x21000n],false,'kernelbase.dll');assert.ok(address);equal(vm.bind('kernelbase.dll','FlushInstructionCache'),address);equal(invoke(t,'GetModuleHandleExW',[6n,address+7n,0x21200n],false,'kernelbase.dll'),1n);const provider=vm.readInt(0x21200n,8);assert.ok(provider);const length=invoke(t,'GetModuleFileNameW',[provider,0x21000n,100n],false,'kernelbase.dll');equal(new TextDecoder('utf-16le').decode(read(vm,0x21000n,Number(length)*2)),'C:\\Windows\\System32\\kernel32.dll');equal(t.time.calls,0);vm.dispose();
}

// Win64 SIZE_T, region sizes and addresses remain wider than DWORD without committing a huge allocation.
{
 const t=await machine(64),{vm}=t,size=0x100002000n,base=invoke(t,'VirtualAlloc',[0n,size,0x102000n,4n],true),last=base+0x100001000n;assert.ok(base>0xffffffffn);equal(invoke(t,'VirtualFree',[base,0n,0x8000n],true),1n);
 equal(invoke(t,'VirtualAlloc',[base,size,0x1234567800002000n,0xabcdef0000000004n],true),base);equal(query(t,base+1n,true).size,size);const tail=query(t,base+0x100000000n,true);equal([tail.base,tail.allocation,tail.size],[base+0x100000000n,base,8192n]);equal(invoke(t,'VirtualAlloc',[last,4096n,0x1000n,4n],true),last);vm.write(last,encode(0xabcdefn,8));equal(query(t,last,true).base,last);equal(invoke(t,'VirtualFree',[base,0n,0x8000n],true),1n);equal(invoke(t,'VirtualAlloc',[0n,4096n,0x103000n,4n]),0x7ffffffe0000n);vm.dispose();
}

// Permissions, decommit and same-address reuse cannot revive an earlier JIT installation.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,base=invoke(t,'VirtualAlloc',[0n,4096n,0x3000n,4n]);vm.write(base,[0xb8,1,0,0,0,0xc3]);equal(invoke(t,'VirtualProtect',[base,4096n,0x20n,0x21200n]),1n);
 const run=async()=>{vm.ip=base;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,ptr(t)));return vm.run(10n);};equal((await run()).reason,Exit.Returned);equal(vm.register(0),1n);const ticket=core.wire_ticket(t.handle);equal(invoke(t,'VirtualProtect',[base,4096n,4n,0x21200n],true),1n);equal(core.wire_live(t.handle,ticket),0);vm.write(base+1n,[2]);equal(invoke(t,'VirtualProtect',[base,4096n,0x20n,0x21200n]),1n);equal(call(t,'FlushInstructionCache',[pseudo(t),base,6n],'kernel32.dll'),1n);equal((await run()).reason,Exit.Returned);equal(vm.register(0),2n);
 equal(invoke(t,'VirtualFree',[base,4096n,0x4000n]),1n);equal((await run()).reason,Exit.Fault);equal(invoke(t,'VirtualAlloc',[base,4096n,0x1000n,4n]),base);vm.write(base,[0xb8,3,0,0,0,0xc3]);equal(invoke(t,'VirtualProtect',[base,4096n,0x20n,0x21200n]),1n);equal((await run()).reason,Exit.Returned);equal(vm.register(0),3n);equal(invoke(t,'VirtualFree',[base,0n,0x8000n]),1n);equal((await run()).reason,Exit.Fault);
 equal(call(t,'FlushInstructionCache',[pseudo(t),0n,0xffffffffffffffffn],'kernel32.dll'),1n);equal(call(t,'FlushInstructionCache',[0n,0xdead0000n,1n],'kernel32.dll'),0n);equal(vm.readInt(t.error),6n);equal(invoke(t,'VirtualAlloc',[base,4096n,0x3000n,4n]),base);vm.write(base,[0xb8,4,0,0,0,0xc3]);equal(invoke(t,'VirtualProtect',[base,4096n,0x20n,0x21200n]),1n);equal((await run()).reason,Exit.Returned);equal(vm.register(0),4n);vm.dispose();
}

// DIB storage stays owned by its bitmap through all public VMM services.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.write(0x21000n,[40n,1n,BigInt.asUintN(32,-1n),0x200001n,0n,0n,0n,0n,0n,0n].flatMap(v=>Array.from(encode(v,4))));const h=gdi(t,'CreateDIBSection',[0n,0x21000n,0n,0x21200n,0n,0n]),base=vm.readInt(0x21200n,ptr(t));assert.ok(h);equal(query(t,base).kind,0x40000n);equal(invoke(t,'VirtualFree',[base,0n,0x8000n]),0n);equal(vm.readInt(t.error),487n);equal(invoke(t,'VirtualFree',[base,4096n,0x4000n],true),0n);equal(invoke(t,'VirtualProtect',[base,4096n,0x40n,0x21200n]),0n);equal(vm.readInt(t.error),5n);equal(query(t,base).protect,4n);equal(gdi(t,'DeleteObject',[h]),1n);equal(query(t,base).state,0x10000n);vm.dispose();
}

console.log(`${checks} virtual-memory ABI/ownership/output/fault/source-reuse checks passed`);
