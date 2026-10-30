import assert from 'node:assert/strict';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';
import {moduleFixture} from './module-fixture.mjs';
import {fp} from './x87-harness.mjs';
import {fromFloat,store} from './x87-reference.mjs';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
const crt=(t,name,args=[],dll='msvcrt.dll')=>{const value=call(t,name,args,dll);equal(t.vm.register(4),t.sp+4n);return value;};
const text=(t,p,s)=>t.vm.write(p,new TextEncoder().encode(s+'\0'));
const bytes=(t,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(t.vm.readInt(p+BigInt(i),1)));
const bits=n=>{const b=new ArrayBuffer(8),v=new DataView(b);v.setFloat64(0,n,true);return v.getBigUint64(0,true);};
const words=n=>{const v=bits(n);return[v&0xffffffffn,v>>32n];};
const reset=(t,values=[],rc=0)=>{const f=fp(t);f.cw=0x37f|(rc<<10);f.sw=0;f.tags=0xffff;f.regs.fill(0n);for(const[n,value]of values.entries()){f.regs[n]=fromFloat(bits(value),64).value;f.tags&=~(3<<(n*2));}fp(t,f);};
const top=t=>{const f=fp(t);return store(f.regs[(f.sw>>11)&7],f.cw,64).value;};
const exports=[['_errno',0],['_ftol',0],['_ftol2',0],['_ftol2_sse',0],['_CIpow',0],['floor',2],['ldexp',3],['pow',4],['time',1],['_time32',1],['_mbsinc',1],['_mbsnbcpy',3],['memcpy',3],['memmove',3],['memset',3],['memcmp',3],['toupper',1],['tolower',1],['strcmp',2],['strncmp',3],['strlen',1],['strtoul',3]];

