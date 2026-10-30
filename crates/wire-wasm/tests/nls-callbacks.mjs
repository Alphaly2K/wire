import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,finish} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';
import {dynamicFixture} from './dynamic-fixture.mjs';
const api=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const dump=0x23100n;
const text=(vm,p,wide=true)=>{let s='';for(let i=0;i<80;i++){const c=Number(vm.readInt(p+BigInt(i*(wide?2:1)),wide?2:1));if(!c)return s;s+=String.fromCharCode(c);}throw new Error('unterminated locale');};
const bytes=(vm,p,n)=>Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
function assembler(t,base=0x30000n){
 const {mode,vm}=t,b=[];const emit=(...x)=>b.push(...x),value=(x,n)=>emit(...encode(BigInt.asUintN(n*8,BigInt(x)),n));
 const store=(p,width=mode/8)=>{if(mode===32){emit(0xa3);value(p,4);}else{emit(0x49,0xbb);value(p,8);emit(width===8?0x49:0x41,0x89,0x03);}};
 const inc=p=>{if(mode===32){emit(0xff,0x05);value(p,4);}else{emit(0x49,0xbb);value(p,8);emit(0x41,0xff,0x03);}};
 const service=(name,args=[])=>{if(mode===32){for(const x of args.toReversed()){emit(0x68);value(x,4);}emit(0xb8);value(vm.bind('kernel32.dll',name),4);emit(0xff,0xd0);}else{emit(0x48,0x83,0xec,56);for(let n=0;n<args.length;n++)if(n<4){emit(...[[0x48,0xb9],[0x48,0xba],[0x49,0xb8],[0x49,0xb9]][n]);value(args[n],8);}else{emit(0x48,0xb8);value(args[n],8);emit(0x48,0x89,0x44,0x24,32+(n-4)*8);}emit(0x48,0xb8);value(vm.bind('kernel32.dll',name),8);emit(0xff,0xd0,0x48,0x83,0xc4,56);}};
 return{b,emit,value,store,inc,service,end(result=1,argc=3){emit(...(mode===32?[0xb8]:[0x48,0xb8]));value(result,mode/8);emit(...(mode===32?[0xc2,argc*4,0]:[0xc3]));vm.write(base,b);return base;}};
}
function callback(t,{extended=true,result=1,base=0x30000n,record=dump}={}){
 const {mode}=t,a=assembler(t,base);
 if(mode===32)a.emit(0x8b,0x44,0x24,4);else a.emit(0x48,0x89,0xc8);a.store(record);
 if(extended){if(mode===32)a.emit(0x8b,0x44,0x24,8);else a.emit(0x48,0x89,0xd0);a.store(record+8n,4);if(mode===32)a.emit(0x8b,0x44,0x24,12);else a.emit(0x4c,0x89,0xc0);a.store(record+16n);}
 a.inc(record+24n);a.emit(0x66,0x0f,0xef,0xdb);return a.end(result,extended?3:1);
}
async function records(t,name,args){
 const {vm}=t;vm.write(dump,new Uint8Array(48));frame(t,name,args,'kernel32.dll');const work=vm.work,retired=vm.retired;
 equal(vm.service().reason,Exit.Dispatch);equal(vm.work,work+1n);equal(vm.retired,retired);const seen=[];let count=0n;
 for(let n=0;n<500;n++){
  if(vm.ip===vm.entryPoints.callback&&vm.readInt(dump+24n)!==count){count=vm.readInt(dump+24n);seen.push([text(vm,vm.readInt(dump,t.mode/8),!name.endsWith('A')),vm.readInt(dump+8n),vm.readInt(dump+16n,t.mode/8)]);}
  const r=await vm.run(1n);if(r.reason===Exit.Returned){equal(vm.register(0),1n);equal(vm.work-work,vm.retired-retired+1n);return seen;}equal(r.reason,Exit.Budget);
 }
 throw new Error('locale enumeration did not return');
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,param=mode===32?0x87654321n:0xfedcba9876543210n,proc=callback(t);
 const all=[['en-US',0x21n,param],['ja-JP',0x21n,param],['en',0x11n,param],['ja',0x11n,param],['',0x21n,param]];
 vm.flags=0x8d5n;vm.mxcsr=0x5f81;vm.xmm(3,new Uint8Array(16).fill(0x79));const saved=fp(t);vm.write(t.error,encode(123n,4));
 for(const[flags,wanted]of [[0,all],[1,all],[0x10,all.slice(2,4)],[0x20,[all[0],all[1],all[4]]],[0x11,all],[0x30,all],[2,[]],[4,[]],[6,[]],[0x12,all.slice(2,4)]])equal(await records(t,'EnumSystemLocalesEx',[proc,BigInt(flags),param,0n]),wanted);
 for(const name of ['EnumSystemLocalesA','EnumSystemLocalesW']){
  const target=callback(t,{extended:false});for(const flags of [0,1,2,5,6])equal((await records(t,name,[target,BigInt(flags)])).map(x=>x[0]),['00000409','00000411']);equal(await records(t,name,[target,4n]),[]);
 }
 for(const name of ['EnumSystemLocalesA','EnumSystemLocalesW','EnumSystemLocalesEx']){
  for(const result of mode===64?[0n,0x100000000n]:[0n]){const target=callback(t,{extended:name.endsWith('Ex'),result});equal((await records(t,name,name.endsWith('Ex')?[target,0n,param,0n]:[target,0n])).length,1);}
  equal(vm.bind('kernelbase.dll',name),vm.bind('kernel32.dll',name));assert.throws(()=>vm.bind('user32.dll',name),/unknown/);
 }
 equal(vm.readInt(t.error),123n);equal(vm.flags,0x8d5n);equal(vm.mxcsr,0x5f81);equal(vm.xmm(3),new Uint8Array(16).fill(0x79));equal(fp(t),saved);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,proc=callback(t);
 for(const[name,args,error]of [
  ['EnumSystemLocalesEx',[0n,0n,0n,0n],87n],['EnumSystemLocalesEx',[proc,0n,0n,1n],87n],
  ['EnumSystemLocalesEx',[proc,8n,0n,0n],1004n],['EnumSystemLocalesA',[proc,3n],1004n],
  ['EnumSystemLocalesW',[proc,7n],1004n],['EnumSystemLocalesW',[proc,0x10n],1004n],
 ]){equal(api(t,name,args),0n);equal(vm.readInt(t.error),error);equal(vm.readInt(dump+24n),0n);}
 frame(t,'EnumSystemLocalesEx',[proc,8n,0n,0n],'kernel32.dll');equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(t.error),1004n);
 for(const page of [0x24000n,0x25000n]){
  t.sp=mode===32?0x25010n:0x25020n;vm.write(0x24fc0n,new Uint8Array(64).fill(0xa5));vm.write(0x25000n,new Uint8Array(32).fill(0xa5));frame(t,'EnumSystemLocalesEx',[proc,0n,0n,0n],'kernel32.dll');const string=mode===32?0x25000n:0x25010n,old=bytes(vm,string,12);equal(core.wire_protect(t.handle,page,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(vm,string,12),old);equal(vm.readInt(dump+24n),0n);equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);await finish(t,1n);equal(vm.readInt(dump+24n),5n);vm.write(dump+24n,encode(0n,4));
 }
 // A fault at the next callback preserves the already returned BOOL and cursor.
 frame(t,'EnumSystemLocalesEx',[proc,0n,0n,0n],'kernel32.dll');equal(vm.service().reason,Exit.Dispatch);
 for(let n=0;vm.ip!==vm.entryPoints.callback;n++){assert.ok(n<50);equal((await vm.run(1n)).reason,Exit.Budget);}
 equal(vm.readInt(dump+24n),1n);equal(core.wire_protect(t.handle,0x24000n,4096n,2),0);before=snapshot(t);equal((await vm.run(1n)).reason,Exit.Fault);equal(snapshot(t),before);vm.register(0,0n);equal(core.wire_protect(t.handle,0x24000n,4096n,4),0);await finish(t,1n);equal(vm.readInt(dump+24n),5n);
 // Unmapped callback code faults through the ordinary execution path.
 vm.write(dump+24n,encode(0n,4));frame(t,'EnumSystemLocalesEx',[0xdead0000n,0n,0n,0n],'kernel32.dll');equal(vm.service().reason,Exit.Dispatch);equal((await vm.run(1n)).reason,Exit.Fault);equal(vm.ip,0xdead0000n);vm.map(0xdead0000n,4096n,7);callback(t,{base:0xdead0000n});await finish(t,1n);equal(vm.readInt(dump+24n),5n);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,at=t.image.teb+BigInt(mode===32?0xc4:0x108),input=0x22000n,out=0x24000n;
 equal(vm.readInt(at),0x411n);equal(api(t,'GetThreadLocale'),0x411n);vm.write(t.error,encode(123n,4));
 for(const[id,wanted]of [[0x409,0x409],[9,0x409],[0x11,0x411],[0,0x411],[0x400,0x411],[0x800,0x411],[0x7f,0x7f],[0x411,0x411]]){equal(api(t,'SetThreadLocale',[BigInt(id)]),1n);equal(api(t,'GetThreadLocale'),BigInt(wanted));equal(vm.readInt(at),BigInt(wanted));}
 for(const id of [9,0x11,0x409,0x411,0x7f])for(const flags of [0,1,2,0x39])equal(api(t,'IsValidLocale',[BigInt(id),BigInt(flags)]),1n);
 for(const id of [0,0x400,0x800,0xdead,0xffffffff])equal(api(t,'IsValidLocale',[BigInt(id),2n]),0n);
 equal(api(t,'IsValidLocale',[0x409n,4n]),0n);equal(api(t,'IsValidLocaleName',[0n]),0n);equal(api(t,'IsValidLocaleName',[(1n<<BigInt(mode))-1n]),0n);equal(vm.readInt(t.error),123n);
 for(const[lcid,cp,decoded]of [[0x409n,1252n,0xa1n],[0x411n,932n,0xff61n]]){
  equal(api(t,'SetThreadLocale',[lcid]),1n);equal(api(t,'GetCPInfoExW',[3n,0n,out]),1n);equal(vm.readInt(out+20n),cp);vm.write(input,[0xa1]);equal(api(t,'MultiByteToWideChar',[3n,0n,input,1n,out,4n]),1n);equal(vm.readInt(out,2),decoded);
  vm.write(input,encode(0x20acn,2));equal(api(t,'WideCharToMultiByte',[3n,0n,input,1n,out,4n,0n,0n]),1n);equal(vm.readInt(out,1),lcid===0x409n?0x80n:63n);
  equal(api(t,'GetACP'),932n);equal(api(t,'GetUserDefaultLCID'),0x411n);
 }
 frame(t,'SetThreadLocale',[0x409n],'kernel32.dll');equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(at),0x411n);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(at),0x409n);
 equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);equal(api(t,'SetThreadLocale',[0x409n]),1n);equal(api(t,'GetThreadLocale'),0x409n);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);
 equal(api(t,'SetThreadLocale',[0xdeadn]),0n);equal(vm.readInt(t.error),87n);equal(vm.readInt(at),0x409n);
 vm.write(at,encode(0n,4));equal(api(t,'GetThreadLocale'),0x411n);equal(vm.readInt(at),0x411n);
 for(const name of ['SetThreadLocale','IsValidLocale'])equal(vm.bind('kernelbase.dll',name),vm.bind('kernel32.dll',name));equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Nested enumeration, yielding and exceptions share the same callback stack.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,inner=callback(t,{extended:false,base:0x31000n,record:dump+64n}),h=assembler(t,0x32000n);h.inc(dump+128n);h.end(0xffffffffn,1);api(t,'AddVectoredExceptionHandler',[1n,0x32000n]);
 const a=assembler(t);a.inc(dump+24n);a.service('EnumSystemLocalesW',[inner,0n]);a.service('SetThreadLocale',[0x409n]);a.service('Sleep',[0n]);a.service('RaiseException',[0xe0420042n,0n,0n,0n]);a.service('SetLastError',[73n]);a.end(0,3);
 frame(t,'EnumSystemLocalesEx',[0x30000n,0n,0n,0n],'kernel32.dll');equal(vm.service().reason,Exit.Dispatch);vm.write(t.sp,encode(0n,mode/8));await finish(t,1n);equal(vm.register(0),1n);equal(vm.readInt(dump+24n),1n);equal(vm.readInt(dump+88n),2n);equal(vm.readInt(dump+128n),1n);equal(vm.readInt(t.error),73n);equal(api(t,'GetThreadLocale'),0x409n);equal(t.time.calls,0);vm.dispose();
}

