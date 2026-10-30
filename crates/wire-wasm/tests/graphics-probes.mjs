import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,window} from './gdi-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';
import {fp} from './x87-harness.mjs';

const groups=[
 ['opengl32.dll',['wglCreateContext','wglCreateLayerContext','wglDeleteContext','wglGetCurrentContext','wglGetCurrentDC','wglGetProcAddress','wglMakeCurrent','wglCopyContext','wglShareLists','wglSwapLayerBuffers']],
 ['gdi32.dll',['ChoosePixelFormat','SetPixelFormat','GetPixelFormat','DescribePixelFormat','SwapBuffers']],
 ['dxgidebug.dll',['DXGIGetDebugInterface']],['dxgi.dll',['DXGIGetDebugInterface1']],
 ['d3d11.dll',['D3D11On12CreateDevice']],['dwmapi.dll',['DwmSetWindowAttribute']],
];
const owner=name=>groups.find(([,names])=>names.includes(name))[0];
const api=(t,name,args=[])=>call(t,name,args,owner(name));
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const error=(t,n)=>equal(t.vm.readInt(t.error),BigInt(n));
const glFailure=[['wglCreateContext',[]],['wglCreateLayerContext',[0n]],['ChoosePixelFormat',[0x24000n]],['SetPixelFormat',[1n,0x24000n]],['DescribePixelFormat',[1n,40n,0x24000n]],['SwapBuffers',[]],['wglSwapLayerBuffers',[1n]]];

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,hwnd=await window(t),dc=call(t,'GetDC',[hwnd],'user32.dll');assert.ok(dc);
 // Resolve an unbound export after loading its declared provider.
 vm.write(0x20000n,Buffer.from('opengl32.dll\0','utf16le'));frame(t,'LoadLibraryExW',[0x20000n,0n,0n],'kernel32.dll');equal((await vm.run(100n)).reason,Exit.Returned);const module=vm.register(0);assert.ok(module);
 vm.write(0x20100n,Buffer.from('wglShareLists\0'));equal(call(t,'GetProcAddress',[module,0x20100n],'kernel32.dll'),vm.bind('opengl32.dll','wglShareLists'));
 vm.write(0x20100n,Buffer.from('glCreateShader\0'));equal(call(t,'GetProcAddress',[module,0x20100n],'kernel32.dll'),0n);error(t,127);
 vm.write(0x24000n,new Uint8Array(40).fill(0xa5));
 for(const [name,tail]of glFailure){
  equal(api(t,name,[dc,...tail]),0n);error(t,50);equal(bytes(vm,0x24000n,40),new Uint8Array(40).fill(0xa5));
  for(const h of [0n,dc+1n,...(mode===64?[dc&0xffffffffn,dc+(1n<<32n)]:[])]){equal(api(t,name,[h,...tail]),0n);error(t,6);}
 }
 vm.write(t.error,encode(123n,4));equal(api(t,'GetPixelFormat',[dc]),0n);error(t,123);
 vm.flags=0x8d5n;vm.mxcsr=0x1fa0;vm.xmm(7,new Uint8Array(16).fill(0xa5));const fpu=fp(t),clock=t.time.calls,wall=t.time.wallCalls;
 for(const name of ['wglGetCurrentContext','wglGetCurrentDC']){equal(api(t,name),0n);error(t,123);}
 // A null rendering context releases the binding and ignores the complete DC argument.
 for(const h of [0n,dc,0xdeadn,...(mode===64?[0x123400000000n]:[])]){equal(api(t,'wglMakeCurrent',[h,0n]),1n);error(t,123);}
 for(const [name,args]of [['wglDeleteContext',[0n]],['wglDeleteContext',[dc]],['wglMakeCurrent',[dc,dc]],['wglCopyContext',[1n,2n,0n]],['wglShareLists',[1n,2n]],['wglGetProcAddress',[0n]],['wglGetProcAddress',[1n]]]){equal(api(t,name,args),0n);error(t,6);}
 equal(vm.flags,0x8d5n);equal(vm.mxcsr,0x1fa0);equal(vm.xmm(7),new Uint8Array(16).fill(0xa5));equal(fp(t),fpu);equal(t.time.calls,clock);equal(t.time.wallCalls,wall);
 vm.write(t.error,encode(123n,4));equal(api(t,'DwmSetWindowAttribute',[hwnd,20n,1n,4n]),0x80263001n);error(t,123);equal(api(t,'DwmSetWindowAttribute',[0n,20n,0n,4n]),0x80070006n);error(t,123);
 equal(call(t,'ReleaseDC',[hwnd,dc],'user32.dll'),1n);equal(api(t,'GetPixelFormat',[dc]),0n);error(t,6);
 for(const[dll,names]of groups)for(const name of names){assert.ok(vm.bind(dll,name));for(const wrong of ['kernel32.dll','user32.dll','d3d12.dll'])if(!(name==='GetPixelFormat'&&mode===64&&wrong!=='d3d12.dll'))assert.throws(()=>vm.bind(wrong,name),/unknown|migrated/,`${mode}: ${wrong}!${name}`);}
 vm.dispose();
}

