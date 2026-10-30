import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';
import {fixture} from './pe-fixture.mjs';
import {moduleFixture} from './module-fixture.mjs';
import {WasmMachine} from '../web/runtime.mjs';
const names=['SysAllocString','SysReAllocString','SysAllocStringLen','SysReAllocStringLen','SysFreeString','SysStringLen','SysStringByteLen','SysAllocStringByteLen'];
const api=(t,name,args=[])=>call(t,name,args,'oleaut32.dll');
const source=0x22000n,out=0x24000n;
const text=(vm,p,units)=>vm.write(p,[...units.flatMap(u=>[u&255,u>>8]),0,0]);
const bytes=(vm,p,n)=>Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,width=mode/8,accepted=names.slice(0,mode===32?7:8);
 for(const name of accepted){assert.ok(vm.bind('oleaut32.dll',name));for(const dll of ['combase.dll','ole32.dll','kernel32.dll','user32.dll'])assert.throws(()=>vm.bind(dll,name),/unknown/);}
 if(mode===32){assert.throws(()=>vm.bind('oleaut32.dll',names[7]),/unknown/);assert.throws(()=>vm.bind('oleaut32.dll','#150'),/unknown/);}for(const [i,ordinal]of [2,3,4,5,6,7,149,150].slice(0,accepted.length).entries())equal(vm.bind('oleaut32.dll','#'+ordinal),vm.bind('oleaut32.dll',names[i]));
 vm.flags=0x8d5n;vm.xmm(3,new Uint8Array(16).fill(0x7b));const fpu=fp(t);vm.write(t.error,encode(123n,4));
 for(const name of [names[0],names[4],names[5],names[6]])equal(api(t,name,[0n]),0n);
 const empty=api(t,names[2],[0n,0n]);assert.ok(empty);equal(vm.readInt(empty-4n),0n);equal(vm.readInt(empty,2),0n);equal(api(t,names[4],[empty]),0n);
 text(vm,source,[65,0,0xd800,0xdfff]);let p=api(t,names[2],[source,0xabcdef0000000004n]);assert.ok(p);equal(api(t,names[5],[p]),4n);equal(api(t,names[6],[p]),8n);equal(bytes(vm,p,10),[65,0,0,0,0,0xd8,0xff,0xdf,0,0]);
 const processHeap=call(t,'GetProcessHeap',[],'kernel32.dll');if(mode===32){equal(call(t,'HeapSize',[processHeap,0n,p-4n],'kernel32.dll'),14n);equal(call(t,'HeapValidate',[processHeap,0n,p-4n],'kernel32.dll'),1n);}else{equal(call(t,'HeapValidate',[processHeap,0n,p-4n],'kernel32.dll'),0n);}
 vm.write(out,encode(p,width));equal(api(t,names[3],[out,p+2n,2n]),1n);let next=vm.readInt(out,width);assert.notEqual(next,p);equal(bytes(vm,next,6),[0,0,0,0xd8,0,0]);
 if(mode===32)equal(call(t,'HeapValidate',[processHeap,0n,p-4n],'kernel32.dll'),0n);else assert.throws(()=>vm.readInt(p),/memory access/);
 equal(api(t,names[3],[out,0n,4n]),1n);p=vm.readInt(out,width);equal(bytes(vm,p,10),[0,0,0,0xd8,0,0,0,0,0,0]);
 equal(api(t,names[1],[out,0n]),1n);equal(vm.readInt(out,width),0n);
 const terminated=api(t,names[0],[source]);equal(api(t,names[5],[terminated]),1n);equal(api(t,names[4],[terminated]),0n);
 vm.write(out,encode(5n,4));equal(api(t,names[5],[out+4n]),2n);equal(api(t,names[6],[out+4n]),5n);
 vm.write(t.error,encode(123n,4));equal(api(t,names[2],[1n,0x80000000n]),0n);equal(vm.readInt(t.error),mode===32?8n:123n);
 if(mode===64){vm.write(source,[97,98,99]);p=api(t,names[7],[source,0x123400000003n]);equal(api(t,names[6],[p]),3n);equal(api(t,names[5],[p]),1n);equal(bytes(vm,p,5),[97,98,99,0,0]);equal(api(t,names[4],[p]),0n);const high=0x123400000000n;vm.map(high,4096n,3);text(vm,high,[0x65e5,0xd800]);p=api(t,names[2],[high,2n]);equal(vm.readInt(p),0xd80065e5n);equal(api(t,names[4],[p]),0n);}
 // A stale or foreign handle must not be accepted for release/replacement.
 p=api(t,names[2],[0n,3n]);equal(api(t,names[4],[p]),0n);if(mode===32){equal(api(t,names[4],[p]),0n);equal(vm.readInt(t.error),87n);}else{frame(t,names[4],[p],'oleaut32.dll');const before=snapshot(t);assert.throws(()=>vm.service(),/invalid or released BSTR/);equal(snapshot(t),before);}
 vm.write(t.error,encode(123n,4));text(vm,source,[65,66,67]);p=api(t,names[2],[source,3n]);vm.write(0x24ffen,encode(p,width));frame(t,names[3],[0x24ffen,source,2n],'oleaut32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(p-4n),6n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);next=vm.readInt(0x24ffen,width);equal(vm.readInt(next-4n),4n);equal(api(t,names[4],[next]),0n);
 p=api(t,names[2],[source,4n]);vm.write(p,encode(p,width));frame(t,names[3],[p,source,1n],'oleaut32.dll');const overlap=snapshot(t);assert.throws(()=>vm.service(),/output overlaps allocation/);equal(snapshot(t),overlap);equal(vm.readInt(p,width),p);equal(api(t,names[4],[p]),0n);
 // Saved return frames permit copyout over the caller's return address.
 p=api(t,names[2],[source,2n]);vm.write(t.sp,encode(p,width));vm.ip=vm.bind('oleaut32.dll',names[3]);vm.register(4,t.sp);if(mode===32){for(const [i,v]of [t.sp,source,1n].entries())vm.write(t.sp+4n+BigInt(i*4),encode(v,4));}else{vm.register(1,t.sp);vm.register(2,source);vm.register(8,1n);}equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,p);next=vm.readInt(t.sp,width);equal(api(t,names[4],[next]),0n);
 equal(vm.readInt(t.error),123n);equal(vm.flags,0x8d5n);equal(vm.xmm(3),new Uint8Array(16).fill(0x7b));equal(fp(t),fpu);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}
