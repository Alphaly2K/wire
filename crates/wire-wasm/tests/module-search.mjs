import assert from 'node:assert/strict';
import {WasmMachine,Exit} from '../web/runtime.mjs';
import {core,stateLayout,encode} from './x87-harness.mjs';
import {moduleFixture} from './module-fixture.mjs';
import {dynamicFixture,trace} from './dynamic-fixture.mjs';
import {delayFixture} from './delay-fixture.mjs';

let checks=0;const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
const narrow=value=>new TextEncoder().encode(value+'\0');
const wide=value=>Uint8Array.from([...value.split('').flatMap(c=>[c.charCodeAt(0)&255,c.charCodeAt(0)>>8]),0,0]);
const read=(vm,at,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(at+BigInt(i),1)));
const snapshot=t=>new Uint8Array(core.memory.buffer,core.wire_state(t.handle),stateLayout[0]).slice();
const plain=mode=>{const f=delayFixture(mode);f.put(f.dirs+13*8,0);f.put(f.dirs+13*8+4,0);return f;};
function create(mode){let handle;const vm=new WasmMachine({...core,wire_create(...a){handle=core.wire_create(...a);return handle;}},{mode,clock(){throw new Error('module searches must not sample time');}});vm.map(0x20000n,0x4000n,7);return {vm,mode,get handle(){return handle;}};}
async function machine(mode,files={},main=plain(mode),command='"C:\\app\\program.exe"'){
 const t=create(mode);for(const [path,file] of Object.entries(files))t.vm.supplyFile(path,file.bytes??file);t.image=t.vm.load(main.bytes,{command});t.sp=t.image.sp-256n;t.error=t.image.teb+BigInt(mode===32?0x34:0x68);equal((await t.vm.run(1000n)).reason,Exit.Returned);return t;
}
function frame(t,name,args=[]){const {vm,sp,mode}=t;vm.ip=vm.bind('kernel32.dll',name);vm.register(4,sp);vm.write(sp,encode(vm.entryPoints.returned,mode/8));for(let n=0;n<args.length;n++)if(mode===32)vm.write(sp+4n+BigInt(n*4),encode(args[n],4));else vm.register([1,2,8,9][n],args[n]);}
async function finish(t,budget=1000n){for(let n=0;n<10000;n++){const result=await t.vm.run(budget);if(result.reason===Exit.Returned)return;equal(result.reason,Exit.Budget);assert.ok(n<9999);}}
async function request(t,name,args=[],budget=1000n){const work=t.vm.work,retired=t.vm.retired;frame(t,name,args);await finish(t,budget);equal(t.vm.work-work,t.vm.retired-retired+1n);return t.vm.register(0);}
async function pathRequest(t,api,path,budget=1000n,flags=0n){if(path!==null)t.vm.write(0x20000n,api.endsWith('W')?wide(path):narrow(path));return request(t,api,[path===null?0n:0x20000n,0n,flags],budget);}
const leaf=(mode,seed,id=1)=>dynamicFixture(mode,{id,seed,tls:true});

for(const mode of [32,64]){
 const t=await machine(mode,{'C:\\app\\leaf.dll':leaf(mode,11),'C:\\Windows\\System32\\leaf.dll':leaf(mode,22),'C:\\leaf.dll':leaf(mode,33)},moduleFixture(mode,{dependency:'leaf.dll'})),{vm}=t;
 equal(vm.register(0),11n,'static dependencies prefer the executable directory');const base=vm.moduleBase('c:/APP/leaf');assert.ok(base);equal(vm.moduleBase('C:\\leaf.dll'),0n);equal(vm.moduleBase('C:\\app\\.\\leaf.dll'),base);equal(await pathRequest(t,'GetModuleHandleW','C:/app/leaf'),base);equal(await request(t,'GetModuleFileNameW',[base,0x20200n,260n]),15n);equal(read(vm,0x20200n,32),wide('C:\\app\\leaf.dll'));
 equal(await pathRequest(t,'LoadLibraryW','KERNEL32'),await pathRequest(t,'GetModuleHandleW','kernel32'));equal(vm.moduleBase('kernel32.dll'),0n);vm.dispose();
}

