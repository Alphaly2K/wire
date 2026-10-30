import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,gdi,finish,invoke,window,rectangle,readRect,color} from './gdi-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';
import {moduleFixture} from './module-fixture.mjs';

const names=['CreateMenu','CreatePopupMenu','DestroyMenu','IsMenu','GetMenu','SetMenu','GetMenuItemCount','InsertMenuItemA','InsertMenuItemW','SetMenuItemInfoA','SetMenuItemInfoW','GetMenuItemInfoA','GetMenuItemInfoW','RemoveMenu','DeleteMenu','GetSubMenu','GetMenuItemID','GetMenuState','EnableMenuItem','CheckMenuItem','DrawMenuBar','TrackPopupMenu','TrackPopupMenuEx','RedrawWindow','LoadMenuA','LoadMenuW','GetSystemMenu'];
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const info=(t,{size=48,mask=2,kind=0,state=0,id=77,sub=0n,checked=0,unchecked=0,data=0,text=0n,count=0,bitmap=0,p=0x22000n}={})=>{
 const fields=[size,mask,kind,state,id,sub,checked,unchecked,data,text,count,bitmap];
 t.vm.write(p,fields.flatMap(f=>Array.from(encode(BigInt(f),4))));return p;
};
const insert=(t,h,options={},position=0xffffffffn,bypos=1n,wide=true)=>call(t,wide?'InsertMenuItemW':'InsertMenuItemA',[h,position,bypos,info(t,options)]);
const query=(t,h,options={},position=0n,bypos=1n,wide=true)=>call(t,wide?'GetMenuItemInfoW':'GetMenuItemInfoA',[h,position,bypos,info(t,options)]);
const client=t=>{call(t,'GetClientRect',[t.h,0x21000n]);return readRect(t,0x21000n);};
const create=(t,atom,menu,{style=0n,procTitle=0n,width=160n,height=120n}={})=>invoke(t,'CreateWindowExW',[0n,atom,procTitle,style,10n,20n,width,height,0n,menu,t.image.base,0n]);

// The complete legacy menu surface binds in PE32 and remains absent in AMD64.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 for(const name of names){
  if(mode===32)assert.ok(vm.bind('user32.dll',name));else assert.throws(()=>vm.bind('user32.dll',name),/migrated|unknown/);
  assert.throws(()=>vm.bind('gdi32.dll',name),/migrated|unknown/);
 }
 if(mode===32)for(const[name,slots]of [['DrawMenuBar',1],['TrackPopupMenu',7],['TrackPopupMenuEx',6],['RedrawWindow',4],['LoadMenuA',2],['LoadMenuW',2],['GetSystemMenu',2]]){
  vm.write(t.error,encode(99n,4));equal(call(t,name,Array(slots).fill(0xdead0000n)),0n);equal(vm.readInt(t.error),120n);equal(vm.register(4),t.sp+BigInt((slots+1)*4));
 }
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Owned raw UTF-16, CP932 byte truncation and masked fields survive source replacement.
for(const size of [44,48]){
 const t=await machine(32),{vm}=t,root=call(t,'CreateMenu'),sub=call(t,'CreatePopupMenu');
 vm.write(0x24000n,Buffer.from('日本\ud800Z\0','utf16le'));
 equal(insert(t,sub,{size,mask:0x6b,state:8,id:77,checked:0x1234,unchecked:0x5678,data:0xfedcba98,text:0x24000n,count:1}),1n);
 vm.write(0x24000n,new Uint8Array(32));equal(insert(t,root,{mask:6,id:77,sub}),1n);
 equal(call(t,'GetSubMenu',[root,0n]),sub);equal(call(t,'GetMenuItemID',[root,0n]),0xffffffffn);
 equal(query(t,root,{size,mask:0x6b,text:0x25000n,count:16},77n,0n),1n);equal(bytes(vm,0x25000n,10),new Uint8Array(Buffer.from('日本\ud800Z\0','utf16le')));equal(vm.readInt(0x22028n),4n);equal(vm.readInt(0x22020n),0xfedcba98n);equal(vm.readInt(0x22018n),0x1234n);equal(vm.readInt(0x2201cn),0x5678n);
 equal(query(t,root,{mask:0x42,text:0x25000n,count:4},77n,0n,false),1n);equal(bytes(vm,0x25000n,4),new Uint8Array([0x93,0xfa,0x96,0]));equal(vm.readInt(0x22028n),3n);
 equal(call(t,'SetMenuItemInfoW',[root,77n,0n,info(t,{mask:1,state:3})]),1n);equal(call(t,'EnableMenuItem',[root,77n,0n]),3n);equal(call(t,'CheckMenuItem',[root,77n,8n]),0n);equal(call(t,'GetMenuState',[root,77n,0n]),8n);
 equal(query(t,root,{mask:0x22},77n,0n),1n);equal(vm.readInt(0x22010n),77n);equal(vm.readInt(0x22020n),0xfedcba98n);equal(vm.readInt(0x22014n),0n);
 equal(call(t,'RemoveMenu',[root,0n,0x400n]),1n);equal(call(t,'IsMenu',[sub]),1n);equal(insert(t,root,{mask:4,sub}),1n);equal(call(t,'DeleteMenu',[root,0n,0x400n]),1n);equal(call(t,'IsMenu',[sub]),0n);
 equal(t.time.calls,0);vm.dispose();
}

