import assert from 'node:assert/strict';
import {WasmMachine,Exit} from '../web/runtime.mjs';
import {core,stateLayout,encode} from './x87-harness.mjs';
import {moduleFixture} from './module-fixture.mjs';
import {fixture} from './pe-fixture.mjs';

let checks=0;const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
const snapshot=test=>new Uint8Array(core.memory.buffer,core.wire_state(test.handle),stateLayout[0]).slice();
const read=(vm,address,length)=>Uint8Array.from({length},(_,n)=>Number(vm.readInt(address+BigInt(n),1)));
const bytes=text=>new TextEncoder().encode(text+'\0');
const wide=text=>Uint8Array.from([...text.split('').flatMap(c=>[c.charCodeAt(0)&255,c.charCodeAt(0)>>8]),0,0]);
async function machine(mode,options={}){
 let handle;const vm=new WasmMachine({...core,wire_create(...args){handle=core.wire_create(...args);return handle;}},{mode,clock(){throw new Error('module/context services must not sample time');}});
 const leaf=moduleFixture(mode,{dll:true,tls:true,seed:73}),main=moduleFixture(mode,{dependency:'leaf.dll',tls:true}),bridge=moduleFixture(mode,{dll:true,entry:false,forward:'leaf.#17'});
 // Both exports are available; only the data forwarder is unused by the main IAT.
 main.put(main.dirs+12,60);main.rput(0x2014,0x2120);main.rput(0x2020,0x2100);main.rput(0x2024,0x2140);main.rtext(0x2100,'bridge.dll\0');main.rput(0x2120,0x20e0,mode/8);
 vm.supplyModule('leaf.dll',leaf.bytes);vm.supplyModule('bridge.dll',bridge.bytes);const image=vm.load(main.bytes,{command:'"C:\\app\\試験.exe" --run',...options});equal((await vm.run(1000n)).reason,Exit.Returned);vm.map(0x20000n,0x4000n);
 return {vm,image,mode,handle,leaf:vm.moduleBase('leaf.dll'),bridge:vm.moduleBase('bridge.dll'),sp:image.sp-128n,error:image.teb+BigInt(mode===32?0x34:0x68)};
}
function frame(test,name,args=[]){
 const {vm,mode,sp}=test;vm.ip=vm.bind('kernel32.dll',name);vm.register(4,sp);vm.write(sp,encode(vm.entryPoints.returned,mode/8));
 for(let n=0;n<args.length;n++){if(mode===32)vm.write(sp+4n+4n*BigInt(n),encode(args[n],4));else if(n<4)vm.register([1,2,8,9][n],args[n]);else vm.write(sp+40n+8n*BigInt(n-4),encode(args[n],8));}
}
function call(test,name,args=[]){
 const before=test.vm.work,retired=test.vm.retired;frame(test,name,args);equal(test.vm.service().reason,Exit.Dispatch);equal(test.vm.work,before+1n);equal(test.vm.retired,retired);equal(test.vm.ip,test.vm.entryPoints.returned);equal(test.vm.register(4),test.sp+BigInt(test.mode/8+(test.mode===32?args.length*4:0)));return test.vm.register(0);
}

