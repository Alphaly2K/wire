import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,finish} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';
const api=(t,name,args=[])=>call(t,name,args,'oleaut32.dll');
const source=0x22000n,out=0x24000n;
function variant(vm,p,type,value=0n){const bytes=new Uint8Array(16).fill(0x55);bytes.set(encode(BigInt(type),2));bytes.set(encode(value,4),8);vm.write(p,bytes);return bytes;}
const raw=(vm,p)=>Uint8Array.from({length:16},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const t=await machine(32),{vm}=t,heap=call(t,'GetProcessHeap',[],'kernel32.dll');
const live=p=>call(t,'HeapValidate',[heap,0n,p-4n],'kernel32.dll');
for(const name of ['VariantInit','VariantClear','VariantCopy','VariantChangeType']){assert.ok(vm.bind('oleaut32.dll',name));for(const dll of ['ole32.dll','combase.dll','kernel32.dll'])assert.throws(()=>vm.bind(dll,name),/unknown/);}
vm.flags=0x8d5n;vm.xmm(3,new Uint8Array(16).fill(0x7b));const fpu=fp(t);vm.write(t.error,encode(123n,4));
for(const type of [0,1,3,5,11,19,23]){
 const expected=variant(vm,source,type,42n);equal(api(t,'VariantInit',[out]),0n);equal(raw(vm,out),new Uint8Array(16));equal(api(t,'VariantCopy',[out,source]),0n);equal(raw(vm,out),expected);equal(api(t,'VariantChangeType',[out,source,0xffffffffn,0x12340000n+BigInt(type)]),0n);equal(raw(vm,out),expected);equal(api(t,'VariantClear',[out]),0n);expected.set([0,0]);equal(raw(vm,out),expected);
}
let p=api(t,'SysAllocStringLen',[0n,6n]);assert.ok(p);variant(vm,source,8,p);equal(api(t,'VariantInit',[out]),0n);equal(api(t,'VariantCopy',[out,source]),0n);let copy=vm.readInt(out+8n);assert.notEqual(copy,p);equal(live(p),1n);equal(live(copy),1n);equal(api(t,'SysStringLen',[copy]),6n);
equal(api(t,'VariantCopy',[source,source]),0n);const replacement=vm.readInt(source+8n);assert.notEqual(replacement,p);equal(live(p),0n);equal(live(replacement),1n);p=replacement;
equal(api(t,'VariantClear',[out]),0n);equal(live(copy),0n);equal(vm.readInt(out+8n),copy);
variant(vm,out,0x4008,p);equal(api(t,'VariantClear',[out]),0n);equal(vm.readInt(out,2),0n);equal(vm.readInt(out+8n),p);equal(live(p),1n);
// Repeated failed destination cleanup must return its speculative BSTR to the heap.
const probe=api(t,'SysAllocStringLen',[0n,6n]);equal(api(t,'SysFreeString',[probe]),0n);variant(vm,out,9,0xfeedn);
for(let n=0;n<8;n++){equal(api(t,'VariantCopy',[out,source]),0x80020008n);equal(vm.readInt(t.error),0x80020008n);equal(vm.readInt(out,2),9n);equal(live(p),1n);}
const recovered=api(t,'SysAllocStringLen',[0n,6n]);equal(recovered,probe);equal(api(t,'SysFreeString',[recovered]),0n);
equal(api(t,'VariantChangeType',[out,source,0n,9n]),0x80020005n);equal(vm.readInt(t.error),0x80020005n);
for(const type of [12,13]){variant(vm,source,type,0xfeedn);equal(api(t,'VariantInit',[out]),0n);equal(api(t,'VariantCopy',[out,source]),0x80020005n);equal(vm.readInt(out,2),0n);const before=variant(vm,out,type,0xfeedn);equal(api(t,'VariantClear',[out]),0x80020008n);equal(raw(vm,out),before);}
variant(vm,source,0);equal(api(t,'VariantInit',[out]),0n);equal(api(t,'VariantChangeType',[out,source,0n,8n]),0n);const empty=vm.readInt(out+8n);assert.ok(empty);equal(api(t,'SysStringLen',[empty]),0n);equal(api(t,'VariantClear',[out]),0n);
variant(vm,source,8,p);vm.write(0x24ffen,new Uint8Array(16).fill(0));frame(t,'VariantCopy',[0x24ffen,source],'oleaut32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(0x24ffen,2),0n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);copy=vm.readInt(0x24ffen+8n);equal(live(p),1n);equal(live(copy),1n);equal(api(t,'VariantClear',[0x24ffen]),0n);
// A readable source and write-only destination reach the tag-read rollback path.
const tagProbe=api(t,'SysAllocStringLen',[0n,6n]);equal(api(t,'SysFreeString',[tagProbe]),0n);vm.map(0x90000n,4096n,2);vm.write(0x90000n,new Uint8Array(16));frame(t,'VariantCopy',[0x90000n,source],'oleaut32.dll');const unreadable=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),unreadable);const tagRecovered=api(t,'SysAllocStringLen',[0n,6n]);equal(tagRecovered,tagProbe);equal(api(t,'SysFreeString',[tagRecovered]),0n);equal(core.wire_protect(t.handle,0x90000n,4096n,4),0);frame(t,'VariantCopy',[0x90000n,source],'oleaut32.dll');equal(vm.service().reason,Exit.Dispatch);equal(api(t,'VariantClear',[0x90000n]),0n);
// Cleanup cannot free the storage that will hold the resulting VARIANT.
const overlapping=api(t,'SysAllocStringLen',[0n,16n]);variant(vm,overlapping,8,overlapping);frame(t,'VariantClear',[overlapping],'oleaut32.dll');const overlap=snapshot(t);assert.throws(()=>vm.service(),/output overlaps allocation/);equal(snapshot(t),overlap);equal(live(overlapping),1n);equal(api(t,'SysFreeString',[overlapping]),0n);
variant(vm,source,3,123n);equal(api(t,'VariantInit',[out]),0n);const target=vm.bind('oleaut32.dll','VariantCopy');vm.write(0x30000n,[0x68,...encode(source,4),0x68,...encode(out,4),0xb8,...encode(target,4),0xff,0xd0,0xc3]);vm.ip=0x30000n;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,4));await finish(t,1n);equal(vm.register(0),0n);equal(vm.register(4),t.sp+4n);equal(vm.readInt(out+8n),123n);
equal(api(t,'SysFreeString',[p]),0n);vm.write(t.error,encode(123n,4));frame(t,'VariantClear',[out+64n],'oleaut32.dll');vm.write(out+64n,encode(9n,2));equal(core.wire_protect(t.handle,t.error&~4095n,4096n,2),0);const errorBefore=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),errorBefore);equal(vm.readInt(out+64n,2),9n);equal(core.wire_protect(t.handle,t.error&~4095n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0x80020008n);
equal(vm.flags,0x8d5n);equal(vm.xmm(3),new Uint8Array(16).fill(0x7b));equal(fp(t),fpu);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
{
 const t=await machine(64),{vm}=t,source=0x100022000n,out=0x100024000n,size=24;vm.map(source,4096n,3);vm.map(out,8192n,3);
 const put=(p,vt,value=0n)=>{const b=new Uint8Array(size).fill(0x55);b.set(encode(BigInt(vt),2));b.set(encode(value,8),8);vm.write(p,b);return b;},raw=p=>Uint8Array.from({length:size},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
 for(const[name,ordinal]of [['VariantInit',8],['VariantClear',9],['VariantCopy',10],['VariantChangeType',12]]){equal(vm.bind('oleaut32.dll','#'+ordinal),vm.bind('oleaut32.dll',name));assert.throws(()=>vm.bind('ole32.dll',name),/unknown/);}
 vm.write(t.error,encode(77n,4));for(const vt of [0,1,3,5,11,14,19,23]){const bytes=put(source,vt,0xabcdef0187654321n);equal(api(t,'VariantInit',[out]),0n);equal(raw(out),new Uint8Array(size));equal(api(t,'VariantCopy',[out,source]),0n);equal(raw(out),bytes);equal(api(t,'VariantChangeType',[out,source,0xffffffffn,0x12340000n+BigInt(vt)]),0n);equal(raw(out),bytes);equal(api(t,'VariantClear',[out]),0n);bytes.set([0,0]);equal(raw(out),bytes);}equal(vm.readInt(t.error),77n);
 // Win64 byte BSTRs keep odd lengths, while the VARIANT payload remains a full pointer.
 vm.write(source+0x100n,[0x41,0,0x42,0,0x43]);let p=api(t,'SysAllocStringByteLen',[source+0x100n,5n]);put(source,8,p);api(t,'VariantInit',[out]);equal(api(t,'VariantCopy',[out,source]),0n);let copy=vm.readInt(out+8n,8);assert.notEqual(copy,p);equal(api(t,'SysStringByteLen',[copy]),5n);equal(Array.from({length:7},(_,i)=>vm.readInt(copy+BigInt(i),1)),[65n,0n,66n,0n,67n,0n,0n]);
 equal(api(t,'VariantCopy',[source,source]),0n);const replaced=vm.readInt(source+8n,8);assert.notEqual(replaced,p);assert.throws(()=>vm.readInt(p),/memory|fault|mapped/i);p=replaced;
 equal(api(t,'VariantClear',[out]),0n);assert.throws(()=>vm.readInt(copy),/memory|fault|mapped/i);equal(vm.readInt(out+8n,8),copy);
 put(out,0x4008,p);equal(api(t,'VariantClear',[out]),0n);equal(vm.readInt(out+8n,8),p);equal(api(t,'SysStringByteLen',[p]),5n);
 for(let n=0;n<8;n++){put(out,9,0xfeedn);equal(api(t,'VariantCopy',[out,source]),0x80020008n);equal(vm.readInt(t.error),0x80020008n);equal(vm.readInt(out,2),9n);equal(api(t,'SysStringByteLen',[p]),5n);}
 equal(api(t,'VariantChangeType',[out,source,0n,9n]),0x80020005n);
 for(const vt of [12,13]){put(source,vt);api(t,'VariantInit',[out]);equal(api(t,'VariantCopy',[out,source]),0x80020005n);const bytes=put(out,vt);equal(api(t,'VariantClear',[out]),0x80020008n);equal(raw(out),bytes);}
 put(source,0);api(t,'VariantInit',[out]);equal(api(t,'VariantChangeType',[out,source,0n,8n]),0n);const empty=vm.readInt(out+8n,8);assert.ok(empty);equal(api(t,'SysStringLen',[empty]),0n);api(t,'VariantClear',[out]);
 put(source,8,0n);api(t,'VariantInit',[out]);equal(api(t,'VariantCopy',[out,source]),0n);equal(vm.readInt(out+8n,8),0n);
 put(source,8,p);const cross=out+0xff0n;vm.write(cross,new Uint8Array(size));frame(t,'VariantCopy',[cross,source],'oleaut32.dll');equal(core.wire_protect(t.handle,out+4096n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(raw(cross),new Uint8Array(size));equal(core.wire_protect(t.handle,out+4096n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);api(t,'VariantClear',[cross]);
 const overlap=api(t,'SysAllocStringLen',[0n,16n]);put(overlap,8,overlap);frame(t,'VariantClear',[overlap],'oleaut32.dll');const saved=snapshot(t);assert.throws(()=>vm.service(),/output overlaps allocation/);equal(snapshot(t),saved);equal(vm.readInt(overlap+8n,8),overlap);api(t,'SysFreeString',[overlap]);
 put(source,3,0xabcdef0187654321n);api(t,'VariantInit',[out]);vm.write(0x30000n,[0x48,0x83,0xec,40,0x48,0xb9,...encode(out,8),0x48,0xba,...encode(source,8),0x48,0xb8,...encode(vm.bind('oleaut32.dll','VariantCopy'),8),0xff,0xd0,0x48,0x83,0xc4,40,0xc3]);vm.ip=0x30000n;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,8));await finish(t,1n);equal(vm.register(0),0n);equal(vm.register(4),t.sp+8n);equal(vm.readInt(out+8n,8),0xabcdef0187654321n);
 put(out,9);frame(t,'VariantClear',[out],'oleaut32.dll');equal(core.wire_protect(t.handle,t.error&~4095n,4096n,2),0);const errorBefore=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),errorBefore);equal(vm.readInt(out,2),9n);equal(core.wire_protect(t.handle,t.error&~4095n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0x80020008n);api(t,'SysFreeString',[p]);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}
// Named and ordinal imports resolve through the production loader and execute through the JIT.
for(const mode of [32,64]){
 const f=fixture(mode,{tls:false}),names=['VariantInit','VariantClear','VariantCopy','VariantChangeType'],ordinals=[8,9,10,12];f.bytes.fill(0,f.offset(0x2080),f.offset(0x2600));f.rtext(0x20c0,'oleaut32.dll\0');f.rput(0x2000,0x2100);f.rput(0x2010,0x2180);
 for(const[i,name]of names.entries()){const entry=i%2?(1n<<BigInt(mode-1))|BigInt(ordinals[i]):BigInt(0x2200+i*64);f.rput(0x2100+i*f.ptr,entry,f.ptr);f.rput(0x2180+i*f.ptr,entry,f.ptr);if(i%2===0)f.rtext(Number(entry)+2,name+'\0');}
 f.bytes.fill(0xa5,f.offset(0x3000),f.offset(0x3020));if(mode===32){f.bytes.set([0x68,...encode(f.preferred+0x3000n,4),0xff,0x15,...encode(f.preferred+0x2180n,4),0xc3],f.offset(0x1000));f.relocations.splice(0,f.relocations.length,0x1001,0x1007);}else{f.bytes.set([0x48,0x83,0xec,40,0x48,0xb9,...encode(f.preferred+0x3000n,8),0xff,0x15,...encode(0x116cn,4),0x48,0x83,0xc4,40,0xc3],f.offset(0x1000));f.relocations.splice(0,f.relocations.length,0x1006);}f.rebuildRelocations();
 const vm=new WasmMachine(core,{mode}),image=vm.load(f.bytes);for(const[i,name]of names.entries())equal(vm.readInt(image.base+0x2180n+BigInt(i*f.ptr),f.ptr),vm.bind('oleaut32.dll',name));equal((await vm.run(100n)).reason,Exit.Returned);const size=mode===32?16:24;for(let i=0;i<32;i++)equal(vm.readInt(image.base+0x3000n+BigInt(i),1),i<size?0n:0xa5n);assert.ok(vm.retired>0n);vm.dispose();
}
console.log(`${checks} VARIANT scalar/deep-copy/BYREF/rollback/heap/ABI/fault checks passed`);
