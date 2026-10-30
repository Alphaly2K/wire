import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';
const api=(t,name,args=[])=>call(t,name,args,'ole32.dll');
const source=0x20000n,data=0x24000n,out=0x23000n;
const value=Uint8Array.from([0x33,0x22,0x11,0,0x55,0x44,0x77,0x66,0x88,0x99,0xaa,0xbb,0xcc,0xdd,0xee,0xff]);
const canonical='{00112233-4455-6677-8899-AABBCCDDEEFF}';
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const text=(vm,p,s)=>vm.write(p,Buffer.from(s+'\0','utf16le'));
const seed=Uint8Array.from({length:16},(_,n)=>n),generated=Uint8Array.from(seed);generated[7]|=0x40;generated[8]|=0x80;

// Canonical mixed-endian GUIDs round-trip through both caller buffers and task allocations.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.write(source,value);vm.write(t.error,encode(123n,4));vm.flags=0x8d5n;vm.xmm(7,new Uint8Array(16).fill(0xa5));const fpu=fp(t);
 vm.write(data,new Uint8Array(84).fill(0xa5));equal(api(t,'StringFromGUID2',[source,data,39n]),39n);equal(bytes(vm,data,80),Uint8Array.from([...Buffer.from(canonical+'\0','utf16le'),0xa5,0xa5]));
 for(const name of ['CLSIDFromString','IIDFromString'])for(const s of [canonical,canonical.toLowerCase()]){text(vm,data,s);equal(api(t,name,[data,out]),0n);equal(bytes(vm,out,16),value);}
 for(const name of ['CLSIDFromString','IIDFromString']){equal(api(t,name,[0n,out]),0n);equal(bytes(vm,out,16),new Uint8Array(16));}
 for(const name of ['StringFromCLSID','StringFromIID']){equal(api(t,name,[source,t.sp]),0n);equal(vm.ip,vm.entryPoints.returned);const p=vm.readInt(t.sp,mode/8);equal(bytes(vm,p,78),Uint8Array.from(Buffer.from(canonical+'\0','utf16le')));api(t,'CoTaskMemFree',[p]);}
 for(const count of [0n,38n,0xffffffffn,0x80000000n]){vm.write(data,new Uint8Array(80).fill(0xa5));equal(api(t,'StringFromGUID2',[0xdead0000n,data,count]),0n);equal(bytes(vm,data,80),new Uint8Array(80).fill(0xa5));}
 equal(api(t,'StringFromGUID2',[0n,0xdead0000n,39n]),0n);equal(vm.flags,0x8d5n);equal(vm.xmm(7),new Uint8Array(16).fill(0xa5));equal(fp(t),fpu);equal(vm.readInt(t.error),123n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Malformed text returns HRESULTs with deterministic outputs and no invented ProgID registration.
for(const mode of [32,64])for(const s of ['',canonical.slice(0,-1),'not-a-class','X'+canonical.slice(1),canonical.slice(0,8)+'G'+canonical.slice(9),canonical.slice(0,9)+'_'+canonical.slice(10),canonical.slice(0,4)+'\ud800'+canonical.slice(5)]){
 const t=await machine(mode),{vm}=t;text(vm,source,s);vm.write(t.error,encode(123n,4));
 vm.write(out,new Uint8Array(16).fill(0xa5));equal(api(t,'IIDFromString',[source,out]),s.length===38?0x800401f4n:0x80070057n);equal(bytes(vm,out,16),new Uint8Array(16).fill(0xa5));
 equal(api(t,'CLSIDFromString',[source,out]),0x800401f3n);equal(bytes(vm,out,16),s.startsWith('{')?new Uint8Array(16).fill(0xa5):new Uint8Array(16));equal(vm.readInt(t.error),123n);vm.dispose();
}

// Injected random bytes preserve the v4/variant encoding; provider failures preserve guest state.
for(const mode of [32,64]){
 let calls=0,answer=seed;const t=await machine(mode,{random(length){calls++;equal(length,16);if(answer instanceof Error)throw answer;return answer;}}),{vm}=t;
 vm.write(t.error,encode(123n,4));frame(t,'CoCreateGuid',[data],'ole32.dll');equal(core.wire_service_clock(t.handle),1);equal(core.wire_service_source(t.handle),257);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0n);equal(bytes(vm,data,16),generated);equal(calls,1);
 answer=null;vm.write(data,new Uint8Array(16).fill(0xa5));equal(api(t,'CoCreateGuid',[data]),0x800706b9n);equal(bytes(vm,data,16),new Uint8Array(16).fill(0xa5));equal(calls,2);
 for(const invalid of [new Uint8Array(15),Promise.resolve(seed)]){
  answer=invalid;frame(t,'CoCreateGuid',[data],'ole32.dll');const before=snapshot(t);assert.throws(()=>vm.service(),/random byte function/);equal(snapshot(t),before);equal(bytes(vm,data,16),new Uint8Array(16).fill(0xa5));
 }
 answer=new Error('random provider unavailable');frame(t,'CoCreateGuid',[data],'ole32.dll');const before=snapshot(t);assert.throws(()=>vm.service(),/random provider unavailable/);equal(snapshot(t),before);
 answer=seed;equal(vm.service().reason,Exit.Dispatch);equal(bytes(vm,data,16),generated);equal(api(t,'CoCreateGuid',[0n]),0x80070057n);
 const count=calls;frame(t,'CoCreateGuid',[0n],'ole32.dll');equal(core.wire_service_source(t.handle),1);equal(vm.service().reason,Exit.Dispatch);equal(calls,count);
 equal(vm.readInt(t.error),123n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Fault observation never consumes guards or entropy; frames and full outputs remain retryable.
for(const mode of [32,64])for(const kind of ['random','return','text','allocated','input']){
 let calls=0;const t=await machine(mode,{random(){calls++;return seed;}}),{vm}=t;vm.write(source,value);vm.write(0x24ffcn,new Uint8Array(84).fill(0xa5));vm.write(out,new Uint8Array(8).fill(0xa5));
 let page=0x25000n,protection=2;
 if(kind==='random')frame(t,'CoCreateGuid',[0x24ffcn],'ole32.dll');
 if(kind==='return'){frame(t,'CoCreateGuid',[data],'ole32.dll');page=t.sp&~4095n;protection=1;}
 if(kind==='text')frame(t,'StringFromGUID2',[source,0x24ffcn,39n],'ole32.dll');
 if(kind==='allocated')frame(t,'StringFromIID',[source,0x24ffen],'ole32.dll');
 if(kind==='input'){frame(t,'StringFromCLSID',[source,out],'ole32.dll');page=source;protection=1;}
 equal(core.wire_protect(t.handle,page,4096n,protection),0);const before=snapshot(t),prefix=bytes(vm,0x24ffcn,4);equal(core.wire_service_source(t.handle)&0x100,0);
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(calls,0);equal(bytes(vm,0x24ffcn,4),prefix);equal(bytes(vm,out,8),new Uint8Array(8).fill(0xa5));
 equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),kind==='text'?39n:0n);equal(calls,kind==='random'||kind==='return'?1:0);vm.dispose();
}
for(const mode of [32,64]){
 let calls=0;const t=await machine(mode,{random(){calls++;return seed;}}),{vm}=t;frame(t,'CoCreateGuid',[data],'ole32.dll');equal(core.wire_protect(t.handle,data,4096n,0x104),0);const before=snapshot(t);
 equal(core.wire_service_source(t.handle),1);equal(core.wire_service_source(t.handle),1);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(calls,0);equal(vm.service().reason,Exit.Dispatch);equal(calls,1);
 frame(t,'CoCreateGuid',[t.sp],'ole32.dll');equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);equal(bytes(vm,t.sp,16),generated);
 const pointer=api(t,'CoTaskMemAlloc',[78n]);api(t,'CoTaskMemFree',[pointer]);vm.write(source,value);vm.write(out,new Uint8Array(8).fill(0xa5));frame(t,'StringFromCLSID',[source,out],'ole32.dll');const page=pointer&~4095n;equal(core.wire_protect(t.handle,page,4096n,2),0);const saved=snapshot(t);
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),saved);equal(bytes(vm,out,8),new Uint8Array(8).fill(0xa5));equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(out,mode/8),pointer);api(t,'CoTaskMemFree',[pointer]);
 if(mode===64){const high=0x123400000000n;vm.map(high,8192n,3);vm.write(high,value);equal(api(t,'StringFromIID',[high,high+4096n]),0n);const p=vm.readInt(high+4096n,8);equal(bytes(vm,p,78),Uint8Array.from(Buffer.from(canonical+'\0','utf16le')));api(t,'CoTaskMemFree',[p]);equal(api(t,'CoCreateGuid',[high]),0n);equal(bytes(vm,high,16),generated);}
 for(const name of ['StringFromGUID2','CLSIDFromString','IIDFromString','CoCreateGuid','StringFromCLSID','StringFromIID']){const address=vm.bind('ole32.dll',name);equal(vm.bind('combase.dll',name),address);for(const dll of ['kernel32.dll','kernelbase.dll','api-ms-win-core-winrt-l1-1-0.dll'])assert.throws(()=>vm.bind(dll,name),/unknown/);}
 vm.dispose();
}
console.log(`${checks} GUID layout/text/task/entropy/source/ABI/alias/fault checks passed`);