for(const mode of [32,64])for(const budget of [1n,1000n]){
 const t=await machine(mode,{'C:\\lib\\choice.dll':leaf(mode,55),'C:\\cwd\\choice.dll':leaf(mode,66),'C:\\cwd\\only.dll':leaf(mode,67),'C:\\Windows\\System32\\choice.dll':leaf(mode,77)}),{vm}=t,out=0x20200n;
 equal(await request(t,'GetDllDirectoryW',[0n,0xdeadbeefn],budget),0n);equal(await pathRequest(t,'SetCurrentDirectoryA','C:\\cwd',budget),1n);equal(await request(t,'GetCurrentDirectoryW',[0n,0xdeadbeefn],budget),7n);
 equal(await request(t,'GetCurrentDirectoryA',[7n,out],budget),6n);equal(read(vm,out,7),narrow('C:\\cwd'));equal(await pathRequest(t,'SetDllDirectoryW','C:/資料/../lib',budget),1n);equal(await request(t,'GetDllDirectoryA',[0n,0xdeadbeefn],budget),7n);vm.write(out,new Uint8Array(16).fill(0xa5));equal(await request(t,'GetDllDirectoryW',[6n,out],budget),7n);equal(read(vm,out,16),new Uint8Array(16).fill(0xa5));equal(vm.readInt(t.error),122n);equal(await request(t,'GetDllDirectoryW',[7n,out],budget),6n);equal(read(vm,out,14),wide('C:\\lib'));
 const choice=await pathRequest(t,'LoadLibraryW','choice',budget);equal(vm.readInt(choice+0x3000n),55n,'SetDllDirectory precedes system directories');equal(await request(t,'FreeLibrary',[choice],budget),1n);
 equal(await pathRequest(t,'SetDllDirectoryA','',budget),1n);equal(await pathRequest(t,'LoadLibraryA','only.dll',budget),0n);equal(vm.readInt(t.error),126n,'an empty DLL directory suppresses CWD lookup');equal(await pathRequest(t,'SetDllDirectoryW',null,budget),1n);const current=await pathRequest(t,'LoadLibraryA','only.dll',budget);equal(vm.readInt(current+0x3000n),67n);equal(await request(t,'FreeLibrary',[current],budget),1n);
 equal(await pathRequest(t,'SetDllDirectoryW','C:\\not-present',budget),1n,'a directory setting does not invent or require file contents');const system=await pathRequest(t,'LoadLibraryA','choice.dll',budget);equal(vm.readInt(system+0x3000n),77n,'missing candidates continue to the next search directory');equal(await request(t,'FreeLibrary',[system],budget),1n);
 equal(await pathRequest(t,'SetCurrentDirectoryW','C:\\absent\\child',budget),0n);equal(vm.readInt(t.error),3n);equal(await request(t,'GetCurrentDirectoryW',[7n,out],budget),6n);equal(read(vm,out,14),wide('C:\\cwd'));vm.dispose();
}

// Explicit paths retain distinct identities even when two DLLs have the same basename.
for(const mode of [32,64]){
 const t=await machine(mode,{'C:\\one\\leaf.dll':leaf(mode,11),'C:\\two\\leaf.dll':leaf(mode,22,2)}),{vm}=t,a=await pathRequest(t,'LoadLibraryA','C:\\one\\leaf.dll',1n),b=await pathRequest(t,'LoadLibraryW','C:/two/leaf',1n);assert.notEqual(a,b);equal([vm.readInt(a+0x3000n),vm.readInt(b+0x3000n)],[11n,22n]);equal(await pathRequest(t,'GetModuleHandleW','c:\\two\\LEAF.dll'),b);equal(await pathRequest(t,'LoadLibraryW','C:/one/../two/LEAF'),b);equal(await request(t,'FreeLibrary',[b]),1n);equal(vm.moduleBase('C:\\two\\leaf.dll'),b);equal(await request(t,'FreeLibrary',[b]),1n);equal(vm.moduleBase('C:\\two\\leaf.dll'),0n);equal(vm.moduleBase('C:\\one\\leaf.dll'),a);equal(await request(t,'FreeLibrary',[a]),1n);vm.dispose();
}

for(const mode of [32,64])for(const [flags,value] of [[0n,14n],[8n,25n],[0x100n,25n],[0x200n,14n],[0x400n,47n],[0x800n,36n],[0x1000n,14n],[0x1100n,25n]]){
 const t=await machine(mode,{'C:\\plugins\\bridge.dll':dynamicFixture(mode,{id:2,dependency:'leaf.dll',delta:3}),'C:\\plugins\\leaf.dll':leaf(mode,22),'C:\\app\\leaf.dll':leaf(mode,11),'C:\\user\\leaf.dll':leaf(mode,44),'C:\\Windows\\System32\\leaf.dll':leaf(mode,33)}),{vm}=t;equal(await pathRequest(t,'SetDllDirectoryW','C:\\user'),1n);const base=await pathRequest(t,'LoadLibraryExW','C:\\plugins\\bridge.dll',1n,flags);assert.ok(base);equal(vm.readInt(base+0x3000n),value,'dependency search follows the selected LoadLibraryEx policy');equal(await request(t,'FreeLibrary',[base],1n),1n);equal(vm.moduleBase('leaf.dll'),0n);vm.dispose();
}
for(const mode of [32,64]){
 const t=await machine(mode,{'C:\\plugins\\bridge.dll':dynamicFixture(mode)}),{vm}=t;
 for(const [path,flags,error] of [['bridge.dll',8n,87n],['bridge.dll',0x100n,87n],['C:\\plugins\\bridge.dll',0x108n,87n],['bridge.dll',1n,50n],['D:\\bridge.dll',0n,15n],['..\\bridge.dll',0n,5n],['\\\\server\\bridge.dll',0n,5n]]){equal(await pathRequest(t,'LoadLibraryExA',path,1n,flags),0n);equal(vm.readInt(t.error),error);equal(vm.moduleBase('bridge.dll'),0n);}
 vm.dispose();
}

