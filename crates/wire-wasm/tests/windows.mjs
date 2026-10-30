import assert from 'node:assert/strict';
import {WasmMachine,Exit} from '../web/runtime.mjs';
import {core,stateLayout,encode} from './x87-harness.mjs';
import {fixture} from './pe-fixture.mjs';
import {dynamicFixture,trace} from './dynamic-fixture.mjs';
let checks=0;const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
const snapshot=t=>new Uint8Array(core.memory.buffer,core.wire_state(t.handle),stateLayout[0]).slice();
async function machine(mode){let handle;const vm=new WasmMachine({...core,wire_create(...a){handle=core.wire_create(...a);return handle;}},{mode,clock(){throw new Error('unexpected USER clock');},wallClock(){throw new Error('unexpected USER wall clock');}});const f=fixture(mode,{tls:false});f.put(f.dirs+8,0);f.put(f.dirs+12,0);f.bytes.fill(0x90,f.offset(0x1000),f.offset(0x1100));f.bytes[f.offset(0x1000)]=0xc3;f.relocations.splice(0,f.relocations.length,...f.relocations.filter(r=>r>=0x1100));f.rebuildRelocations();const image=vm.load(f.bytes);equal((await vm.run(100n)).reason,Exit.Returned);vm.map(0x20000n,0x30000n,7);return {vm,mode,handle,image,sp:image.sp-2048n,error:image.teb+BigInt(mode===32?0x34:0x68)};}
function frame(t,name,args=[],dll='user32.dll'){const{vm,sp,mode}=t;vm.ip=vm.bind(dll,name);vm.register(4,sp);vm.write(sp,encode(vm.entryPoints.returned,mode/8));for(let n=0;n<args.length;n++)if(mode===32)vm.write(sp+4n+BigInt(n*4),encode(args[n],4));else if(n<4)vm.register([1,2,8,9][n],args[n]);else vm.write(sp+40n+BigInt(n-4)*8n,encode(args[n],8));}
function call(t,name,args=[],dll){frame(t,name,args,dll);const w=t.vm.work,r=t.vm.retired;equal(t.vm.service().reason,Exit.Dispatch);equal(t.vm.work,w+1n);equal(t.vm.retired,r);return t.vm.register(0);}
async function finish(t,budget=1000n){for(let n=0;n<10000;n++){const r=await t.vm.run(budget);if(r.reason===Exit.Returned)return t.vm.register(0);equal(r.reason,Exit.Budget);}throw new Error('window call did not finish');}
async function invoke(t,name,args=[],budget=1000n){call(t,name,args);return finish(t,budget);}
function text(t,address,value,wide){t.vm.write(address,wide?Uint8Array.from([...value+'\0'].flatMap(c=>[c.charCodeAt(0)&255,c.charCodeAt(0)>>8])):new TextEncoder().encode(value+'\0'));}
function registration(t,wide,procedure,className='WindowClass'){const{vm,mode}=t;vm.write(0x20000n,new Uint8Array(80));vm.write(0x20000n,encode(BigInt(mode===32?48:80),4));vm.write(0x20000n+BigInt(mode===32?8:8),encode(procedure,mode/8));vm.write(0x20000n+BigInt(mode===32?16:20),encode(16n,4));vm.write(0x20000n+BigInt(mode===32?20:24),encode(t.image.base,mode/8));vm.write(0x20000n+BigInt(mode===32?40:64),encode(0x20100n,mode/8));text(t,0x20100n,className,wide);text(t,0x20180n,'Window title',wide);return call(t,wide?'RegisterClassExW':'RegisterClassExA',[0x20000n]);}
const createArgs=(t,atom)=>[0n,atom,0x20180n,0n,10n,20n,640n,480n,0n,0n,t.image.base,0x76543210n];
function procedure(t,{reject=0,base=0x30000n}={}){
 const {mode,vm}=t,b=[];const emit=(...v)=>b.push(...v),v=(n,size)=>emit(...encode(BigInt(n),size));
 if(mode===32){emit(0x8b,0x44,0x24,4,0xa3);v(0x20280n,4);emit(0x8b,0x44,0x24,16,0xa3);v(0x20288n,4);emit(0x8b,0x44,0x24,8,0x8b,0x0d);v(0x20200n,4);emit(0x89,0x04,0x8d);v(0x20204n,4);emit(0x41,0x89,0x0d);v(0x20200n,4);}else{emit(0x48,0x89,0xc8,0x49,0xbb);v(0x20280n,8);emit(0x49,0x89,0x03,0x4c,0x89,0xc8,0x49,0xbb);v(0x20288n,8);emit(0x49,0x89,0x03,0x89,0xd0,0x49,0xbb);v(0x20200n,8);emit(0x41,0x8b,0x0b,0x41,0x89,0x44,0x8b,4,0xff,0xc1,0x41,0x89,0x0b);}
 emit(0x3d);v(0x8001n,4);emit(0x75,mode===32?8:11);if(mode===64)emit(0x48);emit(0xb8);v(mode===32?0x76543210n:0xfedcba9876543210n,mode/8);emit(...(mode===32?[0xc2,16,0]:[0xc3]));
 if(reject){emit(0x3d);v(reject===1?0x81n:1n,4);emit(0x75,8,0xb8);v(reject===1?0n:0xffffffffn,4);emit(...(mode===32?[0xc2,16,0]:[0xc3]));if(mode===64)b[b.length-7]=6;}
 emit(0x3d);v(0x81n,4);emit(0x0f,0x94,0xc0,0x0f,0xb6,0xc0,...(mode===32?[0xc2,16,0]:[0xc3]));vm.write(base,b);return base;
}
const messages=t=>Array.from({length:Number(t.vm.readInt(0x20200n))},(_,n)=>t.vm.readInt(0x20204n+BigInt(n*4)));

