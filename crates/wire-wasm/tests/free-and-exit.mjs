import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,finish} from './gdi-harness.mjs';
import {dynamicFixture,trace} from './dynamic-fixture.mjs';
const api=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
function worker(t){const pc=t.vm.bind('kernel32.dll','FreeLibraryAndExitThread');return t.mode===32?[0x6a,37,0x8b,0x44,0x24,8,0x50,0xb8,...encode(pc,4),0xff,0xd0,0xcc]:[0x48,0x83,0xec,40,0xba,37,0,0,0,0x48,0xb8,...encode(pc,8),0xff,0xd0,0xcc];}
for(const mode of [32,64])for(const kind of ['unload','retained','invalid','provider','faults']){
 const t=await machine(mode),{vm}=t,f=dynamicFixture(mode,{tls:true});f.rput(0x2540,0x1300);f.bytes.set(worker(t),f.offset(0x1300));
 vm.supplyModule('self-exit.dll',f.bytes);vm.write(0x20000n,new TextEncoder().encode('self-exit.dll\0'));frame(t,'LoadLibraryA',[0x20000n],'kernel32.dll');await finish(t);const base=vm.register(0);assert.ok(base);
 if(kind==='retained'){frame(t,'LoadLibraryA',[0x20000n],'kernel32.dll');await finish(t);equal(vm.register(0),base);}
 let parameter=base;if(kind==='invalid')parameter=0n;
 if(kind==='provider'){vm.write(0x20200n,new TextEncoder().encode('kernel32.dll\0'));parameter=api(t,'GetModuleHandleA',[0x20200n]);}
 const thread=api(t,'CreateThread',[0n,65536n,base+0x1300n,parameter,0n,0n]);frame(t,'WaitForSingleObject',[thread,0xffffffffn],'kernel32.dll');
 if(kind==='faults'){
  const gateway=vm.bind('kernel32.dll','FreeLibraryAndExitThread');
  for(let n=0;vm.ip!==gateway;n++){assert.ok(n<300);equal((await vm.run(1n)).reason,Exit.Budget);}
  const page=vm.register(4)&~4095n;equal(core.wire_protect(t.handle,page,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.moduleBase('self-exit.dll'),base);equal(vm.readInt(trace+20n),1n);
  equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);
  for(let n=0;!(vm.ip===vm.entryPoints.callback&&vm.readInt(trace+20n)===2n);n++){assert.ok(n<200);equal((await vm.run(1n)).reason,Exit.Budget);}
  const arrayPage=t.image.tlsArray&~4095n;equal(core.wire_protect(t.handle,arrayPage,4096n,2),0);equal((await vm.run(100n)).reason,Exit.Fault);equal(vm.readInt(trace+20n),2n);equal(vm.moduleBase('self-exit.dll'),base);
  equal(core.wire_protect(t.handle,arrayPage,4096n,4),0);
 }
 await finish(t,1n);equal(api(t,'GetExitCodeThread',[thread,0x22000n]),1n);equal(vm.readInt(0x22000n),37n);equal(api(t,'CloseHandle',[thread]),1n);
 const removed=kind==='unload'||kind==='faults';equal(vm.moduleBase('self-exit.dll'),removed?0n:base);equal(vm.readInt(trace+20n),2n);
 if(!removed){frame(t,'FreeLibrary',[base],'kernel32.dll');await finish(t);equal(vm.moduleBase('self-exit.dll'),0n);}
 equal(vm.bind('kernelbase.dll','FreeLibraryAndExitThread'),vm.bind('kernel32.dll','FreeLibraryAndExitThread'));equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,f=dynamicFixture(mode),gateway=vm.bind('kernel32.dll','FreeLibraryAndExitThread');
 f.bytes.set(worker(t),f.offset(0x1180));const kept=f.relocations.filter(rva=>rva<0x1180||rva>=0x1200);f.relocations.splice(0,f.relocations.length,...kept);f.rebuildRelocations();
 vm.supplyModule('notification.dll',f.bytes);vm.write(0x20000n,new TextEncoder().encode('notification.dll\0'));frame(t,'LoadLibraryA',[0x20000n],'kernel32.dll');
 for(let n=0;vm.ip!==gateway;n++){assert.ok(n<100);equal((await vm.run(1n)).reason,Exit.Budget);}
 const before=snapshot(t),base=vm.moduleBase('notification.dll');assert.ok(base);assert.throws(()=>vm.service(),/active loader callback/);equal(snapshot(t),before);equal(vm.moduleBase('notification.dll'),base);vm.dispose();
}
console.log(`${checks} FreeLibraryAndExitThread self-unload/retained/provider/invalid/notification/fault/resource checks passed`);