// A fresh worker uses the user locale and keeps subsequent changes private.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;equal(api(t,'SetThreadLocale',[0x409n]),1n);const a=assembler(t);a.service('GetThreadLocale');a.store(0x23200n,4);a.service('SetThreadLocale',[0x7fn]);a.service('Sleep',[0n]);a.service('GetThreadLocale');a.store(0x23204n,4);a.end(73,1);
 const h=api(t,'CreateThread',[0n,65536n,0x30000n,0n,0n,0n]);frame(t,'WaitForSingleObject',[h,0xffffffffn],'kernel32.dll');await finish(t,1n);equal(vm.readInt(0x23200n),0x411n);equal(vm.readInt(0x23204n),0x7fn);equal(api(t,'GetThreadLocale'),0x409n);equal(api(t,'CloseHandle',[h]),1n);equal(t.time.calls,0);vm.dispose();
}

// Enumeration within loader notifications resumes its own loader depth.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,proc=callback(t,{result:0}),f=dynamicFixture(mode,{entry:false}),init=assembler(t,0x34000n);
 init.service('EnumSystemLocalesEx',[proc,0n,0n,0n]);init.end(1,3);f.put(f.opt+16,0x1000);f.bytes.fill(0x90,f.offset(0x1000),f.offset(0x1400));f.bytes.set(init.b,f.offset(0x1000));f.relocations.splice(0,f.relocations.length,...f.relocations.filter(r=>r>=0x1400));f.rebuildRelocations();vm.supplyModule('locale-notify.dll',f.bytes);vm.write(0x22000n,new TextEncoder().encode('locale-notify.dll\0'));
 frame(t,'LoadLibraryA',[0x22000n],'kernel32.dll');await finish(t,1n);const module=vm.register(0);assert.ok(module);equal(vm.readInt(dump+24n),1n);frame(t,'FreeLibrary',[module],'kernel32.dll');await finish(t,1n);equal(vm.readInt(dump+24n),2n);equal(vm.moduleBase('locale-notify.dll'),0n);vm.dispose();
}

