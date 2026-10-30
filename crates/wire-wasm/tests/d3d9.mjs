import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';
import {moduleFixture} from './module-fixture.mjs';
import {core,encode,Exit,equal,checks,snapshot,machine,call,window,color} from './gdi-harness.mjs';
const invalid=0x8876086cn, unavailable=0x8876086an, out=0x22000n, params=0x22100n;
const abi=readFileSync(new URL('../../../third_party/hangover/d3d9-methods.tsv',import.meta.url),'utf8').trim().split('\n').filter(s=>!s.startsWith('#')).map(s=>s.split('\t'));
function words(t,p,values){t.vm.write(p,values.flatMap(n=>Array.from(encode(BigInt.asUintN(32,BigInt(n)),4))));}
function frame(t,address,args=[]){const{vm,sp}=t;vm.ip=address;vm.register(4,sp);vm.write(sp,encode(vm.entryPoints.returned,4));words(t,sp+4n,args);}
const method=(t,p,slot)=>t.vm.readInt(t.vm.readInt(p)+BigInt(slot*4));
function invoke(t,p,slot,args=[]){frame(t,method(t,p,slot),[p,...args]);const w=t.vm.work,r=t.vm.retired;equal(t.vm.service().reason,Exit.Dispatch);equal(t.vm.work,w+1n);equal(t.vm.retired,r);return t.vm.register(0);}
function created(t,p,slot,args){equal(invoke(t,p,slot,args),0n);const value=t.vm.readInt(out);assert.ok(value);return value;}
function texture(t,d){return created(t,d,23,[1n,1n,1n,0n,21n,1n,out,0n]);}
function shader(t,d,slot,tokens,p=0x23000n){words(t,p,tokens);return created(t,d,slot,[p,out]);}
const ps=[0xffff0200,0x0200001f,0x80000000,0x900f0000,0x02000001,0x800f0800,0x90e40000,0xffff];
const vs=[0xfffe0200,0x0200001f,0x80000000,0x900f0000,0x0200001f,0x80000005,0x900f0001,0x0200001f,0x8000000a,0x900f0002,0x02000001,0xc00f0000,0x90e40000,0x02000001,0xe0030000,0x90e40001,0x02000001,0xd00f0000,0x90e40002,0xffff];
async function setup({depth=false}={}){const t=await machine(32),{vm}=t;const hwnd=await window(t);const factory=call(t,'Direct3DCreate9',[32n],'d3d9.dll');assert.ok(factory);words(t,params,[4,4,21,1,0,0,1,hwnd,1,depth?1:0,depth?75:0,0,0,0]);const device=created(t,factory,16,[0n,1n,hwnd,0x20n,params,out]);return{...t,factory,device,hwnd};}
function back(t){return created(t,t.device,18,[0n,0n,0n,out]);}
function pixels(t,s){equal(invoke(t,s,13,[0x22200n,0n,0x10n]),0n);const bits=t.vm.readInt(0x22204n);equal(invoke(t,s,14),0n);return bits;}
function quad(t,{clip=false}={}){const points=clip?[[-1,1],[1,1],[-1,-1],[1,-1]]:[[-0.5,-0.5],[3.5,-0.5],[-0.5,3.5],[3.5,3.5]];let p=0x24000n;const f=new DataView(new ArrayBuffer(4));for(const[x,y]of points){const values=[x,y,0.5,1].map(v=>{f.setFloat32(0,v,true);return f.getUint32(0,true);});words(t,p,[...values,0xff336699,0,0]);p+=28n;}}
async function jit(t,p,slot,args=[]){const target=method(t,p,slot),code=[];for(const a of[p,...args].reverse())code.push(0x68,...encode(a,4));code.push(0xb8,...encode(target,4),0xff,0xd0,0xc3);t.vm.write(0x40000n,code);frame(t,0x40000n);equal((await t.vm.run(1000n)).reason,Exit.Returned);return t.vm.register(0);}
// Every legacy COM slot is installed on a real object; internal method names stay private.
{
 const t=await setup(),{vm,factory:f,device:d}=t,tex=texture(t,d),surface=created(t,tex,18,[0n,out]),swap=created(t,d,14,[0n,out]);
 const vb=created(t,d,26,[112n,0n,0x144n,1n,out,0n]),ib=created(t,d,27,[12n,0n,101n,1n,out,0n]);
 const decl=[0,0,0,0,3,0,9,0,0,0,16,0,4,0,10,0,0,0,20,0,1,0,5,0,255,0,0,0,17,0,0,0];vm.write(0x22300n,decl);const vd=created(t,d,86,[0x22300n,out]);
 const vertex=shader(t,d,91,vs),pixel=shader(t,d,106,ps),block=created(t,d,59,[1n,out]);
 const objects={IDirect3D9:f,IDirect3DDevice9:d,IDirect3DTexture9:tex,IDirect3DSurface9:surface,IDirect3DSwapChain9:swap,IDirect3DVertexBuffer9:vb,IDirect3DIndexBuffer9:ib,IDirect3DVertexDeclaration9:vd,IDirect3DVertexShader9:vertex,IDirect3DPixelShader9:pixel,IDirect3DStateBlock9:block};
 equal(abi.length,234);const gateways=new Set();for(const[name,slot]of abi){const kind=name.split('.')[0],address=method(t,objects[kind],Number(slot));assert.ok(address);assert.ok(!gateways.has(address));gateways.add(address);assert.throws(()=>vm.bind('d3d9.dll',name),/export|provider/);}
 equal(gateways.size,234);equal(await jit(t,f,4),1n);equal(await jit(t,d,3),0n);equal(invoke(t,tex,4,[0n,0n,0n,0n]),unavailable);
 equal(invoke(t,vertex,4,[0n,out]),0n);equal(vm.readInt(out),BigInt(vs.length*4));words(t,out,[vs.length*4]);equal(invoke(t,vertex,4,[0x22500n,out]),0n);equal(vm.readInt(0x22500n),0xfffe0200n);
 const more=texture(t,d);equal(vm.readInt(more),vm.readInt(tex));invoke(t,more,2);
 for(const p of[block,pixel,vertex,vd,ib,vb,swap,surface,tex,d,f])invoke(t,p,2);vm.dispose();
 const v=new WasmMachine(core,{mode:64});assert.throws(()=>v.bind('d3d9.dll','Direct3DCreate9'),/export|provider/);v.dispose();
}
// Fixed-function and VS2/PS2 share the real pixel backing; Present also updates a pre-existing GDI DC.
for(const programmable of[false,true]){
 const t=await setup(),{vm,device:d,hwnd}=t,s=back(t),bits=pixels(t,s),dc=call(t,'GetDC',[hwnd]);
 let shaderObjects=[];
 if(programmable){const vertex=shader(t,d,91,vs),pixel=shader(t,d,106,ps);shaderObjects=[vertex,pixel];const decl=[0,0,0,0,3,0,0,0,0,0,16,0,4,0,10,0,0,0,20,0,1,0,5,0,255,0,0,0,17,0,0,0];vm.write(0x22300n,decl);const vd=created(t,d,86,[0x22300n,out]);shaderObjects.push(vd);equal(invoke(t,d,87,[vd]),0n);equal(invoke(t,d,92,[vertex]),0n);equal(invoke(t,d,107,[pixel]),0n);}else equal(invoke(t,d,89,[0x144n]),0n);
 equal(invoke(t,d,57,[22n,1n]),0n);equal(invoke(t,d,43,[0n,0n,1n,0xff000000n,0n,0n]),0n);quad(t,{clip:programmable});equal(invoke(t,d,41),0n);equal(await jit(t,d,83,[5n,2n,0x24000n,28n]),0n);equal(invoke(t,d,42),0n);
 for(let i=0n;i<16n;i++)equal(vm.readInt(bits+i*4n),0xff336699n);equal(await jit(t,d,17,[0n,0n,0n,0n]),0n);const image=vm.windowPixels(hwnd);equal([image.width,image.height],[16,12]);equal(color(image,8,6),[0x33,0x66,0x99,255]);equal(call(t,'GetPixel',[dc,8n,6n],'gdi32.dll'),0x996633n);
 equal(call(t,'SetPixel',[dc,0n,0n,0x563412n],'gdi32.dll'),0x563412n);equal(color(vm.windowPixels(hwnd),0,0),[0x12,0x34,0x56,255]);equal(invoke(t,d,17,[0n,0n,0n,0n]),0n);equal(color(vm.windowPixels(hwnd),0,0),[0x33,0x66,0x99,255]);
 call(t,'ReleaseDC',[hwnd,dc]);for(const p of[s,...shaderObjects,d,t.factory])invoke(t,p,2);vm.dispose();
}
// Guard/output/input failures preserve the saved frame, COM count, recording and destination pixels.
for(const kind of['query','texture','shader','declaration','stream','draw','present','stateblock','return']){
 const t=await setup(),{vm,device:d}=t,s=back(t),bits=pixels(t,s);vm.write(0x24ffen,new Uint8Array(32).fill(0xa5));words(t,out,[0x12345678]);words(t,0x22600n,[0xabcdef]);let p=d,slot,args,page=0x25000n,protection=2;
 if(kind==='query'){p=s;slot=0;words(t,0x22400n,[0x0cfbaf3a,0x429a9ff6,0x79a2b399,0x9bb8f86a]);args=[0x25000n,out];protection=1;}
 if(kind==='texture'){slot=23;args=[1n,1n,1n,0n,21n,1n,0x24ffen,0n];}
 if(kind==='shader'){slot=106;words(t,0x24ffcn,ps);args=[0x24ffcn,out];protection=1;}
 if(kind==='declaration'){slot=86;vm.write(0x24ffcn,[0,0,0,0,3,0,9,0]);args=[0x24ffcn,out];protection=1;}
 if(kind==='stream'){slot=101;args=[0n,out,0x22600n,0x24ffen];}
 if(kind==='draw'){equal(invoke(t,d,89,[0x144n]),0n);equal(invoke(t,d,57,[22n,1n]),0n);equal(invoke(t,d,41),0n);slot=83;args=[5n,2n,0x24ffcn,28n];protection=1;}
 if(kind==='present'){slot=17;args=[0n,0x24ffcn,0n,0n];protection=1;}
 if(kind==='stateblock'){equal(invoke(t,d,60),0n);equal(invoke(t,d,57,[22n,1n]),0n);slot=61;args=[0x24ffen];}
 if(kind==='return'){p=s;slot=1;args=[];page=t.sp&~4095n;protection=1;}
 frame(t,method(t,p,slot),[p,...args]);const before=snapshot(t),pixel=vm.readInt(bits),countValue=vm.readInt(out),marker=vm.readInt(0x24ffen,1);equal(core.wire_protect(t.handle,page,4096n,protection),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(out),countValue);equal(vm.readInt(0x22600n),0xabcdefn);equal(vm.readInt(0x24ffen,1),marker);equal(vm.readInt(bits),pixel);equal(core.wire_protect(t.handle,page,4096n,4),0);
 if(kind==='query'){words(t,0x25000n,[0x0cfbaf3a,0x429a9ff6,0x79a2b399,0x9bb8f86a]);}
 if(kind==='declaration'){vm.write(0x24ffcn,[0,0,0,0,3,0,9,0,0,0,16,0,1,0,5,0,255,0,0,0,17,0,0,0]);}
 if(kind==='draw'){quad(t);frame(t,method(t,d,83),[d,5n,2n,0x24000n,28n]);}
 if(kind==='present'){words(t,0x24ffcn,[0,0,16,12]);}
 equal(vm.service().reason,Exit.Dispatch);if(kind==='query'||kind==='return')equal(invoke(t,s,2),1n);if(kind==='stateblock')equal(invoke(t,vm.readInt(0x24ffen),5),0n);vm.dispose();
}
// A reusable-heap fault after vtable allocation rolls back creation and keeps the recording active.
for(const cached of[false,true]){
 const t=await setup(),{vm,device:d}=t,heap=call(t,'GetProcessHeap',[],'kernel32.dll');
 if(cached){const block=created(t,d,59,[1n,out]);invoke(t,block,2);}
 equal(invoke(t,d,60),0n);equal(invoke(t,d,57,[22n,1n]),0n);
 const victim=call(t,'HeapAlloc',[heap,0n,16n],'kernel32.dll'),anchor=call(t,'HeapAlloc',[heap,0n,16n],'kernel32.dll'),padding=call(t,'HeapAlloc',[heap,0n,8192n],'kernel32.dll');
 equal(call(t,'HeapFree',[heap,0n,victim],'kernel32.dll'),1n);words(t,out,[0x12345678]);frame(t,method(t,d,61),[d,out]);const before=snapshot(t),page=victim&~4095n;
 equal(core.wire_protect(t.handle,page,4096n,1),0);const failure=vm.service();equal(failure.reason,Exit.Fault);equal(failure.fault.address,victim);equal(snapshot(t),before);equal(vm.readInt(out),0x12345678n);
 equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);const block=vm.readInt(out);equal(block,victim);equal(invoke(t,block,5),0n);equal(invoke(t,d,58,[22n,0x22600n]),0n);equal(vm.readInt(0x22600n),1n);
 invoke(t,block,2);for(const p of[anchor,padding])equal(call(t,'HeapFree',[heap,0n,p],'kernel32.dll'),1n);vm.dispose();
}
// Partial depth clears preserve color and accept a larger attached depth surface.
{
 const t=await setup({depth:true}),{vm,device:d}=t,s=back(t),bits=pixels(t,s);equal(invoke(t,d,40,[out]),0n);const depth=vm.readInt(out);equal(invoke(t,depth,12,[0x22200n]),0n);
 // Allocate enough backing to keep depth pages separate from color and object headers.
 const large=created(t,d,29,[64n,64n,75n,0n,0n,0n,out,0n]);equal(invoke(t,d,40,[out]),0n);invoke(t,vm.readInt(out),2);equal(invoke(t,d,39,[large]),0n);
 equal(invoke(t,d,43,[0n,0n,7n,0xff102030n,0x3f800000n,7n]),0n);equal(vm.readInt(bits),0xff102030n);equal(invoke(t,d,43,[0n,0n,2n,0n,0x3f000000n,0n]),0n);equal(vm.readInt(bits),0xff102030n);
 for(const p of[large,depth,s,d,t.factory])invoke(t,p,2);vm.dispose();
}
// Reset retains swap identity, rejects outstanding default-pool references, then rebuilds implicit surfaces.
{
 const t=await setup(),{vm,device:d}=t,swap=created(t,d,14,[0n,out]),s=back(t);equal(invoke(t,d,16,[params]),invalid);invoke(t,s,2);invoke(t,swap,2);words(t,params,[8,8,21,1,0,0,1,t.hwnd,1,0,0,0,0,0]);equal(invoke(t,d,16,[params]),0n);equal(created(t,d,14,[0n,out]),swap);equal(invoke(t,swap,9,[0x22300n]),0n);equal(vm.readInt(0x22300n),8n);equal(invoke(t,d,43,[0n,0n,1n,0xff102030n,0n,0n]),0n);equal(invoke(t,d,17,[0n,0n,0n,0n]),0n);equal(color(vm.windowPixels(t.hwnd),8,6),[0x10,0x20,0x30,255]);invoke(t,swap,2);invoke(t,d,2);invoke(t,t.factory,2);vm.dispose();
}
// PE import and COM dispatch run through generated x86 code; ownership is retired at process exit/rejection.
{
 const f=fixture(32,{tls:false});f.rtext(0x20c0,'d3d9.dll\0');f.rtext(0x20e2,'Direct3DCreate9\0');f.relocations.splice(0);f.bytes.set([0x6a,32,0xff,0x15,0,0,0,0,0xc3],f.offset(0x1000));f.absolute(0x1004,f.preferred+0x20a0n);f.rebuildRelocations();const vm=new WasmMachine(core,{mode:32,clock:()=>0n}),image=vm.load(f.bytes);equal((await vm.run(100n)).reason,Exit.Returned);const p=vm.register(0);assert.ok(p);vm.map(0x20000n,0x30000n,7);const t={vm,mode:32,sp:image.sp-2048n};equal(await jit(t,p,4),1n);equal(invoke(t,p,2),0n);vm.dispose();
 const t2=await setup(),tex=texture(t2,t2.device),heap=call(t2,'GetProcessHeap',[],'kernel32.dll');equal(invoke(t2,tex,19,[0n,0x22200n,0n,0n]),0n);const owned=t2.vm.readInt(0x22204n);invoke(t2,tex,20,[0n]);invoke(t2,tex,2);equal(call(t2,'HeapSize',[heap,0n,owned],'kernel32.dll'),0xffffffffn);frame(t2,t2.vm.bind('kernel32.dll','ExitProcess'),[0n]);const exit=t2.vm.service();if(exit.reason!==Exit.ProcessExited)equal((await t2.vm.run(100n)).reason,Exit.ProcessExited);equal(t2.vm.windowPixels(t2.hwnd),null);t2.vm.dispose();
 const v=new WasmMachine(core,{mode:32,clock:()=>0n});v.supplyModule('bad.dll',moduleFixture(32,{dll:true,attach:false,tls:false}).bytes);const image2=v.load(moduleFixture(32,{dependency:'bad.dll',tls:false}).bytes);const sp=image2.sp-2048n,ip=v.ip,regs=Array.from({length:8},(_,i)=>v.register(i));v.map(0x20000n,0x30000n,7);const t3={vm:v,mode:32,sp};const fac=call(t3,'Direct3DCreate9',[32n],'d3d9.dll');v.ip=ip;regs.forEach((n,i)=>v.register(i,n));await assert.rejects(v.run(10000n),/attach returned FALSE/);assert.throws(()=>v.readInt(fac),/memory access/);v.dispose();
}
console.log(`${checks} D3D9 COM/JIT/software shader/pixel/Present/fault/lifetime checks passed`);
