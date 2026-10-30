import assert from 'node:assert/strict';
import {WasmMachine,Exit} from '../web/runtime.mjs';
import {core,stateLayout,encode} from './x87-harness.mjs';
import {delayFixture} from './delay-fixture.mjs';
import {dynamicFixture} from './dynamic-fixture.mjs';

let checks=0;const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
const text=s=>new TextEncoder().encode(s+'\0');
const wide=s=>Uint8Array.from([...s.split('').flatMap(c=>[c.charCodeAt(0)&255,c.charCodeAt(0)>>8]),0,0]);
const read=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const snapshot=t=>new Uint8Array(core.memory.buffer,core.wire_state(t.handle),stateLayout[0]).slice();
async function machine(mode){
 let handle;const wall={value:0x1122334455667788n,calls:0,fail:false};const vm=new WasmMachine({...core,wire_create(...a){handle=core.wire_create(...a);return handle;}},{mode,clock(){throw new Error('file services must not sample elapsed time');},wallClock(){wall.calls++;if(wall.fail)throw new Error('file clock unavailable');return wall.value;}});
 const input=Uint8Array.from([1,2,3,4,5,6]);vm.supplyFile('C:\\data\\archive.bin',input);input.fill(0);vm.supplyFile('C:\\app\\leaf.dll',dynamicFixture(mode,{seed:73}).bytes);const main=delayFixture(mode);main.put(main.dirs+13*8,0);main.put(main.dirs+13*8+4,0);const image=vm.load(main.bytes,{command:'"C:\\app\\program.exe"'});equal((await vm.run(100n)).reason,Exit.Returned);vm.map(0x20000n,0x20000n,7);return {vm,handle,mode,image,wall,sp:image.sp-512n,error:image.teb+BigInt(mode===32?0x34:0x68),invalid:(1n<<BigInt(mode))-1n};
}
function frame(t,name,args=[]){const {vm,sp,mode}=t;vm.ip=vm.bind('kernel32.dll',name);vm.register(4,sp);vm.write(sp,encode(vm.entryPoints.returned,mode/8));let offset=4n;
 for(let n=0;n<args.length;n++)if(mode===32){const width=name==='SetFilePointerEx'&&n===1?8:4;vm.write(sp+offset,encode(BigInt.asUintN(width*8,args[n]),width));offset+=BigInt(width);}else if(n<4)vm.register([1,2,8,9][n],BigInt.asUintN(64,args[n]));else vm.write(sp+40n+BigInt(n-4)*8n,encode(BigInt.asUintN(64,args[n]),8));
}
function call(t,name,args=[]){const work=t.vm.work,retired=t.vm.retired;frame(t,name,args);equal(t.vm.service().reason,Exit.Dispatch);equal(t.vm.work,work+1n);equal(t.vm.retired,retired);return t.vm.register(0);}
function pathCall(t,api,path,tail=[]){t.vm.write(0x20000n,api.endsWith('W')?wide(path):text(path));return call(t,api,[0x20000n,...tail]);}
function open(t,path='C:\\data\\archive.bin',access=0x80000000n,share=7n,disposition=3n,security=0n,api='CreateFileW'){return pathCall(t,api,path,[access,share,security,disposition,0x80n,0n]);}
const seek=(t,h,distance,method=0n,out=0n)=>call(t,'SetFilePointerEx',[h,distance,out,method]);
const transfer=(t,api,h,count,pointer=0x21000n,out=0x20100n,overlap=0n)=>call(t,api,[h,pointer,count,out,overlap]);
function bytes(t,h,count){equal(transfer(t,'ReadFile',h,BigInt(count)),1n);return read(t.vm,0x21000n,Number(t.vm.readInt(0x20100n)));}
function size(t,h){equal(call(t,'GetFileSizeEx',[h,0x20200n]),1n);return t.vm.readInt(0x20200n,8);}
async function moduleCall(t,name,args){frame(t,name,args);for(let n=0;n<1000;n++){const result=await t.vm.run(1n);if(result.reason===Exit.Returned)return t.vm.register(0);equal(result.reason,Exit.Budget);assert.ok(n<999);}}

