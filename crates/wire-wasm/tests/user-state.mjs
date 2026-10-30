import assert from 'node:assert/strict';
import {WasmMachine,Exit} from '../web/runtime.mjs';
import {core,stateLayout,encode} from './x87-harness.mjs';
import {fixture} from './pe-fixture.mjs';
let checks=0;const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
const snapshot=t=>new Uint8Array(core.memory.buffer,core.wire_state(t.handle),stateLayout[0]).slice();
async function machine(mode){let handle;const time={calls:0};const vm=new WasmMachine({...core,wire_create(...a){handle=core.wire_create(...a);return handle;}},{mode,clock(){time.calls++;return 123456789n;},wallClock(){throw new Error('unexpected USER wall clock');}});const f=fixture(mode,{tls:false});f.put(f.dirs+8,0);f.put(f.dirs+12,0);f.bytes.fill(0x90,f.offset(0x1000),f.offset(0x1100));f.bytes[f.offset(0x1000)]=0xc3;f.relocations.splice(0,f.relocations.length,...f.relocations.filter(r=>r>=0x1100));f.rebuildRelocations();const image=vm.load(f.bytes);equal((await vm.run(100n)).reason,Exit.Returned);vm.map(0x20000n,0x30000n,7);return {vm,mode,handle,image,time,sp:image.sp-2048n,error:image.teb+BigInt(mode===32?0x34:0x68),classes:0,mask:(1n<<BigInt(mode))-1n};}
function frame(t,name,args=[],dll='user32.dll'){const {vm,mode,sp}=t;vm.ip=vm.bind(dll,name);vm.register(4,sp);vm.write(sp,encode(vm.entryPoints.returned,mode/8));for(let n=0;n<args.length;n++)if(mode===32)vm.write(sp+4n+BigInt(n*4),encode(args[n],4));else if(n<4)vm.register([1,2,8,9][n],args[n]);else vm.write(sp+40n+BigInt(n-4)*8n,encode(args[n],8));}
function call(t,name,args=[],dll){const {vm}=t;frame(t,name,args,dll);const w=vm.work,r=vm.retired;equal(vm.service().reason,Exit.Dispatch);equal(vm.work,w+1n);equal(vm.retired,r);return vm.register(0);}
async function finish(t,budget=1000n){for(let n=0;n<10000;n++){const r=await t.vm.run(budget);if(r.reason===Exit.Returned)return t.vm.register(0);equal(r.reason,Exit.Budget);}throw new Error('USER call did not finish');}
async function invoke(t,name,args=[],budget=1000n){frame(t,name,args);return finish(t,budget);}
function text(t,p,s,wide=true){t.vm.write(p,wide?Uint8Array.from([...s+'\0'].flatMap(c=>[c.charCodeAt(0)&255,c.charCodeAt(0)>>8])):new TextEncoder().encode(s+'\0'));}
async function window(t,{proc,wide=true,x=10,y=20}={}){const {vm,mode}=t;vm.write(0x20000n,new Uint8Array(80));for(const [at,value,size] of [[0,mode===32?48n:80n,4],[8,proc??vm.bind('user32.dll',wide?'DefWindowProcW':'DefWindowProcA'),mode/8],[mode===32?20:24,t.image.base,mode/8],[mode===32?40:64,0x20100n,mode/8]])vm.write(0x20000n+BigInt(at),encode(value,size));text(t,0x20100n,'State'+t.classes++,wide);text(t,0x20180n,'Title',wide);const atom=call(t,wide?'RegisterClassExW':'RegisterClassExA',[0x20000n]);assert.ok(atom);const h=await invoke(t,wide?'CreateWindowExW':'CreateWindowExA',[0n,atom,0x20180n,0n,BigInt.asUintN(32,BigInt(x)),BigInt.asUintN(32,BigInt(y)),640n,480n,0n,0n,t.image.base,0n]);assert.ok(h);return h;}
const rect=(t,p=0x21000n)=>[0n,4n,8n,12n].map(n=>Number(BigInt.asIntN(32,t.vm.readInt(p+n))));
const messages=t=>Array.from({length:Number(t.vm.readInt(0x23000n))},(_,n)=>t.vm.readInt(0x23004n+BigInt(n*4)));
function trace(t){const {vm,mode}=t,b=[];if(mode===32)b.push(0x8b,0x44,0x24,8,0x8b,0x0d,...encode(0x23000n,4),0x89,0x04,0x8d,...encode(0x23004n,4),0x41,0x89,0x0d,...encode(0x23000n,4));else b.push(0x89,0xd0,0x49,0xbb,...encode(0x23000n,8),0x45,0x8b,0x13,0x43,0x89,0x44,0x93,4,0x41,0xff,0xc2,0x45,0x89,0x13);b.push(...(mode===64?[0x48]:[]),0xb8,...encode(vm.bind('user32.dll','DefWindowProcW'),mode/8),0xff,0xe0);vm.write(0x30000n,b);return 0x30000n;}

