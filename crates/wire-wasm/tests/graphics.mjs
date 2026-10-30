import assert from 'node:assert/strict';
import {WasmMachine,Exit} from '../web/runtime.mjs';
import {core,stateLayout,encode} from './x87-harness.mjs';
import {fixture} from './pe-fixture.mjs';
import {window,invoke as userCall} from './gdi-harness.mjs';
let checks=0;const equal=(a,b)=>{assert.deepEqual(a,b);checks++;};
const guid=s=>{const [a,b,c,d,e]=s.split('-');return Uint8Array.from([...encode(BigInt('0x'+a),4),...encode(BigInt('0x'+b),2),...encode(BigInt('0x'+c),2),...d.match(/../g).map(v=>parseInt(v,16)),...e.match(/../g).map(v=>parseInt(v,16))]);};
const readBytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const factoryIID=guid('770aae78-f26f-4dba-a829-253c83d1b387'),textureIID=guid('6f15aaf2-d208-4e89-9ab4-489535d34f9c');

// The bridge records resource ownership and typed pending readback, independently of COM dispatch.
class Bridge {
 constructor(memory){this.memory=memory;this.next=1;this.objects=new Map();this.log=[];this.closed=false;this.prepares=0;this.jobs=0;this.flushes=0;this.pause=false;this.ready=false;this.failure=false;this.pauseOperation=0;this.operationReady=false;this.operationJobs=0;this.compilations=0;this.draws=0;this.drawPackets=[];}
 profile(){const name=new TextEncoder().encode('Control GPU'),b=new Uint8Array(96+name.length),v=new DataView(b.buffer);v.setUint32(0,1,true);v.setUint32(4,3,true);v.setUint32(8,name.length,true);b.fill(0x83,16,32);v.setBigUint64(48,512n*1024n*1024n,true);v.setUint32(56,0x1234,true);v.setUint32(60,0x5678,true);for(const[n,id]of[28,87,40].entries()){v.setUint32(72+n*8,id,true);v.setUint32(76+n*8,id===40?0x11720:0x8d323,true);}b.set(name,96);return b;}
 put(kind,bytes=new Uint8Array()){const id=this.next++;this.objects.set(id,{kind,bytes});return id;}
 dispatch(op,at){const h=new Uint32Array(this.memory.buffer,at,22),a=Array.from(h.slice(6)),input=new Uint8Array(this.memory.buffer,h[0],h[1]),out=new Uint8Array(this.memory.buffer,h[2],h[3]);this.log.push([op,...a]);
  try{switch(op){
   case 1:return this.put('buffer',input.slice());
   case 2:this.objects.get(a[0]).bytes.set(input,a[1]);return 0;
   case 17:this.prepares++;if(this.failure){this.failure=false;throw new Error('resident WebGPU operation pending');}if(this.pause){if(!this.ready){if(!this.jobs)this.jobs++;return -2;}this.pause=false;this.ready=false;}return 0;
   case 3:out.set(this.objects.get(a[0]).bytes.subarray(a[1],a[1]+out.length));return 0;
   case 4:return this.put('texture');
   case 8:return this.put('view');
   case 9:case 10:return 0;
   case 14:Bridge.dispatching=true;try{this.options.getCanvas?.(BigInt(a[1])+(BigInt(a[2])<<32n));return 0;}finally{Bridge.dispatching=false;}
   case 12:case 13:
    if(this.pauseOperation===op){if(!this.operationReady){if(!this.activeOperation){this.activeOperation=op;this.operationJobs++;}return -2;}this.pauseOperation=0;this.activeOperation=0;this.operationReady=false;}
    if(op===12){out.fill(0);this.compilations++;return this.put('shader');}
    this.draws++;this.drawPackets.push(Array.from(new Uint32Array(input.slice().buffer)));return 0;
   case 16:assert.ok(this.objects.delete(a[0]));return 0;
   default:throw new Error('unexpected bridge operation '+op);
  }}catch(error){new Uint8Array(this.memory.buffer,h[4],h[5]).set(new TextEncoder().encode(error.message).subarray(0,h[5]-1));return -1;}
 }
 submit(){}
 async flush(){this.flushes++;this.ready=true;this.operationReady=true;}
 close(){equal(this.objects.size,0);this.closed=true;}
}
let last,setupFailure=false;const owners=[];
globalThis.WireWebGPU={async create(memory,options){if(setupFailure){setupFailure=false;throw new Error('graphics setup failed');}last=new Bridge(memory);last.options=options;owners.push(last);return last;}};
async function machine(mode=64,attach=true,options={}){let handle;const vm=new WasmMachine({...core,wire_create(...args){handle=core.wire_create(...args);return handle;},wire_process_id(...args){assert.ok(!Bridge.dispatching,'graphics callbacks must not reenter the core');return core.wire_process_id(...args);}},{mode,clock(){throw new Error('unexpected clock');},wallClock(){throw new Error('unexpected wall clock');}}),f=fixture(mode,{tls:false});f.put(f.dirs+8,0);f.put(f.dirs+12,0);f.bytes[f.offset(0x1000)]=0xc3;f.relocations.splice(0,f.relocations.length);f.rebuildRelocations();const image=vm.load(f.bytes);if(attach)await vm.enableGraphics(options);equal((await vm.run(100n)).reason,Exit.Returned);vm.map(0x20000n,0x30000n,7);return{vm,handle,mode,image,gpu:attach?last:null,sp:image.sp-2048n};}
const snapshot=t=>new Uint8Array(core.memory.buffer,core.wire_state(t.handle),stateLayout[0]).slice();
function frame(t,pc,args=[]){const{vm,sp}=t;vm.ip=pc;vm.register(4,sp);vm.write(sp,encode(vm.entryPoints.returned,8));for(const[n,v]of args.entries())if(n<4)vm.register([1,2,8,9][n],v);else vm.write(sp+40n+BigInt((n-4)*8),encode(v,8));}
function callAt(t,pc,args=[]){frame(t,pc,args);const work=t.vm.work,retired=t.vm.retired;equal(t.vm.service().reason,Exit.Dispatch);equal(t.vm.work,work+1n);equal(t.vm.retired,retired);equal(t.vm.ip,t.vm.entryPoints.returned);return t.vm.register(0);}
const api=(t,name,args=[],dll='dxgi.dll')=>callAt(t,t.vm.bind(dll,name),args);
const method=(t,p,slot)=>t.vm.readInt(t.vm.readInt(p,8)+BigInt(slot*8),8);
const invoke=(t,p,slot,args=[])=>callAt(t,method(t,p,slot),[p,...args]);
function objects(t){const{vm}=t;vm.write(0x20000n,factoryIID);equal(api(t,'CreateDXGIFactory1',[0x20000n,0x20100n]),0n);const factory=vm.readInt(0x20100n,8);equal(invoke(t,factory,12,[0n,0x20108n]),0n);const adapter=vm.readInt(0x20108n,8);equal(api(t,'D3D11CreateDevice',[adapter,0n,0n,0n,0n,0n,7n,0x20110n,0x20118n,0x20120n],'d3d11.dll'),0n);equal(vm.readInt(0x20118n),0xa000n);return{factory,adapter,device:vm.readInt(0x20110n,8),context:vm.readInt(0x20120n,8)};}
function buffer(t,device){t.vm.write(0x20200n,[16,3,0,0x30000,0,0].flatMap(n=>Array.from(encode(BigInt(n),4))));equal(invoke(t,device,3,[0x20200n,0n,0x20128n]),0n);return t.vm.readInt(0x20128n,8);}