// HRESULT failures clear only their actual output widths and preserve LastError.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,width=mode/8;vm.write(0x20000n,new Uint8Array(16).fill(1));vm.write(t.error,encode(123n,4));
 for(const name of ['DXGIGetDebugInterface','DXGIGetDebugInterface1']){
  const args=(iid,out)=>name.endsWith('1')?[0n,iid,out]:[iid,out];
  vm.write(0x22000n,new Uint8Array(16).fill(0xa5));equal(api(t,name,args(0x20000n,0x22000n)),0x80004002n);equal(bytes(vm,0x22000n,16),Uint8Array.from([...new Uint8Array(width),...new Uint8Array(16-width).fill(0xa5)]));error(t,123);
  for(const[iid,out]of [[0n,0x22000n],[0x20000n,0n]])equal(api(t,name,args(iid,out)),0x80070057n);
  vm.write(0x20000n,new Uint8Array(16).fill(1));equal(api(t,name,args(0x20000n,0x20000n)),0x80004002n);equal(vm.readInt(0x20000n,width),0n);
  vm.write(0x20000n,new Uint8Array(16).fill(1));equal(api(t,name,args(0x20000n,t.sp)),0x80004002n);equal(vm.ip,vm.entryPoints.returned);
 }
 const device=mode===32?0x1234n:0x123400001234n;
 const args=[device,0n,1n,0xffffffffn,1n,0xffffffffn,0n,0x22000n,0x22010n,0x22020n];
 for(const p of args.slice(7))vm.write(p,new Uint8Array(16).fill(0xa5));
 equal(api(t,'D3D11On12CreateDevice',args),0x887a0004n);error(t,123);
 for(const[i,p]of args.slice(7).entries())equal(bytes(vm,p,16),Uint8Array.from([...new Uint8Array(i===2?4:width),...new Uint8Array(16-(i===2?4:width)).fill(0xa5)]));
 equal(api(t,'D3D11On12CreateDevice',[device]),0x887a0004n);equal(api(t,'D3D11On12CreateDevice',[0n]),0x80070057n);
 equal(api(t,'D3D11On12CreateDevice',[device,0n,0n,0n,0n,0n,0n,t.sp,t.sp,0n]),0x887a0004n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp,width),0n);
 if(mode===64){const high=0x123400000000n;vm.map(high,4096n,7);vm.write(high,new Uint8Array(16).fill(1));equal(api(t,'DXGIGetDebugInterface1',[(1n<<32n)|0xffffffffn,high,high+32n]),0x80004002n);equal(vm.readInt(high+32n,8),0n);}
 vm.dispose();
}

// A late copyout or IID read fault must keep every earlier output and the saved CPU.
for(const mode of [32,64])for(const operation of ['debug-output','debug-input','on12-context','on12-feature','return']){
 const t=await machine(mode),{vm}=t;vm.write(0x20000n,new Uint8Array(16).fill(1));for(const p of [0x22000n,0x22010n,0x22020n,0x24ffen])vm.write(p,new Uint8Array(16).fill(0xa5));
 let page=0x25000n,protection=2;
 if(operation==='debug-output')frame(t,'DXGIGetDebugInterface1',[0n,0x20000n,0x24ffen],'dxgi.dll');
 if(operation==='debug-input'){vm.write(0x24ff8n,new Uint8Array(16).fill(1));protection=1;frame(t,'DXGIGetDebugInterface1',[0n,0x24ff8n,0x22000n],'dxgi.dll');}
 if(operation==='on12-context')frame(t,'D3D11On12CreateDevice',[1n,0n,0n,0n,0n,0n,0n,0x22000n,0x24ffen,0x22020n],'d3d11.dll');
 if(operation==='on12-feature')frame(t,'D3D11On12CreateDevice',[1n,0n,0n,0n,0n,0n,0n,0x22000n,0x22010n,0x24ffen],'d3d11.dll');
 if(operation==='return'){page=t.sp&~4095n;protection=1;frame(t,'D3D11On12CreateDevice',[1n,0n,0n,0n,0n,0n,0n,0x22000n,0x22010n,0x22020n],'d3d11.dll');}
 equal(core.wire_protect(t.handle,page,4096n,protection),0);const before=snapshot(t),work=vm.work,retired=vm.retired;
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.work,work);equal(vm.retired,retired);for(const p of [0x22000n,0x22010n,0x22020n])equal(bytes(vm,p,16),new Uint8Array(16).fill(0xa5));
 equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),operation.startsWith('debug')?0x80004002n:0x887a0004n);vm.dispose();
}