// Window text calls the guest WNDPROC on both ABIs; output faults retain the accepted callback.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t,{proc:trace(t)});vm.write(0x23000n,encode(0n,4));text(t,0x20400n,'Changed あ');equal(await invoke(t,'SetWindowTextW',[h,0x20400n],1n),1n);equal(await invoke(t,'GetWindowTextLengthW',[h],1n),9n);equal(await invoke(t,'GetWindowTextW',[h,0x21000n,4n],1n),3n);equal([0n,2n,4n,6n].map(o=>vm.readInt(0x21000n+o,2)),[67n,104n,97n,0n]);equal(messages(t),[0xcn,0xen,0xdn]);equal(vm.windows()[0].title,'Changed あ');equal(await invoke(t,'GetWindowTextW',[h,0xdead0000n,0xffffffffn]),0n);equal(messages(t).length,3);equal(await invoke(t,'GetWindowTextA',[h,0xdead0000n,4n]),0n);equal(vm.readInt(t.error),50n);
 vm.write(0x24fffn,[0xcc]);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);frame(t,'GetWindowTextW',[h,0x24fffn,10n]);equal((await vm.run(100n)).reason,Exit.Fault);equal(messages(t).filter(m=>m===0xdn).length,2);equal(vm.readInt(0x24fffn,1),0xccn);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(await finish(t),9n);equal(messages(t).filter(m=>m===0xdn).length,2);equal(t.time.calls,0);vm.dispose();
}

// Focus, active and capture route host events, and destruction clears their references.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t),other=await window(t,{x:100,y:200});equal(call(t,'GetFocus'),0n);equal(await invoke(t,'SetFocus',[h]),0n);equal(call(t,'GetFocus'),h);equal(await invoke(t,'SetActiveWindow',[h]),0n);equal(call(t,'GetActiveWindow'),h);equal(call(t,'GetForegroundWindow'),0n);equal(await invoke(t,'SetForegroundWindow',[h]),1n);equal(call(t,'GetForegroundWindow'),h);equal(await invoke(t,'SetCapture',[h]),0n);equal(call(t,'GetCapture'),h);
 vm.queueWindowEvents([{window:other,message:0x200,wparam:0n,lparam:0x00060005n,time:1},{window:other,message:0x100,wparam:65n,lparam:0x1e0001n,time:2}]);equal(call(t,'PeekMessageW',[0x21000n,0n,0n,0n,1n]),1n);equal(vm.readInt(0x21000n,mode/8),h);const lp=vm.readInt(0x21000n+BigInt(mode===32?12:24),mode/8);equal(lp,0x00ba005fn);equal(call(t,'PeekMessageW',[0x21000n,0n,0n,0n,1n]),1n);equal(vm.readInt(0x21000n,mode/8),h);
 equal(await invoke(t,'ReleaseCapture'),1n);equal(call(t,'GetCapture'),0n);equal(await invoke(t,'SetFocus',[0n]),h);equal(call(t,'GetFocus'),0n);equal(await invoke(t,'SetFocus',[h]),0n);equal(await invoke(t,'SetCapture',[h]),0n);equal(await invoke(t,'DestroyWindow',[h]),1n);for(const name of ['GetFocus','GetCapture','GetActiveWindow','GetForegroundWindow'])equal(call(t,name),0n);
 vm.queueWindowEvents([{window:other,message:6,wparam:1n,lparam:0n,time:3},{window:other,message:7,wparam:0n,lparam:0n,time:4}]);equal(call(t,'GetFocus'),other);equal(call(t,'GetActiveWindow'),other);equal(call(t,'GetForegroundWindow'),other);vm.dispose();
}

// Positioning keeps WINDOWPOS/NCCALCSIZE layouts, default MOVE/SIZE notifications and short-budget continuation.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t,{proc:trace(t)});vm.write(0x23000n,encode(0n,4));const w=vm.work,r=vm.retired;equal(await invoke(t,'MoveWindow',[h,30n,40n,320n,240n,1n],1n),1n);equal(messages(t),[0x46n,0x83n,0x47n,3n,5n]);equal(call(t,'GetWindowRect',[h,0x21000n]),1n);equal(rect(t),[30,40,350,280]);equal(call(t,'GetClientRect',[h,0x21000n]),1n);equal(rect(t),[0,0,320,240]);equal(vm.work-w,vm.retired-r+8n);equal(vm.windows()[0].width,320);
 vm.write(0x23000n,encode(0n,4));equal(await invoke(t,'SetWindowPos',[h,t.mask,50n,60n,0n,0n,0x41n],1n),1n);equal(messages(t),[0x46n,0x47n,3n]);equal(vm.windows()[0].topmost,true);equal(vm.windows()[0].visible,true);equal(await invoke(t,'SetWindowPos',[h,t.mask-1n,0n,0n,0n,0n,0x13n]),1n);equal(vm.windows()[0].topmost,false);
 equal(await invoke(t,'SetWindowPos',[h,0n,0n,0n,0n,0n,0x4000n]),0n);equal(vm.readInt(t.error),50n);equal(await invoke(t,'MoveWindow',[h,0x7fffffffn,0n,1n,1n,0n]),0n);equal(vm.readInt(t.error),87n);vm.dispose();
}