// Legacy bitmap/type normalization and insertion order remain shared with the old gateway.
{
 const t=await machine(32),{vm}=t,h=call(t,'CreateMenu');
 equal(insert(t,h,{mask:0x82,id:1,bitmap:1}),1n);equal(insert(t,h,{mask:2,id:2}),1n);equal(call(t,'GetMenuItemID',[h,0n]),2n);equal(call(t,'GetMenuItemID',[h,1n]),1n);
 equal(insert(t,h,{mask:0x12,kind:4,id:3,text:0x12345678n}),1n);equal(query(t,h,{mask:0x90},1n),0n);equal(vm.readInt(t.error),87n);
 equal(query(t,h,{mask:0x10},1n),1n);equal(vm.readInt(0x22008n)&4n,4n);equal(vm.readInt(0x22024n),0x5678n);equal(vm.readInt(0x22028n),0n);
 equal(call(t,'GetMenuState',[h,0n,0x400n])&0x800n,0x800n);vm.dispose();
}

// Graph depth is validated from every ancestor, with cycle and replacement rollback.
{
 const t=await machine(32),{vm}=t,root=call(t,'CreateMenu'),other=call(t,'CreateMenu');let parent=root;
 for(let n=1;n<30;n++){const child=call(t,'CreatePopupMenu');equal(insert(t,parent,{mask:4,sub:child}),1n);parent=child;}
 const extra=call(t,'CreatePopupMenu');equal(insert(t,parent,{mask:4,sub:extra}),0n);equal(vm.readInt(t.error),87n);equal(call(t,'GetMenuItemCount',[parent]),0n);
 equal(insert(t,other,{mask:4,sub:root}),0n);equal(call(t,'GetMenuItemCount',[other]),0n);equal(insert(t,parent,{mask:4,sub:root}),0n);
 equal(insert(t,other,{mask:4,sub:parent}),1n);equal(call(t,'DestroyMenu',[root]),1n);equal(call(t,'IsMenu',[parent]),0n);equal(call(t,'IsMenu',[extra]),1n);equal(call(t,'GetSubMenu',[other,0n]),0n);equal(call(t,'GetMenuItemCount',[other]),1n);vm.dispose();
}