for(const mode of [32,64])for(const api of ['CreateFileA','CreateFileW']){
 const t=await machine(mode),{vm}=t,h=open(t,'c:/DATA/./archive.bin',0x80000000n,7n,3n,0n,api);assert.notEqual(h,t.invalid);equal(size(t,h),6n);equal(call(t,'AreFileApisANSI'),1n);equal(call(t,'GetFileType',[h]),1n);equal(call(t,'FlushFileBuffers',[h]),0n);equal(vm.readInt(t.error),5n);vm.write(t.error,encode(91n,4));equal(call(t,'GetFileSize',[h,0x20200n]),6n);equal(vm.readInt(0x20200n),0n);equal(vm.readInt(t.error),0n);
 equal(bytes(t,h,3),Uint8Array.from([1,2,3]));equal(transfer(t,'ReadFile',h,0n,0xffffffffffffffffn),1n);equal(vm.readInt(0x20100n),0n);equal(seek(t,h,0n,1n,0x20200n),1n);equal(vm.readInt(0x20200n,8),3n);equal(bytes(t,h,2),Uint8Array.from([4,5]));
 equal(call(t,'SetFilePointer',[h,-1n,0n,2n]),5n);vm.write(0x21000n,new Uint8Array(16).fill(0xa5));equal(bytes(t,h,9),Uint8Array.from([6]));equal(vm.readInt(0x21001n,1),0xa5n);equal(bytes(t,h,3),new Uint8Array());equal(seek(t,h,-7n,1n),0n);equal(vm.readInt(t.error),131n);equal(seek(t,h,0n,1n,0x20200n),1n);equal(vm.readInt(0x20200n,8),6n);
 equal(transfer(t,'WriteFile',h,1n),0n);equal(vm.readInt(0x20100n),0n);equal(vm.readInt(t.error),5n);equal(size(t,h),6n);equal(call(t,'CloseHandle',[h]),1n);equal(call(t,'GetFileType',[h]),0n);equal(vm.readInt(t.error),6n);equal(call(t,'GetFileSize',[h,0n]),0xffffffffn);equal(call(t,'CloseHandle',[h]),0n);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;equal(pathCall(t,'CreateDirectoryW','C:\\files',[0n]),1n);equal(pathCall(t,'CreateDirectoryA','C:\\files',[0n]),0n);equal(vm.readInt(t.error),183n);equal(pathCall(t,'GetFileAttributesW','C:\\files'),0x10n);let h=open(t,'C:\\files\\new.bin',0xc0000000n,7n,1n);assert.notEqual(h,t.invalid);vm.write(0x21000n,[10,11,12,13,14]);equal(transfer(t,'WriteFile',h,5n),1n);equal(vm.readInt(0x20100n),5n);equal(size(t,h),5n);equal(call(t,'GetFileTime',[h,0x20300n,0x20308n,0x20310n]),1n);equal([vm.readInt(0x20300n,8),vm.readInt(0x20308n,8),vm.readInt(0x20310n,8)],[0n,0n,t.wall.value]);
 equal(call(t,'GetFileInformationByHandle',[h,0x20400n]),1n);equal(vm.readInt(0x20400n),0x80n);equal(vm.readInt(0x20424n),5n);const id=vm.readInt(0x20430n);assert.ok(id);for(const [kind,length] of [[0n,40],[1n,24],[9n,8],[18n,24]]){equal(call(t,'GetFileInformationByHandleEx',[h,kind,0x20500n,BigInt(length)]),1n);if(kind===1n){equal(vm.readInt(0x20500n,8),4096n);equal(vm.readInt(0x20508n,8),5n);}if(kind===18n)equal(vm.readInt(0x20508n,8),id);}
 equal(seek(t,h,8n),1n);vm.write(0x21000n,[21,22]);t.wall.value++;equal(transfer(t,'WriteFile',h,2n),1n);equal(size(t,h),10n);equal(seek(t,h,0n),1n);equal(bytes(t,h,10),Uint8Array.from([10,11,12,13,14,0,0,0,21,22]));equal(seek(t,h,3n),1n);equal(call(t,'SetEndOfFile',[h]),1n);equal(size(t,h),3n);equal(call(t,'GetFileTime',[h,0n,0n,0x20300n]),1n);equal(vm.readInt(0x20300n,8),t.wall.value);
 equal(pathCall(t,'GetFileAttributesExW','C:\\files\\new.bin',[0n,0x20600n]),1n);equal(vm.readInt(0x20600n),0x80n);equal(vm.readInt(0x20620n),3n);equal(vm.readInt(0x20614n,8),t.wall.value);equal(pathCall(t,'DeleteFileW','C:\\files\\new.bin'),0n);equal(vm.readInt(t.error),32n);equal(call(t,'CloseHandle',[h]),1n);
 equal(pathCall(t,'MoveFileA','C:\\files\\new.bin',[0x20080n]),0n,'an empty destination cannot rename the source');vm.write(0x20080n,text('C:\\files\\moved.bin'));equal(pathCall(t,'MoveFileA','C:\\files\\new.bin',[0x20080n]),1n);h=open(t,'C:\\files\\moved.bin');equal(call(t,'GetFileInformationByHandle',[h,0x20400n]),1n);equal(vm.readInt(0x20430n),id);equal(bytes(t,h,9),Uint8Array.from([10,11,12]));equal(call(t,'CloseHandle',[h]),1n);
 equal(pathCall(t,'RemoveDirectoryW','C:\\files'),0n);equal(vm.readInt(t.error),145n);equal(pathCall(t,'DeleteFileA','C:\\files\\moved.bin'),1n);equal(pathCall(t,'RemoveDirectoryA','C:\\files'),1n);equal(pathCall(t,'GetFileAttributesA','C:\\files\\moved.bin'),0xffffffffn);equal(vm.readInt(t.error),3n);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 for(const disposition of [1n,4n]){
  const path=`C:\\data\\readonly-${disposition}.bin`,h=open(t,path,0x80000000n,7n,disposition);assert.notEqual(h,t.invalid);equal(size(t,h),0n);equal(transfer(t,'WriteFile',h,1n),0n);equal(vm.readInt(t.error),5n);equal(call(t,'CloseHandle',[h]),1n);
  const existing=open(t,path,0x80000000n,7n,4n);assert.notEqual(existing,t.invalid);equal(vm.readInt(t.error),183n);equal(call(t,'CloseHandle',[existing]),1n);
 }
 const sa=0x20200n;vm.write(sa,new Uint8Array(24));equal(open(t,'C:\\data\\rejected.bin',0xc0000000n,7n,1n,sa),t.invalid);equal(vm.readInt(t.error),87n);vm.write(sa,encode(BigInt(mode===32?12:24),4));vm.write(sa+BigInt(mode===32?4:8),encode(1n,mode/8));equal(open(t,'C:\\data\\rejected.bin',0xc0000000n,7n,1n,sa),t.invalid);equal(vm.readInt(t.error),50n);equal(pathCall(t,'GetFileAttributesW','C:\\data\\rejected.bin'),0xffffffffn);equal(vm.readInt(t.error),2n);
 const h=open(t,undefined,0xc0000000n),samples=t.wall.calls;equal(call(t,'FlushFileBuffers',[h]),1n);equal(t.wall.calls,samples);equal(call(t,'CloseHandle',[h]),1n);vm.dispose();
}