// WNDPROC uses each ABI's real arguments/return, and HWND/return values retain their width.
for(const mode of [32,64])for(const wide of [false,true]){
 const t=await machine(mode),{vm}=t,p=procedure(t),atom=registration(t,wide,p);assert.ok(atom);vm.write(t.error,encode(99n,4));const work=vm.work,retired=vm.retired;
 const window=await invoke(t,wide?'CreateWindowExW':'CreateWindowExA',createArgs(t,atom),1n);assert.ok(window);equal(vm.readInt(0x20280n,mode/8),window);equal(messages(t),mode===32?[0x81n,1n]:[0x24n,0x81n,0x83n,1n]);equal(vm.work-work,vm.retired-retired+1n);equal(vm.readInt(t.error),99n);equal(call(t,'IsWindow',[window]),1n);equal(call(t,'IsWindowUnicode',[window]),BigInt(wide));equal(call(t,'GetWindowThreadProcessId',[window,0x20300n]),1n);equal(vm.readInt(0x20300n),1n);equal(call(t,'GetClientRect',[window,0x20300n]),1n);equal(Array.from({length:4},(_,n)=>vm.readInt(0x20300n+BigInt(n*4))),[0n,0n,640n,480n]);
 const create=vm.readInt(0x20288n,mode/8);equal(vm.readInt(create,mode/8),0x76543210n);equal(vm.readInt(create+BigInt(mode===32?4:8),mode/8),t.image.base);
 const pointer=mode===32?'':'Ptr';equal(call(t,`SetWindowLong${pointer}W`,[window,BigInt.asUintN(32,-21n),0x76543210n]),0n);equal(call(t,`GetWindowLong${pointer}W`,[window,BigInt.asUintN(32,-21n)]),0x76543210n);
 equal(await invoke(t,'SendMessageW',[window,0x8001n,0xfedcn,0x1234n],1n),mode===32?0x76543210n:0xfedcba9876543210n);equal(await invoke(t,'CallWindowProcW',[p,window,0x8001n,1n,2n],1n),mode===32?0x76543210n:0xfedcba9876543210n);equal(await invoke(t,'ShowWindow',[window,1n],1n),0n);equal(call(t,'IsWindowVisible',[window]),1n);equal(call(t,wide?'UnregisterClassW':'UnregisterClassA',[atom,t.image.base]),0n);equal(vm.readInt(t.error),1412n);equal(await invoke(t,'DestroyWindow',[window],1n),1n);equal(messages(t).slice(-2),[2n,0x82n]);equal(call(t,'IsWindow',[window]),0n);equal(call(t,wide?'UnregisterClassW':'UnregisterClassA',[atom,t.image.base]),1n);vm.dispose();
}

// Cross-encoding Win64 creation supplies class-encoded CREATESTRUCT strings throughout the callbacks.
for(const wide of [false,true]){
 const t=await machine(64),{vm}=t,atom=registration(t,wide,procedure(t),'CrossClass');assert.ok(atom);
 const expected=Buffer.from('Cross title\0',wide?'utf16le':'ascii');text(t,0x20180n,'Cross title',!wide);text(t,0x20100n,'CrossClass',!wide);
 const args=createArgs(t,0x20100n),h=await invoke(t,wide?'CreateWindowExA':'CreateWindowExW',args,1n);assert.ok(h);equal(call(t,'IsWindowUnicode',[h]),BigInt(wide));
 const record=vm.readInt(0x20288n,8),title=vm.readInt(record+56n,8),name=vm.readInt(record+64n,8);assert.notEqual(title,0x20180n);assert.notEqual(name,0x20100n);
 const read=(p,n)=>Buffer.from(Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1))));equal(read(title,expected.length),expected);const classBytes=Buffer.from('CrossClass\0',wide?'utf16le':'ascii');equal(read(name,classBytes.length),classBytes);equal(messages(t),[0x24n,0x81n,0x83n,1n]);vm.dispose();
}