// Public creation admission remains separate from internal COM gateways and architecture policy.
{
 const t=await machine(),{vm}=t;for(const[dll,names]of[['dxgi.dll',['CreateDXGIFactory','CreateDXGIFactory1','CreateDXGIFactory2']],['d3d11.dll',['D3D11CreateDevice','D3D11CreateDeviceAndSwapChain']],['d3d12.dll',['D3D12CreateDevice','D3D12GetDebugInterface']]])for(const name of names){assert.ok(vm.bind(dll,name));assert.throws(()=>vm.bind('user32.dll',name),/unknown/);assert.throws(()=>vm.bind('WIRE.DXGI',name),/unknown/);}
 vm.write(0x20000n,factoryIID);equal(api(t,'D3D12CreateDevice',[0n,0xb000n,0x20000n,0x20100n],'d3d12.dll'),0x887a0004n);equal(vm.readInt(0x20100n,8),0n);equal(api(t,'D3D12GetDebugInterface',[0x20000n,0x20100n],'d3d12.dll'),0x80004002n);assert.throws(()=>api(t,'D3D11CreateDeviceAndSwapChain',[],'d3d11.dll'),/combined/);vm.dispose();
 const q=await machine(32,false);assert.throws(()=>q.vm.bind('dxgi.dll','CreateDXGIFactory1'),/unknown/);await assert.rejects(()=>q.vm.enableGraphics(),/AMD64/);q.vm.dispose();
}

