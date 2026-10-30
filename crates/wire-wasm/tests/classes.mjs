import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,finish} from './gdi-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';
import {moduleFixture} from './module-fixture.mjs';

const user=['RegisterClassA','RegisterClassW','RegisterClassExA','RegisterClassExW','UnregisterClassA','UnregisterClassW','GetClassInfoA','GetClassInfoW','GetClassInfoExA','GetClassInfoExW','GetClassLongA','GetClassLongW','SetClassLongA','SetClassLongW','GetClassWord','GetClassNameA','GetClassNameW'];
const controls=['InitCommonControls','InitCommonControlsEx'];
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const encoded=(s,wide=false)=>wide?Buffer.from(s+'\0','utf16le'):Buffer.from([...s+'\0'].flatMap(c=>({'日':[0x93,0xfa],'本':[0x96,0x7b]})[c]??[c.charCodeAt(0)]));
const text=(vm,p,s,wide=false)=>vm.write(p,encoded(s,wide));
const contents=(vm,p,s,wide=false)=>equal(bytes(vm,p,encoded(s,wide).length),new Uint8Array(encoded(s,wide)));
const init=(t,name,args=[])=>call(t,name,args,'comctl32.dll');
const invoke=async(t,name,args)=>{frame(t,name,args);return finish(t,1n);};
function record(t,{wide=true,ex=true,name='Meta日本',menu=42n,ce=4n,we=8n,style=0n,proc}={}){
 const {vm,image}=t,fields=[style,proc??vm.bind('user32.dll',wide?'DefWindowProcW':'DefWindowProcA'),ce,we,image.base,0x12345678n,0x23456789n,0n,menu,0x23000n,ex?0x3456789an:0n];
 vm.write(0x22000n,new Uint8Array(48));text(vm,0x23000n,name,wide);if(ex)vm.write(0x22000n,encode(48n,4));
 for(let n=0;n<(ex?11:10);n++)vm.write(0x22000n+BigInt((n+Number(ex))*4),encode(fields[n],4));
 return{fields,name,wide,ex,register:()=>call(t,`RegisterClass${ex?'Ex':''}${wide?'W':'A'}`,[0x22000n])};
}
const create=(t,atom,{wide=true,instance=t.image.base,title=0x24100n}={})=>invoke(t,wide?'CreateWindowExW':'CreateWindowExA',[0n,atom,title,0n,10n,20n,32n,24n,0n,0n,instance,0n]);

// All legacy PE32 exports bind, while absent AMD64 metadata/controls stay absent.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 for(const name of [...user,...controls]){
  const provider=controls.includes(name)?'comctl32.dll':'user32.dll';
  if(mode===64 && !user.slice(0,6).includes(name))assert.throws(()=>vm.bind(provider,name),/migrated|unknown/);
  else assert.ok(vm.bind(provider,name));
  for(const wrong of ['gdi32.dll',controls.includes(name)?'user32.dll':'comctl32.dll'])assert.throws(()=>vm.bind(wrong,name),/migrated|unknown/);
 }vm.dispose();
}