// Menu presence updates coordinates and existing DC backing together; clipped clients round trip.
for(const small of [false,true]){
 const t=await machine(32),{vm}=t;t.h=await window(t);const h=t.h;
 equal(await invoke(t,'MoveWindow',[h,10n,20n,160n,small?12n:120n,0n]),1n);const before=client(t),dc=call(t,'GetDC',[h]);equal(gdi(t,'SetPixel',[dc,0n,0n,0x123456n]),0x123456n);
 const menu=call(t,'CreateMenu'),other=call(t,'CreateMenu');equal(call(t,'ValidateRect',[h,0n]),1n);equal(call(t,'SetMenu',[h,menu]),1n);equal(call(t,'GetMenu',[h]),menu);equal(client(t),[0,0,160,small?0:101]);
 rectangle(t,0x21000n,[0,0,0,0]);equal(call(t,'ClientToScreen',[h,0x21000n]),1n);equal(readRect(t,0x21000n).slice(0,2),[10,39]);equal(call(t,'ScreenToClient',[h,0x21000n]),1n);equal(readRect(t,0x21000n).slice(0,2),[0,0]);
 equal([vm.windowPixels(h).width,vm.windowPixels(h).height],[160,small?1:101]);equal(color(vm.windowPixels(h),0,0),[0x56,0x34,0x12,255]);equal(gdi(t,'GetObjectType',[dc]),3n);
 equal(call(t,'SetMenu',[h,other]),1n);equal(call(t,'GetMenu',[h]),other);equal(client(t),[0,0,160,small?0:101]);equal(call(t,'SetMenu',[h,0n]),1n);equal(client(t),before);equal(gdi(t,'GetPixel',[dc,0n,0n]),0x123456n);
 equal(call(t,'SetMenu',[h,menu]),1n);equal(await invoke(t,'DestroyWindow',[h]),1n);equal(call(t,'IsMenu',[menu]),0n);equal(call(t,'IsMenu',[other]),1n);equal(gdi(t,'GetObjectType',[dc]),0n);vm.dispose();
}

// Recursive Delete/Destroy clear every top-level attachment, preserving numeric child IDs and DCs.
for(const retiring of ['destroy','delete','window']){
 const t=await machine(32),{vm}=t,parent=call(t,'CreateMenu'),sub=call(t,'CreatePopupMenu');equal(insert(t,parent,{mask:4,sub}),1n);
 const top=await window(t),atom=call(t,'GetClassLongW',[top,0xffffffe0n]),shared=await create(t,atom,0n),child=await create(t,atom,0n);for(const h of [top,shared,child]){equal(await invoke(t,'MoveWindow',[h,10n,20n,160n,120n,0n]),1n);equal(call(t,'SetMenu',[h,sub]),1n);}
 call(t,'SetWindowLongW',[child,0xfffffff0n,0x40000000n]);equal(call(t,'SetMenu',[child,parent]),0n);equal(vm.readInt(t.error),1436n);
 const dc=call(t,'GetDC',[shared]);gdi(t,'SetPixel',[dc,0n,0n,0xabcdefn]);equal(retiring==='delete'?call(t,'DeleteMenu',[parent,0n,0x400n]):retiring==='window'?await invoke(t,'DestroyWindow',[top]):call(t,'DestroyMenu',[sub]),1n);
 for(const h of retiring==='window'?[shared]:[top,shared]){equal(call(t,'GetMenu',[h]),0n);equal(call(t,'GetClientRect',[h,0x21000n]),1n);equal(readRect(t,0x21000n),[0,0,160,120]);}
 equal(call(t,'GetWindowLongW',[child,0xfffffff4n]),sub);equal(call(t,'GetMenu',[child]),0n);equal(gdi(t,'GetPixel',[dc,0n,0n]),0xabcdefn);equal(vm.windowPixels(shared).height,120);equal(call(t,'GetMenuItemCount',[parent]),retiring==='delete'?0n:1n);vm.dispose();
}

// Creation accepts a real menu; failed creation retires its attached tree through the same lifecycle.
{
 const t=await machine(32),{vm}=t,base=await window(t),atom=call(t,'GetClassLongW',[base,0xffffffe0n]);
 const menu=call(t,'CreateMenu'),h=await create(t,atom,menu);assert.ok(h);equal(call(t,'GetMenu',[h]),menu);equal(call(t,'GetClientRect',[h,0x21000n]),1n);equal(readRect(t,0x21000n),[0,0,160,101]);
 equal(await create(t,atom,0xbadn),0n);equal(vm.readInt(t.error),1401n);
 vm.write(0x30000n,[0x31,0xc0,0xc2,16,0]);call(t,'SetClassLongW',[base,0xffffffe8n,0x30000n]);const rejected=call(t,'CreateMenu');equal(await create(t,atom,rejected),0n);equal(call(t,'IsMenu',[rejected]),0n);
 equal(call(t,'SetMenu',[0n,menu]),0n);equal(vm.readInt(t.error),1400n);equal(call(t,'SetMenu',[h,0xbadn]),0n);equal(vm.readInt(t.error),1401n);equal(call(t,'GetMenu',[h]),menu);vm.dispose();
}