// Real COM identity, parent references, private data, adapter/output records and JIT calls share owners.
{
 const t=await machine(),{vm,gpu}=t;vm.write(0x20000n,factoryIID);
 equal(api(t,'DXGIGetDebugInterface1',[0n,0x20000n,0x20100n]),0x80004002n);equal(api(t,'D3D11On12CreateDevice',[1n],'d3d11.dll'),0x887a0004n);equal(gpu.log.length,0);equal(gpu.objects.size,0);
 const{factory,adapter,device,context}=objects(t);
 equal(invoke(t,adapter,10,[0x21000n]),0n);equal(vm.readInt(0x21100n),0x1234n);equal(vm.readInt(0x21104n),0x5678n);equal(vm.readInt(0x21128n,8),0x8383838383838383n);
 equal(invoke(t,adapter,7,[0n,0x20130n]),0n);const output=vm.readInt(0x20130n,8);equal(invoke(t,output,7,[0x21200n]),0n);equal(vm.readInt(0x21248n),1024n);equal(vm.readInt(0x2124cn),768n);equal(vm.readInt(0x21258n,8),0x7ffd03000001n);
 vm.write(0x20500n,[4,5,6]);equal(invoke(t,factory,3,[0x20000n,3n,0x20500n]),0n);vm.write(0x20510n,encode(3n,4));equal(invoke(t,factory,5,[0x20000n,0x20510n,0x20520n]),0n);equal(readBytes(vm,0x20520n,3),new Uint8Array([4,5,6]));
 const pc=method(t,device,37);vm.write(0x30000n,[0x48,0x83,0xec,0x28,0x48,0xb9,...encode(device,8),0x48,0xb8,...encode(pc,8),0xff,0xd0,0x48,0x83,0xc4,0x28,0xc3]);frame(t,0x30000n);equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),0xa000n);
 for(const p of [output,context,device,adapter,factory])equal(invoke(t,p,2),0n);equal(gpu.objects.size,0);vm.dispose();equal(gpu.closed,true);
}