// Class info owns persistent A/W names and complete legacy fields, including menus and extra counts.
for(const wide of [false,true])for(const ex of [false,true]){
 const t=await machine(32),{vm}=t;text(vm,0x24000n,'Menu日本',wide);
 const r=record(t,{wide,ex,menu:0x24000n}),atom=r.register();equal(atom,0xc000n);vm.write(t.error,encode(77n,4));
 vm.write(0x23000n,new Uint8Array(64));vm.write(0x24000n,new Uint8Array(64));
 for(const queryWide of [false,true])for(const extended of [false,true]){
  vm.write(0x25000n,new Uint8Array(64).fill(0xa5));equal(call(t,`GetClassInfo${extended?'Ex':''}${queryWide?'W':'A'}`,[t.image.base,atom,0x25000n]),1n);
  const at=0x25000n+BigInt(extended?4:0),count=extended?11:10;
  if(extended)equal(vm.readInt(0x25000n),48n);
  for(let n=0;n<count;n++)if(![8,9].includes(n))equal(vm.readInt(at+BigInt(n*4)),r.fields[n]);
  contents(vm,vm.readInt(at+32n),'Menu日本',queryWide);contents(vm,vm.readInt(at+36n),r.name,queryWide);equal(vm.readInt(at+BigInt(count*4)),0xa5a5a5a5n);
 }
 equal(call(t,'GetClassInfoExW',[t.image.base,atom,t.sp]),1n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp),48n);
 text(vm,0x24100n,'Title',wide);const hwnd=await create(t,atom,{wide});assert.ok(hwnd);
 for(const queryWide of [false,true]){equal(call(t,queryWide?'GetClassNameW':'GetClassNameA',[hwnd,0x25000n,64n]),BigInt(queryWide?r.name.length:encoded(r.name).length-1));contents(vm,0x25000n,r.name,queryWide);}
 equal(call(t,'GetClassLongW',[hwnd,BigInt.asUintN(32,-20n)]),4n);equal(call(t,'GetClassLongA',[hwnd,BigInt.asUintN(32,-8n)]),0x24000n);
 equal(call(t,'SetClassLongW',[hwnd,BigInt.asUintN(32,-14n),0xfedcba98n]),0x12345678n);equal(call(t,'GetClassWord',[hwnd,BigInt.asUintN(32,-14n)]),0xfedcba98n);
 for(const[name,args,error]of [['SetClassLongA',[hwnd,BigInt.asUintN(32,-20n),4097n],87n],['SetClassLongW',[hwnd,BigInt.asUintN(32,-8n),1n],120n],['GetClassLongA',[hwnd,0n],1413n],['GetClassWord',[0n,BigInt.asUintN(32,-32n)],1400n]]){equal(call(t,name,args),0n);equal(vm.readInt(t.error),error);}
 vm.write(0x25000n,new Uint8Array(16).fill(0xa5));equal(call(t,'GetClassNameW',[hwnd,0x25000n,1n]),0n);equal(vm.readInt(0x25000n),0xa5a50000n);
 for(const[p,cap]of [[0n,8n],[0x60000n,0n],[0x60000n,0xffffffffn]])equal(call(t,'GetClassNameA',[hwnd,p,cap]),0n);
 equal(call(t,wide?'UnregisterClassW':'UnregisterClassA',[atom,t.image.base]),0n);equal(vm.readInt(t.error),1412n);equal(await invoke(t,'DestroyWindow',[hwnd]),1n);equal(call(t,wide?'UnregisterClassW':'UnregisterClassA',[atom,t.image.base]),1n);equal(call(t,'GetClassInfoW',[t.image.base,atom,0x25000n]),0n);equal(vm.readInt(t.error),1411n);
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Wide class/menu names retain the legacy replacement of unpaired UTF-16 units.
{
 const t=await machine(32),{vm}=t;text(vm,0x24000n,'\udc00Y',true);const r=record(t,{name:'\ud800X',menu:0x24000n,ce:0n}),atom=r.register();assert.ok(atom);
 equal(call(t,'GetClassInfoExW',[t.image.base,0x23000n,0x25000n]),1n);contents(vm,vm.readInt(0x25028n),'\ufffdX',true);contents(vm,vm.readInt(0x25024n),'\ufffdY',true);
 text(vm,0x24100n,'Title',true);const hwnd=await create(t,0x23000n);assert.ok(hwnd);equal(call(t,'GetClassNameW',[hwnd,0x25000n,8n]),2n);contents(vm,0x25000n,'\ufffdX',true);vm.dispose();
}

