import assert from 'node:assert/strict';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';
import {moduleFixture} from './module-fixture.mjs';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
const kernel=(t,name,a=[])=>call(t,name,a,'kernel32.dll');
const gdip=(t,name,a=[])=>{const r=call(t,name,a,'gdiplus.dll');equal(t.vm.register(4),t.sp+4n+BigInt(a.length*4));return r;};
const bits=n=>{const b=new ArrayBuffer(4),v=new DataView(b);v.setFloat32(0,n,true);return BigInt(v.getUint32(0,true));};
const floats=(t,p,a)=>t.vm.write(p,a.flatMap(n=>Array.from(encode(bits(n),4))));
const values=(t,p,n=6)=>Array.from({length:n},(_,i)=>{const b=encode(t.vm.readInt(p+BigInt(i*4)),4);return new DataView(b.buffer,b.byteOffset,4).getFloat32(0,true);});
const create=(t,m=[1,0,0,1,0,0],out=0x21000n)=>{equal(gdip(t,'GdipCreateMatrix2',[...m.map(bits),out]),0n);return t.vm.readInt(out);};
const elements=(t,p)=>{equal(gdip(t,'GdipGetMatrixElements',[p,0x22000n]),0n);return values(t,0x22000n);};
const read=(t,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(t.vm.readInt(p+BigInt(i),1)));
const input=t=>{t.vm.write(0x20500n,new Uint8Array(16));t.vm.write(0x20500n,encode(1n,4));};