// Output/return faults precede GPU work; typed pending differs from an identical error string.
{
 const t=await machine(),{vm,gpu}=t,{factory,adapter,device,context}=objects(t),b=buffer(t,device),map=method(t,context,14),out=0x24fffn;
 vm.write(out,new Uint8Array(16).fill(0xa5));frame(t,map,[context,b,0n,1n,0n,out]);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(gpu.prepares,0);equal(vm.readInt(out,1),0xa5n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);
 gpu.failure=true;assert.throws(()=>vm.service(),/resident WebGPU operation pending/);equal(snapshot(t),before);equal(core.wire_graphics_pending(t.handle),0);
 gpu.pause=true;equal(vm.service().reason,Exit.Graphics);equal(snapshot(t),before);equal(core.wire_graphics_pending(t.handle),1);assert.throws(()=>{vm.ip=0n;},/pending graphics/);vm.write(t.sp,encode(0xdeadn,8));await vm.flushGraphics();equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);const mapped=vm.readInt(out,8);assert.ok(mapped);equal(readBytes(vm,mapped,16),new Uint8Array(16));equal(gpu.jobs,1);equal(invoke(t,context,15,[b,0n]),0n);
 gpu.pause=true;frame(t,map,[context,b,0n,1n,0n,0x22000n]);const work=vm.work,retired=vm.retired;equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.work,work+1n);equal(vm.retired,retired);equal(core.wire_graphics_pending(t.handle),0);equal(invoke(t,context,15,[b,0n]),0n);
 for(const p of [b,context,device,adapter,factory])equal(invoke(t,p,2),0n);equal(gpu.objects.size,0);vm.dispose();
}

