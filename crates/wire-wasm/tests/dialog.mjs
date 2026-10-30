import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,invoke,finish,window} from './gdi-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';
import {moduleFixture} from './module-fixture.mjs';
const names=['MessageBoxA','MessageBoxW','GetDlgItemTextW','GetDlgItemTextA','SetDlgItemTextW','SetDlgItemTextA','SendDlgItemMessageW','SendDlgItemMessageA','GetDlgItem','GetDlgCtrlID','GetDlgItemInt','SetDlgItemInt','EnumChildWindows','DialogBoxIndirectParamW','CreateDialogIndirectParamW','MapDialogRect','IsDialogMessageW','CreateIconIndirect','SetWindowPlacement','GetScrollInfo','SetScrollInfo','LockWindowUpdate','EnumWindows','EnumThreadWindows'];
const slots=[4,4,4,4,3,3,5,5,2,1,4,4,3,5,5,2,2,1,2,3,4,1,2,3];
const create=(t,atom,parent=0n,id=0n,style=0x40000000n,wide=true)=>invoke(t,wide?'CreateWindowExW':'CreateWindowExA',[0n,atom,0n,style,10n,20n,100n,80n,parent,id,t.image.base,0n]);
const text=(vm,p,s,wide=true)=>vm.write(p,Buffer.from(s+'\0',wide?'utf16le':'ascii'));
const ret=(value=1)=>[0xb8,...encode(BigInt(value),4),0xc2,8,0];
const service=(t,name,args=[],dll='user32.dll')=>{
 const b=[];for(const arg of args.toReversed())if(typeof arg==='object')b.push(0xff,0x35,...encode(arg.memory,4));else b.push(0x68,...encode(arg,4));b.push(0xb8,...encode(t.vm.bind(dll,name),4),0xff,0xd0);return b;
};

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;for(const name of names){assert.ok(vm.bind('user32.dll',name));assert.throws(()=>vm.bind('gdi32.dll',name),/migrated|unknown/);}
 for(const index of [0,1,13,14,15,16,17,18,19,20,21]){equal(call(t,names[index],Array(slots[index]).fill(0xdead0000n)),index===13?(1n<<BigInt(mode))-1n:0n);equal(vm.readInt(t.error),120n);equal(vm.register(4),t.sp+BigInt(mode===32?(slots[index]+1)*4:8));}equal(t.time.calls,0);vm.dispose();
}