// Thread retirement destroys its attached menu and repairs surviving shared window/DC state.
{
 const t=await machine(32),{vm}=t,h=await window(t),atom=call(t,'GetClassLongW',[h,0xffffffe0n]),menu=call(t,'CreateMenu');
 equal(await invoke(t,'MoveWindow',[h,10n,20n,160n,120n,0n]),1n);equal(call(t,'SetMenu',[h,menu]),1n);const dc=call(t,'GetDC',[h]);gdi(t,'SetPixel',[dc,0n,0n,0x123456n]);
 const args=[0n,atom,0n,0n,10n,20n,160n,120n,0n,menu,t.image.base,0n],code=[];for(const a of args.toReversed())code.push(0x68,...encode(a,4));code.push(0xb8,...encode(vm.bind('user32.dll','CreateWindowExW'),4),0xff,0xd0,0xa3,...encode(0x23500n,4),0x31,0xc0,0xc2,4,0);vm.write(0x30000n,code);
 const worker=call(t,'CreateThread',[0n,65536n,0x30000n,0n,0n,0n],'kernel32.dll');assert.ok(worker);frame(t,'WaitForSingleObject',[worker,0xffffffffn],'kernel32.dll');equal(await finish(t,1n),0n);const owned=vm.readInt(0x23500n);assert.ok(owned);equal(call(t,'IsWindow',[owned]),0n);equal(call(t,'IsMenu',[menu]),0n);equal(call(t,'GetMenu',[h]),0n);equal(vm.windowPixels(h).height,120);equal(gdi(t,'GetPixel',[dc,0n,0n]),0x123456n);equal(t.time.calls,0);vm.dispose();
}

// Both outputs are checked before either commits; overlapping text/structure preserves commit order.
for(const fault of ['text','struct']){
 const t=await machine(32),{vm}=t,h=call(t,'CreateMenu');vm.write(0x24000n,Buffer.from('owned\0','utf16le'));equal(insert(t,h,{mask:0x42,text:0x24000n}),1n);
 const p=info(t,{mask:0x42,text:0x24ffen,count:8});vm.write(0x24ffen,new Uint8Array(16).fill(0xa5));frame(t,'GetMenuItemInfoW',[h,0n,1n,p]);const before=snapshot(t),old=bytes(vm,p,48),page=fault==='text'?0x25000n:0x22000n;
 equal(core.wire_protect(t.handle,page,4096n,2),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(vm,p,48),old);equal(vm.readInt(0x24ffen,2),0xa5a5n);equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(0x22010n),77n);equal(bytes(vm,0x24ffen,12),new Uint8Array(Buffer.from('owned\0','utf16le')));
 equal(query(t,h,{mask:0x42,text:0x22000n,count:8}),1n);equal(vm.readInt(0x22000n),48n);equal(vm.readInt(0x22010n),77n);
 equal(query(t,h,{mask:0x42,text:t.sp,count:8}),1n);equal(vm.ip,vm.entryPoints.returned);equal(bytes(vm,t.sp,12),new Uint8Array(Buffer.from('owned\0','utf16le')));vm.dispose();
}

// Faults in the input, error output or initial return frame cannot consume a handle or mutate items.
for(const fault of ['input','error','return']){
 const t=await machine(32),{vm}=t,h=call(t,'CreateMenu');vm.write(t.error,encode(77n,4));
 const name=fault==='input'?'InsertMenuItemW':fault==='error'?'DeleteMenu':'CreateMenu';
 const args=fault==='input'?[h,0n,1n,info(t,{mask:0x40,text:0x60000n})]:fault==='error'?[h,0n,0x400n]:[];
 frame(t,name,args);const before=snapshot(t),page=fault==='error'?t.image.teb:t.sp&~4095n;
 if(fault!=='input')equal(core.wire_protect(t.handle,page,4096n,fault==='return'?1:2),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);
 if(fault==='input'){vm.map(0x60000n,4096n,3);vm.write(0x60000n,[65,0,0,0]);}else equal(core.wire_protect(t.handle,page,4096n,4),0);
 equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),fault==='return'?h+4n:fault==='input'?1n:0n);if(fault==='error')equal(vm.readInt(t.error),1456n);else equal(vm.readInt(t.error),77n);equal(call(t,'GetMenuItemCount',[h]),fault==='input'?1n:0n);vm.dispose();
}