// A frame fault cannot publish a factory; table slots return to the pool on disposal.
{
 const t=await machine(),{vm}=t;vm.write(0x20000n,factoryIID);frame(t,vm.bind('dxgi.dll','CreateDXGIFactory1'),[0x20000n,0x20100n]);equal(core.wire_protect(t.handle,t.sp&~4095n,4096n,1),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(0x20100n,8),0n);equal(core.wire_protect(t.handle,t.sp&~4095n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(invoke(t,vm.readInt(0x20100n,8),2),0n);const length=core.__indirect_function_table.length;vm.dispose();const q=await machine();equal(core.__indirect_function_table.length,length);q.vm.dispose();
}
// Swapchain resources, clear/present and scalar XMM arguments retain the original object semantics.
{
 const presented=[];const t=await machine(64,true,{getCanvas(window,process){presented.push([window,process]);return null;}}),{vm,gpu}=t,{factory,adapter,device,context}=objects(t),hwnd=await window(t);await userCall(t,'ShowWindow',[hwnd,5n]);
 vm.write(0x22000n,new Uint8Array(72));for(const[at,n]of[[0,8],[4,8],[8,60],[12,1],[16,28],[28,1],[36,0x20],[40,1],[56,1]])vm.write(0x22000n+BigInt(at),encode(BigInt(n),4));vm.write(0x22030n,encode(hwnd,8));
 equal(invoke(t,factory,10,[device,0x22000n,0x20150n]),0n);const chain=vm.readInt(0x20150n,8);vm.write(0x22200n,textureIID);equal(invoke(t,chain,9,[0n,0x22200n,0x20158n]),0n);const backbuffer=vm.readInt(0x20158n,8);equal(invoke(t,device,9,[backbuffer,0n,0x20160n]),0n);const view=vm.readInt(0x20160n,8);
 vm.write(0x22300n,[0,0x3f800000,0,0x3f800000].flatMap(n=>Array.from(encode(BigInt(n),4))));equal(invoke(t,context,50,[view,0x22300n]),0n);equal(gpu.log.filter(a=>a[0]===9).length,1);equal(invoke(t,chain,8,[0n,0n]),0n);equal(invoke(t,chain,8,[0n,1n]),0n);equal(gpu.log.filter(a=>a[0]===14).length,1);const packet=gpu.log.find(a=>a[0]===14);equal(BigInt(packet[2])+(BigInt(packet[3])<<32n),hwnd);equal(presented,[[hwnd,vm.processId]]);equal(invoke(t,chain,17,[0x20168n]),0n);equal(vm.readInt(0x20168n),1n);
 for(const p of [view,backbuffer,chain]){invoke(t,p,2);}
 vm.write(0x22400n,[2,2,1,1,40,1,0,0,0x40,0,0].flatMap(n=>Array.from(encode(BigInt(n),4))));equal(invoke(t,device,5,[0x22400n,0n,0x20170n]),0n);const depth=vm.readInt(0x20170n,8);equal(invoke(t,device,10,[depth,0n,0x20178n]),0n);const dsv=vm.readInt(0x20178n,8);
 frame(t,method(t,context,53),[context,dsv,1n,0xdeadbeefn,0n]);vm.xmm(3,encode(0x3e800000n,16));equal(vm.service().reason,Exit.Dispatch);const cleared=gpu.log.find(a=>a[0]===10);equal(cleared[3],0x3e800000);
 for(const p of [dsv,depth,context,device,adapter,factory])invoke(t,p,2);equal(gpu.objects.size,0);vm.dispose();
}
// A pending GPU call keeps its thread across an expired scheduler quantum.
{
 const t=await machine(),{vm,gpu}=t,{device,context}=objects(t),b=buffer(t,device);vm.write(0x30000n,[0xb9,0x10,0x27,0,0,0xff,0xc9,0x75,0xfc,0xc3]);frame(t,0x30000n);equal((await vm.run(10000n)).reason,Exit.Budget);
 vm.write(0x31000n,[0xb8,73,0,0,0,0xc3]);api(t,'CreateThread',[0n,65536n,0x31000n,0n,0n,0n],'kernel32.dll');frame(t,method(t,context,14),[context,b,0n,1n,0n,0x22000n]);gpu.pause=true;equal(vm.service().reason,Exit.Graphics);const retired=vm.retired;equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.retired,retired);equal(vm.ip,vm.entryPoints.returned);vm.dispose();
}
// Shader preparation and cold Draw keep one saved call through GPU waits and output faults.
{
 const t=await machine(),{vm,gpu}=t,{factory,adapter,device,context}=objects(t);
 vm.write(0x23000n,new Uint8Array(32).fill(0x53));vm.write(0x24fffn,new Uint8Array(8).fill(0xa5));
 frame(t,method(t,device,12),[device,0x23000n,32n,0n,0x24fffn]);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(gpu.compilations,0);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);
 gpu.pauseOperation=12;equal(vm.service().reason,Exit.Graphics);equal(snapshot(t),before);vm.write(t.sp,encode(0xdeadn,8));await vm.flushGraphics();equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);
 equal(vm.service().reason,Exit.Fault);equal(gpu.compilations,0);equal(core.wire_graphics_pending(t.handle),1);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);
 equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);equal(gpu.compilations,1);equal(gpu.operationJobs,1);const vertex=vm.readInt(0x24fffn,8);
 frame(t,method(t,device,15),[device,0x23000n,32n,0n,t.sp]);equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);const pixel=vm.readInt(t.sp,8);equal(gpu.compilations,2);
 vm.write(0x24000n,[8,8,1,1,28,1,0,0,0x20,0,0].flatMap(n=>Array.from(encode(BigInt(n),4))));equal(invoke(t,device,5,[0x24000n,0n,0x22000n]),0n);const target=vm.readInt(0x22000n,8);
 equal(invoke(t,device,9,[target,0n,0x22008n]),0n);const view=vm.readInt(0x22008n,8);vm.write(0x22010n,encode(view,8));invoke(t,context,33,[1n,0x22010n,0n]);
 invoke(t,context,11,[vertex,0n,0n]);invoke(t,context,9,[pixel,0n,0n]);invoke(t,context,24,[4n]);vm.write(0x24100n,[0,0,0x41000000,0x41000000,0,0x3f800000].flatMap(n=>Array.from(encode(BigInt(n),4))));invoke(t,context,44,[1n,0x24100n]);
 gpu.pauseOperation=13;frame(t,method(t,context,13),[context,0x100000003n,7n]);const drawBefore=snapshot(t);equal(vm.service().reason,Exit.Graphics);equal(snapshot(t),drawBefore);equal(gpu.draws,0);await vm.flushGraphics();vm.write(t.sp,encode(0xdeadn,8));equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);equal(gpu.draws,1);equal(gpu.operationJobs,2);equal(gpu.drawPackets[0][9],3);equal(gpu.drawPackets[0][10],7);
 invoke(t,context,110);for(const p of[view,target,pixel,vertex,context,device,adapter,factory])invoke(t,p,2);equal(gpu.objects.size,0);vm.dispose();
}
// A process may attach while suspended after ordinary execution and before creating graphics objects.
{
 const t=await machine(64,false);assert.ok(t.vm.work>0n);await t.vm.enableGraphics();const{factory,adapter,device,context}=objects(t);for(const p of[context,device,adapter,factory])invoke(t,p,2);const owner=last;frame(t,t.vm.bind('kernel32.dll','ExitProcess'),[0n]);equal(t.vm.service().reason,Exit.Dispatch);equal((await t.vm.run(100n)).reason,Exit.ProcessExited);equal(owner.closed,true);t.vm.dispose();
}
// Inherited GPU options stay lazy, process scoped and retryable after the parent exits.
for(const graphics of[false,true]){
 const t=await machine(64,false),{vm}=t,from=owners.length,canvases=[],compiler={closed:false,close(){this.closed=true;}};
 await vm.enableGraphics({shaderCompiler:compiler,getCanvas(window,process){canvases.push([window,process]);return null;}});const parent=owners[from];
 const f=fixture(64,{tls:false});f.relocations.splice(0);f.put(f.dirs+8,graphics?0x2000:0);f.put(f.dirs+12,graphics?40:0);f.bytes.fill(0x90,f.offset(0x1000),f.offset(0x1100));
 if(graphics){f.rtext(0x20c0,'dxgi.dll\0');f.rtext(0x20e2,'CreateDXGIFactory1\0');f.bytes.set(factoryIID,f.offset(0x3000));f.bytes.set([0x48,0x83,0xec,0x28,0xb9,100,0,0,0,0xff,0xc9,0x75,0xfc,0x48,0x8d,0x0d,...encode(0x3000n-0x1014n,4),0x48,0x8d,0x15,...encode(0x3020n-0x101bn,4),0xff,0x15,...encode(0x20a0n-0x1021n,4),0x48,0x83,0xc4,0x28,0xc3],f.offset(0x1000));}
 else f.bytes.set([0x31,0xc0,0xc3],f.offset(0x1000));f.rebuildRelocations();vm.supplyFile('C:\\child.exe',f.bytes);
 vm.write(0x20000n,Uint8Array.from([...'C:\\child.exe\0'].flatMap(c=>[c.charCodeAt(0),0])));vm.write(0x21000n,new Uint8Array(104));vm.write(0x21000n,encode(104n,4));equal(api(t,'CreateProcessW',[0x20000n,0n,0n,0n,0n,0n,0n,0n,0x21000n,0x22000n],'kernel32.dll'),1n);equal(owners.length,from+1);
 frame(t,vm.bind('kernel32.dll','TerminateProcess'),[0xffffffffffffffffn,0n]);equal(vm.service().reason,Exit.ProcessExited);equal(parent.closed,true);equal(compiler.closed,false);
 if(graphics){setupFailure=true;await assert.rejects(vm.run(10000n),e=>e.processId===2&&/graphics setup failed/.test(e.message));equal(owners.length,from+1);}
 equal((await vm.run(10000n)).reason,Exit.ProcessExited);equal(vm.processes()[1].exitCode,0);equal(owners.length,from+(graphics?2:1));
 if(graphics){const child=owners[from+1];equal(child.closed,true);equal(child.options.shaderCompiler,compiler);child.options.getCanvas(123n);equal(canvases,[[123n,2]]);}
 vm.dispose();equal(compiler.closed,false);
}
console.log(`${checks} shared DXGI/D3D11/COM/bridge/JIT/pending/fault/lifetime checks passed`);