for(const mode of [32,64]){
 const f=fixture(mode,{tls:false}),accepted=names.slice(0,mode===32?7:8);f.bytes.fill(0,f.offset(0x2080),f.offset(0x2600));f.rtext(0x20c0,'oleaut32.dll\0');f.rput(0x2000,0x2100);f.rput(0x2010,0x2180);f.bytes[f.offset(0x1000)]=0xc3;
 for(const [n,name]of accepted.entries()){const entry=0x2200+n*64;f.rput(0x2100+n*f.ptr,entry,f.ptr);f.rput(0x2180+n*f.ptr,entry,f.ptr);f.rtext(entry+2,name+'\0');}
 f.rput(0x2100,(1n<<BigInt(mode-1))|2n,f.ptr);f.rput(0x2180,(1n<<BigInt(mode-1))|2n,f.ptr);
 const vm=new WasmMachine(core,{mode}),image=vm.load(f.bytes);for(const[n,name]of accepted.entries())equal(vm.readInt(image.base+0x2180n+BigInt(n*f.ptr),f.ptr),vm.bind('oleaut32.dll',name));equal((await vm.run(100n)).reason,Exit.Returned);vm.dispose();
}
for(const mode of [32,64]){
 const vm=new WasmMachine(core,{mode}),scratch=0x20000n,target=vm.bind('oleaut32.dll','SysAllocStringLen');vm.map(scratch,4096n,3);
 const f=moduleFixture(mode,{dll:true,attach:false});const code=mode===32?[
  0x6a,4,0x6a,0,0xb8,...encode(target,4),0xff,0xd0,0xa3,...encode(scratch,4),0x31,0xc0,0xc2,12,0
 ]:[
  0x48,0x83,0xec,40,0x31,0xc9,0xba,4,0,0,0,0x48,0xb8,...encode(target,8),0xff,0xd0,0x48,0xa3,...encode(scratch,8),0x48,0x83,0xc4,40,0x31,0xc0,0xc3
 ];f.bytes.set(code,f.offset(0x1000));f.relocations.splice(0,f.relocations.length,...f.relocations.filter(rva=>rva>=0x1080));f.rebuildRelocations();vm.supplyModule('bad.dll',f.bytes);vm.load(moduleFixture(mode,{dependency:'bad.dll'}).bytes);
 await assert.rejects(vm.run(1000n),/attach returned FALSE/);const p=vm.readInt(scratch,mode/8);assert.ok(p);assert.throws(()=>vm.readInt(p-4n),/memory access/);equal(vm.stats(),{modules:0,chargedBytes:0,instances:0});vm.dispose();
}
console.log(`${checks} BSTR ownership/ABI/heap isolation/alias/ordinal/fault checks passed`);
