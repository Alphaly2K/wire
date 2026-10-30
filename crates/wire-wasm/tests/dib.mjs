import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,gdi,window,color} from './gdi-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {moduleFixture} from './module-fixture.mjs';

function info(t,{width=3,height=2,compression=0,masks=[0xff0000,0xff00,0xff],pointer=0x21000n}={}) {
 const fields=[40,width,height,0x200001,compression,0,0,0,0,0,...masks];
 t.vm.write(pointer,fields.flatMap(n=>Array.from(encode(BigInt.asUintN(32,BigInt(n)),4))));
 return pointer;
}
function dib(t,options={}){
 const p=info(t,options),h=gdi(t,'CreateDIBSection',[0n,p,0n,0x21100n,0n,0n]);assert.ok(h);
 return {h,base:t.vm.readInt(0x21100n,t.mode/8)};
}
const get=(t,p)=>Number(t.vm.readInt(p));
const read=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));

// Direct guest BGRA writes, both row directions, DIBSECTION ABI layouts and GDI write-through.
for(const mode of [32,64])for(const height of [2,-2])for(const compression of [0,3]){
 const t=await machine(mode),{vm}=t,{h,base}=dib(t,{height,compression}),dc=gdi(t,'CreateCompatibleDC',[0n]),stock=gdi(t,'SelectObject',[dc,h]);
 if(height===2&&compression===0)equal(gdi(t,'GetDeviceCaps',[dc,38n]),0x2001n);
 equal(read(vm,base,24),new Uint8Array(24));vm.write(base,[...encode(0x99112233n,4),...encode(0x88445566n,4)]);const y=height<0?0n:1n;
 equal(gdi(t,'GetPixel',[dc,0n,y]),0x332211n);equal(gdi(t,'GetPixel',[dc,1n,y]),0x665544n);equal(gdi(t,'SetPixel',[dc,2n,y,0xabcdefn]),0xabcdefn);equal(vm.readInt(base+8n),0xefcdabn);
 const full=mode===32?84n:104n,bm=mode===32?24n:32n;equal(gdi(t,'GetObjectW',[h,0n,0n]),full);equal(gdi(t,'GetObjectW',[h,full,0x21200n]),full);equal(vm.readInt(0x21200n+BigInt(mode===32?20:24),mode/8),base);equal(vm.readInt(0x21200n+bm+8n),BigInt.asUintN(32,BigInt(height)));equal(vm.readInt(0x21200n+bm+16n),BigInt(compression));equal(gdi(t,'GetObjectA',[h,bm,0x21200n]),bm);
 const compatible=gdi(t,'CreateCompatibleBitmap',[dc,2n,3n]);assert.ok(compatible);equal(gdi(t,'GetObjectW',[compatible,full,0x21200n]),full);equal(vm.readInt(0x21200n+bm+8n),BigInt.asUintN(32,height<0?-3n:3n));equal(vm.readInt(0x21200n+bm+16n),BigInt(compression));const copied=vm.readInt(0x21200n+BigInt(mode===32?20:24),mode/8);assert.ok(copied&&copied!==base);equal(vm.readInt(copied),0n);equal(gdi(t,'DeleteObject',[compatible]),1n);const empty=gdi(t,'CreateCompatibleBitmap',[dc,0n,3n]);equal(gdi(t,'GetObjectW',[empty,0n,0n]),bm);equal(gdi(t,'DeleteObject',[empty]),1n);
 equal(gdi(t,'DeleteObject',[h]),0n);equal(vm.readInt(t.error),170n);
 equal(gdi(t,'SelectObject',[dc,stock]),h);equal(gdi(t,'DeleteObject',[h]),1n);assert.throws(()=>vm.readInt(base),/memory access/);equal(gdi(t,'DeleteDC',[dc]),1n);equal(t.time.calls,0);vm.dispose();
}