// Query/release do not access LastError; failing operations preflight its real slot.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,hwnd=await window(t),dc=call(t,'GetDC',[hwnd],'user32.dll');
 equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);
 for(const[name,args,result]of [['wglGetCurrentContext',[],0n],['wglGetCurrentDC',[],0n],['wglMakeCurrent',[1n,0n],1n],['GetPixelFormat',[dc],0n]])equal(api(t,name,args),result);
 frame(t,'wglCreateContext',[dc],'opengl32.dll');const before=snapshot(t),work=vm.work;equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.work,work);
 equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0n);error(t,50);equal(call(t,'ReleaseDC',[hwnd,dc],'user32.dll'),1n);vm.dispose();
}

// Every optional import binds in formal mode; a real guest branch handles GL absence.
for(const mode of [32,64]){
 const all=[...groups,['kernel32.dll',['GetCurrentProcessId']]],f=fixture(mode,{tls:false}),slots=[];
 f.bytes.fill(0,f.offset(0x2000),f.offset(0x2600));f.put(f.dirs+12,(all.length+1)*20);let at=0x2000+(all.length+1)*20;
 for(const[index,[dll,names]]of all.entries()){
  const desc=0x2000+index*20;f.rput(desc+12,at);f.rtext(at,dll+'\0');at+=dll.length+1;at=Math.ceil(at/f.ptr)*f.ptr;
  const lookup=at;at+=(names.length+1)*f.ptr;const iat=at;at+=(names.length+1)*f.ptr;f.rput(desc,lookup);f.rput(desc+16,iat);
  for(const[i,name]of names.entries()){f.rput(lookup+i*f.ptr,at,f.ptr);f.rput(iat+i*f.ptr,at,f.ptr);f.rtext(at+2,name+'\0');at+=name.length+3;slots.push({dll,name,rva:iat+i*f.ptr});}
 }
 assert.ok(at<0x2600);const create=BigInt(slots.find(s=>s.name==='wglCreateContext').rva),fallback=BigInt(slots.at(-1).rva);
 if(mode===32){f.bytes.set([0x6a,0,0xff,0x15,...encode(f.preferred+create,4),0x85,0xc0,0x75,7,0xff,0x15,...encode(f.preferred+fallback,4),0xc3,0xb8,0xad,0xde,0,0,0xc3],f.offset(0x1000));f.relocations.splice(0,f.relocations.length,0x1004,0x100e);}
 else{f.bytes.set([0x48,0x83,0xec,40,0x31,0xc9,0xff,0x15,...encode(create-0x100cn,4),0x48,0x85,0xc0,0x75,11,0xff,0x15,...encode(fallback-0x1017n,4),0x48,0x83,0xc4,40,0xc3,0xb8,0xad,0xde,0,0,0x48,0x83,0xc4,40,0xc3],f.offset(0x1000));f.relocations.length=0;}
 f.rebuildRelocations();const vm=new WasmMachine(core,{mode}),image=vm.load(f.bytes);for(const{dll,name,rva}of slots)equal(vm.readInt(image.base+BigInt(rva),f.ptr),vm.bind(dll,name));
 equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),1n);assert.ok(vm.retired>0n);equal(vm.readInt(image.teb+BigInt(mode===32?0x34:0x68)),6n);vm.dispose();
}
console.log(`${checks} optional graphics availability/ABI/fault/formal-import/JIT checks passed`);
