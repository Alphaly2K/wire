import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,finish,gdi} from './gdi-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';

const names=['LoadIconA','LoadIconW','LoadCursorA','LoadCursorW','LoadImageA','LoadImageW','DestroyIcon','DestroyCursor'];
const text=(vm,p,s,wide=false)=>vm.write(p,Buffer.from(s+'\0',wide?'utf16le':'ascii'));
const start=mode=>mode===32?0x44000000n:0x7ffd00610000n;

// Image identities must not overlap GDI stock or dynamic objects.
{
 const t=await machine(32),{vm}=t,icons=[];
 for(let id=32512;id<=32516;id++)icons.push(call(t,'LoadIconW',[0n,BigInt(id)]));
 const brush=gdi(t,'GetStockObject',[0n]);assert.ok(icons.every(icon=>icon!==brush));
 equal(call(t,'DestroyIcon',[brush]),0n);equal(vm.readInt(t.error),6n);equal(gdi(t,'DeleteObject',[icons[4]]),0n);
 equal(call(t,'LoadIconW',[0n,32516n]),icons[4]);equal(gdi(t,'GetStockObject',[0n]),brush);vm.dispose();
}
function resourceFixture(mode){
 const f=fixture(mode,{tls:false}),p=(offset,value,size=4)=>f.rput(0x3000+offset,value,size);
 f.put(f.dirs+8,0);f.put(f.dirs+12,0);f.bytes[f.offset(0x1000)]=0xc3;
 f.put(f.dirs+16,0x3000);f.put(f.dirs+20,0x160);
 f.bytes.fill(0,f.offset(0x3000),f.offset(0x3200));
 for(const [dir,entries]of [[0,[[3,0x80000030],[12,0x80000060],[14,0x80000090]]],[0x30,[[5,0x800000c0]]],[0x60,[[104,0x800000d8]]],[0x90,[[0x80000130,0x800000f0],[103,0x800000f0]]],[0xc0,[[0x409,0x108]]],[0xd8,[[0x409,0x118]]],[0xf0,[[0x409,0x118]]]]){
  p(dir+14,entries.length,2);entries.forEach(([name,value],n)=>{p(dir+16+n*8,name);p(dir+20+n*8,value);});
 }
 p(0x108,0x3180);p(0x10c,64);p(0x118,0x31c0);p(0x11c,20);
 p(0x130,3,2);[0x30bd,0x30d5,0x30c8].forEach((c,n)=>p(0x132+n*2,c,2));
 p(0x180,40);p(0x184,2);p(0x188,4);p(0x18c,1,2);p(0x18e,32,2);
 p(0x1c2,1,2);p(0x1c4,1,2);p(0x1c6,0x202,2);p(0x1ca,1,2);p(0x1cc,32,2);p(0x1ce,64);p(0x1d2,5,2);
 return f;
}
async function resources(mode,base){
 let handle;const vm=new WasmMachine({...core,wire_create(...a){handle=core.wire_create(...a);return handle;}},{mode,clock(){throw new Error('unexpected clock');}});
 const f=resourceFixture(mode),image=vm.load(f.bytes,{base});equal((await vm.run(100n)).reason,Exit.Returned);vm.map(0x20000n,0x30000n,7);
 return{vm,mode,handle,image,sp:image.sp-2048n,error:image.teb+BigInt(mode===32?0x34:0x68),f};
}