// Control lookup and the eight message wrappers use the actual application procedure and integer buffer lifetime.
for(const mode of [32,64])for(const wide of [false,true]){
 const t=await machine(mode),{vm}=t,parent=await window(t);let atom=mode===32?call(t,'GetClassLongW',[parent,0xffffffe0n]):0x20100n;
 if(!wide){vm.write(0x22000n,new Uint8Array(80));text(vm,0x23000n,'Ansi',false);for(const[at,value,size]of [[0,mode===32?48n:80n,4],[8,vm.bind('user32.dll','DefWindowProcA'),mode/8],[mode===32?20:24,t.image.base,mode/8],[mode===32?40:64,0x23000n,mode/8]])vm.write(0x22000n+BigInt(at),encode(value,size));atom=call(t,'RegisterClassExA',[0x22000n]);}
 const child=await create(t,atom,parent,42n,0x40000000n,wide);assert.ok(child);equal(call(t,'GetDlgItem',[parent,42n]),child);equal(call(t,'GetDlgItem',[parent,43n]),0n);equal(call(t,'GetDlgCtrlID',[0xbadn]),0n);equal(call(t,'GetDlgCtrlID',[child]),42n);
 const suffix=wide?'W':'A';text(vm,0x24000n,'copied',wide);equal(await invoke(t,'SetDlgItemText'+suffix,[parent,42n,0x24000n]),1n);equal(await invoke(t,'GetDlgItemText'+suffix,[parent,42n,0x24100n,4n]),3n);equal(vm.readInt(0x24100n,wide?2:1),99n);equal(await invoke(t,'SendDlgItemMessage'+suffix,[parent,42n,0xen,0n,0n]),6n);
 for(const [s,signed,value,ok]of [[' -2147483648tail',1,0x80000000n,1],['2147483648',1,0n,0],['4294967295',0,0xffffffffn,1],['-1',0,0xffffffffn,1],['4294967296',0,0n,0],['\t+17x',1,17n,1],['word',1,0n,0]]){
  text(vm,0x24000n,s,wide);equal(await invoke(t,'SetDlgItemText'+suffix,[parent,42n,0x24000n]),1n);vm.write(0x25000n,encode(0xa5a5a5a5n,4));equal(await invoke(t,'GetDlgItemInt',[parent,42n,0x25000n,BigInt(signed)],1n),value);equal(vm.readInt(0x25000n),BigInt(ok));
 }
 equal(await invoke(t,'SetDlgItemInt',[parent,42n,0x80000000n,1n]),1n);equal(await invoke(t,'GetDlgItemInt',[parent,42n,0x25000n,1n]),0x80000000n);equal(await invoke(t,'SetDlgItemInt',[parent,42n,0xffffffffn,0n]),1n);equal(await invoke(t,'GetDlgItemInt',[parent,42n,0x25000n,0n]),0xffffffffn);
 equal(await invoke(t,'GetDlgItemText'+(wide?'A':'W'),[parent,42n,0xdead0000n,10n]),0n);equal(vm.readInt(t.error),50n);equal(await invoke(t,'GetDlgItemInt',[parent,43n,0xdead0000n,1n]),0n);equal(vm.readInt(t.error),1400n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// A WM_GETTEXT fault leaves translated cleared; a later output fault retains the completed result without replay.
{
 const t=await machine(32),{vm}=t,parent=await window(t),atom=call(t,'GetClassLongW',[parent,0xffffffe0n]),child=await create(t,atom,parent,42n);vm.write(0x25000n,encode(0xa5a5a5a5n,4));vm.write(0x30000n,[0xa1,...encode(0x60000n,4),0x31,0xc0,0xc2,16,0]);call(t,'SetWindowLongW',[child,0xfffffffcn,0x30000n]);frame(t,'GetDlgItemInt',[parent,42n,0x25000n,1n]);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(0x25000n),0n);equal((await vm.run(100n)).reason,Exit.Fault);vm.map(0x60000n,4096n,3);equal(await finish(t,1n),0n);
 vm.write(0x30000n,[0xff,0x05,...encode(0x23500n,4),0x8b,0x44,0x24,16,0x66,0xc7,0x00,55,0,0xb8,1,0,0,0,0xc2,16,0]);frame(t,'GetDlgItemInt',[parent,42n,0x25000n,1n]);equal(vm.service().reason,Exit.Dispatch);for(let n=0;vm.ip!==vm.entryPoints.callback;n++){assert.ok(n<100);equal((await vm.run(1n)).reason,Exit.Budget);}equal(vm.readInt(0x23500n),1n);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal((await vm.run(100n)).reason,Exit.Fault);equal(snapshot(t),before);vm.register(0,0n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(await finish(t,1n),7n);equal(vm.readInt(0x25000n),1n);equal(vm.readInt(0x23500n),1n);vm.dispose();
}

// Initial callback-frame faults precede the intentionally visible translated-output clear.
{
 const t=await machine(32),{vm}=t,parent=await window(t),atom=call(t,'GetClassLongW',[parent,0xffffffe0n]);await create(t,atom,parent,42n);vm.write(0x25000n,encode(0xa5a5a5a5n,4));t.sp=(t.image.sp&~4095n)+64n;frame(t,'GetDlgItemInt',[parent,42n,0x25000n,1n]);equal(core.wire_protect(t.handle,(t.sp&~4095n)-4096n,4096n,0x104),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(0x25000n),0xa5a5a5a5n);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(0x25000n),0n);equal(await finish(t,1n),0n);vm.dispose();
}

// Top-level snapshots descend by HWND; child snapshots retain the old recursive order and FALSE carrier.
{
 const t=await machine(32),{vm}=t,parent=await window(t),atom=call(t,'GetClassLongW',[parent,0xffffffe0n]),a=await create(t,atom,parent,1n),b=await create(t,atom,parent,2n),aa=await create(t,atom,a,3n),bb=await create(t,atom,b,4n),top=await create(t,atom,0n,0n,0n);await create(t,atom,0xfffffffdn,0n,0n);
 vm.write(0x30000n,[0x8b,0x0d,...encode(0x23000n,4),0x8b,0x44,0x24,4,0x89,0x04,0x8d,...encode(0x23100n,4),0xff,0x05,...encode(0x23000n,4),...ret(73)]);
 equal(await invoke(t,'EnumWindows',[0x30000n,0x12345678n],1n),73n);equal([vm.readInt(0x23100n),vm.readInt(0x23104n)],[top,parent]);vm.write(0x23000n,encode(0n,4));equal(await invoke(t,'EnumChildWindows',[parent,0x30000n,0n],1n),1n);equal([0n,4n,8n,12n].map(o=>vm.readInt(0x23100n+o)),[a,b,bb,aa]);
 vm.write(0x30000n,ret(0));equal(await invoke(t,'EnumWindows',[0x30000n,0n]),0n);equal(await invoke(t,'EnumChildWindows',[parent,0x30000n,0n]),1n);equal(call(t,'EnumWindows',[0n,0n]),0n);equal(vm.readInt(t.error),87n);equal(call(t,'EnumThreadWindows',[999n,0xdead0000n,0n]),1n);equal(call(t,'EnumChildWindows',[0xbadn,0n,0n]),1n);vm.dispose();
}

// Foreign controls reject before touching outputs; thread enumeration executes on the calling thread.
{
 const t=await machine(32),{vm}=t,parent=await window(t),atom=call(t,'GetClassLongW',[parent,0xffffffe0n]),k=(name,args=[])=>call(t,name,args,'kernel32.dll');
 const ready=k('CreateEventW',[0n,1n,0n,0n]),release=k('CreateEventW',[0n,1n,0n,0n]);
 vm.write(0x30000n,[...service(t,'CreateWindowExW',[0n,atom,0n,0n,10n,20n,100n,80n,0n,0n,t.image.base,0n]),0xa3,...encode(0x23520n,4),...service(t,'CreateWindowExW',[0n,atom,0n,0x40000000n,1n,2n,32n,24n,{memory:0x23520n},42n,t.image.base,0n]),0xa3,...encode(0x23524n,4),...service(t,'SetEvent',[ready],'kernel32.dll'),...service(t,'WaitForSingleObject',[release,0xffffffffn],'kernel32.dll'),0xb8,17,0,0,0,0xc2,4,0]);
 const worker=k('CreateThread',[0n,65536n,0x30000n,0n,0n,0x23500n]);frame(t,'WaitForSingleObject',[ready,0xffffffffn],'kernel32.dll');equal(await finish(t,1n),0n);const top=vm.readInt(0x23520n),child=vm.readInt(0x23524n),tid=vm.readInt(0x23500n);assert.ok(top);assert.ok(child);
 vm.write(0x25000n,encode(0xa5a5a5a5n,4));equal(await invoke(t,'GetDlgItemInt',[top,42n,0x25000n,1n]),0n);equal(vm.readInt(t.error),120n);equal(vm.readInt(0x25000n),0xa5a5a5a5n);equal(await create(t,atom,top,99n),0n);equal(vm.readInt(t.error),50n);
 vm.write(0x31000n,[0x8b,0x44,0x24,4,0xa3,...encode(0x23540n,4),...service(t,'GetCurrentThreadId',[],'kernel32.dll'),0xa3,...encode(0x23544n,4),...ret(1)]);equal(await invoke(t,'EnumThreadWindows',[tid,0x31000n,0n],1n),1n);equal(vm.readInt(0x23540n),top);equal(vm.readInt(0x23544n),1n);
 equal(k('SetEvent',[release]),1n);frame(t,'WaitForSingleObject',[worker,0xffffffffn],'kernel32.dll');equal(await finish(t,1n),0n);equal(call(t,'IsWindow',[top]),0n);equal(call(t,'IsWindow',[child]),0n);equal(t.time.calls,0);vm.dispose();
}

// Enumeration inside DllMain retains loader depth across a yielding callback.
{
 const t=await machine(32),{vm}=t;await window(t);const f=moduleFixture(32,{dll:true,dependency:'user32.dll'});f.rtext(0x20e2,'EnumWindows\0');f.bytes.set([0x68,...encode(0x87654321n,4),0x68,...encode(0x30000n,4),0xff,0x15,...encode(f.preferred+0x20a0n,4),0xb8,1,0,0,0,0xc2,12,0],f.offset(0x1000));f.relocations.splice(0,f.relocations.length,0x100c);f.rebuildRelocations();vm.supplyModule('enum-start.dll',f.bytes);
 vm.write(0x30000n,[0xff,0x05,...encode(0x23500n,4),0x8b,0x44,0x24,8,0xa3,...encode(0x23504n,4),...service(t,'Sleep',[0n],'kernel32.dll'),...ret(73)]);text(vm,0x26000n,'enum-start.dll');vm.flags=0xad5n;frame(t,'LoadLibraryW',[0x26000n],'kernel32.dll');const loaded=await finish(t,1n);assert.ok(loaded);equal(vm.moduleBase('enum-start.dll'),loaded);equal(vm.readInt(0x23500n),1n);equal(vm.readInt(0x23504n),0x87654321n);equal(vm.flags,0xad5n);equal(t.time.calls,0);vm.dispose();
}

// A callback may destroy a later snapshot member; a next-frame guard fault never replays a completed callback.
{
 const t=await machine(32),{vm}=t,first=await window(t),atom=call(t,'GetClassLongW',[first,0xffffffe0n]);await create(t,atom,0n,0n,0n);const latest=await create(t,atom,0n,0n,0n);
 vm.write(0x30000n,[0xff,0x05,...encode(0x23000n,4),0x68,...encode(first,4),0xb8,...encode(vm.bind('user32.dll','DestroyWindow'),4),0xff,0xd0,...ret(1)]);equal(await invoke(t,'EnumWindows',[0x30000n,0n],1n),1n);equal(vm.readInt(0x23000n),2n);equal(call(t,'IsWindow',[first]),0n);
 vm.write(0x23000n,encode(0n,4));vm.write(0x30000n,[0xff,0x05,...encode(0x23000n,4),...ret(1)]);t.sp=t.image.sp&~4095n;frame(t,'EnumWindows',[0x30000n,0n]);equal(vm.service().reason,Exit.Dispatch);for(let n=0;vm.ip!==vm.entryPoints.callback;n++){assert.ok(n<100);equal((await vm.run(1n)).reason,Exit.Budget);}equal(vm.readInt(0x23000n),1n);const page=(t.sp&~4095n)-4096n;equal(core.wire_protect(t.handle,page,4096n,0x104),0);equal((await vm.run(100n)).reason,Exit.Fault);equal(vm.readInt(0x23000n),1n);vm.register(0,0n);equal(await finish(t,1n),1n);equal(vm.readInt(0x23000n),2n);equal(call(t,'IsWindow',[latest]),1n);equal(t.time.calls,0);vm.dispose();
}

// Win64 callbacks preserve full handles/parameters and continue across short budgets and yielding services.
{
 const t=await machine(64),{vm}=t,parent=await window(t),atom=0x20100n;
 const a=await create(t,atom,parent,0x12340000002an),b=await create(t,atom,parent,43n),nested=await create(t,atom,a,44n),top=await create(t,atom,0n,0n,0n);
 await create(t,atom,0xfffffffffffffffdn,0n,0n);assert.ok(parent>0xffffffffn);equal(call(t,'GetDlgItem',[parent,0xffff00000000002an]),a);equal(call(t,'GetDlgCtrlID',[a]),42n);
 const wp=0x1234567887654321n,lp=0xfedcba9876543210n,proc=0x100030000n;vm.map(proc,4096n,7);
 vm.write(proc,[0x49,0xbb,...encode(0x23200n,8),0x49,0x89,0x0b,0x49,0x89,0x53,8,0x4d,0x89,0x43,16,0x4d,0x89,0x4b,24,0x4c,0x89,0xc8,0xc3]);
 call(t,'SetWindowLongPtrW',[a,0xfffffffcn,proc]);equal(await invoke(t,'SendDlgItemMessageW',[parent,42n,0x1234n,wp,lp],1n),lp);equal([0n,8n,16n,24n].map(o=>vm.readInt(0x23200n+o,8)),[a,0x1234n,wp,lp]);
 const sleep=vm.bind('kernel32.dll','Sleep'),enumerate=[0x49,0xbb,...encode(0x23000n,8),0x41,0x8b,0x03,0x49,0x89,0x8c,0xc3,0,1,0,0,0x49,0x89,0x53,16,0x41,0xff,0x03,0x48,0x83,0xec,40,0x31,0xc9,0x48,0xb8,...encode(sleep,8),0xff,0xd0,0x48,0x83,0xc4,40,0xb8,73,0,0,0,0xc3];vm.write(0x30000n,enumerate);
 const reset=()=>vm.write(0x23000n,new Uint8Array(0x140)),seen=n=>Array.from({length:n},(_,i)=>vm.readInt(0x23100n+BigInt(i*8),8));
 equal(await invoke(t,'EnumWindows',[0x30000n,lp],1n),73n);equal(seen(2),[top,parent]);equal(vm.readInt(0x23010n,8),lp);
 reset();equal(await invoke(t,'EnumChildWindows',[parent,0x30000n,lp],1n),1n);equal(seen(3),[a,b,nested]);equal(vm.readInt(0x23010n,8),lp);
 reset();equal(await invoke(t,'EnumThreadWindows',[1n,0x30000n,lp],1n),73n);equal(seen(2),[top,parent]);
 vm.write(0x30000n,[0x31,0xc0,0xc3]);equal(await invoke(t,'EnumWindows',[0x30000n,lp]),0n);equal(await invoke(t,'EnumChildWindows',[parent,0x30000n,lp]),1n);
 // A completed callback stays completed when the next callback frame faults.
 reset();vm.write(0x30000n,[0x49,0xbb,...encode(0x23000n,8),0x41,0xff,0x03,0xb8,1,0,0,0,0xc3]);t.sp=t.image.sp&~4095n;frame(t,'EnumWindows',[0x30000n,lp]);equal(vm.service().reason,Exit.Dispatch);
 for(let n=0;vm.ip!==vm.entryPoints.callback;n++){assert.ok(n<100);equal((await vm.run(1n)).reason,Exit.Budget);}equal(vm.readInt(0x23000n),1n);
 const page=(t.sp&~4095n)-4096n;equal(core.wire_protect(t.handle,page,4096n,0x104),0);equal((await vm.run(100n)).reason,Exit.Fault);equal(vm.readInt(0x23000n),1n);vm.register(0,0n);equal(await finish(t,1n),1n);equal(vm.readInt(0x23000n),2n);equal(t.time.calls,0);vm.dispose();
}

// All dialog/control exports import together, with an ordinary JIT call through the generated IAT.
for(const mode of [32,64]){
 const all=[...names,'EnableWindow','SetParent','DefDlgProcA','DefDlgProcW'],f=fixture(mode,{tls:false});f.bytes.fill(0,f.offset(0x2000),f.offset(0x2600));f.rput(0x2000,0x2080);f.rput(0x200c,0x2180);f.rput(0x2010,0x21a0);f.rtext(0x2190,'user32.dll\0');f.rput(0x200c,0x2190);let at=0x22a0;for(const[i,name]of all.entries()){f.rput(0x2080+i*f.ptr,at,f.ptr);f.rput(0x21a0+i*f.ptr,at,f.ptr);f.rtext(at+2,name+'\0');at+=name.length+3;}assert.ok(at<0x2600);
 if(mode===32){f.bytes.set([0x6a,0,0xff,0x15,...encode(f.preferred+0x21c4n,4),0xc3],f.offset(0x1000));f.relocations.splice(0,f.relocations.length,0x1004);}else{f.bytes.set([0x48,0x83,0xec,40,0x31,0xc9,0xff,0x15,...encode(0x11dcn,4),0x48,0x83,0xc4,40,0xc3],f.offset(0x1000));f.relocations.length=0;}f.rebuildRelocations();const vm=new WasmMachine(core,{mode,clock(){throw new Error('unexpected clock');}}),image=vm.load(f.bytes);for(const[i,name]of all.entries())equal(vm.readInt(image.base+0x21a0n+BigInt(i*f.ptr),f.ptr),vm.bind('user32.dll',name));equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),0n);assert.ok(vm.retired>0n);vm.dispose();
}
console.log(`${checks} dialog control/integer/enumeration/ABI/JIT/lifetime/fault checks passed`);