// Public lookup, static imports and generated PE32 code share the same callable ABI.
{
 const t=await machine(32),{vm}=t;input(t);const mod=(()=>{vm.write(0x20000n,new TextEncoder().encode('gdiplus.dll\0'));return kernel(t,'LoadLibraryA',[0x20000n]);})();assert.ok(mod);
 vm.write(0x20000n,new TextEncoder().encode('GdipCreateMatrix\0'));const pc=kernel(t,'GetProcAddress',[mod,0x20000n]);assert.ok(pc);equal(kernel(t,'GetModuleHandleExW',[6n,pc,0x21004n]),1n);equal(vm.readInt(0x21004n),mod);equal(pc,vm.bind('GDIPLUS','GdipCreateMatrix'));assert.throws(()=>vm.bind('user32.dll','GdipCreateMatrix'),/unknown/);assert.throws(()=>vm.bind('gdiplus.dll','GdipMadeUpExport'),/unknown/);assert.throws(()=>vm.bind('gdiplus.dll','#1'),/unknown/);
 equal(gdip(t,'GdiplusStartup',[0x20600n,0x20500n,0n]),0n);equal(vm.readInt(0x20600n),1n);equal(gdip(t,'GdiplusStartup',[0x20604n,0x20500n,0n]),0n);equal(vm.readInt(0x20604n),2n);gdip(t,'GdiplusShutdown',[1n]);gdip(t,'GdiplusShutdown',[2n]);
 vm.write(0x30000n,[0x68,...encode(0x21000n,4),0xb8,...encode(pc,4),0xff,0xd0,0xa1,...encode(0x21000n,4),0xc3]);vm.ip=0x30000n;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,4));equal((await vm.run(100n)).reason,Exit.Returned);const p=vm.register(0);equal(elements(t,p),[1,0,0,1,0,0]);gdip(t,'GdipDeleteMatrix',[p]);equal(t.time.calls,0);vm.dispose();
 const f=fixture(32,{tls:false});f.rtext(0x20c0,'gdiplus.dll\0');f.rtext(0x20e2,'GdipCreateMatrix\0');const direct=new WasmMachine(core,{mode:32,clock(){throw new Error('unexpected clock');}}),image=direct.load(f.bytes);equal((await direct.run(100n)).reason,Exit.Returned);equal(direct.register(0),direct.readInt(image.base+0x3000n));assert.ok(direct.register(0));direct.dispose();
 const q=await machine(64);assert.throws(()=>q.vm.bind('gdiplus.dll','GdipCreateMatrix'),/unknown/);q.vm.dispose();
}
// Declared unavailable exports return status 6 and clean GUID/scalar argument widths.
{
 const t=await machine(32),{vm}=t;vm.write(0x21000n,encode(0xa5a5a5a5n,4));
 for(const[name,width]of[['GdipCreateSolidFill',8],['GdipCreateEffect',20],['GdipDrawImagePointsRect',48],['GdipDrawImageRectRectI',56]]){equal(gdip(t,name,new Array(width/4).fill(0x21000n)),6n);equal(vm.readInt(0x21000n),0xa5a5a5a5n);}
 input(t);vm.write(0x20500n,encode(3n,4));equal(gdip(t,'GdiplusStartup',[0x21000n,0x20500n,0n]),17n);equal(vm.readInt(0x21000n),0xa5a5a5a5n);vm.write(0x20500n,encode(1n,4));vm.write(0x20508n,encode(1n,4));equal(gdip(t,'GdiplusStartup',[0x21000n,0x20500n,0n]),2n);equal(gdip(t,'GdiplusStartup',[0x21000n,0x20500n,0x21100n]),6n);equal(vm.readInt(0x21000n),0xa5a5a5a5n);equal(gdip(t,'GdipCreateMatrix',[0n]),2n);equal(t.time.calls,0);vm.dispose();
}
// The retained affine algorithm preserves prepend/append, clone, inverse and object ownership.
{
 const t=await machine(32),{vm}=t;equal(gdip(t,'GdipCreateMatrix',[0x21000n]),0n);const m=vm.readInt(0x21000n);equal(gdip(t,'GdipTranslateMatrix',[m,bits(3),bits(5),1n]),0n);equal(gdip(t,'GdipCloneMatrix',[m,0x21004n]),0n);const c=vm.readInt(0x21004n);
 equal(gdip(t,'GdipScaleMatrix',[m,bits(2),bits(4),1n]),0n);equal(elements(t,m),[2,0,0,4,6,20]);equal(gdip(t,'GdipScaleMatrix',[c,bits(2),bits(4),0n]),0n);equal(elements(t,c),[2,0,0,4,3,5]);equal(gdip(t,'GdipInvertMatrix',[m]),0n);equal(elements(t,m),[0.5,0,0,0.25,-3,-5]);equal(gdip(t,'GdipMultiplyMatrix',[m,c,1n]),0n);equal(elements(t,m),[1,0,0,1,-3,-15]);
 equal(gdip(t,'GdipRotateMatrix',[m,bits(90),0n]),0n);elements(t,m).forEach((n,i)=>assert.ok(Math.abs(n-[0,1,-1,0,-3,-15][i])<1e-6));equal(gdip(t,'GdipSetMatrixElements',[m,...[1,0,0,1,0,0].map(bits)]),0n);equal(gdip(t,'GdipShearMatrix',[m,bits(2),bits(3),1n]),0n);equal(elements(t,m),[1,3,2,1,0,0]);equal(gdip(t,'GdipTranslateMatrix',[m,bits(1),bits(2),2n]),2n);equal(elements(t,m),[1,3,2,1,0,0]);
 equal(gdip(t,'GdipFree',[m]),0n);equal(elements(t,m),[1,3,2,1,0,0]);equal(gdip(t,'GdipDeleteMatrix',[m]),0n);equal(gdip(t,'GdipDeleteMatrix',[m]),2n);equal(gdip(t,'GdipGetMatrixElements',[m,0x22000n]),2n);gdip(t,'GdipDeleteMatrix',[c]);vm.dispose();
}
// Rectangles map three supplied points; integer point conversion keeps Wine's half rounding.
for(const integer of[false,true]){
 const t=await machine(32),{vm}=t;const rect=[2,4,4,8],points=[10,20,18,24,34,52];if(integer){vm.write(0x23000n,rect.flatMap(n=>Array.from(encode(BigInt(n),4))));vm.write(0x23100n,points.flatMap(n=>Array.from(encode(BigInt(n),4))));}else{floats(t,0x23000n,rect);floats(t,0x23100n,points);}equal(gdip(t,'GdipCreateMatrix3'+(integer?'I':''),[0x23000n,0x23100n,0x21000n]),0n);const m=vm.readInt(0x21000n);equal(elements(t,m),[2,1,3,4,-6,2]);gdip(t,'GdipDeleteMatrix',[m]);
 const p=create(t,[2,0,0,3,4,5]);for(const vector of[false,true]){const name='Gdip'+(vector?'Vector':'')+'TransformMatrixPoints'+(integer?'I':'');if(integer)vm.write(0x23000n,[1,2,-2,-3].flatMap(n=>Array.from(encode(BigInt.asUintN(32,BigInt(n)),4))));else floats(t,0x23000n,[1,2,-2,-3]);equal(gdip(t,name,[p,0x23000n,2n]),0n);equal(integer?[0n,4n,8n,12n].map(o=>Number(BigInt.asIntN(32,vm.readInt(0x23000n+o)))):values(t,0x23000n,4),vector?[2,6,-4,-9]:[6,11,0,-4]);equal(gdip(t,name,[p,0x23000n,0n]),2n);}
 if(integer){gdip(t,'GdipSetMatrixElements',[p,...[1,0,0,1,-0.5,0.5].map(bits)]);vm.write(0x23000n,new Uint8Array(8));equal(gdip(t,'GdipTransformMatrixPointsI',[p,0x23000n,1n]),0n);equal(vm.readInt(0x23000n),0n);equal(vm.readInt(0x23004n),1n);}
 gdip(t,'GdipDeleteMatrix',[p]);vm.dispose();
}
// Identity compares raw float bits; invertibility uses the original f32/double threshold.
{
 const t=await machine(32),{vm}=t;const m=create(t),c=create(t);equal(gdip(t,'GdipIsMatrixIdentity',[m,0x21010n]),0n);equal(vm.readInt(0x21010n),1n);equal(gdip(t,'GdipIsMatrixEqual',[m,c,0x21010n]),0n);equal(vm.readInt(0x21010n),1n);gdip(t,'GdipSetMatrixElements',[m,...[1,-0,0,1,0,0].map(bits)]);gdip(t,'GdipIsMatrixIdentity',[m,0x21010n]);equal(vm.readInt(0x21010n),0n);gdip(t,'GdipIsMatrixEqual',[m,c,0x21010n]);equal(vm.readInt(0x21010n),0n);
 for(const[det,result]of[[bits(0.00001),0n],[bits(0.00001)+1n,1n]]){gdip(t,'GdipSetMatrixElements',[m,bits(1),bits(1),0n,det,0n,0n]);gdip(t,'GdipIsMatrixInvertible',[m,0x21010n]);equal(vm.readInt(0x21010n),result);if(!result){const before=elements(t,m);equal(gdip(t,'GdipInvertMatrix',[m]),2n);equal(elements(t,m),before);}}
 for(const p of[m,c])gdip(t,'GdipDeleteMatrix',[p]);vm.dispose();
}
// Output/input/frame faults preserve CPU, startup identities and entire point batches.
for(const kind of['startup','startup-input','create','matrix','get','frame']){
 const t=await machine(32),{vm}=t;input(t);const m=create(t,[2,0,0,3,4,5]);vm.write(0x24ffen,new Uint8Array(32).fill(0xa5));let name,args,page=0x25000n,protect=2;
 if(kind==='startup'){name='GdiplusStartup';args=[0x24ffen,0x20500n,0n];}
 if(kind==='startup-input'){name='GdiplusStartup';args=[0x21010n,0x25000n,0n];vm.write(0x25000n,new Uint8Array(16));vm.write(0x25000n,encode(1n,4));protect=1;}
 if(kind==='create'){name='GdipCloneMatrix';args=[m,0x24ffen];}
 if(kind==='matrix'){name='GdipSetMatrixElements';args=[m,...[1,0,0,1,0,0].map(bits)];page=m&~4095n;}
 if(kind==='get'){name='GdipGetMatrixElements';args=[m,0x24ffen];}
 if(kind==='frame'){name='GdipDeleteMatrix';args=[m];page=t.sp&~4095n;protect=1;}
 frame(t,name,args,'gdiplus.dll');equal(core.wire_protect(t.handle,page,4096n,protect),0);const before=snapshot(t),prefix=read(t,0x24ffen,2);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(read(t,0x24ffen,2),prefix);equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);
 if(kind==='startup'||kind==='startup-input')equal(vm.readInt(args[0]),1n);if(kind==='create')gdip(t,'GdipDeleteMatrix',[vm.readInt(0x24ffen)]);if(kind!=='frame')gdip(t,'GdipDeleteMatrix',[m]);vm.dispose();
}
{
 const t=await machine(32),{vm}=t,m=create(t,[2,0,0,3,4,5]);vm.map(0x50000n,4096n,3);vm.map(0x51000n,4096n,2);floats(t,0x50ff8n,[1,2,3,4]);frame(t,'GdipTransformMatrixPoints',[m,0x50ff8n,2n],'gdiplus.dll');const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(values(t,0x50ff8n,2),[1,2]);equal(core.wire_protect(t.handle,0x51000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(values(t,0x50ff8n,4),[6,11,10,17]);gdip(t,'GdipDeleteMatrix',[m]);vm.dispose();
}
// Zeroed allocations are process-owned and failed reuse does not consume their membership.
{
 const t=await machine(32),{vm}=t,heap=kernel(t,'GetProcessHeap');vm.write(t.error,encode(123n,4));vm.flags=0x8d5n;
 const p=gdip(t,'GdipAlloc',[128n]);assert.ok(p);equal(read(t,p,128),new Uint8Array(128));equal(gdip(t,'GdipDeleteMatrix',[p]),2n);gdip(t,'GdipFree',[p]);frame(t,'GdipAlloc',[128n],'gdiplus.dll');equal(core.wire_protect(t.handle,p&~4095n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,p&~4095n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),p);gdip(t,'GdipFree',[p]);equal(gdip(t,'GdipAlloc',[0xffffffffn]),0n);equal(vm.flags,0x8d5n);equal(vm.readInt(t.error),123n);equal(kernel(t,'HeapSize',[heap,0n,p]),0xffffffffn);vm.dispose();
}
// Output aliasing uses saved values and saved returns; static rejection releases owned backing.
{
 const t=await machine(32),{vm}=t;const m=create(t);frame(t,'GdipCloneMatrix',[m,t.sp],'gdiplus.dll');equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);const clone=vm.readInt(t.sp);equal(elements(t,clone),[1,0,0,1,0,0]);floats(t,0x23000n,[2,4,4,8]);floats(t,0x23100n,[10,20,18,24,34,52]);equal(gdip(t,'GdipCreateMatrix3',[0x23000n,0x23100n,0x23000n]),0n);const p=vm.readInt(0x23000n);equal(elements(t,p),[2,1,3,4,-6,2]);for(const p of[m,clone,vm.readInt(0x23000n)])gdip(t,'GdipDeleteMatrix',[p]);vm.dispose();
 const bad=moduleFixture(32,{dll:true,attach:false,tls:false}),v=new WasmMachine(core,{mode:32});v.supplyModule('bad.dll',bad.bytes);const image=v.load(moduleFixture(32,{dependency:'bad.dll',tls:false}).bytes),u={vm:v,mode:32,image,sp:image.sp-2048n};v.map(0x20000n,0x10000n,7);const ip=v.ip,regs=Array.from({length:8},(_,i)=>v.register(i)),owned=create(u);v.ip=ip;regs.forEach((r,i)=>v.register(i,r));await assert.rejects(v.run(10000n),/attach returned FALSE/);assert.throws(()=>v.readInt(owned),/memory access/);v.dispose();
}
console.log(`${checks} GDI+ matrix/ABI/JIT/import/failure/fault/ownership checks passed`);