for(const mode of [32,64]){
 const test=await machine(mode),{vm,image,leaf,bridge,error}=test,source=0x20000n,out=0x21000n;
 equal(call(test,'GetModuleHandleA',[0n]),image.base);equal(call(test,'GetModuleHandleW',[0n]),image.base);
 for(const [text,expected] of [['LEAF',leaf],['bridge.DLL',bridge],['試験.exe',image.base],['C:\\app\\試験.exe',image.base],['C:/app/試験.exe',image.base]]){
  vm.write(source,wide(text));equal(call(test,'GetModuleHandleW',[source]),expected);
 }
 for(const name of ['KERNEL32','kernelbase.dll','ntdll.dll']){
  if(name==='kernelbase.dll'&&mode===32)continue;
  if(name==='kernelbase.dll')vm.bind(name,'GetTickCount');
  vm.write(source,bytes(name));const handle=call(test,'GetModuleHandleA',[source]);assert.ok(handle);equal(handle%65536n,0n);
  vm.write(source,bytes(`C:\\Windows\\System32\\${name.includes('.')?name:name+'.dll'}`));equal(call(test,'GetModuleHandleA',[source]),handle);
  equal(call(test,'GetModuleFileNameW',[handle,out,260n]),BigInt(`C:\\Windows\\System32\\${name.toLowerCase().includes('.')?name.toLowerCase():name.toLowerCase()+'.dll'}`.length));
 }
 call(test,'SetLastError',[0xabcdef01n]);equal(vm.readInt(error),0xabcdef01n);equal(call(test,'GetLastError'),0xabcdef01n);
 for(const [symbol,expected] of [['GetValue',leaf+0x1080n],['Data',leaf+0x3000n]]){
  vm.write(source,bytes(symbol));equal(call(test,'GetProcAddress',[leaf,source]),expected);equal(call(test,'GetLastError'),0xabcdef01n);
 }
 equal(call(test,'GetProcAddress',[leaf,17n]),leaf+0x1080n);equal(call(test,'GetProcAddress',[leaf,18n]),leaf+0x3000n);equal(vm.readInt(leaf+0x3000n),73n);
 vm.write(source,bytes('GetValue'));equal(call(test,'GetProcAddress',[bridge,source]),leaf+0x1080n,'forwarded dynamic lookup uses the published export graph');
 vm.write(source,bytes('kernel32'));const kernel=call(test,'GetModuleHandleA',[source]);vm.write(source,bytes('GetTickCount64'));equal(call(test,'GetProcAddress',[kernel,source]),vm.bind('kernel32.dll','GetTickCount64'));vm.write(source,bytes('gettickcount64'));equal(call(test,'GetProcAddress',[kernel,source]),0n);equal(call(test,'GetLastError'),127n);
 for(const [dll,symbol,code] of [[leaf,19n,127n],[leaf+1n,0xffffffffff00n,6n],[leaf,source,127n],[kernel,17n,127n]]){equal(call(test,'GetProcAddress',[dll,symbol]),0n);equal(call(test,'GetLastError'),code);}
 for(const [name,expected] of [['GetCurrentProcessId',1n],['GetCurrentThreadId',1n],['GetCurrentProcess',BigInt.asUintN(mode,-1n)],['GetCurrentThread',BigInt.asUintN(mode,-2n)],['IsDebuggerPresent',0n]])equal(call(test,name),expected);
 vm.write(image.peb+2n,[0x80]);equal(call(test,'IsDebuggerPresent'),1n);vm.write(image.peb+2n,[0]);
 vm.write(source,bytes('missing.dll'));equal(call(test,'GetModuleHandleA',[source]),0n);equal(call(test,'GetLastError'),126n);
 vm.write(source,wide('C:\\other\\試験.exe'));equal(call(test,'GetModuleHandleW',[source]),0n);equal(call(test,'GetLastError'),126n);
 for(const [name,expected] of [['GetModuleFileNameA',Uint8Array.from([67,58,92,97,112,112,92,0x8e,0x8e,0x8c,0xb1,46,101,120,101,0])],['GetModuleFileNameW',wide('C:\\app\\試験.exe')]]){
  call(test,'SetLastError',[91n]);const width=name.endsWith('W')?2:1,length=expected.length/width-1;
  equal(call(test,name,[0n,out,260n]),BigInt(length));equal(read(vm,out,expected.length),expected);equal(call(test,'GetLastError'),91n);
  vm.write(out,new Uint8Array(expected.length+4).fill(0xa5));equal(call(test,name,[0n,out,4n]),4n);equal(read(vm,out,4*width),Uint8Array.from([...expected.slice(0,3*width),...new Uint8Array(width)]));equal(vm.readInt(out+BigInt(4*width),1),0xa5n);equal(call(test,'GetLastError'),122n);
  equal(call(test,name,[0n,0xffffffffff00n,0n]),0n);equal(call(test,'GetLastError'),122n);
 }
 equal(call(test,'GetModuleFileNameA',[leaf,out,12n]),11n);equal(read(vm,out,12),bytes('C:\\leaf.dll'));
 equal(call(test,'GetModuleFileNameA',[leaf+1n,out,20n]),0n);equal(call(test,'GetLastError'),6n);
 const flags=0xed7n;vm.flags=flags;vm.xmm(0,new Uint8Array(16).fill(0x97));const xmm=vm.xmm(0);equal(call(test,'GetCurrentProcessId'),1n);equal(vm.flags,flags);equal(vm.xmm(0),xmm);

 // Return and arguments are captured before a filename output overwrites them.
 frame(test,'GetModuleFileNameA',[leaf,test.sp,32n]);equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);equal(read(vm,test.sp,12),bytes('C:\\leaf.dll'));

 // A cross-page output fault must preserve its prefix, CPU and LastError.
 const cross=out+4093n;vm.write(cross,new Uint8Array(16).fill(0xa5));frame(test,'GetModuleFileNameW',[0n,cross,4n]);equal(core.wire_protect(test.handle,out+4096n,4096n,2),0);const before=snapshot(test),last=vm.readInt(error);equal(vm.service().reason,Exit.Fault);equal(snapshot(test),before);equal(vm.readInt(error),last);equal(read(vm,cross,3),new Uint8Array(3).fill(0xa5));equal(core.wire_protect(test.handle,out+4096n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(error),122n);

 // A failing LastError write also leaves the truncated output untouched.
 vm.write(out,new Uint8Array(8).fill(0xa5));frame(test,'GetModuleFileNameA',[leaf,out,4n]);equal(core.wire_protect(test.handle,image.teb,4096n,2),0);const state=snapshot(test);equal(vm.service().reason,Exit.Fault);equal(snapshot(test),state);equal(read(vm,out,8),new Uint8Array(8).fill(0xa5));equal(core.wire_protect(test.handle,image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);

 // Missing source/return reads preserve exact CPU progress and can be retried.
 frame(test,'GetProcAddress',[leaf,0x30000n]);const missing=snapshot(test);equal(vm.service().reason,Exit.Fault);equal(snapshot(test),missing);vm.map(0x30000n,4096n);vm.write(0x30000n,bytes('GetValue'));equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),leaf+0x1080n);
 frame(test,'SetLastError',[17n]);equal(core.wire_protect(test.handle,test.sp&~4095n,4096n,1),0);const unreadable=snapshot(test),prior=vm.readInt(error);equal(vm.service().reason,Exit.Fault);equal(snapshot(test),unreadable);equal(vm.readInt(error),prior);equal(core.wire_protect(test.handle,test.sp&~4095n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(error),17n);

 const code=0x23000n;equal(core.wire_protect(test.handle,code,4096n,0x40),0);const getModule=vm.bind('kernel32.dll','GetModuleHandleA'),getProc=vm.bind('kernel32.dll','GetProcAddress');vm.write(source,bytes('leaf.dll'));vm.write(source+64n,bytes('GetValue'));
 const program=mode===32?[0x68,...encode(source,4),0xb8,...encode(getModule,4),0xff,0xd0,0x68,...encode(source+64n,4),0x50,0xb8,...encode(getProc,4),0xff,0xd0,0xff,0xd0,0xc3]:[0x48,0x83,0xec,40,0x48,0xb9,...encode(source,8),0x48,0xb8,...encode(getModule,8),0xff,0xd0,0x48,0x89,0xc1,0x48,0xba,...encode(source+64n,8),0x48,0xb8,...encode(getProc,8),0xff,0xd0,0xff,0xd0,0x48,0x83,0xc4,40,0xc3];vm.write(code,program);
 for(const budget of [1n,1000n]){
  vm.ip=code;vm.register(4,test.sp);vm.write(test.sp,encode(vm.entryPoints.returned,mode/8));const before=vm.work,retired=vm.retired;let work=0n;
  for(let n=0;n<1000;n++){const result=await vm.run(budget);work+=result.work;if(result.reason===Exit.Returned)break;equal(result.reason,Exit.Budget);assert.ok(n<999);}
  equal(vm.register(0),73n,'ordinary JIT code queries a module, resolves its export and calls it');equal(vm.work-before,work);equal(work,vm.retired-retired+2n,'both successful query services consume work without synthetic retirement');
 }
 // Writing a filename over observed code revokes its previous JIT module.
 vm.write(code,[0xcc]);vm.ip=code;equal((await vm.run(1n)).reason,Exit.Unsupported);const ticket=core.wire_ticket(test.handle);frame(test,'GetModuleFileNameA',[leaf,code,32n]);equal(vm.service().reason,Exit.Dispatch);equal(core.wire_live(test.handle,ticket),0);
 vm.dispose();
}

for(const mode of [32,64]){
 const vm=new WasmMachine(core,{mode});vm.map(0x10000n,4096n);vm.register(4,0x10000n);vm.write(0x10000n,encode(vm.entryPoints.returned,mode/8));vm.ip=vm.bind('kernel32.dll','GetModuleHandleA');assert.throws(()=>vm.service(),/loaded image/);equal(vm.work,0n);vm.dispose();
 let handle;const blocked=new WasmMachine({...core,wire_create(...args){handle=core.wire_create(...args);return handle;}},{mode});const region=mode===32?0x71000000n:0x7ffe20000000n,first=blocked.bind('kernel32.dll','GetTickCount');blocked.map(region,4096n);blocked.write(region,[0x73]);blocked.register(0,0xabcdefn);const before=snapshot({handle});
 assert.throws(()=>blocked.load(fixture(mode,{tls:false}).bytes),/overlaps/);equal(blocked.image,null);equal(snapshot({handle}),before);equal(blocked.readInt(region,1),0x73n);equal(blocked.bind('kernel32.dll','GetTickCount64'),first+16n,'failed load does not publish its staged QPC binding');blocked.dispose();
}
console.log(`${checks} module lookup/context/LastError service checks passed`);