// Creation failure sends cleanup exactly once and removes the published window.
for(const mode of [32,64])for(const reject of [1,2]){
 const t=await machine(mode),{vm}=t,atom=registration(t,true,procedure(t,{reject}));equal(await invoke(t,'CreateWindowExW',createArgs(t,atom),1n),0n);equal(messages(t).slice(-2),reject===1?[0x81n,0x82n]:[2n,0x82n]);const h=vm.readInt(0x20280n,mode/8);equal(call(t,'IsWindow',[h]),0n);equal(call(t,'UnregisterClassW',[atom,t.image.base]),1n);vm.dispose();
}

// Initial and next-frame faults keep the API charge and completed callback count precise.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,atom=registration(t,true,procedure(t));t.sp=(t.image.sp&~4095n)+128n;frame(t,'CreateWindowExW',createArgs(t,atom));const page=(t.sp&~4095n)-4096n;equal(core.wire_protect(t.handle,page,4096n,0x104),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.service().reason,Exit.Dispatch);
 for(let n=0;vm.ip!==vm.entryPoints.callback;n++){assert.ok(n<100);equal((await vm.run(1n)).reason,Exit.Budget);}const count=vm.readInt(0x20200n);equal(core.wire_protect(t.handle,page,4096n,2),0);equal((await vm.run(100n)).reason,Exit.Fault);equal(vm.readInt(0x20200n),count);equal(core.wire_protect(t.handle,page,4096n,4),0);const window=await finish(t,1n);assert.ok(window);equal(messages(t).filter(m=>m===0x81n).length,1);vm.dispose();
}
// A completed single callback does not need another writable return frame.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,p=procedure(t);call(t,'CallWindowProcW',[p,0n,0x8001n,1n,2n]);for(let n=0;vm.ip!==vm.entryPoints.callback;n++){assert.ok(n<100);equal((await vm.run(1n)).reason,Exit.Budget);}const page=vm.register(4)&~4095n;equal(core.wire_protect(t.handle,page,4096n,2),0);equal(await finish(t),mode===32?0x76543210n:0xfedcba9876543210n);equal(core.wire_protect(t.handle,page,4096n,4),0);vm.dispose();
}

// A window callback owns its continuation across nested TLS/DllMain and loader returns.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.supplyModule('leaf.dll',dynamicFixture(mode,{id:1,seed:73,tls:true}).bytes);text(t,0x20100n,'leaf.dll',false);const target=vm.bind('kernel32.dll','LoadLibraryA');vm.write(0x30000n,mode===32?[0x68,...encode(0x20100n,4),0xb8,...encode(target,4),0xff,0xd0,0xc2,16,0]:[0x48,0x83,0xec,56,0x48,0xb9,...encode(0x20100n,8),0x48,0xb8,...encode(target,8),0xff,0xd0,0x48,0x83,0xc4,56,0xc3]);const work=vm.work,retired=vm.retired;const base=await invoke(t,'CallWindowProcW',[0x30000n,0n,0n,0n,0n],1n);assert.ok(base);equal(vm.moduleBase('leaf.dll'),base);equal(vm.readInt(trace+16n),1n);equal(vm.work-work,vm.retired-retired+2n);call(t,'FreeLibrary',[base],'kernel32.dll');await finish(t,1n);equal(vm.moduleBase('leaf.dll'),0n);vm.dispose();
}
// Default text uses bounded output lengths and resumes an accepted SendMessage after output faults.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,atom=registration(t,true,vm.bind('user32.dll','DefWindowProcW')),window=await invoke(t,'CreateWindowExW',createArgs(t,atom));text(t,0x20400n,'Changed',true);equal(await invoke(t,'SendMessageW',[window,0xcn,0n,0x20400n]),1n);equal(await invoke(t,'SendMessageW',[window,0xen,0n,0n]),7n);equal(await invoke(t,'SendMessageW',[window,0xdn,BigInt.asUintN(mode,-1n),0x20440n]),7n);equal(Array.from({length:8},(_,n)=>vm.readInt(0x20440n+BigInt(n*2),2)),[...new TextEncoder().encode('Changed'),0].map(BigInt));
 vm.write(0x24fffn,[0xcc]);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const work=vm.work,retired=vm.retired;call(t,'SendMessageW',[window,0xdn,8n,0x24fffn]);const before=snapshot(t);equal((await vm.run(100n)).reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(0x24fffn,1),0xccn);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(await finish(t),7n);equal(vm.work-work,vm.retired-retired+2n);equal(await invoke(t,'SendMessageA',[window,0xcn,0n,0xdead000n]),0n);equal(vm.readInt(t.error),50n);equal(await invoke(t,'SendMessageW',[window,0xen,0n,0n]),7n);equal(await invoke(t,'SendMessageW',[window,0x10n,0n,0n]),0n);equal(call(t,'IsWindow',[window]),0n);equal(call(t,'UnregisterClassW',[atom,t.image.base]),1n);vm.dispose();
}