// Physical asynchronous state and per-thread queued state commit at message removal, independently of Unicode queries.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t);vm.queueWindowEvents([{window:h,message:0x100,wparam:16n,lparam:0x360001n,time:1},{window:h,message:0x100,wparam:65n,lparam:0x1e0001n,time:2},{window:h,message:0x102,wparam:0x3042n,lparam:0x1e0001n,time:3}]);equal(call(t,'GetAsyncKeyState',[16n]),0xffff8001n);equal(call(t,'GetAsyncKeyState',[0xa1n]),0xffff8001n);equal(call(t,'GetAsyncKeyState',[16n]),0xffff8000n);equal(call(t,'GetKeyState',[16n]),0n);equal(call(t,'PeekMessageW',[0x21000n,0n,0n,0n,0n]),1n);equal(call(t,'GetKeyState',[16n]),0n);equal(call(t,'PeekMessageW',[0x21000n,0n,0n,0n,1n]),1n);equal(call(t,'GetKeyState',[16n]),0xffff8000n);equal(call(t,'GetKeyState',[0xa0n]),0n);equal(call(t,'GetKeyState',[0xa1n]),0xffff8000n);equal(call(t,'GetKeyboardState',[0x22000n]),1n);equal(vm.readInt(0x22010n,1),0x80n);
 equal(call(t,'PeekMessageW',[0x21000n,0n,0n,0n,1n]),1n);vm.write(0x22200n,new Uint8Array(256));equal(call(t,'ToUnicode',[65n,0x1en,0x22200n,0x22400n,2n,0n]),1n);equal(vm.readInt(0x22400n,2),97n);equal(call(t,'TranslateMessage',[0x21000n]),1n);equal(call(t,'PeekMessageW',[0x21000n,0n,0n,0n,1n]),1n);equal(vm.readInt(0x21000n+BigInt(mode===32?8:16),mode/8),0x3042n);
 vm.write(0x24f80n,new Uint8Array(256).fill(0xcc));equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);frame(t,'GetKeyboardState',[0x24f80n]);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(0x24f80n,8),0xccccccccccccccccn);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(0x24f90n,1),0x80n);equal(t.time.calls,1);vm.dispose();
}
// A reentrant focus callback sees the committed new owner and can destroy its pending notification target.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,old=await window(t),target=await window(t);equal(await invoke(t,'SetFocus',[old]),0n);const get=vm.bind('user32.dll','GetFocus'),destroy=vm.bind('user32.dll','DestroyWindow');
 const b=mode===32?[0x83,0x7c,0x24,8,8,0x75,0,0xb8,...encode(get,4),0xff,0xd0,0xa3,...encode(0x23300n,4),0x68,...encode(target,4),0xb8,...encode(destroy,4),0xff,0xd0]:[0x83,0xfa,8,0x75,0,0x48,0x83,0xec,56,0x48,0xb8,...encode(get,8),0xff,0xd0,0x49,0xbb,...encode(0x23300n,8),0x49,0x89,0x03,0x48,0xb9,...encode(target,8),0x48,0xb8,...encode(destroy,8),0xff,0xd0,0x48,0x83,0xc4,56];const branch=mode===32?6:4;b[branch]=b.length-branch-1;b.push(0x31,0xc0,...(mode===32?[0xc2,16,0]:[0xc3]));vm.write(0x31000n,b);call(t,mode===32?'SetWindowLongW':'SetWindowLongPtrW',[old,0xfffffffcn,0x31000n]);equal(await invoke(t,'SetFocus',[target],1n),old);equal(vm.readInt(0x23300n,mode/8),target);equal(call(t,'IsWindow',[target]),0n);equal(call(t,'GetFocus'),0n);vm.dispose();
}

// A failed initial notification frame leaves ownership unchanged, before the accepted retry.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t),target=await window(t);await invoke(t,'SetFocus',[h]);t.sp=(t.image.sp&~4095n)+16n;frame(t,'SetFocus',[target]);const page=(t.sp&~4095n)-4096n;equal(core.wire_protect(t.handle,page,4096n,0x104),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(call(t,'GetFocus'),h);equal(await invoke(t,'SetFocus',[target]),h);equal(call(t,'GetFocus'),target);vm.dispose();
}