// Exiting inside a worker callback discards its queue before reclaiming the stack.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,c=assembler(t,0x31000n);c.service('ExitThread',[76n]);c.end(0);const w=assembler(t);w.service('EnumSystemLocalesEx',[0x31000n,0n,0n,0n]);w.inc(dump+24n);w.end(1,1);
 const h=api(t,'CreateThread',[0n,65536n,0x30000n,0n,0n,0n]);frame(t,'WaitForSingleObject',[h,0xffffffffn],'kernel32.dll');await finish(t,1n);equal(vm.readInt(dump+24n),0n);equal(api(t,'GetExitCodeThread',[h,0x24000n]),1n);equal(vm.readInt(0x24000n),76n);const proc=callback(t,{result:0});equal((await records(t,'EnumSystemLocalesEx',[proc,0n,0n,0n])).length,1);equal(api(t,'CloseHandle',[h]),1n);vm.dispose();
}

// Target unwind crosses the logical return bridge and discards the enumeration.
{
 const t=await machine(64),{vm}=t,body=0x32000n,target=body+0x80n;
 const a=assembler(t);a.inc(dump+24n);a.service('RtlUnwindEx',[t.sp-40n,target,0n,73n,0x26000n,0n]);a.end(0);
 vm.write(0x28000n,[...encode(0x30000n,4),...encode(0x30200n,4),...encode(0x28200n,4),...encode(body,4),...encode(body+0x100n,4),...encode(0x28100n,4)]);vm.write(0x28100n,[1,4,1,0,4,0x42,0,0]);
 // The callback's allocation precedes its unwind call after one counter update.
 vm.write(0x28200n,[1,17,1,0,17,0x62,0,0]);equal(api(t,'RtlAddFunctionTable',[0x28000n,2n,0n]),1n);
 vm.write(body,[0x48,0x83,0xec,40,0x48,0xb9,...encode(0x30000n,8),0x31,0xd2,0x45,0x31,0xc0,0x45,0x31,0xc9,0x48,0xb8,...encode(vm.bind('kernel32.dll','EnumSystemLocalesEx'),8),0xff,0xd0,0xcc]);vm.write(target,[0x48,0x83,0xc4,40,0xc3]);vm.ip=body;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,8));await finish(t,1n);equal(vm.register(0),73n);equal(vm.readInt(dump+24n),1n);
 const proc=callback(t,{result:0});equal((await records(t,'EnumSystemLocalesEx',[proc,0n,0n,0n])).length,1);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),a=assembler(t);a.service('EnumSystemLocalesEx',[0x30000n,0n,0n,0n]);a.end(0);frame(t,'EnumSystemLocalesEx',[0x30000n,0n,0n,0n],'kernel32.dll');await assert.rejects(()=>t.vm.run(10000n),/locale callback nesting capacity exceeded/);t.vm.dispose();
}
console.log(`${checks} locale enumeration/BOOL/stack transaction/continuation/unwind/thread locale/codepage checks passed`);