// Reentrant destruction during WM_CREATE does not leave a window or a creation continuation.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,destroy=vm.bind('user32.dll','DestroyWindow'),bytes=mode===32?[0x8b,0x44,0x24,8,0x83,0xf8,1,0x75,0,0xff,0x05,...encode(0x20200n,4),0xff,0x74,0x24,4,0xb8,...encode(destroy,4),0xff,0xd0,0x31,0xc0,0xc2,16,0]:[0x83,0xfa,1,0x75,0,0x49,0xbb,...encode(0x20200n,8),0x41,0xff,0x03,0x48,0x83,0xec,56,0x48,0xb8,...encode(destroy,8),0xff,0xd0,0x48,0x83,0xc4,56,0x31,0xc0,0xc3];const branch=mode===32?8:4;bytes[branch]=bytes.length-branch-1;bytes.push(...(mode===32?[0x3d,0x81,0,0,0]:[0x81,0xfa,0x81,0,0,0]),0x0f,0x94,0xc0,0x0f,0xb6,0xc0,...(mode===32?[0xc2,16,0]:[0xc3]));vm.write(0x30000n,bytes);const atom=registration(t,true,0x30000n);equal(await invoke(t,'CreateWindowExW',createArgs(t,atom),1n),0n);equal(vm.readInt(0x20200n),1n);equal(call(t,'UnregisterClassW',[atom,t.image.base]),1n);vm.dispose();
}

// A target unwind out of the creation callback releases the unfinished USER window.
{
 const t=await machine(64),{vm}=t,atom=registration(t,true,0x34000n),value=0xfedcba9876543210n;
 const api=(name,args)=>{const b=[];for(let n=0;n<args.length;n++){if(n<4)b.push(...[[0x48,0xb9],[0x48,0xba],[0x49,0xb8],[0x49,0xb9]][n],...encode(args[n],8));else b.push(0x48,0xb8,...encode(args[n],8),0x48,0x89,0x44,0x24,32+(n-4)*8);}b.push(0x48,0xb8,...encode(vm.bind(name==='RtlUnwindEx'?'kernel32.dll':'user32.dll',name),8),0xff,0xd0);return b;};
 vm.write(0x33000n,[0x48,0x83,0xec,104,...api('CreateWindowExW',createArgs(t,atom)),0xcc]);vm.write(0x331f0n,[0x48,0x83,0xc4,104,0xc3]);vm.write(0x34000n,[0x48,0x83,0xec,56,...api('RtlUnwindEx',[t.sp-104n,0x331f0n,0n,value,0x26000n,0n]),0x48,0x83,0xc4,56,0xc3]);vm.write(0x27000n,[...encode(0x33000n,4),...encode(0x33200n,4),...encode(0x28000n,4),...encode(0x34000n,4),...encode(0x34100n,4),...encode(0x28100n,4)]);vm.write(0x28000n,[1,4,1,0,4,0xc2,0,0]);vm.write(0x28100n,[1,4,1,0,4,0x62,0,0]);equal(call(t,'RtlAddFunctionTable',[0x27000n,2n,0n],'kernel32.dll'),1n);vm.ip=0x33000n;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,8));const work=vm.work,retired=vm.retired;equal(await finish(t,1n),value);equal(vm.work-work,vm.retired-retired+2n);equal(call(t,'IsWindow',[0x7ffd00630000n]),0n);equal(call(t,'UnregisterClassW',[atom,t.image.base]),1n);vm.dispose();
}
// Class identity follows the architecture's name policy; an error-slot fault consumes no atom.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,atom=registration(t,true,0x30000n,'ÄClass');equal(atom,0xc000n);frame(t,'RegisterClassExW',[0x20000n]);equal(core.wire_protect(t.handle,t.error&~4095n,4096n,0x104),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0n);equal(vm.readInt(t.error),1410n);text(t,0x20100n,'äclass',true);equal(call(t,'RegisterClassExW',[0x20000n]),mode===32?0n:0xc001n);text(t,0x20100n,'UniqueClass',true);equal(call(t,'RegisterClassExW',[0x20000n]),mode===32?0xc001n:0xc002n);if(mode===64)assert.throws(()=>vm.bind('user32.dll','GetClassInfoW'),/migrated|unknown/);vm.dispose();
}
console.log(`${checks} USER class/window/WNDPROC/lifecycle/frame checks passed`);