// GetDIBits snapshots before aliased output, preserves scan order and checks the complete output.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,{h,base}=dib(t,{width:2,height:-3}),dc=gdi(t,'CreateCompatibleDC',[0n]);
 vm.write(base,[1,2,3,99,4,5,6,99,7,8,9,99,10,11,12,99,13,14,15,99,16,17,18,99]);
 info(t,{width:2,height:-3});equal(gdi(t,'GetDIBits',[dc,h,1n,9n,0x21200n,0x21000n,0n]),2n);equal(read(vm,0x21200n,16),Uint8Array.from([7,8,9,0,10,11,12,0,13,14,15,0,16,17,18,0]));
 info(t,{width:2,height:3});equal(gdi(t,'GetDIBits',[dc,h,0n,2n,0x21200n,0x21000n,0n]),2n);equal(read(vm,0x21200n,16),Uint8Array.from([13,14,15,0,16,17,18,0,7,8,9,0,10,11,12,0]));
 info(t,{width:2,height:-3});frame(t,'GetDIBits',[dc,h,0n,3n,0x24ffcn,0x21000n,0n],'gdi32.dll');vm.write(0x24ffcn,[0xcc,0xcc,0xcc,0xcc]);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(0x24ffcn),0xccccccccn);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),3n);
 info(t,{width:2,height:3});equal(gdi(t,'GetDIBits',[dc,h,0n,3n,base,0x21000n,0n]),3n);equal(read(vm,base,8),Uint8Array.from([13,14,15,0,16,17,18,0]));
 info(t,{width:2,height:3});equal(gdi(t,'GetDIBits',[dc,h,0n,0n,0n,0x21000n,0n]),3n);equal([get(t,0x21004n),get(t,0x21008n),get(t,0x2100cn),get(t,0x21014n)],[2,3,0x200001,24]);
 equal(gdi(t,'GetDIBits',[dc,h,3n,1n,0xdead0000n,0x21000n,0n]),0n);equal(t.time.calls,0);vm.dispose();
}

// Blits snapshot overlap and preflight every destination row before changing guest memory.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,{h,base}=dib(t,{width:4,height:-2}),dc=gdi(t,'CreateCompatibleDC',[0n]);gdi(t,'SelectObject',[dc,h]);for(let x=0;x<4;x++)gdi(t,'SetPixelV',[dc,BigInt(x),0n,BigInt(x+1)]);equal(gdi(t,'BitBlt',[dc,1n,0n,3n,1n,dc,0n,0n,0xcc0020n]),1n);equal([0n,1n,2n,3n].map(x=>gdi(t,'GetPixel',[dc,x,0n])),[1n,1n,2n,3n]);
 equal(core.wire_protect(t.handle,base,4096n,2),0);frame(t,'SetPixelV',[dc,0n,0n,9n],'gdi32.dll');const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(base),0x10000n);equal(core.wire_protect(t.handle,base,4096n,4),0);equal(gdi(t,'SetPixelV',[dc,0n,0n,9n]),1n);
 const big=dib(t,{width:1024,height:-2});const other=gdi(t,'CreateCompatibleDC',[0n]);gdi(t,'SelectObject',[other,big.h]);vm.write(big.base,[0xcc,0xcc,0xcc,0xcc]);equal(core.wire_protect(t.handle,big.base+4096n,4096n,2),0);frame(t,'BitBlt',[other,0n,0n,4n,2n,dc,0n,0n,0xcc0020n],'gdi32.dll');equal(vm.service().reason,Exit.Fault);equal(vm.readInt(big.base),0xccccccccn);equal(core.wire_protect(t.handle,big.base+4096n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(big.base),0x90000n);equal(vm.readInt(big.base+4096n),0n);vm.dispose();
}

// StretchDIBits uses 13 captured arguments, source snapshot, scaling, orientation and destination clipping.
for(const mode of [32,64])for(const height of [2,-2]){
 const t=await machine(mode),{vm}=t,w=await window(t),dc=call(t,'GetDC',[w]),p=info(t,{width:2,height,compression:3});vm.write(0x22000n,[1,2,3,255,4,5,6,255,7,8,9,255,10,11,12,255]);
 equal(gdi(t,'StretchDIBits',[dc,0n,0n,4n,4n,0n,0n,2n,2n,0x22000n,p,0n,0xcc0020n]),2n);equal(color(vm.windowPixels(w),0,0),height<0?[3,2,1,255]:[9,8,7,255]);equal(color(vm.windowPixels(w),3,3),height<0?[12,11,10,255]:[6,5,4,255]);equal(gdi(t,'StretchDIBits',[dc,0n,0n,1n,1n,0n,0n,1n,1n,0x22000n,p,0n,0xcc0020n]),1n);equal(color(vm.windowPixels(w),0,0),[3,2,1,255]);
 const {h,base}=dib(t,{width:2,height:-2}),mem=gdi(t,'CreateCompatibleDC',[0n]);gdi(t,'SelectObject',[mem,h]);vm.write(base,[1,2,3,0,4,5,6,0,7,8,9,0,10,11,12,0]);info(t,{width:2,height:-2});equal(gdi(t,'StretchDIBits',[mem,1n,0n,2n,2n,0n,0n,2n,2n,base,0x21000n,0n,0xcc0020n]),2n);equal(vm.readInt(base+4n),0x30201n);equal(vm.readInt(base+12n),0x90807n);equal(t.time.calls,0);vm.dispose();
}

