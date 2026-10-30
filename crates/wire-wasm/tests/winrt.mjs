import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';
import {fixture} from './pe-fixture.mjs';
import {moduleFixture} from './module-fixture.mjs';
import {WasmMachine} from '../web/runtime.mjs';
const names=['WindowsCreateString','WindowsCreateStringReference','WindowsDeleteString','WindowsDuplicateString','WindowsGetStringLen','WindowsIsStringEmpty','WindowsGetStringRawBuffer','WindowsStringHasEmbeddedNull','WindowsCompareStringOrdinal','RoGetActivationFactory','RoActivateInstance'];
const api=(t,name,args=[])=>call(t,name,args,'combase.dll');
const source=0x22000n,header=0x23000n,out=0x24000n;
const raw=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
function text(vm,p,units){vm.write(p,[...units.flatMap(u=>[u&255,u>>8]),0,0]);}
function create(t,units){text(t.vm,source,units);equal(api(t,names[0],[source,BigInt(units.length),out]),0n);return t.vm.readInt(out,t.mode/8);}
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,width=mode/8,offset=mode===32?12:16;
 for(const name of names){assert.ok(vm.bind('combase.dll',name));assert.throws(()=>vm.bind('ole32.dll',name),/unknown/);}
 vm.flags=0x8d5n;vm.xmm(3,new Uint8Array(16).fill(0x7b));const fpu=fp(t);vm.write(t.error,encode(123n,4));
 const units=[0x41,0,0xd800,0xdc00],owned=create(t,units);assert.ok(owned);equal(api(t,names[4],[owned]),4n);equal(api(t,names[5],[owned]),0n);const pointer=api(t,names[6],[owned,out+8n]);equal(vm.readInt(out+8n),4n);equal(raw(vm,pointer,10),Uint8Array.from([...units.flatMap(u=>[u&255,u>>8]),0,0]));assert.throws(()=>vm.write(pointer,[0]),/Protection/);
 equal(api(t,names[7],[owned,out+8n]),0n);equal(vm.readInt(out+8n),1n);equal(api(t,names[3],[owned,out]),0n);equal(vm.readInt(out,width),owned);equal(api(t,names[2],[owned]),0n);equal(api(t,names[4],[owned]),4n);
 text(vm,source,[0x41,0,0xd800,0xdc00]);vm.write(header,new Uint8Array(24).fill(0xa5));equal(api(t,names[1],[source,4n,header,out]),0n);equal(vm.readInt(out,width),header);equal(vm.readInt(header+8n),0xa5a5a5a5n);equal(vm.readInt(header+BigInt(offset),width),source);
 equal(api(t,names[8],[owned,header,out+8n]),0n);equal(vm.readInt(out+8n),0n);equal(api(t,names[3],[header,out]),0n);const copy=vm.readInt(out,width);assert.notEqual(copy,header);assert.notEqual(copy,owned);
 text(vm,source,[0x42,0,0xd800,0xdc00]);equal(api(t,names[8],[owned,header,out+8n]),0n);equal(vm.readInt(out+8n),0xffffffffn);equal(api(t,names[8],[header,owned,out+8n]),0n);equal(vm.readInt(out+8n),1n);equal(api(t,names[8],[copy,owned,out+8n]),0n);equal(vm.readInt(out+8n),0n);
 vm.write(header+4n,encode(1n,4));vm.write(header+BigInt(offset),encode(source+2n,width));equal(api(t,names[4],[header]),1n);equal(api(t,names[6],[header,0n]),source+2n);equal(api(t,names[2],[header]),0n);equal(vm.readInt(header),1n);
 equal(api(t,names[2],[copy]),0n);equal(api(t,names[2],[owned]),0n);assert.throws(()=>vm.readInt(owned),/memory access/);frame(t,names[4],[owned],'combase.dll');equal(vm.service().reason,Exit.Fault);
 equal(api(t,names[0],[0n,0n,out]),0n);equal(vm.readInt(out,width),0n);equal(api(t,names[3],[0n,out]),0n);equal(vm.readInt(out,width),0n);equal(api(t,names[4],[0n]),0n);equal(api(t,names[5],[0n]),1n);const empty=api(t,names[6],[0n,out]);assert.ok(empty);equal(vm.readInt(empty,2),0n);equal(vm.readInt(out),0n);equal(api(t,names[6],[0n,0n]),empty);equal(api(t,names[2],[0n]),0n);
 for(const [name,args,result] of [[names[0],[0n,1n,out],0x80004003n],[names[0],[source,0x400001n,out],0x8007000en],[names[0],[source,1n,0n],0x80070057n],[names[1],[0n,0n,0n,out],0x80070057n],[names[3],[1n,0n],0x80070057n],[names[7],[1n,0n],0x80070057n],[names[8],[1n,1n,0n],0x80070057n]]){vm.write(out,encode(0xfeedn,width));equal(api(t,name,args),result);equal(vm.readInt(out,width),0xfeedn);}
 const className=create(t,[0x57,0x69,0x72,0x65]);vm.write(out,encode(0xfeedn,width));equal(api(t,names[9],[className,header,out]),0x80040154n);equal(api(t,names[10],[className,out]),0x80040154n);equal(vm.readInt(out,width),0xfeedn);equal(api(t,names[2],[className]),0n);
 text(vm,source,[65]);equal(api(t,names[0],[source,1n,t.sp]),0n);const alias=vm.readInt(t.sp,width);equal(vm.ip,vm.entryPoints.returned);equal(api(t,names[2],[alias]),0n);
 if(mode===64){const high=0x123400000000n;vm.map(high,8192n,3);text(vm,high,[0x65e5]);equal(api(t,names[1],[high,1n,high+32n,high+64n]),0n);equal(api(t,names[6],[high+32n,0n]),high);equal(api(t,names[4],[high+32n]),1n);}
 equal(vm.readInt(t.error),123n);equal(vm.flags,0x8d5n);equal(vm.xmm(3),new Uint8Array(16).fill(0x7b));equal(fp(t),fpu);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}
