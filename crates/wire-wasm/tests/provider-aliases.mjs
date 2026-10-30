import assert from 'node:assert/strict';
import {WasmMachine,Exit} from '../web/runtime.mjs';
import {core,stateLayout,encode} from './x87-harness.mjs';
import {fixture} from './pe-fixture.mjs';
import {moduleFixture} from './module-fixture.mjs';
import {delayFixture} from './delay-fixture.mjs';

let checks=0;const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
const proc='api-ms-win-core-processthreads-l1-1-0.dll',heapDll='api-ms-win-core-heap-l1-1-0.dll',fileDll='api-ms-win-core-file-l1-1-0.dll',loader='api-ms-win-core-libraryloader-l1-2-0.dll',errors='api-ms-win-core-errorhandling-l1-1-0.dll',com='api-ms-win-core-com-l1-1-0.dll';
const groups=[[proc,['GetCurrentProcessId'],'kernel32.dll'],[heapDll,['GetProcessHeap','HeapAlloc','HeapSize','HeapFree'],'kernel32.dll'],[fileDll,['CreateFileW','ReadFile','GetFileSizeEx'],'kernel32.dll'],[loader,['GetModuleHandleW','GetProcAddress','LoadLibraryExW','FreeLibrary'],'kernel32.dll'],[errors,['GetLastError','SetLastError'],'kernel32.dll'],['sspicli.dll',['FreeContextBuffer'],'secur32.dll'],[com,['CoInitializeEx','CoUninitialize'],'ole32.dll']];
const wide=s=>Buffer.from(s+'\0','utf16le'),text=s=>Buffer.from(s+'\0');
const snapshot=t=>new Uint8Array(core.memory.buffer,core.wire_state(t.handle),stateLayout[0]).slice();

function main(mode){
 const f=fixture(mode,{tls:false}),slots=[];f.bytes.fill(0,f.offset(0x2000),f.offset(0x2600));let at=0x2000+(groups.length+1)*20;
 f.put(f.dirs+8,0x2000);f.put(f.dirs+12,(groups.length+1)*20);
 for(const[index,[dll,names,canonical]]of groups.entries()){
  const descriptor=0x2000+index*20;f.rput(descriptor+12,at);f.rtext(at,dll+'\0');at+=dll.length+1;at=Math.ceil(at/f.ptr)*f.ptr;
  const ilt=at;at+=(names.length+1)*f.ptr;const iat=at;at+=(names.length+1)*f.ptr;f.rput(descriptor,ilt);f.rput(descriptor+16,iat);
  for(const[n,name]of names.entries()){f.rput(ilt+n*f.ptr,at,f.ptr);f.rput(iat+n*f.ptr,at,f.ptr);f.rtext(at+2,name+'\0');at+=name.length+3;slots.push({dll,name,canonical,rva:iat+n*f.ptr});}
 }
 assert.ok(at<0x2600);
 const iat=BigInt(slots[0].rva);
 if(mode===32){f.bytes.set([0xff,0x15,...encode(f.preferred+iat,4),0xc3],f.offset(0x1000));f.relocations.splice(0,f.relocations.length,0x1002);}else{f.bytes.set([0x48,0x83,0xec,40,0xff,0x15,...encode(iat-0x100an,4),0x48,0x83,0xc4,40,0xc3],f.offset(0x1000));f.relocations.length=0;}
 f.rebuildRelocations();return {f,slots};
}
function frame(t,dll,name,args=[]){const {vm,sp,mode}=t;vm.ip=vm.bind(dll,name);vm.register(4,sp);vm.write(sp,encode(vm.entryPoints.returned,mode/8));for(const[n,arg]of args.entries())if(mode===32)vm.write(sp+4n+BigInt(n*4),encode(arg,4));else if(n<4)vm.register([1,2,8,9][n],arg);else vm.write(sp+40n+BigInt((n-4)*8),encode(arg,8));}
function call(t,dll,name,args=[]){const work=t.vm.work,retired=t.vm.retired;frame(t,dll,name,args);equal(t.vm.service().reason,Exit.Dispatch);equal(t.vm.work,work+1n);equal(t.vm.retired,retired);return t.vm.register(0);}
async function invoke(t,dll,name,args=[]){frame(t,dll,name,args);for(let n=0;n<1000;n++){const result=await t.vm.run(1n);if(result.reason===Exit.Returned)return t.vm.register(0);equal(result.reason,Exit.Budget);}throw Error('provider request did not finish');}

