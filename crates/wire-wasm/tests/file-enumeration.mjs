import assert from 'node:assert/strict';
import {WasmMachine,Exit} from '../web/runtime.mjs';
import {core,stateLayout,encode} from './x87-harness.mjs';
import {delayFixture} from './delay-fixture.mjs';

let checks=0;
const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
const text=s=>new TextEncoder().encode(s+'\0');
const wide=s=>Uint8Array.from([...s.split('').flatMap(c=>[c.charCodeAt(0)&255,c.charCodeAt(0)>>8]),0,0]);
const read=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const snapshot=t=>new Uint8Array(core.memory.buffer,core.wire_state(t.handle),stateLayout[0]).slice();
async function machine(mode){
 let handle;
 const vm=new WasmMachine({...core,wire_create(...a){handle=core.wire_create(...a);return handle;}},{mode,clock(){throw new Error('unexpected elapsed clock');},wallClock(){throw new Error('unexpected wall clock');}});
 for(const [name,data] of [['A.TXT',[1,2,3]],['b.txt',[4]],['data.bin',[7,8]],['日本.txt',[5,6]],['folder/inside.bin',[9]]])vm.supplyFile('C:/records/'+name,data);
 const main=delayFixture(mode);main.put(main.dirs+13*8,0);main.put(main.dirs+13*8+4,0);
 const image=vm.load(main.bytes);equal((await vm.run(100n)).reason,Exit.Returned);vm.map(0x20000n,0x20000n,7);
 return {vm,handle,mode,image,sp:image.sp-2048n,error:image.teb+BigInt(mode===32?0x34:0x68),invalid:(1n<<BigInt(mode))-1n};
}
function frame(t,name,args=[]){
 const {vm,sp,mode}=t;vm.ip=vm.bind(name==='NtQueryDirectoryFile'?'ntdll.dll':'kernel32.dll',name);vm.register(4,sp);vm.write(sp,encode(vm.entryPoints.returned,mode/8));
 for(let n=0;n<args.length;n++)if(mode===32)vm.write(sp+4n+BigInt(n*4),encode(BigInt.asUintN(32,args[n]),4));else if(n<4)vm.register([1,2,8,9][n],BigInt.asUintN(64,args[n]));else vm.write(sp+40n+BigInt(n-4)*8n,encode(BigInt.asUintN(64,args[n]),8));
}
function call(t,name,args=[]){const work=t.vm.work,retired=t.vm.retired;frame(t,name,args);equal(t.vm.service().reason,Exit.Dispatch);equal(t.vm.work,work+1n);equal(t.vm.retired,retired);return t.vm.register(0);}
function pathCall(t,api,path,tail=[]){t.vm.write(0x20000n,api.endsWith('W')?wide(path):text(path));return call(t,api,[0x20000n,...tail]);}
function first(t,path,wide=true,extended=false,out=0x21000n){return pathCall(t,`FindFirstFile${extended?'Ex':''}${wide?'W':'A'}`,path,extended?[1n,out,0n,0n,2n]:[out]);}
function filename(t,p=0x21000n,unicode=true){const bytes=read(t.vm,p+44n,unicode?520:260);let length=0;while(length<bytes.length&&(bytes[length]||(unicode&&bytes[length+1])))length+=unicode?2:1;return new TextDecoder(unicode?'utf-16le':'shift_jis').decode(bytes.slice(0,length));}
function open(t,path='C:/records',access=0x80000000n){return pathCall(t,'CreateFileW',path,[access,7n,0n,3n,0x02000000n,0n]);}
const info=(t,h,kind,capacity=4096n,out=0x21000n)=>call(t,'GetFileInformationByHandleEx',[h,kind,out,capacity]);
function records(t,p=0x21000n){const result=[];for(let i=0;i<100;i++){const next=t.vm.readInt(p),length=Number(t.vm.readInt(p+60n));result.push({name:new TextDecoder('utf-16le').decode(read(t.vm,p+68n,length)),attributes:t.vm.readInt(p+56n),size:t.vm.readInt(p+40n,8)});if(!next)return result;equal(next%8n,0n);p+=next;}assert.fail('directory batch did not terminate');}
const all=['A.TXT','b.txt','data.bin','folder','日本.txt'];