// Field changes affect future windows and lookup identity; existing windows keep their procedure/extra storage.
{
 const t=await machine(32),{vm}=t,r=record(t,{menu:0n,ce:0n}),atom=r.register();text(vm,0x24100n,'Title',true);const first=await create(t,atom);assert.ok(first);
 const index=n=>BigInt.asUintN(32,BigInt(n));
 equal(call(t,'SetWindowLongW',[first,4n,77n]),0n);equal(call(t,'SetClassLongW',[first,index(-18),16n]),8n);
 equal(call(t,'SetClassLongW',[first,index(-24),vm.bind('user32.dll','DefWindowProcA')]),r.fields[1]);equal(call(t,'GetWindowLongW',[first,index(-4)]),r.fields[1]);
 equal(call(t,'SetClassLongW',[first,index(-16),0x12340000n]),t.image.base);equal(await create(t,atom),0n);equal(vm.readInt(t.error),1407n);
 equal(call(t,'SetClassLongA',[first,index(-26),0x4000n]),0n);const second=await create(t,atom,{instance:0x55550000n});assert.ok(second);equal(call(t,'SetWindowLongW',[second,12n,88n]),0n);equal(call(t,'GetWindowLongW',[second,12n]),88n);equal(call(t,'GetWindowLongW',[first,4n]),77n);equal(call(t,'SetWindowLongW',[first,12n,88n]),0n);equal(vm.readInt(t.error),1413n);
 equal(call(t,'GetClassLongW',[first,index(-32)]),atom);equal(call(t,'GetClassInfoExW',[0x99990000n,atom,0x25000n]),1n);equal(vm.readInt(0x25000n+20n),0x12340000n);vm.dispose();
}

// Basic and common classes use their real metadata and the existing JIT default procedure.
{
 const t=await machine(32),{vm}=t;vm.write(0x24ffcn,encode(7n,4));equal(init(t,'InitCommonControlsEx',[0x24ffcn]),0n);equal(vm.readInt(t.error),87n);const probe=record(t,{name:'Probe',menu:0n});equal(probe.register(),0xc000n);equal(call(t,'UnregisterClassW',[0xc000n,t.image.base]),1n);
 text(vm,0x23000n,'button',true);vm.write(t.error,encode(77n,4));equal(call(t,'GetClassInfoExW',[1n,0x23000n,0x25000n]),1n);
 equal(vm.readInt(0x25000n),48n);equal(vm.readInt(0x25004n),0x4000n);equal(vm.readInt(0x25010n),16n);const instance=vm.readInt(0x25014n),proc=vm.readInt(0x25008n);equal(proc,vm.bind('user32.dll','DefWindowProcW'));
 vm.write(0x24ffcn,encode(8n,4));equal(core.wire_protect(t.handle,0x25000n,4096n,1),0);equal(init(t,'InitCommonControlsEx',[0x24ffcn]),1n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);
 for(const [n,name]of ['Button','Edit','Static','ListBox','ComboBox','ScrollBar','SysListView32','SysTreeView32','SysTabControl32','msctls_progress32'].entries()){
  text(vm,0x23000n,name,true);equal(call(t,'GetClassInfoW',[0n,0x23000n,0x25000n]),1n);equal(vm.readInt(0x25000n),0x4000n);equal(vm.readInt(0x25004n),proc);if(n===0)equal(vm.readInt(0x25010n),instance);
 }
 equal(init(t,'InitCommonControls'),1n);text(vm,0x23000n,'Button');text(vm,0x24100n,'日本');const hwnd=await create(t,0x23000n,{wide:false});assert.ok(hwnd);equal(call(t,'IsWindowUnicode',[hwnd]),1n);equal(call(t,'GetClassNameW',[hwnd,0x25000n,64n]),6n);contents(vm,0x25000n,'Button',true);equal(await invoke(t,'GetWindowTextW',[hwnd,0x25000n,64n]),2n);contents(vm,0x25000n,'日本',true);equal(vm.readInt(t.error),77n);
 vm.dispose();
}