// Shared resources keep identity across A/W calls; owned objects are never resurrected.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 for(const name of names){assert.ok(vm.bind('user32.dll',name));for(const dll of ['kernel32.dll','kernelbase.dll'])assert.throws(()=>vm.bind(dll,name),/unknown/);}
 vm.flags=0x8d5n;vm.write(t.error,encode(77n,4));
 const shared=call(t,'LoadCursorA',[0n,32512n]);equal(shared,start(mode));equal(call(t,'LoadCursorW',[0n,32512n]),shared);
 equal(call(t,'LoadImageW',[0n,32512n,2n,0n,0n,0x8040n]),shared);equal(call(t,'DestroyCursor',[shared]),1n);equal(call(t,'LoadCursorA',[0n,32512n]),shared);
 const owned=call(t,'LoadImageA',[0n,32512n,2n,0n,0n,0n]);equal(owned,shared+16n);equal(call(t,'SetCursor',[owned]),0n);equal(call(t,'GetCursor'),owned);equal(vm.flags,0x8d5n);equal(vm.readInt(t.error),77n);
 equal(call(t,'DestroyIcon',[owned]),0n);equal(vm.readInt(t.error),6n);equal(call(t,'GetCursor'),owned);
 equal(call(t,'DestroyCursor',[owned]),1n);equal(call(t,'GetCursor'),owned);equal(call(t,'SetCursor',[owned]),0n);equal(vm.readInt(t.error),6n);equal(call(t,'GetCursor'),owned);
 equal(call(t,'DestroyCursor',[owned]),0n);equal(call(t,'LoadImageW',[0n,32512n,2n,0n,0n,0n]),owned+16n);
 const icon=call(t,'LoadIconW',[0n,32512n]);equal(call(t,'SetCursor',[icon]),0n);equal(vm.readInt(t.error),6n);equal(call(t,'DestroyIcon',[icon]),1n);equal(call(t,'SetCursor',[shared]),owned);equal(call(t,'GetCursor'),shared);
 if(mode===64){equal(call(t,'SetCursor',[shared&0xffffffffn]),0n);equal(call(t,'DestroyCursor',[shared&0xffffffffn]),0n);equal(vm.readInt(t.error),6n);equal(call(t,'LoadImageW',[0n,32512n,0xabcdef0000000002n,1n<<32n,1n<<32n,0x1234000000008000n]),shared);}
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Unsupported formats and dimensions fail before dereferencing the resource name.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 for(const args of [[0n,0x60000n,2n,1n,0n,0n],[0n,0x60000n,0n,0n,0n,0n],[0n,0x60000n,1n,0n,0n,0x10n]]){equal(call(t,'LoadImageA',args),0n);equal(vm.readInt(t.error),50n);}
 equal(call(t,'LoadIconA',[0n,32519n]),0n);equal(vm.readInt(t.error),1814n);
 text(vm,0x20000n,'missing');equal(call(t,'LoadIconA',[0n,0x20000n]),0n);equal(vm.readInt(t.error),1814n);
 equal(call(t,'LoadIconW',[123n,103n]),0n);equal(vm.readInt(t.error),6n);
 frame(t,'LoadIconW',[0n,0x60000n]);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(call(t,'LoadIconA',[0n,32512n]),start(mode));vm.dispose();
}

// Loaded PE resources use complete module and string pointers, with the original payload retained.
for(const mode of [32,64]){
 const t=await resources(mode,mode===64?0x123400400000n:undefined),{vm,image}=t;
 vm.write(t.error,encode(91n,4));const icon=call(t,'LoadIconA',[image.base,103n]);equal(icon,start(mode));equal(call(t,'LoadIconW',[image.base,103n]),icon);
 const pointer=mode===64?0x123500000000n:0x21000n;if(mode===64)vm.map(pointer,4096n,7);
 text(vm,pointer,'ソフト',true);const named=call(t,'LoadIconW',[image.base,pointer]);assert.notEqual(named,0n);vm.write(pointer,[0x83,0x5c,0x83,0x74,0x83,0x67,0]);equal(call(t,'LoadIconA',[image.base,pointer]),named);
 equal(vm.readInt(t.error),91n);const owned=call(t,'LoadImageA',[image.base,103n,1n,0n,0n,0n]);assert.notEqual(owned,icon);equal(call(t,'DestroyIcon',[owned]),1n);equal(call(t,'DestroyIcon',[owned]),0n);equal(vm.readInt(t.error),6n);
 equal(call(t,'LoadCursorW',[image.base,104n]),0n);equal(vm.readInt(t.error),50n);equal(call(t,'LoadIconW',[image.base,999n]),0n);equal(vm.readInt(t.error),1814n);
 if(mode===64){equal(call(t,'LoadIconW',[image.base&0xffffffffn,103n]),0n);equal(vm.readInt(t.error),6n);}
 // Shared loads preserve their independent resource after the original bytes become inaccessible.
 equal(core.wire_protect(t.handle,image.base+0x3000n,4096n,1),0);equal(call(t,'LoadIconA',[image.base,103n]),icon);
 frame(t,'LoadImageW',[image.base,103n,1n,0n,0n,0n]);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);
 equal(core.wire_protect(t.handle,image.base+0x3000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);assert.notEqual(vm.register(0),0n);vm.dispose();
}