for(const mode of [32,64])for(const unicode of [false,true])for(const extended of [false,true]){
 const t=await machine(mode),{vm}=t;vm.write(t.error,encode(91n,4));const h=first(t,'c:/RECORDS/*.TxT',unicode,extended);assert.notEqual(h,t.invalid);equal(filename(t,0x21000n,unicode),'A.TXT');equal(vm.readInt(0x21000n),0x80n);equal(vm.readInt(0x21020n),3n);equal(read(vm,0x21004n,24),new Uint8Array(24));equal(vm.readInt(t.error),91n);
 if(mode===64){equal(call(t,'FindNextFileW',[h+(1n<<32n),0n]),0n);equal(vm.readInt(t.error),6n);}vm.supplyFile('C:/records/later.txt',[10]);const names=['A.TXT'];while(call(t,unicode?'FindNextFileW':'FindNextFileA',[h,0x21000n]))names.push(filename(t,0x21000n,unicode));equal(names,['A.TXT','b.txt','日本.txt']);equal(vm.readInt(t.error),18n);
 vm.write(0x21000n,new Uint8Array(592).fill(0xa5));equal(call(t,'FindNextFileW',[h,0x21000n]),0n);equal(read(vm,0x21000n,592),new Uint8Array(592).fill(0xa5));equal(call(t,'CloseHandle',[h]),0n);equal(vm.readInt(t.error),6n);equal(call(t,'FindClose',[h]),1n);equal(call(t,'FindNextFileW',[h,0n]),0n);equal(vm.readInt(t.error),6n);
 equal(first(t,'C:/records/*.absent',unicode,extended),t.invalid);equal(vm.readInt(t.error),2n);equal(first(t,'C:/missing/*',unicode,extended),t.invalid);equal(vm.readInt(t.error),3n);equal(first(t,'C:/records/',unicode,extended),t.invalid);equal(vm.readInt(t.error),2n);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 for(const args of [[0n,2n,0n,0n,0n,0n],[0n,0n,0n,1n,0n,0n],[0n,0n,0n,0n,1n,0n],[0n,0n,0n,0n,0n,1n]]){equal(call(t,'FindFirstFileExW',args),t.invalid);equal(vm.readInt(t.error),87n);}
 const h=first(t,'C:/records/*.*',true,false,t.sp);assert.notEqual(h,t.invalid);equal(filename(t,t.sp),'A.TXT');equal(vm.ip,vm.entryPoints.returned);equal(call(t,'FindNextFileA',[h,0x21000n]),1n);equal(filename(t,0x21000n,false),'b.txt');equal(call(t,'FindClose',[h]),1n);
 const directory=open(t),file=open(t,'C:/records/A.TXT');assert.notEqual(directory,t.invalid);assert.notEqual(file,t.invalid);equal(pathCall(t,'SetFileAttributesW','C:/records',[0x2010n]),1n);equal(info(t,directory,9n,8n),1n);equal(vm.readInt(0x21000n),0x2010n);equal(pathCall(t,'SetFileAttributesA','C:/records/A.TXT',[0x2000n]),1n);equal(info(t,file,9n,8n),1n);equal(vm.readInt(0x21000n),0x2000n);equal(pathCall(t,'SetFileAttributesW','C:/records/A.TXT',[1n]),0n);equal(vm.readInt(t.error),50n);equal(pathCall(t,'GetFileAttributesW','C:/records/A.TXT'),0x2000n);equal(pathCall(t,'SetFileAttributesW','C:/records/A.TXT',[0x80n]),1n);
 equal(info(t,directory,14n,1n),0n);equal(vm.readInt(t.error),24n);equal(info(t,directory,14n,80n,t.sp),1n);equal(vm.ip,vm.entryPoints.returned);equal(records(t,t.sp).map(r=>r.name),['A.TXT']);equal(call(t,'DuplicateHandle',[t.invalid,directory,t.invalid,0x20200n,0n,0n,2n]),1n);const alias=vm.readInt(0x20200n,mode/8);equal(info(t,alias,14n,80n),1n);equal(records(t).map(r=>r.name),['b.txt']);equal(info(t,directory,14n),1n);equal(records(t).map(r=>r.name),all.slice(2));equal(info(t,alias,14n),0n);equal(vm.readInt(t.error),18n);equal(info(t,alias,15n),1n);equal(records(t).map(r=>r.name),all);equal(records(t).find(r=>r.name==='folder').attributes,0x10n);equal(records(t).find(r=>r.name==='A.TXT').size,3n);
 equal(info(t,file,14n),0n);equal(vm.readInt(t.error),267n);const noAccess=open(t,undefined,0n);equal(info(t,noAccess,14n),0n);equal(vm.readInt(t.error),5n);equal(info(t,directory,15n,0x1000001n,0n),0n);equal(vm.readInt(t.error),8n);for(const x of [directory,alias,file,noAccess])equal(call(t,'CloseHandle',[x]),1n);vm.dispose();
}

// Faulted outputs must neither allocate search tokens nor consume entries.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,cross=0x21ed4n;vm.write(0x20000n,wide('C:/records/*.txt'));vm.write(cross,new Uint8Array(592).fill(0xa5));frame(t,'FindFirstFileW',[0x20000n,cross]);equal(core.wire_protect(t.handle,0x22000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(read(vm,cross,300),new Uint8Array(300).fill(0xa5));equal(core.wire_protect(t.handle,0x22000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);const h=vm.register(0);equal(h,0x80000000n);equal(filename(t,cross),'A.TXT');
 frame(t,'FindNextFileW',[h,cross]);equal(core.wire_protect(t.handle,0x22000n,4096n,2),0);const next=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),next);equal(core.wire_protect(t.handle,0x22000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(filename(t,cross),'b.txt');equal(call(t,'FindClose',[h]),1n);
 vm.write(0x20000n,wide('C:/records/*.absent'));frame(t,'FindFirstFileW',[0x20000n,0x21000n]);equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);const missing=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),missing);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),t.invalid);vm.dispose();
}