for(const mode of [32,64]){
 let handle;const vm=new WasmMachine({...core,wire_create(...a){handle=core.wire_create(...a);return handle;}},{mode,clock(){throw Error('unexpected clock');}}),{f,slots}=main(mode);vm.supplyFile('C:/data.bin',[11,12,13,14]);const image=vm.load(f.bytes),t={vm,handle,mode,image,sp:image.sp-256n,error:image.teb+BigInt(mode===32?0x34:0x68)};
 equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),1n);assert.ok(vm.retired>0n);vm.map(0x20000n,0x4000n,3);
 const securitySlot=slots.find(slot=>slot.dll==='sspicli.dll'),securityAddress=vm.readInt(image.base+BigInt(securitySlot.rva),mode/8);vm.write(0x20000n,wide('sspicli.dll'));const securityModule=call(t,loader,'GetModuleHandleW',[0x20000n]);equal(call(t,'kernel32.dll','GetModuleHandleExW',[6n,securityAddress,0x20100n]),1n);equal(vm.readInt(0x20100n,mode/8),securityModule,'an alias gateway has an owner before its canonical DLL is registered');
 for(const{dll,name,canonical,rva}of slots){const gateway=vm.readInt(image.base+BigInt(rva),mode/8);equal(gateway,vm.bind(canonical,name));equal(vm.bind(dll.toUpperCase(),name),gateway);}
 vm.write(0x20000n,wide('secur32.dll'));const canonicalSecurity=call(t,loader,'GetModuleHandleW',[0x20000n]);assert.notEqual(canonicalSecurity,securityModule);equal(call(t,'kernel32.dll','GetModuleHandleExW',[6n,securityAddress,0x20100n]),1n);equal(vm.readInt(0x20100n,mode/8),canonicalSecurity);
 const heap=call(t,heapDll,'GetProcessHeap'),allocated=call(t,heapDll,'HeapAlloc',[heap,8n,32n]);assert.ok(allocated);equal(vm.readInt(allocated,8),0n);equal(call(t,'kernel32.dll','HeapSize',[heap,0n,allocated]),32n);equal(call(t,'kernel32.dll','HeapFree',[heap,0n,allocated]),1n);
 call(t,errors,'SetLastError',[543n]);equal(call(t,'kernel32.dll','GetLastError'),543n);call(t,'kernel32.dll','SetLastError',[765n]);equal(call(t,errors,'GetLastError'),765n);
 vm.write(0x20000n,wide('C:/data.bin'));const file=call(t,fileDll,'CreateFileW',[0x20000n,0x80000000n,7n,0n,3n,0n,0n]);assert.notEqual(file,BigInt.asUintN(mode,-1n));equal(call(t,fileDll,'GetFileSizeEx',[file,0x20100n]),1n);equal(vm.readInt(0x20100n,8),4n);
 frame(t,fileDll,'ReadFile',[file,0x21ffen,3n,0x20100n,0n]);equal(core.wire_protect(handle,0x22000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(handle,0x22000n,4096n,4),0);equal(call(t,'kernel32.dll','ReadFile',[file,0x21000n,2n,0x20100n,0n]),1n);equal(vm.readInt(0x21000n,2),0x0c0bn);equal(call(t,fileDll,'ReadFile',[file,0x21000n,2n,0x20100n,0n]),1n);equal(vm.readInt(0x21000n,2),0x0e0dn);
 equal(call(t,'sspicli.dll','FreeContextBuffer',[0n]),0n);equal(call(t,com,'CoInitializeEx',[0n,0n]),0n);equal(call(t,'ole32.dll','CoInitializeEx',[0n,0n]),1n);call(t,com,'CoUninitialize');call(t,'ole32.dll','CoUninitialize');
 // A fresh dynamic lookup must work before that specific service has been bound.
 vm.write(0x20000n,wide(heapDll));const module=await invoke(t,loader,'LoadLibraryExW',[0x20000n,0n,0n]);assert.ok(module);vm.write(0x20080n,text('HeapValidate'));const validate=call(t,loader,'GetProcAddress',[module,0x20080n]);assert.ok(validate);equal(validate,vm.bind('kernel32.dll','HeapValidate'));
 vm.write(0x20080n,text('ReadFile'));equal(call(t,loader,'GetProcAddress',[module,0x20080n]),0n);equal(vm.readInt(t.error),127n);equal(call(t,loader,'GetProcAddress',[module,8n]),0n);equal(vm.readInt(t.error),127n);
 vm.write(0x20000n,wide(heapDll));equal(call(t,loader,'GetModuleHandleW',[0x20000n]),module);vm.write(0x20000n,wide('kernel32.dll'));assert.notEqual(call(t,loader,'GetModuleHandleW',[0x20000n]),module);equal(await invoke(t,loader,'FreeLibrary',[module]),1n);
 for(const[dll,name]of [[heapDll,'ReadFile'],[fileDll,'HeapAlloc'],[heapDll,'heapalloc'],[loader,'LoadLibraryW'],['api-ms-win-core-file-l1-999-0.dll','ReadFile']])assert.throws(()=>vm.bind(dll,name),/unknown|migrated/);
 equal(call(t,'kernel32.dll','CloseHandle',[file]),1n);vm.dispose();
}

// Forwarders and delay descriptors retain the provider identity and charge only the actual API.
for(const mode of [32,64]){
 const vm=new WasmMachine(core,{mode,clock(){throw Error('unexpected clock');}});vm.supplyModule('forward.dll',moduleFixture(mode,{dll:true,entry:false,forward:proc.slice(0,-4)+'.GetCurrentProcessId'}).bytes);vm.load(moduleFixture(mode,{dependency:'forward.dll'}).bytes);equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),1n);vm.dispose();
 const lazy=new WasmMachine(core,{mode,clock(){throw Error('unexpected clock');}}),image=lazy.load(delayFixture(mode,{library:proc,symbols:['GetCurrentProcessId']}).bytes);equal((await lazy.run(100n)).reason,Exit.Returned);const thunk=lazy.readInt(image.base+0x3120n,mode/8),sp=image.sp-128n;lazy.ip=thunk;lazy.register(4,sp);lazy.write(sp,encode(lazy.entryPoints.returned,mode/8));const retired=lazy.retired,work=lazy.work;equal((await lazy.run(100n)).reason,Exit.Returned);equal(lazy.register(0),1n);equal(lazy.retired,retired);equal(lazy.work,work+1n);equal(lazy.readInt(image.base+0x3120n,mode/8),lazy.bind('kernel32.dll','GetCurrentProcessId'));assert.ok(lazy.readInt(image.base+0x3100n,mode/8));lazy.dispose();
}
console.log(`${checks} provider aliases/static/dynamic/forward/delay/shared-state checks passed`);
