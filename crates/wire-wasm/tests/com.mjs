import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {moduleFixture} from './module-fixture.mjs';
const api=(t,name,args=[])=>call(t,name,args,'ole32.dll');
const ro=(t,name,args=[])=>call(t,name,args,'api-ms-win-core-winrt-l1-1-0.dll');
const kernel=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const text=(vm,p,s)=>vm.write(p,Buffer.from(s+'\0','utf16le'));
const wrong=0x80010106n,invalid=0x80070057n;

// COM and WinRT initialization share the same thread reference count and mode.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.write(t.error,encode(123n,4));vm.flags=0x8d5n;vm.xmm(7,new Uint8Array(16).fill(0xa5));const fpu=fp(t);
 equal(api(t,'CoInitialize',[0n]),0n);equal(api(t,'CoInitializeEx',[0n,2n]),1n);equal(ro(t,'RoInitialize',[0n]),1n);
 equal(api(t,'CoInitializeEx',[0n,0n]),wrong);equal(ro(t,'RoInitialize',[1n]),wrong);
 api(t,'CoUninitialize');api(t,'CoUninitialize');equal(api(t,'CoInitializeEx',[0n,0n]),wrong);ro(t,'RoUninitialize');
 equal(api(t,'CoInitializeEx',[0n,0n]),0n);ro(t,'RoUninitialize');equal(api(t,'CoInitialize',[0n]),0n);
 for(const flags of [4n,8n,12n])equal(api(t,'CoInitializeEx',[0n,flags]),wrong);
 for(const flags of [6n,10n,14n])equal(api(t,'CoInitializeEx',[0n,flags]),1n);
 for(const flags of [1n,3n,16n,0xffffffffn])equal(api(t,'CoInitializeEx',[0n,flags]),invalid);
 for(const reserved of [1n,...(mode===64?[0x100000000n]:[])])equal(api(t,'CoInitialize',[reserved]),invalid);
 equal(ro(t,'RoInitialize',[2n]),invalid);
 for(let n=0;n<4;n++)api(t,'CoUninitialize');equal(ro(t,'RoInitialize',[1n]),0n);ro(t,'RoUninitialize');api(t,'CoUninitialize');
 if(mode===64){equal(api(t,'CoInitializeEx',[0n,0xabcdef0000000002n]),0n);api(t,'CoUninitialize');}
 equal(vm.flags,0x8d5n);equal(vm.xmm(7),new Uint8Array(16).fill(0xa5));equal(fp(t),fpu);equal(vm.readInt(t.error),123n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Task storage is process-owned and separate from public Heap/Local/Global allocations.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.write(t.error,encode(123n,4));
 for(const size of [0n,1n,17n,65537n]){
  const pointer=api(t,'CoTaskMemAlloc',[size]);assert.ok(pointer);equal(pointer%16n,0n);
  if(size){vm.write(pointer,[0x5a]);vm.write(pointer+size-1n,[0x73]);equal(vm.readInt(pointer+size-1n,1),0x73n);}
  equal(api(t,'CoTaskMemFree',[pointer]),0n);
  frame(t,'CoTaskMemFree',[pointer],'ole32.dll');const before=snapshot(t);assert.throws(()=>vm.service(),/invalid COM task/);equal(snapshot(t),before);
 }
 equal(api(t,'CoTaskMemFree',[0n]),0n);equal(api(t,'CoTaskMemAlloc',[0x10000001n]),0n);
 if(mode===64)equal(api(t,'CoTaskMemAlloc',[0x100000010n]),0n);
 equal(vm.readInt(t.error),123n);
 const heap=kernel(t,'GetProcessHeap'),pointer=kernel(t,'HeapAlloc',[heap,0n,16n]);frame(t,'CoTaskMemFree',[pointer],'ole32.dll');const before=snapshot(t);assert.throws(()=>vm.service(),/invalid COM task/);equal(snapshot(t),before);equal(kernel(t,'HeapSize',[heap,0n,pointer]),16n);equal(kernel(t,'HeapFree',[heap,0n,pointer]),1n);
 const task=api(t,'CoTaskMemAlloc',[16n]),other=await machine(mode);vm.write(task,[0x5a]);frame(other,'CoTaskMemFree',[task],'ole32.dll');assert.throws(()=>other.vm.service(),/invalid COM task/);equal(vm.readInt(task,1),0x5an);other.vm.dispose();api(t,'CoTaskMemFree',[task]);vm.dispose();
}

