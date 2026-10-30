import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,gdi,call,window} from './gdi-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';
const names=['CreateFontA','CreateFontW','CreateFontIndirectA','CreateFontIndirectW','GetTextMetricsW'];
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const direct=(p=0n)=>[0xffffffe0n,11n,23n,34n,700n,0x12345678n,0x22334455n,1n,128n,2n,3n,4n,5n,p];
const logfont=(vm,p,wide=true)=>{vm.write(p,new Uint8Array(wide?92:60));vm.write(p,encode(0xffffffe8n,4));vm.write(p+16n,encode(500n,4));};

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;for(const name of names){if(mode===32)assert.ok(vm.bind('gdi32.dll',name));else assert.throws(()=>vm.bind('gdi32.dll',name),/migrated|unknown/);assert.throws(()=>vm.bind('user32.dll',name),/migrated|unknown/);}vm.dispose();
}

// Direct 14-argument and indirect LOGFONT paths retain exact fields, byte flags and owned A/W names.
for(const wide of [false,true])for(const indirect of [false,true]){
 const t=await machine(32),{vm}=t;logfont(vm,0x22000n,wide);const title=wide?Buffer.from('日本font\ud800\0','utf16le'):Buffer.from([0x93,0xfa,0x96,0x7b,102,111,110,116,0]);vm.write(indirect?0x2201cn:0x23000n,title);vm.write(t.error,encode(77n,4));
 const h=gdi(t,`CreateFont${indirect?'Indirect':''}${wide?'W':'A'}`,indirect?[0x22000n]:direct(0x23000n));assert.ok(h);equal(vm.register(4),t.sp+BigInt(indirect?8:60));vm.write(0x22000n,new Uint8Array(92));vm.write(0x23000n,new Uint8Array(64));equal(gdi(t,'GetObjectType',[h]),6n);equal(gdi(t,'GetObjectW',[h,0n,0n]),92n);equal(gdi(t,'GetObjectW',[h,92n,0x24000n]),92n);equal(vm.readInt(0x24000n),indirect?0xffffffe8n:0xffffffe0n);equal(vm.readInt(0x24010n),indirect?500n:700n);if(!indirect){equal(bytes(vm,0x24014n,8),new Uint8Array([0x78,0x55,1,128,2,3,4,5]));}
 equal(bytes(vm,0x2401cn,wide?16:14),new Uint8Array(Buffer.from(wide?'日本font\ud800\0':'日本font\0','utf16le')));equal(vm.readInt(0x2405an,2),0n);equal(gdi(t,'GetObjectA',[h,60n,0x24100n]),60n);equal(bytes(vm,0x2411cn,4),new Uint8Array([0x93,0xfa,0x96,0x7b]));equal(gdi(t,'GetObjectW',[h,91n,0xdead0000n]),0n);equal(vm.readInt(t.error),87n);equal(gdi(t,'GetObjectW',[h,92n,t.sp]),92n);equal(vm.ip,vm.entryPoints.returned);equal(gdi(t,'DeleteObject',[h]),1n);equal(gdi(t,'GetObjectType',[h]),0n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Stock fonts and selections share the same process GDI ownership; selected and stock objects have distinct lifetimes.
{
 const t=await machine(32),{vm}=t,dc=gdi(t,'CreateCompatibleDC',[0n]),other=gdi(t,'CreateCompatibleDC',[dc]),system=gdi(t,'GetCurrentObject',[dc,6n]);equal(system,gdi(t,'GetStockObject',[13n]));
 for(const id of [10n,11n,12n,13n,14n,16n,17n]){const h=gdi(t,'GetStockObject',[id]);equal(gdi(t,'GetObjectType',[h]),6n);equal(gdi(t,'DeleteObject',[h]),1n);equal(gdi(t,'GetObjectType',[h]),6n);}
 const font=gdi(t,'CreateFontW',direct());equal(gdi(t,'SelectObject',[dc,font]),system);equal(gdi(t,'SelectObject',[other,font]),system);equal(gdi(t,'DeleteObject',[font]),0n);equal(vm.readInt(t.error),170n);equal(gdi(t,'SelectObject',[dc,system]),font);equal(gdi(t,'DeleteObject',[font]),0n);equal(gdi(t,'DeleteDC',[other]),1n);equal(gdi(t,'DeleteObject',[font]),1n);equal(gdi(t,'GetCurrentObject',[dc,6n]),system);
 const hwnd=await window(t),windowDc=call(t,'GetDC',[hwnd]),fresh=gdi(t,'CreateFontA',direct());equal(gdi(t,'SelectObject',[windowDc,fresh]),system);equal(gdi(t,'DeleteObject',[fresh]),0n);equal(call(t,'ReleaseDC',[hwnd,windowDc]),1n);equal(gdi(t,'DeleteObject',[fresh]),1n);vm.dispose();
}

// Metrics have no Wasm realization backend; errors retain the caller output and validate the DC first.
{
 const t=await machine(32),{vm}=t,dc=gdi(t,'CreateCompatibleDC',[0n]);vm.write(0x24000n,new Uint8Array(60).fill(0xa5));equal(gdi(t,'GetTextMetricsW',[dc,0x24000n]),0n);equal(vm.readInt(t.error),120n);equal(bytes(vm,0x24000n,60),new Uint8Array(60).fill(0xa5));equal(gdi(t,'GetTextMetricsW',[0n,0xdead0000n]),0n);equal(vm.readInt(t.error),6n);
 frame(t,'GetTextMetricsW',[dc,0xdead0000n],'gdi32.dll');const before=snapshot(t);equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(t.error),120n);vm.dispose();
}

// Indirect A is strict CP932, direct A is lossy, and both layouts read exactly their declared size.
{
 const t=await machine(32),{vm}=t;logfont(vm,0x22000n,false);vm.write(0x2201cn,[0x82,0]);equal(gdi(t,'CreateFontIndirectA',[0x22000n]),0n);equal(vm.readInt(t.error),87n);vm.write(0x23000n,[0x82,0]);const lossy=gdi(t,'CreateFontA',direct(0x23000n));assert.ok(lossy);gdi(t,'GetObjectW',[lossy,92n,0x24000n]);equal(vm.readInt(0x2401cn,2),0xfffdn);
 for(const wide of [false,true]){const size=wide?92n:60n,p=0x25000n-size;logfont(vm,p,wide);vm.write(p+28n,wide?Buffer.from('Z\0','utf16le'):[90,0]);equal(core.wire_protect(t.handle,0x25000n,4096n,1),0);assert.ok(gdi(t,wide?'CreateFontIndirectW':'CreateFontIndirectA',[p]));equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);}
 vm.write(0x23000n,Buffer.from('X'.repeat(40)+'\0','utf16le'));const long=gdi(t,'CreateFontW',direct(0x23000n));gdi(t,'GetObjectW',[long,92n,0x24000n]);equal(vm.readInt(0x24058n,2),88n);equal(vm.readInt(0x2405an,2),0n);vm.dispose();
}