// A position record fault after a completed callback resumes without replaying CHANGING or NCCALCSIZE.
for(const mode of [32,64])for(const completed of [1,2]){
 const t=await machine(mode),{vm}=t,h=await window(t,{proc:trace(t)});vm.write(0x23000n,encode(0n,4));frame(t,'MoveWindow',[h,30n,40n,320n,240n,1n]);equal(vm.service().reason,Exit.Dispatch);
 for(let n=0;vm.ip!==vm.entryPoints.callback||messages(t).length!==completed;n++){assert.ok(n<1000);equal((await vm.run(1n)).reason,Exit.Budget);}const before=messages(t);const block=(t.sp&~15n)-128n;equal(core.wire_protect(t.handle,block&~4095n,4096n,0x104),0);equal((await vm.run(100n)).reason,Exit.Fault);equal(messages(t),before);equal(vm.windows()[0].width,640);equal(await finish(t,1n),1n);equal(messages(t),[0x46n,0x83n,0x47n,3n,5n]);equal(vm.windows()[0].width,320);vm.dispose();
}

// ANSI text retains CP932 bytes and uses the registered ANSI procedure.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t,{wide:false});vm.write(0x20400n,[0x82,0xa0,0]);equal(await invoke(t,'SetWindowTextA',[h,0x20400n]),1n);equal(await invoke(t,'GetWindowTextLengthA',[h]),2n);equal(await invoke(t,'GetWindowTextA',[h,0x21000n,3n]),2n);equal(vm.readInt(0x21000n,2),0xa082n);equal(vm.readInt(0x21002n,1),0n);equal(call(t,'GetLastActivePopup',[h]),h);vm.dispose();
}

// Layout queries retain guest pointer widths and signed capacities; ToUnicode copies the input before output.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,hkl=0x04090409n;equal(call(t,'GetKeyboardLayout',[0n]),hkl);equal(call(t,'GetKeyboardLayout',[999n]),0n);equal(call(t,'GetKeyboardLayoutList',[0n,0xdead0000n]),1n);equal(call(t,'GetKeyboardLayoutList',[1n,t.sp]),1n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp,mode/8),hkl);equal(call(t,'GetKeyboardLayoutList',[0xffffffffn,0xdead0000n]),0n);equal(vm.readInt(t.error),87n);
 equal(call(t,'GetKeyboardLayoutNameW',[0x21000n]),1n);equal(Array.from({length:9},(_,n)=>vm.readInt(0x21000n+BigInt(n*2),2)),[48n,48n,48n,48n,48n,52n,48n,57n,0n]);equal(call(t,'GetKeyboardLayoutNameA',[0x21000n]),1n);equal(vm.readInt(0x21000n,8),0x3930343030303030n);equal(call(t,'GetKeyboardType',[0n]),4n);equal(call(t,'GetKeyboardType',[2n]),12n);equal(call(t,'MapVirtualKeyW',[0xa1n,4n]),0x36n);equal(call(t,'MapVirtualKeyExA',[0xe01dn,3n,hkl]),0xa3n);equal(call(t,'MapVirtualKeyExW',[65n,0n,hkl+1n]),0n);equal(vm.readInt(t.error),87n);if(mode===64){equal(call(t,'MapVirtualKeyExW',[65n,0n,hkl+(1n<<32n)]),0n);equal(vm.readInt(t.error),87n);}
 equal(call(t,'GetKeyNameTextW',[0x360000n,0x21000n,3n]),2n);equal([0n,2n,4n].map(o=>vm.readInt(0x21000n+o,2)),[82n,105n,0n]);equal(call(t,'GetKeyNameTextA',[0x2360000n,0x21000n,16n]),5n);equal(vm.readInt(0x21000n,4),0x66696853n);equal(call(t,'GetKeyNameTextA',[0n,0xdead0000n,0xffffffffn]),0n);
 vm.write(0x22000n,new Uint8Array(256));vm.write(0x22010n,[0x80]);equal(call(t,'ToUnicodeEx',[65n,0x1en,0x22000n,0x22000n,2n,4n,hkl]),1n);equal(vm.readInt(0x22000n,4),65n);equal(call(t,'ToUnicode',[65n,0x801en,0x22000n,0x21000n,2n,0n]),0n);equal(vm.readInt(0x21000n,4),0n);equal(call(t,'ToUnicode',[65n,0x801en,0x22000n,0x21000n,2n,2n]),1n);equal(vm.readInt(0x21000n,2),65n);equal(call(t,'ToUnicode',[65n,0n,0xdead0000n,0xdead0000n,0xffffffffn,0n]),0n);equal(call(t,'ToUnicode',[65n,0n,0xdead0000n,0xdead0000n,1n,8n]),0n);equal(vm.readInt(t.error),87n);equal(t.time.calls,0);vm.dispose();
}

console.log(`${checks} USER state/position/keyboard checks passed`);