// Unloading retires shared cache keys before a different DLL payload reuses the same base.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,f=resourceFixture(mode);f.put(0x96,0x2102,2);f.bytes.set([0xb8,1,0,0,0,...(mode===32?[0xc2,12,0]:[0xc3])],f.offset(0x1000));
 const load=async(name)=>{vm.supplyModule(name,f.bytes);text(vm,0x20000n,name);frame(t,'LoadLibraryA',[0x20000n],'kernel32.dll');await finish(t);return vm.register(0);};
 const base=await load('icons.dll');assert.notEqual(base,0n);const old=call(t,'LoadIconW',[base,103n]);assert.notEqual(old,0n);
 frame(t,'FreeLibrary',[base],'kernel32.dll');equal(await finish(t),1n);equal(call(t,'LoadIconW',[base,103n]),0n);equal(vm.readInt(t.error),6n);
 f.rput(0x318e,24,2);equal(await load('replacement.dll'),base);equal(call(t,'LoadIconW',[base,103n]),0n);equal(vm.readInt(t.error),50n);
 vm.write(base+0x318en,encode(32n,2));const fresh=call(t,'LoadIconW',[base,103n]);assert.notEqual(fresh,0n);assert.notEqual(fresh,old);equal(call(t,'DestroyIcon',[old]),1n);equal(call(t,'LoadIconW',[base,103n]),fresh);vm.dispose();
}

// Malformed trees, groups and DIBs cannot publish an image; format rejection is distinct.
for(const mode of [32,64])for(const [at,size,value,error]of [[0x14,4,0x80000160n,13n],[0x3e,2,65535n,13n],[0x108,4,0xfffffff0n,13n],[0x10c,4,0x400001n,13n],[0x1c0,2,7n,13n],[0x1c4,2,0n,13n],[0x184,4,0n,13n],[0x18e,2,24n,50n],[0x1d2,2,99n,1814n]]){
 const t=await resources(mode),{vm,image}=t;vm.write(image.base+0x3000n+BigInt(at),encode(value,size));equal(call(t,'LoadIconW',[image.base,103n]),0n);equal(vm.readInt(t.error),error);equal(call(t,'LoadCursorA',[0n,32512n]),start(mode));vm.dispose();
}

// An inaccessible return or error slot leaves CPU and the image namespace unchanged.
for(const mode of [32,64])for(const kind of ['return','error']){
 const t=await machine(mode),{vm}=t;frame(t,kind==='return'?'LoadCursorA':'DestroyCursor',kind==='return'?[0n,32512n]:[123n]);
 const page=kind==='return'?t.sp&~4095n:t.image.teb;equal(core.wire_protect(t.handle,page,4096n,kind==='return'?1:2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);
 equal(call(t,'LoadCursorW',[0n,32512n]),start(mode));vm.dispose();
}

// Capacity is bounded and destroyed identities are not reused, including after an error-slot fault.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;let first,last;
 for(let n=0;n<4096;n++){last=call(t,'LoadImageA',[0n,32512n,2n,0n,0n,0n]);if(n===0)first=last;}
 equal(last,first+4095n*16n);frame(t,'LoadImageW',[0n,32512n,2n,0n,0n,0n]);equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0n);equal(vm.readInt(t.error),8n);
 equal(call(t,'DestroyCursor',[first]),1n);equal(call(t,'LoadCursorW',[0n,32512n]),0n);equal(vm.readInt(t.error),8n);vm.dispose();
}

// Every export binds through a real static import; execution uses the normal JIT service boundary.
for(const mode of [32,64]){
 const f=fixture(mode,{tls:false});f.rtext(0x2220,'user32.dll\0');f.rput(0x200c,0x2220);f.rput(0x2010,0x2140);f.rput(0x2080,0);let at=0x2300;
 for(const[n,name]of names.entries()){f.rput(0x2080+n*f.ptr,at,f.ptr);f.rput(0x2140+n*f.ptr,at,f.ptr);f.rtext(at+2,name+'\0');at+=name.length+3;}
 f.rput(0x2080+names.length*f.ptr,0,f.ptr);f.rput(0x2140+names.length*f.ptr,0,f.ptr);
 const code=mode===32?[0x68,...encode(32512n,4),0x6a,0,0xff,0x15,...encode(f.preferred+0x2140n+BigInt(2*f.ptr),4),0xc3]:[0x48,0x83,0xec,0x28,0x31,0xc9,0xba,...encode(32512n,4),0xff,0x15,...encode(BigInt(0x2140+2*f.ptr-0x1011),4),0x48,0x83,0xc4,0x28,0xc3];
 f.bytes.set(code,f.offset(0x1000));f.relocations.splice(0,f.relocations.length,...(mode===32?[0x1009]:[]));f.rebuildRelocations();
 const vm=new WasmMachine(core,{mode}),image=vm.load(f.bytes);for(const[n,name]of names.entries())equal(vm.readInt(image.base+0x2140n+BigInt(n*f.ptr),f.ptr),vm.bind('user32.dll',name));equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),start(mode));vm.dispose();
}
console.log(`${checks} image ownership/resource/ABI/capacity/JIT/fault checks passed`);