// Cross-encoding CREATESTRUCT strings are converted before WNDPROC sees them, including long titles.
for(const wide of [false,true]){
 const t=await machine(32),{vm}=t,r=record(t,{wide,menu:0n,ce:0n,name:'Cross日本',proc:0x30000n}),atom=r.register();assert.ok(atom);
 const def=vm.bind('user32.dll',wide?'DefWindowProcW':'DefWindowProcA');
 vm.write(0x30000n,[0x8b,0x44,0x24,16,0xa3,...encode(0x26000n,4),0xb8,...encode(def,4),0xff,0xe0]);text(vm,0x23000n,r.name,!wide);const title='日本'.repeat(300);text(vm,0x24100n,title,!wide);
 const hwnd=await create(t,0x23000n,{wide:!wide});assert.ok(hwnd);const createStruct=vm.readInt(0x26000n);contents(vm,vm.readInt(createStruct+36n),title,wide);contents(vm,vm.readInt(createStruct+40n),r.name,wide);equal(call(t,'IsWindowUnicode',[hwnd]),BigInt(wide));equal(call(t,'GetWindowLongW',[hwnd,BigInt.asUintN(32,-4n)]),0x30000n);vm.dispose();
}

// Menu/source, output, error and first-return faults cannot consume a class identity or modify fields.
{
 const t=await machine(32),{vm}=t,r=record(t,{wide:false,menu:0n,ce:0n,proc:0x30000n}),atom=r.register();assert.ok(atom);
 vm.write(0x30000n,[0xff,0x05,...encode(0x26000n,4),0xb8,...encode(vm.bind('user32.dll','DefWindowProcA'),4),0xff,0xe0]);text(vm,0x24100n,'日本'.repeat(300),true);
 t.sp=(t.image.sp&~4095n)+1024n;frame(t,'CreateWindowExW',[0n,atom,0x24100n,0n,10n,20n,32n,24n,0n,0n,t.image.base,0n]);const page=(t.sp&~4095n)-4096n,work=vm.work,retired=vm.retired;
 equal(core.wire_protect(t.handle,page,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.work,work);equal(vm.readInt(0x26000n),0n);
 equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);assert.ok(await finish(t,1n));equal(vm.readInt(0x26000n),2n);equal(vm.work-work,vm.retired-retired+3n);vm.dispose();
}

for(const kind of ['menu','output','error','return','builtin-output']){
 const t=await machine(32),{vm}=t;let name,args;
 if(kind==='menu'||kind==='return'){record(t,{menu:kind==='menu'?0x60000n:0n});name='RegisterClassExW';args=[0x22000n];}
 else if(kind==='builtin-output'){text(vm,0x23000n,'Button',true);name='GetClassInfoExW';args=[0n,0x23000n,0x24ffen];}
 else{const r=record(t,{menu:0n}),atom=r.register();text(vm,0x24100n,'Title',true);const hwnd=await create(t,atom);name=kind==='output'?'GetClassNameW':'SetClassLongW';args=kind==='output'?[hwnd,0x24ffen,64n]:[hwnd,0n,9n];}
 vm.write(0x24ffen,new Uint8Array(64).fill(0xa5));frame(t,name,args);const before=snapshot(t),work=vm.work,page=kind==='return'?t.sp&~4095n:kind==='error'?t.image.teb:0x25000n;
 if(kind!=='menu')equal(core.wire_protect(t.handle,page,4096n,kind==='return'?1:2),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.work,work);equal(vm.readInt(0x24ffen,2),0xa5a5n);
 if(kind==='menu'){vm.map(0x60000n,4096n,3);text(vm,0x60000n,'Menu',true);}else equal(core.wire_protect(t.handle,page,4096n,4),0);
 equal(vm.service().reason,Exit.Dispatch);equal(vm.work,work+1n);if(kind==='menu'||kind==='return')equal(vm.register(0),0xc000n);if(kind==='builtin-output'){equal(vm.register(0),1n);text(vm,0x23000n,'Next',true);const next=record(t,{name:'Next',menu:0n});equal(next.register(),0xc001n);}vm.dispose();
}