// Failed pointer/mask validation leaves capacity and identities available; output can alias the return slot.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;info(t);frame(t,'CreateDIBSection',[0n,0x21000n,0n,0x24ffen,0n,0n],'gdi32.dll');vm.write(0x24ffen,[0xcc,0xcc]);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(read(vm,0x24ffen,2),Uint8Array.from([0xcc,0xcc]));equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);const first=vm.register(0);equal(gdi(t,'DeleteObject',[first]),1n);
 info(t,{compression:3,masks:[1,2,3]});equal(gdi(t,'CreateDIBSection',[0n,0x21000n,0n,0x21100n,0n,0n]),0n);equal(vm.readInt(t.error),50n);info(t);equal(gdi(t,'CreateDIBSection',[0n,0x21000n,0n,t.sp,0n,0n]),first+16n);equal(vm.ip,vm.entryPoints.returned);assert.ok(vm.readInt(t.sp,mode/8));vm.dispose();
}

// GDI writes revoke observed executable DIB source in PE32 without DEP; deletion removes the code authority.
{
 const t=await machine(32,{dep:false}),{vm}=t,{h,base}=dib(t,{width:2,height:-1}),dc=gdi(t,'CreateCompatibleDC',[0n]),stock=gdi(t,'SelectObject',[dc,h]);
 vm.write(base,[0xb8,1,0,0,0,0xc3,0x90,0x90]);const run=async()=>{vm.ip=base;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,4));return vm.run(10n);};equal((await run()).reason,Exit.Returned);equal(vm.register(0),1n);equal(gdi(t,'SetPixelV',[dc,0n,0n,0xb80200n]),1n);equal((await run()).reason,Exit.Returned);equal(vm.register(0),2n);gdi(t,'SelectObject',[dc,stock]);equal(gdi(t,'DeleteObject',[h]),1n);equal((await run()).reason,Exit.Fault);vm.dispose();
}

// A DLL that rejects static startup cannot leave its DIB allocation mapped.
for(const mode of [32,64]){
 const vm=new WasmMachine(core,{mode}),api=vm.bind('gdi32.dll','CreateDIBSection'),f=moduleFixture(mode,{dll:true,attach:false}),code=mode===32?[0x83,0x7c,0x24,8,1,0x75,0]:[0x83,0xfa,1,0x75,0],jump=code.length-1;
 if(mode===32)code.push(0x6a,0,0x6a,0,0x68,...encode(0x21100n,4),0x6a,0,0x68,...encode(0x21000n,4),0x6a,0,0xb8,...encode(api,4),0xff,0xd0);
 else code.push(0x48,0x83,0xec,56,0x48,0x31,0xc9,0x48,0xba,...encode(0x21000n,8),0x45,0x31,0xc0,0x49,0xb9,...encode(0x21100n,8),0x48,0xc7,0x44,0x24,32,0,0,0,0,0x48,0xc7,0x44,0x24,40,0,0,0,0,0x48,0xb8,...encode(api,8),0xff,0xd0,0x48,0x83,0xc4,56);
 code.push(0x31,0xc0,...(mode===32?[0xc2,12,0]:[0xc3]));code[jump]=code.length-jump-1;code.push(0xb8,1,0,0,0,...(mode===32?[0xc2,12,0]:[0xc3]));f.bytes.fill(0x90,f.offset(0x1000),f.offset(0x1080));f.bytes.set(code,f.offset(0x1000));f.relocations.splice(0,f.relocations.length,...f.relocations.filter(r=>r<0x1000||r>=0x1080));f.rebuildRelocations();
 vm.map(0x20000n,0x4000n);info({vm},{width:2,height:-2});vm.supplyModule('dib.dll',f.bytes);vm.load(moduleFixture(mode,{dependency:'dib.dll'}).bytes);await assert.rejects(vm.run(10000n),/attach returned FALSE/);const base=vm.readInt(0x21100n,mode/8);assert.ok(base);assert.throws(()=>vm.readInt(base),/memory access/);equal(vm.stats(),{modules:0,chargedBytes:0,instances:0});vm.dispose();
}

console.log(`${checks} DIB backing/layout/transfer/ownership/transaction checks passed`);
