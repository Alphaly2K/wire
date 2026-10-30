import assert from 'node:assert/strict';
import {core,Exit,equal,checks,machine,frame,finish} from './gdi-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {moduleFixture} from './module-fixture.mjs';
import {delayFixture} from './delay-fixture.mjs';
const exports=[[2,'SysAllocString'],[3,'SysReAllocString'],[4,'SysAllocStringLen'],[5,'SysReAllocStringLen'],[6,'SysFreeString'],[7,'SysStringLen'],[149,'SysStringByteLen'],[150,'SysAllocStringByteLen']];
const text=(vm,p,value)=>vm.write(p,new TextEncoder().encode(value+'\0'));
async function request(t,name,args){frame(t,name,args,'kernel32.dll');await finish(t,1n);return t.vm.register(0);}
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,accepted=exports.slice(0,mode===32?7:8);text(vm,0x22000n,'oleaut32.dll');const library=await request(t,'LoadLibraryA',[0x22000n]);assert.ok(library);
 for(const [ordinal,name]of accepted){const p=await request(t,'GetProcAddress',[library,BigInt(ordinal)]);assert.ok(p);equal(p,vm.bind('oleaut32.dll',name));}
 equal(await request(t,'GetProcAddress',[library,65535n]),0n);equal(vm.readInt(t.error),127n);if(mode===32)equal(await request(t,'GetProcAddress',[library,150n]),0n);
 text(vm,0x22000n,'kernel32.dll');const kernel=await request(t,'LoadLibraryA',[0x22000n]);equal(await request(t,'GetProcAddress',[kernel,2n]),0n);equal(vm.readInt(t.error),127n);
 vm.supplyModule('bridge.dll',moduleFixture(mode,{dll:true,forward:'oleaut32.#2'}).bytes);text(vm,0x22000n,'bridge.dll');const bridge=await request(t,'LoadLibraryA',[0x22000n]);assert.ok(bridge);equal(await request(t,'GetProcAddress',[bridge,17n]),vm.bind('oleaut32.dll','SysAllocString'));vm.dispose();
 // A static import forwarded to a provider ordinal must use the same binder.
 const staticVm=new WasmMachine(core,{mode}),main=moduleFixture(mode,{dependency:'bridge.dll'});main.bytes[main.offset(0x1000)]=0xc3;staticVm.supplyModule('bridge.dll',moduleFixture(mode,{dll:true,forward:'oleaut32.#2'}).bytes);const image=staticVm.load(main.bytes);equal(staticVm.readInt(image.base+0x20a0n,mode/8),staticVm.bind('oleaut32.dll','SysAllocString'));equal((await staticVm.run(1000n)).reason,Exit.Returned);staticVm.dispose();
 // Lazy provider ordinals preserve the call arguments and incur one API work.
 const delayed=new WasmMachine(core,{mode}),delayedImage=delayed.load(delayFixture(mode,{library:'oleaut32.dll',symbols:[2]}).bytes);equal((await delayed.run(1000n)).reason,Exit.Returned);const d={vm:delayed,mode,sp:delayedImage.sp-256n},thunk=delayed.readInt(delayedImage.base+0x3120n,mode/8);frame(d,'SysAllocString',[0n],'oleaut32.dll');delayed.ip=thunk;const work=delayed.work,retired=delayed.retired;await finish(d,1n);equal(delayed.register(0),0n);equal(delayed.work,work+1n);equal(delayed.retired,retired);equal(delayed.readInt(delayedImage.base+0x3120n,mode/8),delayed.bind('oleaut32.dll','SysAllocString'));delayed.dispose();
}
console.log(`${checks} provider ordinal/import/forwarder/lookup/delay/budget checks passed`);