// Forwarders and delay calls consult the same current search configuration.
for(const mode of [32,64]){
 const t=await machine(mode,{'C:\\forward\\forward.dll':dynamicFixture(mode,{id:2,forward:'leaf.#17'}),'C:\\lib\\leaf.dll':leaf(mode,73),'C:\\leaf.dll':leaf(mode,99,3)},delayFixture(mode)),{vm,image}=t;equal(vm.moduleBase('leaf.dll'),0n);equal(await pathRequest(t,'SetDllDirectoryA','C:\\lib'),1n);const forward=await pathRequest(t,'LoadLibraryA','C:\\forward\\forward.dll');vm.write(0x20040n,narrow('GetValue'));const address=await request(t,'GetProcAddress',[forward,0x20040n],1n),base=vm.moduleBase('C:\\lib\\leaf.dll');equal(address,base+0x1080n);equal(vm.readInt(base+0x3000n),73n);vm.ip=vm.readInt(image.base+0x3120n,mode/8);vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,mode/8));await finish(t,1n);equal(vm.register(0),73n);equal(await request(t,'FreeLibrary',[forward]),1n);equal(vm.moduleBase('C:\\lib\\leaf.dll'),base);equal(await request(t,'FreeLibrary',[base]),1n);equal(vm.moduleBase('leaf.dll'),0n);vm.dispose();
}

// Directory output and error faults preserve the saved call and configuration.
for(const mode of [32,64]){
 const t=await machine(mode,{'C:\\資料\\marker':[]}),{vm}=t;equal(await pathRequest(t,'SetDllDirectoryW','C:\\資料'),1n);const out=0x21ffdn;vm.write(out,new Uint8Array(32).fill(0xa5));frame(t,'GetDllDirectoryW',[100n,out]);equal(core.wire_protect(t.handle,0x22000n,4096n,2),0);const before=snapshot(t),last=vm.readInt(t.error);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(read(vm,out,3),new Uint8Array(3).fill(0xa5));equal(vm.readInt(t.error),last);equal(core.wire_protect(t.handle,0x22000n,4096n,4),0);await finish(t);equal(read(vm,out,12),wide('C:\\資料'));
 vm.write(0x20000n,wide('D:\\invalid'));frame(t,'SetDllDirectoryW',[0x20000n]);equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);const saved=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),saved);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(await request(t,'GetDllDirectoryW',[100n,0x20200n]),5n);equal(read(vm,0x20200n,12),wide('C:\\資料'));
 frame(t,'GetDllDirectoryW',[1n,0xdeadbeefn]);equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);const small=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),small);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);await finish(t);equal(vm.register(0),6n);equal(vm.readInt(t.error),122n);
 frame(t,'GetCurrentDirectoryA',[32n,t.sp]);await finish(t);equal(vm.ip,vm.entryPoints.returned);equal(read(vm,t.sp,4),narrow('C:\\'));vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode,{'C:\\private\\kernel32.dll':moduleFixture(mode,{dll:true})}),{vm}=t;equal(await pathRequest(t,'LoadLibraryA','C:\\private\\kernel32.dll'),0n);equal(vm.readInt(t.error),126n);equal(await pathRequest(t,'GetModuleHandleW','D:\\missing.dll'),0n);equal(vm.readInt(t.error),126n);vm.dispose();
 const stat=create(mode);stat.vm.supplyFile('C:\\private\\kernel32.dll',moduleFixture(mode,{dll:true}).bytes);assert.throws(()=>stat.vm.load(moduleFixture(mode,{dependency:'C:\\private\\kernel32.dll'}).bytes),/canonical system path/);equal(stat.vm.image,null);stat.vm.dispose();
}

for(const mode of [32,64]){
 const t=create(mode),{vm}=t;vm.supplyFile('C:\\lib\\leaf.dll',leaf(mode,1).bytes);assert.throws(()=>vm.supplyFile('c:/LIB/leaf.dll',[]),/duplicate/);for(const path of ['D:\\bad','..\\bad','\\\\server\\bad'])assert.throws(()=>vm.supplyFile(path,[]),Error);
 const before=snapshot(t);assert.throws(()=>vm.load(moduleFixture(mode,{dependency:'absent.dll'}).bytes,{command:'"C:\\failed\\program.exe"'}),/not supplied/);equal(snapshot(t),before);equal(vm.image,null);vm.supplyFile('C:\\failed\\program.exe',plain(mode).bytes);equal(vm.load(plain(mode).bytes,{command:'"C:\\failed\\program.exe"'}).base,plain(mode).preferred);equal((await vm.run(1000n)).reason,Exit.Returned);vm.dispose();
}

console.log(`${checks} VFS module search/directory/identity checks passed`);
