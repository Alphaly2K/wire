import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,invoke,window,rectangle,readRect} from './gdi-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';

const added=['FindWindowA','FindWindowW','GetDesktopWindow','GetWindowPlacement','IsZoomed','IsWindowEnabled','MapWindowPoints'];
const text=(vm,p,s)=>vm.write(p,Buffer.from(s+'\0','utf16le'));
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const create=(t,atom,title=0n)=>invoke(t,'CreateWindowExW',[0n,atom,title,0n,100n,200n,160n,120n,0n,0n,t.image.base,0n]);

// Every remaining legacy export binds in its original architecture and provider range.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 for(const name of added){if(mode===32||!name.startsWith('FindWindow'))assert.ok(vm.bind('user32.dll',name));else assert.throws(()=>vm.bind('user32.dll',name),/migrated|unknown/);assert.throws(()=>vm.bind('gdi32.dll',name),/migrated|unknown/);}
 equal(call(t,'GetWindowPlacement',[0xbadn,0xdead0000n]),0n);equal(vm.readInt(t.error),mode===32?120n:1400n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Lookup shares Unicode case folding, atom/name selection, title matching and child exclusion.
{
 const t=await machine(32),{vm}=t,first=await window(t),atom=call(t,'GetClassLongW',[first,0xffffffe0n]);text(vm,0x24000n,'日本ABC');const second=await create(t,atom,0x24000n);assert.ok(second);
 vm.write(t.error,encode(77n,4));text(vm,0x24100n,'pAiNt');equal(call(t,'FindWindowW',[0x24100n,0n]),first);equal(call(t,'FindWindowA',[atom,0n]),first);equal(call(t,'FindWindowW',[0n,0n]),first);
 text(vm,0x24200n,'日本abc');equal(call(t,'FindWindowW',[atom,0x24200n]),second);vm.write(0x24300n,[0x93,0xfa,0x96,0x7b,0x61,0x62,0x63,0]);equal(call(t,'FindWindowA',[0n,0x24300n]),second);equal(vm.readInt(t.error),77n);
 call(t,'SetWindowLongW',[first,0xfffffff0n,0x40000000n]);equal(call(t,'FindWindowW',[0n,0n]),second);call(t,'SetWindowLongW',[second,0xfffffff0n,0x40000000n]);equal(call(t,'FindWindowW',[atom,0n]),0n);equal(vm.readInt(t.error),77n);
 text(vm,0x24400n,'\u03a3');vm.write(0x22000n,new Uint8Array(48));for(const[at,value]of [[0,48n],[8,vm.bind('user32.dll','DefWindowProcW')],[20,t.image.base],[40,0x24400n]])vm.write(0x22000n+BigInt(at),encode(value,4));const greek=call(t,'RegisterClassExW',[0x22000n]),h=await create(t,greek);text(vm,0x24500n,'\u03c3');equal(call(t,'FindWindowW',[0x24500n,0n]),h);
 frame(t,'FindWindowW',[0x60000n,0n]);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);vm.map(0x60000n,4096n,3);text(vm,0x60000n,'\u03c3');equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),h);vm.dispose();
}