// An empty batch is captured only after its error output can be committed.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;equal(pathCall(t,'CreateDirectoryW','C:/empty',[0n]),1n);const h=open(t,'C:/empty');frame(t,'GetFileInformationByHandleEx',[h,14n,0x21000n,4096n]);equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);vm.supplyFile('C:/empty/added.bin',[11]);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),1n);equal(records(t).map(r=>r.name),['added.bin']);equal(call(t,'CloseHandle',[h]),1n);vm.dispose();
}

{
 const t=await machine(64),{vm}=t,h=open(t),iosb=0x20400n,pattern=0x20500n;vm.write(t.error,encode(91n,4));
 function nt({handle=h,capacity=4096n,single=0n,restart=0n,filter=0n,output=0x21000n,status=iosb,kind=2n,event=0n}={}){return call(t,'NtQueryDirectoryFile',[handle,event,0n,0n,status,output,capacity,kind,single,filter,restart]);}
 function filter(value){const bytes=wide(value).slice(0,-2);vm.write(pattern,new Uint8Array(16));vm.write(pattern,encode(BigInt(bytes.length),2));vm.write(pattern+2n,encode(BigInt(bytes.length),2));vm.write(pattern+8n,encode(0x20600n,8));vm.write(0x20600n,bytes);return pattern;}
 equal(nt({capacity:1n,filter:filter('*.txt')}),0xc0000023n);equal(vm.readInt(iosb),0xc0000023n);equal(vm.readInt(iosb+8n,8),0n);equal(nt({single:1n,filter:filter('*.txt')}),0n);equal(records(t).map(r=>r.name),['A.TXT']);equal(vm.readInt(iosb+8n,8),80n);equal(nt({single:1n,filter:filter('*.bin')}),0n);equal(records(t).map(r=>r.name),['b.txt']);equal(nt({restart:1n,single:0x100n,filter:filter('*.bin')}),0n);equal(records(t).map(r=>r.name),['A.TXT','b.txt','日本.txt']);equal(nt(),0x80000006n);equal(vm.readInt(iosb+8n,8),0n);equal(vm.readInt(t.error),91n);
 for(const [options,status] of [[{handle:0x123456n},0xc0000008n],[{kind:1n},0xc0000003n],[{event:1n},0xc00000bbn],[{capacity:0x1000001n},0xc0000017n]]){equal(nt(options),status);equal(vm.readInt(iosb),status);equal(vm.readInt(iosb+8n,8),0n);equal(vm.readInt(t.error),91n);}
 equal(nt({restart:0x100n}),0x80000006n);vm.write(pattern,encode(3n,2));equal(nt({filter:pattern}),0xc000000dn);equal(vm.readInt(iosb),0xc000000dn);filter('\ud800');equal(nt({filter:pattern}),0xc000000dn);equal(vm.readInt(iosb),0xc000000dn);
 equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);equal(nt({handle:0x123456n}),0xc0000008n);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);
 const fresh=open(t),cross=0x21ff8n;frame(t,'NtQueryDirectoryFile',[fresh,0n,0n,0n,cross,0x21000n,4096n,2n,1n,0n,0n]);equal(core.wire_protect(t.handle,0x22000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,0x22000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(records(t).map(r=>r.name),['A.TXT']);equal(vm.readInt(cross),0n);equal(vm.readInt(cross+8n,8),80n);equal(call(t,'CloseHandle',[fresh]),1n);equal(call(t,'CloseHandle',[h]),1n);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,long='あ'.repeat(130);vm.supplyFile(`C:/encoded/${long}`,[1]);equal(first(t,'C:/encoded/*',false),t.invalid);equal(vm.readInt(t.error),206n);const h=first(t,'C:/encoded/*');equal(h,0x80000000n);equal(filename(t),long);equal(call(t,'FindClose',[h]),1n);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,handles=[];
 for(let n=0;n<4096;n++){const h=first(t,'C:/records/A.TXT');assert.notEqual(h,t.invalid);handles.push(h);}
 equal(first(t,'C:/records/A.TXT'),t.invalid);equal(vm.readInt(t.error),8n);for(const h of handles)equal(call(t,'FindClose',[h]),1n);const next=first(t,'C:/records/A.TXT');assert.notEqual(next,t.invalid);assert.ok(!handles.includes(next));equal(call(t,'FindNextFileW',[handles[0],0n]),0n);equal(vm.readInt(t.error),6n);equal(call(t,'FindClose',[next]),1n);vm.dispose();
}

console.log(`${checks} file enumeration/attributes/cursor/NT/bounds/fault checks passed`);