// Duplicates share the file object/cursor while inheritable/protected flags stay per handle.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,current=t.invalid,sa=0x20200n,width=mode/8;vm.write(sa,new Uint8Array(24));vm.write(sa,encode(BigInt(mode===32?12:24),4));vm.write(sa+BigInt(mode===32?8:16),encode(1n,4));const h=open(t,undefined,0xc0000000n,7n,3n,sa);equal(call(t,'GetHandleInformation',[h,0x20240n]),1n);equal(vm.readInt(0x20240n),1n);equal(call(t,'DuplicateHandle',[current,h,current,0x20280n,0n,0n,2n]),1n);const alias=vm.readInt(0x20280n,width);assert.notEqual(alias,h);equal(call(t,'GetHandleInformation',[alias,0x20240n]),1n);equal(vm.readInt(0x20240n),0n);equal(bytes(t,alias,2),Uint8Array.from([1,2]));equal(bytes(t,h,2),Uint8Array.from([3,4]));equal(call(t,'SetHandleInformation',[h,2n,2n]),1n);equal(call(t,'CloseHandle',[h]),0n);equal(vm.readInt(t.error),6n);equal(call(t,'DuplicateHandle',[current,h,current,0x20280n,0n,0n,3n]),0n);equal(size(t,h),6n);
 equal(call(t,'SetHandleInformation',[h,2n,0n]),1n);equal(call(t,'DuplicateHandle',[current,h,current,0x20280n,0x80000000n,1n,1n]),1n);const narrow=vm.readInt(0x20280n,width);equal(call(t,'GetFileSizeEx',[h,0x20200n]),0n);equal(transfer(t,'WriteFile',narrow,1n),0n);equal(vm.readInt(t.error),5n);equal(bytes(t,narrow,2),Uint8Array.from([5,6]));equal(bytes(t,alias,1),new Uint8Array());equal(call(t,'CloseHandle',[alias]),1n);equal(call(t,'CloseHandle',[narrow]),1n);const exclusive=open(t,undefined,0xc0000000n,0n);assert.notEqual(exclusive,t.invalid);equal(open(t),t.invalid);equal(vm.readInt(t.error),32n);equal(call(t,'CloseHandle',[exclusive]),1n);vm.dispose();
}