// No class factories are registered: activation returns a real HRESULT and a null interface.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,args=[0x20000n,0n,1n,0x21000n,0x23000n];vm.write(0x20000n,new Uint8Array(16).fill(1));vm.write(0x21000n,new Uint8Array(16).fill(2));vm.write(t.error,encode(123n,4));
 vm.write(0x23000n,new Uint8Array(8).fill(0xa5));equal(api(t,'CoCreateInstance',args),0x800401f0n);equal(vm.readInt(0x23000n,mode/8),0n);
 equal(api(t,'CoInitialize',[0n]),0n);equal(api(t,'CoCreateInstance',args),0x80040154n);equal(vm.readInt(0x23000n,mode/8),0n);
 for(const [index,value,result] of [[4,0n,0x80004003n],[0,0n,invalid],[3,0n,invalid],[2,0n,invalid]]){const a=args.slice();a[index]=value;equal(api(t,'CoCreateInstance',a),result);}
 equal(api(t,'CoCreateInstance',[...args.slice(0,4),t.sp]),0x80040154n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp,mode/8),0n);equal(vm.readInt(t.error),123n);vm.dispose();
}

// Faulted frames do not alter references or release storage; all activation outputs preflight.
for(const mode of [32,64])for(const kind of ['initialize','uninitialize','free','activation']){
 const t=await machine(mode),{vm}=t;let page=t.sp&~4095n,protection=1,pointer;
 if(kind==='initialize')frame(t,'CoInitialize',[0n],'ole32.dll');
 if(kind==='uninitialize'){api(t,'CoInitialize',[0n]);frame(t,'CoUninitialize',[],'ole32.dll');}
 if(kind==='free'){pointer=api(t,'CoTaskMemAlloc',[16n]);vm.write(pointer,[0x5a]);frame(t,'CoTaskMemFree',[pointer],'ole32.dll');}
 if(kind==='activation'){vm.write(0x20000n,new Uint8Array(16));vm.write(0x24ffcn,new Uint8Array(8).fill(0xa5));frame(t,'CoCreateInstance',[0x20000n,0n,1n,0x20000n,0x24ffen],'ole32.dll');page=0x25000n;protection=2;}
 equal(core.wire_protect(t.handle,page,4096n,protection),0);const before=snapshot(t),work=vm.work;equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.work,work);
 equal(core.wire_protect(t.handle,page,4096n,4),0);
 if(kind==='initialize')equal(api(t,'CoInitializeEx',[0n,0n]),0n);
 if(kind==='uninitialize')equal(api(t,'CoInitializeEx',[0n,0n]),wrong);
 if(kind==='free'){equal(vm.readInt(pointer,1),0x5an);api(t,'CoTaskMemFree',[pointer]);}
 if(kind==='activation'){equal(bytes(vm,0x24ffcn,4),new Uint8Array(4).fill(0xa5));equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0x800401f0n);}
 vm.dispose();
}

// Static startup rejection releases private task slabs with the other process storage.
for(const mode of [32,64]){
 const vm=new WasmMachine(core,{mode}),bad=moduleFixture(mode,{dll:true,attach:false,tls:false});vm.supplyModule('bad.dll',bad.bytes);
 const image=vm.load(moduleFixture(mode,{dependency:'bad.dll',tls:false}).bytes),t={vm,mode,image,sp:image.sp-2048n};
 const ip=vm.ip,regs=Array.from({length:mode===32?8:16},(_,i)=>vm.register(i)),pointer=api(t,'CoTaskMemAlloc',[70000n]);assert.ok(pointer);vm.ip=ip;regs.forEach((v,i)=>vm.register(i,v));
 await assert.rejects(vm.run(10000n),/attach returned FALSE/);assert.throws(()=>vm.readInt(pointer),/memory access/);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 for(const name of ['CoInitializeEx','CoUninitialize','CoTaskMemAlloc','CoTaskMemFree','CoCreateInstance']){const address=vm.bind('ole32.dll',name);equal(vm.bind('combase.dll',name),address);for(const dll of ['kernel32.dll','kernelbase.dll','api-ms-win-core-winrt-l1-1-0.dll'])assert.throws(()=>vm.bind(dll,name),/unknown/);}
 equal(vm.bind('ole32.dll','CoInitialize')>0n,true);assert.throws(()=>vm.bind('combase.dll','CoInitialize'),/unknown/);
 for(const name of ['RoInitialize','RoUninitialize']){const address=vm.bind('combase.dll',name);equal(vm.bind('api-ms-win-core-winrt-l1-1-0.dll',name),address);assert.throws(()=>vm.bind('ole32.dll',name),/unknown/);}
 for(const name of ['OleInitialize','OleUninitialize'])assert.throws(()=>vm.bind('ole32.dll',name),/not been migrated/);
 text(vm,0x20000n,'ole32.dll');const module=kernel(t,'LoadLibraryW',[0x20000n]);vm.write(0x20000n,new TextEncoder().encode('CoTaskMemAlloc\0'));equal(kernel(t,'GetProcAddress',[module,0x20000n]),vm.bind('ole32.dll','CoTaskMemAlloc'));vm.dispose();
}
console.log(`${checks} COM/WinRT references/task ownership/activation failure/ABI/fault checks passed`);