// The desktop is a persistent owned window/class, excluded from application presentation snapshots.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.write(t.error,encode(77n,4));const desktop=call(t,'GetDesktopWindow');equal(desktop,0x4000fffcn);equal(call(t,'GetDesktopWindow'),desktop);equal(call(t,'IsWindow',[desktop]),1n);equal(call(t,'IsWindowVisible',[desktop]),1n);equal(call(t,'IsWindowUnicode',[desktop]),1n);equal(call(t,'GetParent',[desktop]),0n);equal(call(t,'GetWindowThreadProcessId',[desktop,0x21000n]),0n);equal(vm.readInt(0x21000n),1n);
 equal(call(t,'GetWindowRect',[desktop,0x21000n]),1n);equal(readRect(t,0x21000n),[0,0,1024,768]);equal(call(t,'GetClientRect',[desktop,0x21000n]),1n);equal(readRect(t,0x21000n),[0,0,1024,768]);equal(vm.windows(),[]);
 if(mode===32){equal(call(t,'GetClassNameW',[desktop,0x24000n,32n]),7n);equal(bytes(vm,0x24000n,16),new Uint8Array(Buffer.from('Desktop\0','utf16le')));text(vm,0x24100n,'desktop');equal(call(t,'FindWindowW',[0x24100n,0n]),desktop);}equal(vm.readInt(t.error),77n);
 equal(await invoke(t,'DestroyWindow',[desktop]),0n);equal(vm.readInt(t.error),5n);equal(call(t,'IsWindow',[desktop]),1n);const h=await window(t);assert.ok(h);equal(vm.windows().map(w=>w.handle),[h]);equal(call(t,'GetDesktopWindow'),desktop);vm.dispose();
}

// State queries use style bits and preserve LastError for missing HWNDs.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t);vm.write(t.error,encode(77n,4));equal(call(t,'IsWindowEnabled',[h]),1n);equal(call(t,'IsZoomed',[h]),0n);call(t,'SetWindowLongW',[h,0xfffffff0n,0x09000000n]);equal(call(t,'IsWindowEnabled',[h]),0n);equal(call(t,'IsZoomed',[h]),1n);for(const name of ['IsWindowEnabled','IsZoomed']){equal(call(t,name,[0xbadn]),0n);equal(vm.readInt(t.error),77n);}vm.dispose();
}

// POINT batches use client origins, signed wrapping and a packed 16-bit delta, including zero count.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,from=await window(t),atom=mode===32?call(t,'GetClassLongW',[from,0xffffffe0n]):0x20100n,to=await create(t,atom);equal(await invoke(t,'MoveWindow',[from,40000n,65539n,160n,120n,0n]),1n);equal(await invoke(t,'MoveWindow',[to,0xfffffff6n,0xffffffecn,160n,120n,0n]),1n);
 const delta=[40010,65559];rectangle(t,0x24000n,[2147483647,-2147483648,0,-1]);const old=bytes(vm,0x24000n,16);equal(call(t,'MapWindowPoints',[from,to,0x24000n,2n]),BigInt((delta[0]&65535)|((delta[1]&65535)<<16)));equal(readRect(t,0x24000n),[-2147443639,-2147418089,40010,65558]);call(t,'MapWindowPoints',[to,from,0x24000n,2n]);equal(bytes(vm,0x24000n,16),old);
 equal(call(t,'MapWindowPoints',[from,to,0xdead0000n,0n]),BigInt((delta[0]&65535)|((delta[1]&65535)<<16)));equal(call(t,'MapWindowPoints',[0n,0n,0xdead0000n,0n]),0n);
 rectangle(t,0x24000n,[0,0,0,0]);call(t,'MapWindowPoints',[from,0n,0x24000n,1n]);equal(readRect(t,0x24000n).slice(0,2),[40000,65539]);call(t,'MapWindowPoints',[0n,from,0x24000n,1n]);equal(readRect(t,0x24000n).slice(0,2),[0,0]);
 equal(call(t,'MapWindowPoints',[0xbadn,0xbadn,0xdead0000n,4097n]),0n);equal(vm.readInt(t.error),87n);equal(call(t,'MapWindowPoints',[0xbadn,0n,0xdead0000n,0n]),0n);equal(vm.readInt(t.error),1400n);
 rectangle(t,0x24ff8n,[1,2,3,4]);frame(t,'MapWindowPoints',[from,to,0x24ff8n,2n]);const before=snapshot(t),points=bytes(vm,0x24ff8n,16);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(vm,0x24ff8n,16),points);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(readRect(t,0x24ff8n),[40011,65561,40013,65563]);
 rectangle(t,t.sp,[0,0,0,0]);frame(t,'MapWindowPoints',[from,to,t.sp,1n]);equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp),BigInt.asUintN(32,vm.entryPoints.returned+40010n));equal(t.time.calls,0);vm.dispose();
}

