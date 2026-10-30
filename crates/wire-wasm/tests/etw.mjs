import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame as ordinaryFrame} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';
import {fixture} from './pe-fixture.mjs';
import {WasmMachine} from '../web/runtime.mjs';

const names=['EventRegister','EventUnregister','EventEnabled','EventProviderEnabled','EventWrite','EventWriteString','EventWriteTransfer','EventSetInformation'];
const guid=0x22000n,out=0x23000n,data=0x24000n;
// REGHANDLE and keyword occupy two DWORD slots in the PE32 ABI.
function frame(t,name,args){
 if(t.mode===64){ordinaryFrame(t,name,args,'advapi32.dll');return;}
 const wide=name==='EventRegister'?[]:['EventProviderEnabled','EventWriteString'].includes(name)?[0,2]:[0];
 t.vm.ip=t.vm.bind('advapi32.dll',name);t.vm.register(4,t.sp);t.vm.write(t.sp,encode(t.vm.entryPoints.returned,4));
 let offset=4;for(let n=0;n<args.length;n++){const width=wide.includes(n)?8:4;t.vm.write(t.sp+BigInt(offset),encode(args[n],width));offset+=width;}
 t.popped=BigInt(offset);
}
function api(t,name,args){
 frame(t,name,args);const w=t.vm.work,r=t.vm.retired;equal(t.vm.service().reason,Exit.Dispatch);equal(t.vm.work,w+1n);equal(t.vm.retired,r);
 equal(t.vm.register(4),t.sp+(t.mode===32?t.popped:8n));equal(t.vm.ip,t.vm.entryPoints.returned);return t.vm.register(0);
}
function fault(t,name,args){
 frame(t,name,args);const before=snapshot(t),w=t.vm.work,r=t.vm.retired;equal(t.vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(t.vm.work,w);equal(t.vm.retired,r);
}
function register(t,output=out,source=guid){equal(api(t,'EventRegister',[source,1n,~0n,output]),0n);return t.vm.readInt(output,8);}

// Shared legacy admission is ABI-only on PE32; strict Wasm loading must bind all eight.
for(const mode of [32,64]){
 const f=fixture(mode,{tls:false});f.bytes.fill(0,f.offset(0x2080),f.offset(0x2600));f.rtext(0x20c0,'advapi32.dll\0');f.rput(0x2000,0x2100);f.rput(0x2010,0x2180);f.bytes[f.offset(0x1000)]=0xc3;
 for(const [n,name] of names.entries()){const entry=0x2200+n*64;f.rput(0x2100+n*f.ptr,entry,f.ptr);f.rput(0x2180+n*f.ptr,entry,f.ptr);f.rtext(entry+2,name+'\0');}
 const vm=new WasmMachine(core,{mode});const image=vm.load(f.bytes);
 for(const [n,name] of names.entries())equal(vm.readInt(image.base+0x2180n+BigInt(n*f.ptr),f.ptr),vm.bind('advapi32.dll',name));
 equal((await vm.run(100n)).reason,Exit.Returned);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.write(guid,new Uint8Array(16).fill(0x42));vm.write(data,[...encode(1n,8),...encode(0xffffffffn,8)]);
 for(const name of names){assert.ok(vm.bind('advapi32.dll',name));for(const dll of ['kernel32.dll','kernelbase.dll','ntdll.dll'])assert.throws(()=>vm.bind(dll,name),/unknown/);}
 vm.flags=0x8d5n;vm.xmm(3,new Uint8Array(16).fill(0x7b));const fpu=fp(t);vm.write(t.error,encode(123n,4));
 const handle=register(t);equal(handle,0x7fff6000n);
 for(const [name,args,result] of [
  ['EventEnabled',[handle,guid],0n],['EventProviderEnabled',[handle,0x105n,~0n],0n],
  ['EventWrite',[handle,guid,0n,1n],0n],['EventWrite',[handle,guid,1n,data],0n],
  ['EventWrite',[handle,guid,129n,1n],87n],['EventWriteTransfer',[handle,guid,1n,1n,1n,data],0n],
  ['EventWriteString',[handle,0x105n,~0n,1n],0n],['EventSetInformation',[handle,0xffffffffn,1n,0xffffffffn],50n],
  ['EventEnabled',[handle|0x100000000n,1n],0n],['EventProviderEnabled',[handle|0x100000000n,1n,~0n],0n],
  ['EventWrite',[handle|0x100000000n,1n,129n,1n],6n],['EventWriteTransfer',[handle|0x100000000n,1n,1n,1n,129n,1n],6n],
  ['EventWriteString',[handle|0x100000000n,1n,~0n,1n],6n],['EventSetInformation',[handle|0x100000000n,1n,1n,1n],6n],
 ])equal(api(t,name,args),result);
 if(mode===64){equal(api(t,'EventWrite',[handle,guid,0x100000001n,data]),0n);const high=0x123400000000n;vm.map(high,4096n,3);vm.write(high,new Uint8Array(16));const h=register(t,high+16n,high);equal(api(t,'EventUnregister',[h]),0n);}
 for(const [name,args] of [['EventEnabled',[handle,1n]],['EventWrite',[handle,1n,129n,1n]],['EventWrite',[handle,guid,1n,1n]],['EventWriteTransfer',[handle,guid,1n,1n,1n,1n]]])fault(t,name,args);
 equal(vm.readInt(t.error),123n);equal(vm.flags,0x8d5n);equal(vm.xmm(3),new Uint8Array(16).fill(0x7b));equal(fp(t),fpu);equal(vm.readInt(data,8),1n);
 const other=await machine(mode);equal(api(other,'EventUnregister',[handle]),6n);other.vm.dispose();
 equal(api(t,'EventUnregister',[handle]),0n);equal(api(t,'EventUnregister',[handle]),6n);equal(api(t,'EventEnabled',[handle,1n]),0n);equal(api(t,'EventWrite',[handle,1n,1n,1n]),6n);
 const alias=register(t,t.sp);equal(vm.ip,vm.entryPoints.returned);equal(api(t,'EventUnregister',[alias]),0n);
 const overlap=register(t,guid,guid);equal(api(t,'EventUnregister',[overlap]),0n);
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 // Source guard precedes output guard; output failure does not allocate a handle.
 vm.write(guid,new Uint8Array(16).fill(0x42));vm.write(out,encode(0xfeedn,8));
 equal(core.wire_protect(t.handle,guid,4096n,0x104),0);equal(core.wire_protect(t.handle,out,4096n,0x104),0);
 fault(t,'EventRegister',[guid,1n,2n,out]);equal(vm.readInt(guid),0x42424242n);assert.throws(()=>vm.readInt(out),/memory access failed/);equal(vm.readInt(out,8),0xfeedn);
 vm.write(0x24ffen,new Uint8Array(8).fill(0xa5));equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);
 fault(t,'EventRegister',[guid,1n,2n,0x24ffen]);equal(vm.readInt(0x24ffen,2),0xa5a5n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);
 equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(0x24ffen,8),0x7fff6000n);
 // A complete array is required, even though payloads are not followed.
 equal(core.wire_protect(t.handle,0x25000n,4096n,1),0);fault(t,'EventWrite',[0x7fff6000n,guid,1n,0x24ff8n]);
 equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0n);
 vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.write(guid,new Uint8Array(16));const handles=Array.from({length:64},()=>register(t));
 vm.write(out,encode(0xfeedn,8));equal(api(t,'EventRegister',[guid,1n,2n,out]),8n);equal(vm.readInt(out,8),0xfeedn);
 fault(t,'EventRegister',[1n,1n,2n,out]);fault(t,'EventRegister',[guid,1n,2n,1n]);
 equal(api(t,'EventUnregister',[handles[0]]),0n);equal(register(t),0x7fff6040n);equal(api(t,'EventUnregister',[handles[0]]),6n);vm.dispose();
}
console.log(`${checks} ETW shared-provider/ABI/lifetime/disabled-event/fault checks passed`);