// Failure carriers and structure validation keep their order without perturbing unrelated CPU state.
{
 const t=await machine(32),{vm}=t;
 for(const name of ['GetMenuItemCount','GetMenuItemID','GetMenuState','EnableMenuItem','CheckMenuItem']){equal(call(t,name,[0xbadn,0n,0n]),0xffffffffn);equal(vm.readInt(t.error),1401n);}
 const h=call(t,'CreateMenu');vm.write(t.error,encode(77n,4));equal(call(t,'IsMenu',[0xbadn]),0n);equal(vm.readInt(t.error),77n);
 for(const options of [{size:40},{mask:0x200},{mask:0x50},{mask:0x100,kind:4},{mask:4,sub:0xbadn}]){equal(insert(t,h,options),0n);equal(vm.readInt(t.error),87n);equal(call(t,'GetMenuItemCount',[h]),0n);}
 frame(t,'GetMenuItemCount',[h]);for(const n of [1,2,3,5,6,7])vm.register(n,BigInt(0x12340000+n));const registers=[1,2,3,5,6,7].map(n=>vm.register(n));equal(vm.service().reason,Exit.Dispatch);equal([1,2,3,5,6,7].map(n=>vm.register(n)),registers);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// All 27 imports resolve together; real guest instructions create, query and destroy a menu.
{
 const f=fixture(32,{tls:false});f.bytes.fill(0,f.offset(0x2000),f.offset(0x2600));f.rput(0x2000,0x2080);f.rput(0x200c,0x2180);f.rput(0x2010,0x2100);f.rtext(0x2180,'user32.dll\0');let at=0x2200;
 for(const[i,name]of names.entries()){f.rput(0x2080+i*4,at);f.rput(0x2100+i*4,at);f.rtext(at+2,name+'\0');at+=name.length+3;}assert.ok(at<0x2600);
 f.relocations.length=0;let pc=0x1000;const emit=(...b)=>{f.bytes.set(b,f.offset(pc));pc+=b.length;},absolute=v=>{f.absolute(pc,v);pc+=4;};
 emit(0xff,0x15);absolute(f.preferred+0x2100n);emit(0xa3);absolute(f.preferred+0x3000n);emit(0x50,0xff,0x15);absolute(f.preferred+0x210cn);emit(0xa3);absolute(f.preferred+0x3004n);emit(0xff,0x35);absolute(f.preferred+0x3000n);emit(0xff,0x15);absolute(f.preferred+0x2108n);emit(0xc3);f.rebuildRelocations();
 const vm=new WasmMachine(core,{mode:32,clock(){throw new Error('unexpected clock');}}),image=vm.load(f.bytes);for(const[i,name]of names.entries())equal(vm.readInt(image.base+0x2100n+BigInt(i*4)),vm.bind('user32.dll',name));equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),1n);equal(vm.readInt(image.base+0x3004n),1n);assert.ok(vm.retired>0n);vm.dispose();
}

// A rejected static startup discards process-owned menus along with the rest of USER state.
{
 const f=moduleFixture(32,{dll:true,dependency:'user32.dll'});f.rtext(0x20e2,'CreateMenu\0');f.bytes.set([0xff,0x15,...encode(f.preferred+0x20a0n,4),0xa3,...encode(0x21000n,4),0x31,0xc0,0xc2,12,0],f.offset(0x1000));f.relocations.splice(0,f.relocations.length,0x1002);f.rebuildRelocations();
 const vm=new WasmMachine(core,{mode:32}),main=moduleFixture(32,{dependency:'menu-start.dll'});vm.map(0x20000n,0x30000n,7);vm.supplyModule('menu-start.dll',f.bytes);const image=vm.load(main.bytes);let rejected=false;
 for(let n=0;n<100;n++){try{equal((await vm.run(1n)).reason,Exit.Budget);}catch(error){assert.match(error.message,/attach returned FALSE/);rejected=true;break;}}
 assert.ok(rejected);assert.ok(vm.readInt(0x21000n));equal(vm.windows(),[]);equal(vm.moduleBase('menu-start.dll'),0n);assert.throws(()=>vm.readInt(image.base),/memory access/);vm.dispose();
}

console.log(`${checks} menu graph/item/window/DC/ABI/JIT/lifetime/fault checks passed`);