// AMD64 preserves WINDOWPLACEMENT ordering: HWND, size, complete output check, then style restriction.
{
 const t=await machine(64),{vm}=t,h=await window(t),p=0x100022000n;vm.map(p,4096n,3);vm.write(p,new Uint8Array(48).fill(0xa5));vm.write(p,encode(44n,4));vm.write(t.error,encode(77n,4));equal(call(t,'GetWindowPlacement',[h,p]),1n);equal(vm.readInt(p),44n);equal(vm.readInt(p+4n),0n);equal(vm.readInt(p+8n),1n);for(const n of [12n,16n,20n,24n])equal(vm.readInt(p+n),0xffffffffn);equal(readRect(t,p+28n),[10,20,26,32]);equal(vm.readInt(p+44n),0xa5a5a5a5n);equal(vm.readInt(t.error),77n);
 equal(call(t,'GetWindowPlacement',[h,0n]),0n);equal(vm.readInt(t.error),87n);vm.write(p,encode(43n,4));equal(call(t,'GetWindowPlacement',[h,p]),0n);equal(vm.readInt(t.error),87n);
 vm.write(p,encode(44n,4));call(t,'SetWindowLongW',[h,0xfffffff0n,0x01000000n]);equal(call(t,'GetWindowPlacement',[h,p]),0n);equal(vm.readInt(t.error),50n);
 vm.write(0x24ff0n,new Uint8Array(48).fill(0xa5));vm.write(0x24ff0n,encode(44n,4));frame(t,'GetWindowPlacement',[h,0x24ff0n]);const before=snapshot(t),old=bytes(vm,0x24ff0n,44);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(vm,0x24ff0n,44),old);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0n);equal(vm.readInt(t.error),50n);
 call(t,'SetWindowLongW',[h,0xfffffff0n,0n]);frame(t,'GetWindowPlacement',[h,t.sp]);const returned=0x123456780000002cn;vm.write(t.sp,encode(returned,8));equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,returned);equal(vm.readInt(t.sp,8),44n);equal(readRect(t,t.sp+28n),[10,20,26,32]);equal(t.time.calls,0);vm.dispose();
}

// Static imports for the complete added family resolve in one image and execute through the real JIT.
for(const mode of [32,64]){
 const names=mode===32?added:added.filter(n=>!n.startsWith('FindWindow')),f=fixture(mode,{tls:false});f.bytes.fill(0,f.offset(0x2000),f.offset(0x2600));f.rput(0x2000,0x2080);f.rput(0x200c,0x2180);f.rput(0x2010,0x2100);f.rtext(0x2180,'user32.dll\0');let at=0x2200;
 for(const[i,name]of names.entries()){f.rput(0x2080+i*f.ptr,at,f.ptr);f.rput(0x2100+i*f.ptr,at,f.ptr);f.rtext(at+2,name+'\0');at+=name.length+3;}
 if(mode===32){f.bytes.set([0xff,0x15,...encode(f.preferred+0x2108n,4),0xc3],f.offset(0x1000));f.relocations.splice(0,f.relocations.length,0x1002);}else{f.bytes.set([0x48,0x83,0xec,40,0xff,0x15,...encode(0x10f6n,4),0x48,0x83,0xc4,40,0xc3],f.offset(0x1000));f.relocations.length=0;}f.rebuildRelocations();
 const vm=new WasmMachine(core,{mode,clock(){throw new Error('unexpected clock');}}),image=vm.load(f.bytes);for(const[i,name]of names.entries())equal(vm.readInt(image.base+0x2100n+BigInt(i*f.ptr),f.ptr),vm.bind('user32.dll',name));equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),0x4000fffcn);assert.ok(vm.retired>0n);vm.dispose();
}

console.log(`${checks} window lookup/desktop/state/coordinates/placement/ABI/JIT/fault checks passed`);