// Failed copyout must neither increment a reference nor publish a new allocation/header.
for(const mode of [32,64])for(const kind of ['create','reference','duplicate']){
 const t=await machine(mode),{vm}=t,width=mode/8,owned=create(t,[65,66]),destination=0x24ffen;
 vm.write(destination,new Uint8Array(32).fill(0xa5));vm.write(header,new Uint8Array(24).fill(0xa5));const args=kind==='create'?[source,2n,destination]:kind==='reference'?[source,2n,header,destination]:[owned,destination],name=kind==='create'?names[0]:kind==='reference'?names[1]:names[3];frame(t,name,args,'combase.dll');const before=snapshot(t);
 equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(destination,2),0xa5a5n);equal(raw(vm,header,24),new Uint8Array(24).fill(0xa5));equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);const result=vm.readInt(destination,width);equal(api(t,names[2],[result]),0n);if(kind!=='duplicate')equal(api(t,names[2],[owned]),0n);else {equal(api(t,names[2],[owned]),0n);assert.throws(()=>vm.readInt(owned),/memory access/);}vm.dispose();
}
for(const mode of [32,64]){
 const f=fixture(mode,{tls:false});f.bytes.fill(0,f.offset(0x2080),f.offset(0x2600));f.rtext(0x20c0,'combase.dll\0');f.rput(0x2000,0x2100);f.rput(0x2010,0x2180);f.bytes[f.offset(0x1000)]=0xc3;
 for(const [n,name] of names.entries()){const entry=0x2200+n*64;f.rput(0x2100+n*f.ptr,entry,f.ptr);f.rput(0x2180+n*f.ptr,entry,f.ptr);f.rtext(entry+2,name+'\0');}
 const vm=new WasmMachine(core,{mode}),image=vm.load(f.bytes);for(const[n,name]of names.entries())equal(vm.readInt(image.base+0x2180n+BigInt(n*f.ptr),f.ptr),vm.bind('combase.dll',name));equal((await vm.run(100n)).reason,Exit.Returned);vm.dispose();
}
// Failed process attachment must release both string payloads and security tables.
for(const mode of [32,64]){
 const vm=new WasmMachine(core,{mode}),width=mode/8,scratch=0x20000n;
 vm.map(scratch,4096n,3);text(vm,scratch+0x100n,[65]);
 const string=vm.bind('combase.dll',names[0]),security=vm.bind('secur32.dll','InitSecurityInterfaceW');
 const f=moduleFixture(mode,{dll:true,attach:false});
 const code=mode===32?[
  0x68,...encode(scratch,4),0x6a,1,0x68,...encode(scratch+0x100n,4),0xb8,...encode(string,4),0xff,0xd0,
  0xb8,...encode(security,4),0xff,0xd0,0xa3,...encode(scratch+4n,4),0x31,0xc0,0xc2,12,0
 ]:[
  0x48,0x83,0xec,40,0x48,0xb9,...encode(scratch+0x100n,8),0xba,1,0,0,0,0x49,0xb8,...encode(scratch,8),0x48,0xb8,...encode(string,8),0xff,0xd0,
  0x48,0xb8,...encode(security,8),0xff,0xd0,0x48,0xa3,...encode(scratch+8n,8),0x48,0x83,0xc4,40,0x31,0xc0,0xc3
 ];
 f.bytes.set(code,f.offset(0x1000));f.relocations.splice(0,f.relocations.length,...f.relocations.filter(rva=>rva>=0x1080));f.rebuildRelocations();
 vm.supplyModule('bad.dll',f.bytes);vm.load(moduleFixture(mode,{dependency:'bad.dll'}).bytes);
 await assert.rejects(vm.run(1000n),/attach returned FALSE/);
 for(const slot of [scratch,scratch+BigInt(width)]){const p=vm.readInt(slot,width);assert.ok(p);assert.throws(()=>vm.readInt(p),/memory access/);}
 equal(vm.moduleBase('bad.dll'),0n);equal(vm.stats(),{modules:0,chargedBytes:0,instances:0});vm.dispose();
}
console.log(`${checks} HSTRING/ABI/reference/immutability/activation/fault checks passed`);