// Fresh dynamic lookup, provider ownership and PE imports use the scalar service ABI.
{
 const t=await machine(32),{vm}=t;
 for(const dll of['msvcrt.dll','ucrtbase.dll','msvcr100.dll','api-ms-win-crt-runtime-l1-1-0.dll']){
  text(t,0x20000n,dll);const provider=call(t,'LoadLibraryA',[0x20000n],'kernel32.dll');assert.ok(provider);
  for(const[name]of exports){text(t,0x20080n,name);const pc=call(t,'GetProcAddress',[provider,0x20080n],'kernel32.dll');equal(pc,vm.bind(dll,name));equal(call(t,'GetModuleHandleExW',[6n,pc,0x21000n],'kernel32.dll'),1n);equal(vm.readInt(0x21000n),provider);}
 }
 assert.throws(()=>vm.bind('user32.dll','strtoul'),/unknown/);assert.throws(()=>vm.bind('msvcrt.dll','STRTOUL'),/unknown/);assert.throws(()=>vm.bind('msvcrt.dll','unknown_crt_export'),/unknown/);
 equal(vm.bind('MSVCRT','strlen'),vm.bind('msvcrt.dll','strlen'));equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
 const q=await machine(64);for(const[name]of exports)assert.throws(()=>q.vm.bind('msvcrt.dll',name),/unknown/);q.vm.dispose();
 const f=fixture(32,{tls:false});f.rtext(0x20c0,'ucrtbase.dll\0');f.rtext(0x20e2,'strlen\0');f.rtext(0x3000,'scalar CRT\0');f.bytes[f.offset(0x100b)]=0x83;f.bytes.set([0xc4,4,0xc3],f.offset(0x100c));f.relocations.splice(f.relocations.indexOf(0x100c),1);f.rebuildRelocations();const v=new WasmMachine(core,{mode:32,clock(){throw new Error('unexpected clock');}});v.load(f.bytes);equal((await v.run(100n)).reason,Exit.Returned);equal(v.register(0),10n);v.dispose();
}
// Unsigned bytes, overlap, exact widths, CP932 truncation and C locale use shared bodies.
{
 const t=await machine(32),{vm}=t;text(t,0x22000n,'abc');text(t,0x22100n,'abd');equal(crt(t,'strlen',[0x22000n]),3n);equal(crt(t,'strcmp',[0x22000n,0x22100n]),0xffffffffn);equal(crt(t,'strncmp',[0x22000n,0x22100n,2n]),0n);equal(crt(t,'strncmp',[0n,0n,0n]),0n);equal(crt(t,'memcmp',[0n,0n,0n]),0n);
 vm.write(0x22000n,[0,128,255,4,5,6]);vm.write(0x22100n,[0,128,254,4,5,6]);equal(crt(t,'memcmp',[0x22000n,0x22100n,6n]),1n);equal(crt(t,'memcmp',[0x22100n,0x22000n,6n]),0xffffffffn);equal(crt(t,'memmove',[0x22002n,0x22000n,4n]),0x22002n);equal(bytes(t,0x22000n,6),Uint8Array.of(0,128,0,128,255,4));equal(crt(t,'memcpy',[0x22100n,0x22000n,6n]),0x22100n);equal(bytes(t,0x22100n,6),bytes(t,0x22000n,6));equal(crt(t,'memset',[0x22101n,0x1abn,3n]),0x22101n);equal(bytes(t,0x22100n,6),Uint8Array.of(0,171,171,171,255,4));
 for(const[name,a,b]of[['toupper',97n,65n],['tolower',90n,122n],['toupper',0xffffffffn,0xffffffffn],['tolower',255n,255n]])equal(crt(t,name,[a]),b);
 vm.write(0x22000n,[0x61,0x83,0x5c,0x62,0]);equal(crt(t,'_mbsinc',[0x22001n]),0x22003n);equal(crt(t,'_mbsnbcpy',[0x22100n,0x22000n,2n]),0x22100n);equal(bytes(t,0x22100n,2),Uint8Array.of(0x61,0));crt(t,'_mbsnbcpy',[0x22100n,0x22000n,6n]);equal(bytes(t,0x22100n,6),Uint8Array.of(0x61,0x83,0x5c,0x62,0,0));vm.write(0x22000n,[0x83,0]);equal(crt(t,'_mbsinc',[0x22000n]),0x22001n);
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}
// Parsing preserves end positions, range errors and a stable per-thread errno cell.
{
 const t=await machine(32),{vm}=t,p=crt(t,'_errno');equal(vm.readInt(p),0n);equal(crt(t,'_errno'),p);
 for(const[input,base,value,end,error]of[['  -0x10rest',0,0xfffffff0n,7,0],['0779',0,63n,3,0],['0xZ',0,0n,1,0],[' +z',10,0n,0,0],['4294967296x',10,0xffffffffn,10,34],['12',1,0n,0,22],['z',36,35n,1,0]]){
  text(t,0x22000n,input);vm.write(p,encode(0n,4));equal(crt(t,'strtoul',[0x22000n,0x22100n,BigInt(base)]),value);equal(vm.readInt(0x22100n),0x22000n+BigInt(end));equal(vm.readInt(p),BigInt(error));
 }
 vm.write(p,encode(73n,4));text(t,0x22000n,'8');equal(crt(t,'strtoul',[0x22000n,0n,10n]),8n);equal(vm.readInt(p),73n);
 // Guest cdecl calls in a worker get distinct storage; the main cell remains owned.
 const errnoPc=vm.bind('msvcrt.dll','_errno');vm.write(0x30000n,[0xb8,...encode(errnoPc,4),0xff,0xd0,0xa3,...encode(0x22200n,4),0xc7,0,99,0,0,0,0xc2,4,0]);const h=call(t,'CreateThread',[0n,65536n,0x30000n,0n,0n,0n],'kernel32.dll');frame(t,'WaitForSingleObject',[h,0xffffffffn],'kernel32.dll');equal((await vm.run(1000n)).reason,Exit.Returned);const worker=vm.readInt(0x22200n);assert.ok(worker!==p);equal(vm.readInt(worker),99n);equal(vm.readInt(p),73n);equal(crt(t,'_errno'),p);vm.dispose();
}
// x87 CRT results consume or push the ABI stack and preserve numerical state on errno faults.
{
 const t=await machine(32),{vm}=t,p=crt(t,'_errno');
 for(const x of[-3.125,3.125,-0,0,Infinity]){reset(t,[42]);crt(t,'floor',words(x));equal(top(t),bits(Math.floor(x)));const f=fp(t);equal((f.sw>>11)&7,7);equal(f.regs[0],fromFloat(bits(42),64).value);}
 for(const[name,args,expected,error]of[['pow',[...words(-2),...words(3)],-8,0],['pow',[...words(2),...words(-3)],0.125,0],['pow',[...words(-0),...words(-3)],-Infinity,34],['ldexp',[...words(1),0xfffffff6n],1/1024,0]]){reset(t);vm.write(p,encode(0n,4));crt(t,name,args);equal(top(t),bits(expected));equal(vm.readInt(p),BigInt(error));}
 reset(t,[-3,2]);crt(t,'_CIpow');equal(top(t),bits(0.125));equal((fp(t).sw>>11)&7,1);
 for(const name of['_ftol','_ftol2','_ftol2_sse']){reset(t,[-4294967296.5],2);crt(t,name);equal(vm.register(0)|(vm.register(2)<<32n),BigInt.asUintN(64,-4294967296n));equal((fp(t).sw>>11)&7,1);}
 for(const rc of[0,1,2,3]){reset(t,[],rc);crt(t,'ldexp',[...words(1),0x7fffffffn]);equal(top(t),rc===1||rc===3?0x7fefffffffffffffn:0x7ff0000000000000n);equal(vm.readInt(p),34n);}
 reset(t);frame(t,'pow',[...words(-2),...words(0.5)],'msvcrt.dll');equal(core.wire_protect(t.handle,p&~4095n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,p&~4095n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(p),33n);assert.ok((top(t)&0x7ff0000000000000n)===0x7ff0000000000000n&&(top(t)&0xfffffffffffffn)!==0n);equal(t.time.calls,0);vm.dispose();
}
// Complete ranges are validated even when comparison differs before a later guard page.
for(const kind of['copy-source','copy-output','compare-tail','parse-end','parse-errno','return']){
 const t=await machine(32),{vm}=t,p=crt(t,'_errno');text(t,0x22000n,'4294967296x');vm.write(0x24ffen,[1,2,3,4]);vm.write(0x22100n,[9,8,7,6]);let name='memcpy',a=[0x22100n,0x24ffen,4n],page=0x25000n,access=1;
 if(kind==='copy-output'){a=[0x24ffen,0x22100n,4n];access=2;}
 if(kind==='compare-tail'){name='memcmp';a=[0x24ffen,0x22100n,4n];}
 if(kind==='parse-end'){name='strtoul';a=[0x22000n,0x24ffen,10n];access=2;}
 if(kind==='parse-errno'){name='strtoul';a=[0x22000n,0x22100n,10n];page=p&~4095n;access=2;}
 if(kind==='return'){name='memset';a=[0x22100n,0n,4n];page=t.sp&~4095n;}
 frame(t,name,a,'msvcrt.dll');equal(core.wire_protect(t.handle,page,4096n,access),0);const before=snapshot(t),prefix=bytes(t,0x24ffen,2),out=bytes(t,0x22100n,4);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(t,0x24ffen,2),prefix);equal(bytes(t,0x22100n,4),out);equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);vm.dispose();
}
// Output can replace the saved return slot without changing the completed target.
{
 const t=await machine(32),{vm}=t;text(t,0x22000n,'123tail');frame(t,'strtoul',[0x22000n,t.sp,10n],'msvcrt.dll');equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp),0x22003n);equal(vm.register(4),t.sp+4n);vm.dispose();
}
// Oversized requests are rejected before allocation or any truncated copy can occur.
{
 const t=await machine(32),{vm}=t;vm.write(0x22000n,[1,2,3,4]);
 for(const name of['memcpy','memmove','memset']){frame(t,name,[0x22000n,0x22100n,0x4000001n],'msvcrt.dll');const before=snapshot(t);assert.throws(()=>vm.service(),/CRT memory range too large/);equal(snapshot(t),before);equal(bytes(t,0x22000n,4),Uint8Array.of(1,2,3,4));}
 vm.dispose();
}
// Only time samples FILETIME; pre-epoch and time32 overflow return the CRT failure value.
for(const[seconds,expected]of[[123456789n,123456789n],[2147483647n,2147483647n],[2147483648n,0xffffffffn],[-1n,0xffffffffn]]){
 const t=await machine(32,{wallClock:()=>116444736000000000n+seconds*10000000n});for(const name of['time','_time32']){equal(crt(t,name,[0x22000n]),expected);equal(t.vm.readInt(0x22000n),expected);}equal(t.time.calls,0);equal(t.time.wallCalls,2);t.vm.dispose();
}
// Runtime errno data is reclaimed along with a statically rejected process.
{
 const v=new WasmMachine(core,{mode:32});v.supplyModule('bad.dll',moduleFixture(32,{dll:true,attach:false,tls:false}).bytes);const image=v.load(moduleFixture(32,{dependency:'bad.dll',tls:false}).bytes),t={vm:v,mode:32,image,sp:image.sp-2048n},ip=v.ip,registers=Array.from({length:8},(_,i)=>v.register(i)),p=crt(t,'_errno');v.ip=ip;registers.forEach((r,i)=>v.register(i,r));await assert.rejects(v.run(10000n),/attach returned FALSE/);assert.throws(()=>v.readInt(p),/memory access/);v.dispose();
}
console.log(`${checks} CRT scalar/cdecl/x87/errno/time/import/fault checks passed`);
