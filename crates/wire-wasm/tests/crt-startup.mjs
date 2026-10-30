import assert from 'node:assert/strict';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';
import {moduleFixture} from './module-fixture.mjs';
import {fp} from './x87-harness.mjs';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
const kernel=(t,n,a=[])=>call(t,n,a,'kernel32.dll');
const crt=(t,n,a=[],dll='msvcrt.dll')=>{const v=call(t,n,a,dll);equal(t.vm.register(4),t.sp+4n);return v;};
const ascii=(t,p,s)=>t.vm.write(p,new TextEncoder().encode(s+'\0'));
const wide=(t,p,s)=>t.vm.write(p,[...s+'\0'].flatMap(c=>[c.charCodeAt(0)&255,c.charCodeAt(0)>>8]));
const string=(vm,p,width=1)=>{const a=[];for(let n=0;n<65536;n++){const v=Number(vm.readInt(p+BigInt(n*width),width));if(!v)return width===1?String.fromCharCode(...a):String.fromCharCode(...a);a.push(v);}throw new Error('unterminated test string');};
const array=(vm,p,width=1)=>{const a=[];for(let n=0;n<2048;n++){const q=vm.readInt(p+BigInt(n*4));if(!q)return a;a.push(string(vm,q,width));}throw new Error('unterminated test vector');};
const bare=()=>{const f=fixture(32,{tls:false});f.put(f.dirs+8,0);f.put(f.dirs+12,0);f.bytes.fill(0x90,f.offset(0x1000),f.offset(0x1100));f.bytes[f.offset(0x1000)]=0xc3;f.relocations.splice(0,f.relocations.length);f.rebuildRelocations();return f;};
async function loaded(command='"C:\\program.exe"'){let handle;const vm=new WasmMachine({...core,wire_create(...a){handle=core.wire_create(...a);return handle;}},{mode:32,clock:()=>0n,wallClock(){throw new Error('unexpected wall clock');}}),image=vm.load(bare().bytes,{command});equal((await vm.run(100n)).reason,Exit.Returned);vm.map(0x20000n,0x40000n,7);return{vm,handle,mode:32,image,sp:image.sp-2048n,error:image.teb+0x34n};}
async function complete(t,budget=1n){for(let n=0;n<20000;n++){const r=await t.vm.run(budget);if(r.reason!==Exit.Budget)return r;}throw new Error('CRT continuation did not finish');}
async function callback(t,name,args=[],budget=1n,dll='msvcrt.dll'){frame(t,name,args,dll);const r=await complete(t,budget);equal(r.reason,Exit.Returned);equal(t.vm.register(4),t.sp+4n);return t.vm.register(0);}
function lookup(t,name,dll='msvcrt.dll'){ascii(t,0x20000n,dll);const h=kernel(t,'LoadLibraryA',[0x20000n]);ascii(t,0x20100n,name);const p=kernel(t,'GetProcAddress',[h,0x20100n]);assert.ok(p);return p;}
const stamp=(t,at,tag,tail=[])=>{t.vm.write(at,[0xa1,...encode(0x23000n,4),0xc1,0xe0,4,0x83,0xc8,tag,0xa3,...encode(0x23000n,4),...tail,0xc3]);return at;};