// Faults must preserve file bytes, cursors, output prefixes and reference ownership.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=open(t,undefined,0xc0000000n),cross=0x21ffdn,page=0x22000n;vm.write(cross,new Uint8Array(16).fill(0xa5));frame(t,'ReadFile',[h,cross,12n,0x20100n,0n]);equal(core.wire_protect(t.handle,page,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(read(vm,cross,3),new Uint8Array(3).fill(0xa5));equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(read(vm,cross,6),Uint8Array.from([1,2,3,4,5,6]));equal(vm.readInt(0x20100n),6n);
 equal(seek(t,h,0n),1n);vm.write(cross,[9,8,7,6]);frame(t,'WriteFile',[h,cross,4n,0x20100n,0n]);equal(core.wire_protect(t.handle,page,4096n,1),0);const source=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),source);equal(core.wire_protect(t.handle,page,4096n,4),0);equal(seek(t,h,0n,1n,0x20200n),1n);equal(vm.readInt(0x20200n,8),0n);equal(bytes(t,h,6),Uint8Array.from([1,2,3,4,5,6]));
 equal(seek(t,h,0n),1n);frame(t,'SetFilePointerEx',[h,4n,cross,0n]);equal(core.wire_protect(t.handle,page,4096n,2),0);const cursor=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),cursor);equal(core.wire_protect(t.handle,page,4096n,4),0);equal(seek(t,h,0n,1n,0x20200n),1n);equal(vm.readInt(0x20200n,8),0n);
 frame(t,'DuplicateHandle',[t.invalid,h,t.invalid,cross,0n,0n,3n]);equal(core.wire_protect(t.handle,page,4096n,2),0);const duplicate=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),duplicate);equal(core.wire_protect(t.handle,page,4096n,4),0);equal(size(t,h),6n);
 vm.write(0x20100n,encode(0xa5a5a5a5n,4));frame(t,'ReadFile',[0x123456n,0x21000n,1n,0x20100n,0n]);equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);const failure=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),failure);equal(vm.readInt(0x20100n),0xa5a5a5a5n);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(0x20100n),0n);equal(vm.readInt(t.error),6n);
 vm.write(0x20000n,wide('C:\\data\\archive.bin'));frame(t,'CreateFileW',[0x20000n,0xc0000000n,7n,0n,2n,0x80n,0n]);equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);const truncate=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),truncate);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(size(t,h),6n);equal(call(t,'CloseHandle',[h]),1n);const exclusive=open(t,undefined,0xc0000000n,0n);assert.notEqual(exclusive,t.invalid);equal(size(t,exclusive),6n);equal(call(t,'CloseHandle',[exclusive]),1n);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=open(t),cross=0x21ffdn;vm.write(0x21000n,new Uint8Array(16).fill(0xa5));frame(t,'ReadFile',[h,0x21000n,2n,cross,0n]);equal(core.wire_protect(t.handle,0x22000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(read(vm,0x21000n,16),new Uint8Array(16).fill(0xa5));equal(core.wire_protect(t.handle,0x22000n,4096n,4),0);equal(bytes(t,h,2),Uint8Array.from([1,2]));
 vm.write(0x20300n,new Uint8Array(16).fill(0xa5));frame(t,'GetFileTime',[h,0x20300n,0x20308n,cross]);equal(core.wire_protect(t.handle,0x22000n,4096n,2),0);const times=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),times);equal(read(vm,0x20300n,16),new Uint8Array(16).fill(0xa5));equal(core.wire_protect(t.handle,0x22000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(read(vm,0x20300n,16),new Uint8Array(16));vm.dispose();
}

// ReadFile writes participate in the same source-authority protocol as guest stores.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,code=0x28000n;vm.write(code,[0xb8,1,0,0,0,0xc3]);vm.ip=code;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,mode/8));equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),1n);vm.supplyFile('C:\\data\\patch.bin',[0xb8,9,0,0,0,0xc3]);const h=open(t,'C:\\data\\patch.bin');equal(transfer(t,'ReadFile',h,6n,code),1n);vm.ip=code;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,mode/8));equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),9n);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=open(t,undefined,0xc0000000n);vm.write(0x21000n,[71,72,73]);frame(t,'WriteFile',[h,0x21000n,3n,0x20100n,0n]);t.wall.fail=true;const before=snapshot(t);assert.throws(()=>vm.service(),/clock unavailable/);equal(snapshot(t),before);t.wall.fail=false;equal(vm.service().reason,Exit.Dispatch);equal(seek(t,h,0n),1n);equal(bytes(t,h,6),Uint8Array.from([71,72,73,4,5,6]));
 equal(seek(t,h,0x100000002n,0n,0x20200n),1n);equal(vm.readInt(0x20200n,8),0x100000002n);equal(call(t,'SetEndOfFile',[h]),0n);equal(vm.readInt(t.error),8n);equal(size(t,h),6n);vm.write(0x20200n,encode(0n,4));equal(call(t,'SetFilePointer',[h,0xffffffffn,0x20200n,0n]),0xffffffffn);equal(vm.readInt(t.error),0n);equal(vm.readInt(0x20200n),0n);
 const x=mode===64?h+(1n<<32n):0x123456n;equal(call(t,'GetFileSizeEx',[x,0x20200n]),0n);equal(vm.readInt(t.error),6n);for(const [api,args,code] of [['ReadFile',[h,0n,0n,0n,0n],87n],['ReadFile',[h,0n,0n,0x20100n,1n],50n],['GetFileInformationByHandleEx',[h,99n,0x20500n,100n],50n],['GetFileInformationByHandleEx',[h,0n,0x20500n,39n],24n],['SetHandleInformation',[h,4n,4n],87n]]){equal(call(t,api,args),0n);equal(vm.readInt(t.error),code);}equal(call(t,'CloseHandle',[h]),1n);vm.dispose();
}