// A failed first builtin output cannot publish an identity before the caller recovers.
{
 const t=await machine(32),{vm}=t;text(vm,0x23000n,'Button',true);frame(t,'GetClassInfoExW',[0n,0x23000n,0x24ffen]);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);equal(vm.service().reason,Exit.Fault);
 const probe=record(t,{name:'Probe',menu:0n});equal(probe.register(),0xc000n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);text(vm,0x23000n,'Button',true);equal(call(t,'GetClassInfoExW',[0n,0x23000n,0x24ffen]),1n);
 equal(record(t,{name:'Next',menu:0n}).register(),0xc002n);vm.dispose();
}

// Rejected static startup releases the owned backing of implicit class names.
{
 const f=moduleFixture(32,{dll:true,dependency:'user32.dll'});f.rtext(0x20e2,'GetClassInfoExW\0');f.bytes.set(encoded('Button',true),f.offset(0x30a0));
 f.bytes.set([0x68,...encode(0x21000n,4),0x68,...encode(f.preferred+0x30a0n,4),0x6a,0,0xff,0x15,...encode(f.preferred+0x20a0n,4),0x31,0xc0,0xc2,12,0],f.offset(0x1000));
 f.relocations.splice(0,f.relocations.length,...f.relocations.filter(r=>r<0x1000||r>=0x1080),0x1006,0x100e);f.rebuildRelocations();
 const vm=new WasmMachine(core,{mode:32}),main=moduleFixture(32,{dependency:'class-start.dll'});vm.map(0x20000n,0x30000n,7);vm.supplyModule('class-start.dll',f.bytes);const image=vm.load(main.bytes);let rejected=false;
 for(let n=0;n<100;n++){try{equal((await vm.run(1n)).reason,Exit.Budget);}catch(error){assert.match(error.message,/attach returned FALSE/);rejected=true;break;}}
 assert.ok(rejected);equal(vm.readInt(0x21000n),48n);const pointer=vm.readInt(0x21028n);assert.ok(pointer);assert.throws(()=>vm.readInt(pointer),/memory access/);assert.throws(()=>vm.readInt(image.base),/memory access/);equal(vm.moduleBase('class-start.dll'),0n);vm.dispose();
}

// Every class/control export is imported together; generated guest code queries a basic class through the JIT.
{
 const f=fixture(32,{tls:false});f.bytes.fill(0,f.offset(0x2000),f.offset(0x2600));f.put(f.dirs+12,60);let at=0x2200;
 const groups=[['user32.dll',user,0x2080,0x2100],['comctl32.dll',controls,0x2180,0x21a0]];
 for(const[n,[provider,names,lookup,iat]]of groups.entries()){const descriptor=0x2000+n*20;f.rput(descriptor,lookup);f.rput(descriptor+12,at);f.rtext(at,provider+'\0');at+=provider.length+1;f.rput(descriptor+16,iat);for(const[i,name]of names.entries()){f.rput(lookup+i*4,at);f.rput(iat+i*4,at);f.rtext(at+2,name+'\0');at+=name.length+3;}}
 assert.ok(at<0x2600);f.bytes.set(encoded('Button',true),f.offset(0x3000));f.bytes.set([0x68,...encode(f.preferred+0x3100n,4),0x68,...encode(f.preferred+0x3000n,4),0x6a,0,0xff,0x15,...encode(f.preferred+0x2124n,4),0xc3],f.offset(0x1000));f.relocations.splice(0,f.relocations.length,0x1001,0x1006,0x100e);f.rebuildRelocations();
 const vm=new WasmMachine(core,{mode:32,clock(){throw new Error('unexpected clock');}}),image=vm.load(f.bytes);for(const[provider,names,,iat]of groups)for(const[n,name]of names.entries())equal(vm.readInt(image.base+BigInt(iat+n*4)),vm.bind(provider,name));equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),1n);equal(vm.readInt(image.base+0x3100n),48n);assert.ok(vm.retired>0n);vm.dispose();
}
console.log(`${checks} class metadata/controls/AW/ABI/JIT/lifetime/fault checks passed`);