// Source, complete output and initial return-frame faults preserve identities, objects and CPU before retry.
for(const fault of ['input','output','return']){
 const t=await machine(32),{vm}=t;let h,name,args;
 if(fault==='output'){h=gdi(t,'CreateFontW',direct());name='GetObjectW';args=[h,92n,0x24fd0n];vm.write(0x24fd0n,new Uint8Array(92).fill(0xa5));}
 else{name='CreateFontIndirectW';args=[fault==='input'?0x60000n:0x22000n];if(fault==='return')logfont(vm,0x22000n);}
 frame(t,name,args,'gdi32.dll');const before=snapshot(t),page=fault==='return'?t.sp&~4095n:0x25000n;if(fault!=='input')equal(core.wire_protect(t.handle,page,4096n,fault==='return'?1:2),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);
 if(fault==='input'){vm.map(0x60000n,4096n,3);logfont(vm,0x60000n);}else equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);if(fault==='output')equal(vm.register(0),92n);else equal(vm.register(0),0x43010000n);equal(vm.ip,vm.entryPoints.returned);vm.dispose();
}

// Font/DC/bitmap objects consume one shared capacity; freeing a font permits a bitmap without reusing identities.
{
 const t=await machine(32),{vm}=t,dc=gdi(t,'CreateCompatibleDC',[0n]);let first,last;
 for(let n=0;n<4087;n++){frame(t,'CreateFontW',direct(),'gdi32.dll');assert.equal(vm.service().reason,Exit.Dispatch);const h=vm.register(0);assert.ok(h);first??=h;last=h;}
 equal(gdi(t,'CreateFontW',direct()),0n);equal(vm.readInt(t.error),8n);equal(gdi(t,'CreateCompatibleDC',[0n]),0n);equal(gdi(t,'DeleteObject',[first]),1n);const bitmap=gdi(t,'CreateCompatibleBitmap',[dc,1n,1n]);equal(bitmap,last+16n);equal(gdi(t,'GetObjectType',[bitmap]),7n);vm.dispose();
}

// Every font export imports together; generated guest code exercises the real 14-slot stdcall/JIT path.
{
 const f=fixture(32,{tls:false});f.bytes.fill(0,f.offset(0x2000),f.offset(0x2600));f.rput(0x2000,0x2080);f.rput(0x200c,0x2180);f.rput(0x2010,0x2100);f.rtext(0x2180,'gdi32.dll\0');let at=0x2200;for(const[i,name]of names.entries()){f.rput(0x2080+i*4,at);f.rput(0x2100+i*4,at);f.rtext(at+2,name+'\0');at+=name.length+3;}
 const code=[];for(const a of direct().toReversed())code.push(0x68,...encode(a,4));const operand=0x1000+code.length+2;code.push(0xff,0x15,...encode(f.preferred+0x2104n,4),0xc3);f.bytes.set(code,f.offset(0x1000));f.relocations.splice(0,f.relocations.length,operand);f.rebuildRelocations();const vm=new WasmMachine(core,{mode:32}),image=vm.load(f.bytes);for(const[i,name]of names.entries())equal(vm.readInt(image.base+0x2100n+BigInt(i*4)),vm.bind('gdi32.dll',name));equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),0x43010000n);equal(vm.register(4),image.sp+4n);assert.ok(vm.retired>0n);vm.dispose();
}
console.log(`${checks} logical font/LOGFONT/stock/selection/ownership/ABI/JIT/fault checks passed`);