// Active images and ordinary file opens participate in the same sharing decisions.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;equal(open(t,'C:\\app\\program.exe',0xc0000000n),t.invalid);equal(vm.readInt(t.error),32n);equal(pathCall(t,'DeleteFileW','C:\\app\\program.exe'),0n);equal(vm.readInt(t.error),32n);const file=open(t,'C:\\app\\leaf.dll',0xc0000000n,0n);vm.write(0x20000n,text('leaf.dll'));equal(await moduleCall(t,'LoadLibraryA',[0x20000n]),0n);equal(vm.readInt(t.error),32n);equal(vm.moduleBase('leaf.dll'),0n);equal(call(t,'CloseHandle',[file]),1n);
 const shared=open(t,'C:\\app\\leaf.dll',0x80000000n,7n);vm.write(0x20000n,text('leaf.dll'));const base=await moduleCall(t,'LoadLibraryA',[0x20000n]);assert.ok(base);equal(open(t,'C:\\app\\leaf.dll',0xc0000000n),t.invalid);equal(vm.readInt(t.error),32n);equal(await moduleCall(t,'FreeLibrary',[base]),1n);const writable=open(t,'C:\\app\\leaf.dll',0xc0000000n);assert.notEqual(writable,t.invalid);equal(call(t,'CloseHandle',[writable]),1n);equal(call(t,'CloseHandle',[shared]),1n);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,handles=[];for(let n=0;n<1024;n++){const h=open(t,undefined,0xc0000000n);assert.notEqual(h,t.invalid);handles.push(h);}equal(open(t,undefined,0xc0000000n,7n,2n),t.invalid);equal(vm.readInt(t.error),8n);equal(size(t,handles[0]),6n,'handle exhaustion cannot truncate a file');for(const h of handles)equal(call(t,'CloseHandle',[h]),1n);const h=open(t,undefined,0xc0000000n,0n);assert.notEqual(h,t.invalid);equal(call(t,'CloseHandle',[h]),1n);vm.dispose();
}

console.log(`${checks} VFS file I/O/handle/metadata/sharing/fault checks passed`);