// All legacy CRT data imports resolve to writable storage shared with lookup and accessors.
for(const name of['__argc','_acmdln','_adjust_fdiv','_commode','_daylight','_fmode','_osver','_timezone','_wcmdln','_winmajor','_winminor','_winver']){
 const f=moduleFixture(32,{dependency:'msvcrt.dll',data:true});f.rtext(0x20e2,name+'\0');let handle;const vm=new WasmMachine({...core,wire_create(...a){handle=core.wire_create(...a);return handle;}},{mode:32}),image=vm.load(f.bytes,{base:name==='__argc'?0x50000000n:0n}),t={vm,handle,mode:32,image,sp:image.sp-2048n};vm.map(0x20000n,0x10000n,7);const p=vm.readInt(image.base+0x20a0n);assert.ok(p<image.base||p>=image.base+image.size);equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),vm.readInt(p));equal(lookup(t,name),p);
 if(['__argc','_acmdln','_commode','_daylight','_fmode','_timezone','_wcmdln'].includes(name)){const accessor='__p_'+name;equal(crt(t,accessor),p);vm.write(p,encode(0x12345678n,4));equal(vm.readInt(crt(t,accessor)),0x12345678n);}
 const provider=lookup(t,'__p___argc');equal(kernel(t,'GetModuleHandleExW',[6n,p,0x20200n]),1n);equal(kernel(t,'GetModuleHandleExW',[6n,provider,0x20204n]),1n);equal(vm.readInt(0x20200n),vm.readInt(0x20204n));assert.throws(()=>vm.bind('msvcrt.dll',name),/unknown/);vm.dispose();
}
// Quoted arguments and distinct narrow/wide vectors use the shared command-line parser.
{
 const t=await loaded('"C:\\games\\test.exe" alpha "two words" 日本'),{vm}=t;wide(t,0x22000n,'CRT_TEST');wide(t,0x22100n,'日本');equal(kernel(t,'SetEnvironmentVariableW',[0x22000n,0x22100n]),1n);
 const argc=crt(t,'__p___argc'),argv=crt(t,'__p___argv'),wargv=crt(t,'__p___wargv'),env=crt(t,'__p__environ'),wenv=crt(t,'__p__wenviron');equal(vm.readInt(argc),4n);equal(array(vm,vm.readInt(wargv),2),['C:\\games\\test.exe','alpha','two words','日本']);equal(array(vm,vm.readInt(argv)).slice(0,3),['C:\\games\\test.exe','alpha','two words']);const japanese=vm.readInt(vm.readInt(argv)+12n);equal([0n,1n,2n,3n].map(o=>vm.readInt(japanese+o,1)),[0x93n,0xfan,0x96n,0x7bn]);assert.ok(array(vm,vm.readInt(wenv),2).includes('CRT_TEST=日本'));equal(vm.readInt(crt(t,'__p___initenv')),vm.readInt(env));equal(vm.readInt(crt(t,'__p___winitenv')),vm.readInt(wenv));equal(string(vm,vm.readInt(crt(t,'__p__pgmptr'))),'C:\\games\\test.exe');equal(string(vm,vm.readInt(crt(t,'__p__wpgmptr')),2),'C:\\games\\test.exe');
 equal(crt(t,'__getmainargs',[0x22200n,0x22204n,0x22208n,0n,0n]),0n);equal([0n,4n,8n].map(o=>vm.readInt(0x22200n+o)),[4n,vm.readInt(argv),vm.readInt(env)]);
 wide(t,0x22100n,'changed');kernel(t,'SetEnvironmentVariableW',[0x22000n,0x22100n]);assert.ok(array(vm,vm.readInt(wenv),2).includes('CRT_TEST=日本'));equal(crt(t,'_initialize_narrow_environment'),0n);equal(crt(t,'_initialize_wide_environment'),0n);
 equal(crt(t,'_configure_narrow_argv',[0n]),0n);equal(vm.readInt(argc),0n);equal(vm.readInt(argv),0n);equal(crt(t,'_configure_narrow_argv',[1n]),0n);equal(vm.readInt(argc),4n);equal(array(vm,vm.readInt(argv)).length,4);equal(crt(t,'_configure_wide_argv',[2n]),22n);equal(vm.readInt(crt(t,'_errno')),22n);
 const mode=lookup(t,'_fmode');equal(mode,crt(t,'__p__fmode'));equal(crt(t,'_set_fmode',[0x8000n]),0n);equal(vm.readInt(mode),0x8000n);equal(crt(t,'_set_fmode',[9n]),22n);equal(vm.readInt(mode),0x8000n);equal(crt(t,'__set_app_type',[2n]),0n);equal(crt(t,'_set_app_type',[1n]),0n);vm.dispose();
}
// Mainargs preflights all outputs and saves values before they replace the caller's frame.
{
 const t=await loaded('"C:\\program.exe" arg'),{vm}=t;vm.write(0x22000n,encode(0xa5a5a5a5n,4));frame(t,'__getmainargs',[0x22000n,0x24ffen,0x22100n,0n,0n],'msvcrt.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(0x22000n),0xa5a5a5a5n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(0x22000n),2n);
 frame(t,'__getmainargs',[t.sp,t.sp+8n,t.sp+12n,0n,0n],'msvcrt.dll');equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp),2n);equal(vm.register(4),t.sp+4n);vm.dispose();
}
// New CRT provider data appears only after a dynamic dependency graph is published.
{
 const bad=moduleFixture(32,{dll:true,dependency:'ucrtbase.dll',entry:false});bad.rtext(0x20e2,'__argc\0');bad.rput(0x2084,0x20f0);bad.rput(0x2088,0);bad.rtext(0x20f2,'not_a_CRT_export\0');const good=moduleFixture(32,{dll:true,dependency:'ucrtbase.dll',entry:false});good.rtext(0x20e2,'__argc\0');const addresses=[];
 for(const fail of[false,true]){const t=await loaded(),{vm}=t;vm.supplyModule('crt-bad.dll',bad.bytes);vm.supplyModule('crt-good.dll',good.bytes);if(fail){ascii(t,0x22000n,'crt-bad.dll');frame(t,'LoadLibraryA',[0x22000n],'kernel32.dll');equal((await complete(t)).reason,Exit.Returned);equal(vm.register(0),0n);equal(vm.moduleBase('crt-bad.dll'),0n);}
  ascii(t,0x22000n,'crt-good.dll');frame(t,'LoadLibraryA',[0x22000n],'kernel32.dll');equal((await complete(t)).reason,Exit.Returned);const base=vm.register(0),p=vm.readInt(base+0x20a0n);addresses.push(p);equal(vm.readInt(p),1n);equal(lookup(t,'__argc','ucrtbase.dll'),p);equal(crt(t,'__p___argc',[],'ucrtbase.dll'),p);vm.dispose();}
 equal(addresses[0],addresses[1]);
}
// Control updates x87 and SSE consistently; a bad output commits neither unit.
{
 const t=await machine(32),{vm}=t;equal(crt(t,'_controlfp_s',[0x22000n,0x10300n,0x30301n]),0n);let f=fp(t);equal((f.cw>>8)&3,2);equal((f.cw>>10)&3,3);equal(f.cw&32,0);equal((vm.mxcsr>>13)&3,3);equal(vm.mxcsr&4096,0);equal(vm.readInt(0x22000n)&0x30301n,0x10300n);
 equal(crt(t,'_controlfp_s',[0x22000n,0x2000000n,0x3000000n]),0n);equal(vm.mxcsr&0x8040,0x40);equal(vm.readInt(0x22000n)&0x3000000n,0x2000000n);const saved=fp(t),mxcsr=vm.mxcsr;equal(crt(t,'_controlfp_s',[0x22000n,0x30000n,0x30000n]),22n);equal(fp(t),saved);equal(vm.mxcsr,mxcsr);
 frame(t,'_controlfp_s',[0x24ffen,0n,0x30300n],'msvcrt.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal((fp(t).cw>>8)&3,3);equal((fp(t).cw>>10)&3,0);vm.dispose();
}
// Initializers reload later entries after callbacks and _e stops at its first failure.
for(const checked of[false,true])for(const budget of[1n,1000n]){
 const t=await machine(32),{vm}=t,sleep=vm.bind('kernel32.dll','Sleep');vm.write(0x22000n,[...encode(0x30000n,4),...encode(0n,4),...encode(0x30200n,4)]);vm.write(0x23000n,new Uint8Array(16));
 vm.write(0x30000n,[0xff,0x05,...encode(0x23000n,4),0xc7,0x05,...encode(0x22008n,4),...encode(0x30100n,4),0x6a,0,0xb8,...encode(sleep,4),0xff,0xd0,0xb8,7,0,0,0,0xc3]);
 vm.write(0x30100n,[0xff,0x05,...encode(0x23004n,4),0x31,0xc0,0xc3]);vm.write(0x30200n,[0xff,0x05,...encode(0x23008n,4),0x31,0xc0,0xc3]);equal(await callback(t,checked?'_initterm_e':'_initterm',[0x22000n,0x2200cn],budget),checked?7n:0n);equal([0n,4n,8n].map(o=>vm.readInt(0x23000n+o)),[1n,checked?0n:1n,0n]);vm.dispose();
}
// A later initializer read fault never replays an initializer that already returned.
{
 const t=await machine(32),{vm}=t;vm.write(0x24ffcn,[...encode(0x30000n,4),...encode(0x30100n,4)]);vm.write(0x30000n,[0xff,0x05,...encode(0x23000n,4),0x31,0xc0,0xc3]);vm.write(0x30100n,[0xff,0x05,...encode(0x23004n,4),0x31,0xc0,0xc3]);equal(core.wire_protect(t.handle,0x25000n,4096n,1),0);frame(t,'_initterm',[0x24ffcn,0x25004n],'msvcrt.dll');equal((await complete(t)).reason,Exit.Fault);equal(vm.readInt(0x23000n),1n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal((await complete(t)).reason,Exit.Returned);equal(vm.readInt(0x23000n),1n);equal(vm.readInt(0x23004n),1n);vm.dispose();
}
// Comparators use ordinary guest calls; record payloads and caller state survive sorting.
for(const budget of[1n,1000n]){
 const t=await machine(32),{vm}=t,records=[[7,90],[-3,11],[7,55],[0,73],[42,8],[-3,6]];vm.write(0x22000n,records.flatMap(r=>r.flatMap(n=>Array.from(encode(BigInt.asUintN(32,BigInt(n)),4)))));vm.write(0x30000n,[0x8b,0x44,0x24,4,0x8b,0,0x8b,0x54,0x24,8,0x2b,2,0xdb,0xe3,0x66,0x0f,0xef,0xc0,0xc3]);vm.flags=0xad7n;vm.xmm(0,new Uint8Array(16).fill(91));const f=fp(t),x=vm.xmm(0);equal(await callback(t,'qsort',[0x22000n,6n,8n,0x30000n],budget),0n);const result=records.map((_,i)=>[Number(BigInt.asIntN(32,vm.readInt(0x22000n+BigInt(i*8)))),Number(vm.readInt(0x22004n+BigInt(i*8)))]);equal(result.map(r=>r[0]),records.map(r=>r[0]).sort((a,b)=>a-b));equal(result.toSorted((a,b)=>a[1]-b[1]),records.toSorted((a,b)=>a[1]-b[1]));equal(fp(t),f);equal(vm.xmm(0),x);equal(vm.flags,0xad7n);vm.dispose();
}
// A fault after a completed heap swap retains both that swap and its comparator result.
{
 const t=await machine(32),{vm}=t,vp=vm.bind('kernel32.dll','VirtualProtect');for(const[i,key]of[1n,3n,2n].entries())vm.write(0x24000n+BigInt(i*4096),encode(key,4));
 const code=[0xff,0x05,...encode(0x23000n,4),0x8b,0x44,0x24,4,0x8b,0x18,0x8b,0x54,0x24,8,0x2b,0x1a,0x83,0x3d,...encode(0x23000n,4),2,0x75,0];const jump=code.length-1,body=[0x68,...encode(0x23004n,4),0x6a,2,0x68,...encode(4096n,4),0x68,...encode(0x26000n,4),0xb8,...encode(vp,4),0xff,0xd0];code[jump]=body.length;vm.write(0x30000n,[...code,...body,0x89,0xd8,0xc3]);frame(t,'qsort',[0x24000n,3n,4096n,0x30000n],'msvcrt.dll');equal((await complete(t)).reason,Exit.Fault);equal(vm.readInt(0x23000n),2n);equal([0n,4096n,8192n].map(o=>vm.readInt(0x24000n+o)),[3n,1n,2n]);vm.register(0,0x7fffffffn);equal(core.wire_protect(t.handle,0x26000n,4096n,4),0);equal((await complete(t)).reason,Exit.Returned);equal([0n,4096n,8192n].map(o=>vm.readInt(0x24000n+o)),[1n,2n,3n]);equal(vm.readInt(0x23000n),3n);vm.dispose();
}
// Initializers may run inside a DLL loader notification and keep the original cdecl caller.
{
 const d=moduleFixture(32,{dll:true,dependency:'msvcrt.dll'});d.rtext(0x20e2,'_initterm\0');d.bytes.fill(0x90,d.offset(0x1000),d.offset(0x1080));const code=[0x68,...encode(d.preferred+0x3104n,4),0x68,...encode(d.preferred+0x3100n,4),0xff,0x15,...encode(d.preferred+0x20a0n,4),0x83,0xc4,8,0xb8,1,0,0,0,0xc2,12,0];d.bytes.set(code,d.offset(0x1000));d.bytes.set([0xff,0x05,...encode(0x23000n,4),0xc3],d.offset(0x1090));d.rput(0x3100,d.preferred+0x1090n);d.relocations.splice(0,d.relocations.length,0x1001,0x1006,0x100c,0x3100);d.rebuildRelocations();const vm=new WasmMachine(core,{mode:32});vm.map(0x23000n,4096n,3);vm.supplyModule('init.dll',d.bytes);vm.load(moduleFixture(32,{dependency:'init.dll'}).bytes);for(let n=0;n<1000;n++){const r=await vm.run(1n);if(r.reason===Exit.Returned)break;equal(r.reason,Exit.Budget);assert.ok(n<999);}equal(vm.readInt(0x23000n),1n);vm.dispose();
}
// _cexit drains only this provider's normal callbacks, including new registrations, then returns.
{
 const t=await machine(32),{vm}=t,reg=vm.bind('msvcrt.dll','_crt_atexit');stamp(t,0x30000n,1);stamp(t,0x30100n,2,[0x68,...encode(0x30200n,4),0xb8,...encode(reg,4),0xff,0xd0,0x83,0xc4,4]);stamp(t,0x30200n,4);stamp(t,0x30300n,7);crt(t,'_crt_atexit',[0x30000n]);crt(t,'_crt_atexit',[0x30100n]);crt(t,'_crt_atexit',[0x30300n],'ucrtbase.dll');equal(await callback(t,'_cexit'),0n);equal(vm.readInt(0x23000n),0x241n);equal(vm.processExitCode,null);equal(await callback(t,'_cexit'),0n);equal(vm.readInt(0x23000n),0x241n);equal(await callback(t,'_cexit',[],1n,'ucrtbase.dll'),0n);equal(vm.readInt(0x23000n),0x2417n);vm.dispose();
}
for(const[name,expected]of[['exit',1n],['_exit',0n],['quick_exit',7n]]){
 const t=await machine(32),{vm}=t;crt(t,'_crt_atexit',[stamp(t,0x30000n,1)]);crt(t,'_crt_at_quick_exit',[stamp(t,0x30100n,7)]);frame(t,name,[89n],'msvcrt.dll');const r=await complete(t);equal(r.reason,Exit.ProcessExited);equal(r.exitCode,89);equal(vm.readInt(0x23000n),expected);equal((await vm.run(10n)).reason,Exit.ProcessExited);vm.dispose();
}
// Nonreturning exits require arguments without reading an inaccessible return slot.
for(const name of['exit','_exit','quick_exit']){
 const t=await machine(32),{vm}=t;vm.ip=vm.bind('msvcrt.dll',name);vm.register(4,0x24ffcn);vm.write(0x25000n,encode(17n,4));equal(core.wire_protect(t.handle,0x24000n,4096n,1),0);equal((await complete(t)).reason,Exit.ProcessExited);equal(vm.processExitCode,17);vm.dispose();
}
// Exit handler entry faults preserve the uncalled handler and already-completed LIFO progress.
for(const afterReturn of[false,true]){
 const t=await machine(32),{vm}=t;t.sp=0x25000n;const vp=vm.bind('kernel32.dll','VirtualProtect');stamp(t,0x30000n,1);stamp(t,0x30100n,2,afterReturn?[0x68,...encode(0x23200n,4),0x6a,2,0x68,...encode(4096n,4),0x68,...encode(0x24000n,4),0xb8,...encode(vp,4),0xff,0xd0]:[]);crt(t,'_crt_atexit',[0x30000n]);crt(t,'_crt_atexit',[0x30100n]);frame(t,'_cexit',[],'msvcrt.dll');if(!afterReturn)equal(core.wire_protect(t.handle,0x24000n,4096n,2),0);const before=snapshot(t);equal((await complete(t)).reason,Exit.Fault);equal(vm.readInt(0x23000n),afterReturn?2n:0n);if(!afterReturn)equal(snapshot(t),before);equal(core.wire_protect(t.handle,0x24000n,4096n,4),0);equal((await complete(t)).reason,Exit.Returned);equal(vm.readInt(0x23000n),0x21n);equal(vm.processExitCode,null);vm.dispose();
}
// Static rejection retires raw CRT cells and lazy vectors through the same data-page owner.
{
 const bad=moduleFixture(32,{dll:true,dependency:'msvcrt.dll',attach:false});bad.rtext(0x20e2,'__argc\0');bad.bytes.fill(0x90,bad.offset(0x1000),bad.offset(0x1080));bad.bytes.set([0x31,0xc0,0xc2,12,0],bad.offset(0x1000));const kept=bad.relocations.filter(r=>r>=0x1080);bad.relocations.splice(0,bad.relocations.length,...kept);bad.rebuildRelocations();const vm=new WasmMachine(core,{mode:32});vm.supplyModule('bad.dll',bad.bytes);const image=vm.load(moduleFixture(32,{dependency:'bad.dll'}).bytes),t={vm,mode:32,image,sp:image.sp-2048n},ip=vm.ip,regs=Array.from({length:8},(_,i)=>vm.register(i)),p=vm.readInt(vm.moduleBase('bad.dll')+0x20a0n),cell=crt(t,'__p___argv'),vector=vm.readInt(cell);vm.ip=ip;regs.forEach((r,i)=>vm.register(i,r));await assert.rejects(vm.run(10000n),/attach returned FALSE/);for(const at of[p,cell,vector])assert.throws(()=>vm.readInt(at),/memory access/);vm.dispose();
}
// Cooperative CRT locks retain recursion and wake an actual blocked guest thread.
{
 const t=await machine(32),{vm}=t,lock=vm.bind('msvcrt.dll','_lock'),unlock=vm.bind('msvcrt.dll','_unlock');crt(t,'_lock',[7n]);crt(t,'_lock',[7n]);vm.write(0x30000n,[0x6a,7,0xb8,...encode(lock,4),0xff,0xd0,0x83,0xc4,4,0xff,0x05,...encode(0x23000n,4),0x6a,7,0xb8,...encode(unlock,4),0xff,0xd0,0x83,0xc4,4,0xc2,4,0]);const h=kernel(t,'CreateThread',[0n,65536n,0x30000n,0n,0n,0n]);frame(t,'Sleep',[0n],'kernel32.dll');equal((await complete(t)).reason,Exit.Returned);equal(vm.readInt(0x23000n),0n);crt(t,'_unlock',[7n]);frame(t,'Sleep',[0n],'kernel32.dll');equal((await complete(t)).reason,Exit.Returned);equal(vm.readInt(0x23000n),0n);crt(t,'_unlock',[7n]);frame(t,'WaitForSingleObject',[h,0xffffffffn],'kernel32.dll');equal((await complete(t)).reason,Exit.Returned);equal(vm.readInt(0x23000n),1n);equal(kernel(t,'CloseHandle',[h]),1n);frame(t,'_unlock',[7n],'msvcrt.dll');assert.throws(()=>vm.service(),/not owned/);vm.dispose();
}
// Unsupported streams and callback protocols report actual failure instead of old fixed success.
{
 const t=await machine(32),{vm}=t;equal(crt(t,'__acrt_iob_func',[0n]),0n);equal(vm.readInt(crt(t,'_errno')),9n);equal(crt(t,'setvbuf',[0n,0n,0n,0n]),0xffffffffn);equal(crt(t,'_register_onexit_function',[0x22000n,0x30000n]),0xffffffffn);equal(vm.readInt(crt(t,'_errno')),38n);equal(crt(t,'_initialize_onexit_table',[0x22000n]),0n);equal([0n,4n,8n].map(o=>vm.readInt(0x22000n+o)),[0n,0n,0n]);for(const name of['_set_invalid_parameter_handler','__setusermatherr','_register_thread_local_exe_atexit_callback']){equal(crt(t,name,[0n]),0n);frame(t,name,[0x30000n],'msvcrt.dll');const before=snapshot(t);assert.throws(()=>vm.service(),/not implemented/);equal(snapshot(t),before);}vm.dispose();
}
console.log(`${checks} CRT data/startup/control/callback/sort/exit/lock checks passed`);
