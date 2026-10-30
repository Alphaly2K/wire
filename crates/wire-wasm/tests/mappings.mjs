import assert from 'node:assert/strict';
import {WasmMachine,Exit} from '../web/runtime.mjs';
import {core,stateLayout,encode} from './x87-harness.mjs';
import {delayFixture} from './delay-fixture.mjs';
import {moduleFixture} from './module-fixture.mjs';
import {dynamicFixture} from './dynamic-fixture.mjs';

let checks=0;const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
const wide=s=>Uint8Array.from([...s.split('').flatMap(c=>[c.charCodeAt(0)&255,c.charCodeAt(0)>>8]),0,0]);
const snapshot=t=>new Uint8Array(core.memory.buffer,core.wire_state(t.handle),stateLayout[0]).slice();
async function machine(mode,reject=false){
 let handle;const vm=new WasmMachine({...core,wire_create(...a){handle=core.wire_create(...a);return handle;}},{mode,clock(){throw new Error('unexpected elapsed clock');},wallClock(){return 123n;}});
 const input=new Uint8Array(65541).fill(0x42);input.set([0xb8,7,0,0,0,0xc3]);vm.supplyFile('C:/data/input.bin',input);vm.supplyFile('C:/data/empty.bin',[]);
 let main;if(reject){vm.supplyModule('bad.dll',dynamicFixture(mode,{id:2,attach:false}).bytes);main=moduleFixture(mode,{dependency:'bad.dll'});}else{main=delayFixture(mode);main.put(main.dirs+13*8,0);main.put(main.dirs+13*8+4,0);}
 if(mode===32)main.put(main.opt+70,main.view.getUint16(main.opt+70,true)&~0x100,2);
 const image=vm.load(main.bytes);if(!reject)equal((await vm.run(100n)).reason,Exit.Returned);vm.map(0x20000n,0x20000n,7);
 return {vm,handle,mode,image,input,sp:image.sp-2048n,error:image.teb+BigInt(mode===32?0x34:0x68),invalid:(1n<<BigInt(mode))-1n};
}
function frame(t,name,args=[]){const {vm,sp,mode}=t;vm.ip=vm.bind('kernel32.dll',name);vm.register(4,sp);vm.write(sp,encode(vm.entryPoints.returned,mode/8));let offset=4n;for(let n=0;n<args.length;n++)if(mode===32){const width=name==='SetFilePointerEx'&&n===1?8:4;vm.write(sp+offset,encode(BigInt.asUintN(width*8,args[n]),width));offset+=BigInt(width);}else if(n<4)vm.register([1,2,8,9][n],BigInt.asUintN(64,args[n]));else vm.write(sp+40n+BigInt(n-4)*8n,encode(BigInt.asUintN(64,args[n]),8));}
function call(t,name,args=[]){const work=t.vm.work,retired=t.vm.retired;frame(t,name,args);equal(t.vm.service().reason,Exit.Dispatch);equal(t.vm.work,work+1n);equal(t.vm.retired,retired);return t.vm.register(0);}
function open(t,path='C:/data/input.bin',access=0xc0000000n,disposition=3n){t.vm.write(0x20000n,wide(path));return call(t,'CreateFileW',[0x20000n,access,7n,0n,disposition,0x80n,0n]);}
const section=(t,h,maximum=0n,sa=0n,api='CreateFileMappingW')=>call(t,api,[h,sa,2n,maximum>>32n,maximum&0xffffffffn,0n]);
const map=(t,h,offset=0n,size=0n,base=0n)=>call(t,base?'MapViewOfFileEx':'MapViewOfFile',base?[h,4n,offset>>32n,offset&0xffffffffn,size,base]:[h,4n,offset>>32n,offset&0xffffffffn,size]);
const seek=(t,h,offset)=>call(t,'SetFilePointerEx',[h,offset,0n,0n]);
async function runView(t,base,expected){t.vm.ip=base;t.vm.register(4,t.sp);t.vm.write(t.sp,encode(t.vm.entryPoints.returned,t.mode/8));equal((await t.vm.run(100n)).reason,Exit.Returned);equal(t.vm.register(0),expected);}

for(const mode of [32,64])for(const api of ['CreateFileMappingA','CreateFileMappingW']){
 const t=await machine(mode),{vm}=t,h=open(t),s=section(t,h,0n,0n,api);equal(s,0x180000n);vm.write(t.error,encode(91n,4));const first=map(t,s),explicit=mode===32?0x16000000n:0x560000000n,second=map(t,s,65536n,5n,explicit);assert.ok(first);equal(second,explicit);equal(vm.readInt(t.error),91n);equal(vm.readInt(first,1),0xb8n);equal(vm.readInt(second,4),0x42424242n);equal(vm.readInt(first+65541n,4),0n);assert.throws(()=>vm.write(first,[1]),/guest memory Protection/);equal(core.wire_protect(t.handle,first,4096n,4),5);
 equal(call(t,'FlushViewOfFile',[first+65536n,4096n]),1n);equal(call(t,'FlushViewOfFile',[first+65536n,4097n]),0n);equal(vm.readInt(t.error),487n);equal(call(t,'UnmapViewOfFile',[first+1n]),0n);equal(vm.readInt(t.error),487n);equal(call(t,'CloseHandle',[s]),1n);
 equal(seek(t,h,65536n),1n);vm.write(0x21000n,[97,98,99]);equal(core.wire_protect(t.handle,second,4096n,0x102),0);equal(call(t,'WriteFile',[h,0x21000n,3n,0x20200n,0n]),1n);equal(vm.readInt(first+65536n,2),0x6261n);assert.throws(()=>vm.readInt(second),/memory access/);equal(vm.readInt(second,2),0x6261n);
 if(mode===32){await runView(t,first,7n);equal(seek(t,h,1n),1n);vm.write(0x21000n,[9,0,0,0]);equal(call(t,'WriteFile',[h,0x21000n,4n,0x20200n,0n]),1n);await runView(t,first,9n);}else{vm.ip=first;equal((await vm.run(1n)).reason,Exit.Fault);}
 equal(call(t,'SetEndOfFile',[h]),0n);equal(vm.readInt(t.error),1224n);equal(open(t,undefined,0xc0000000n,2n),t.invalid);equal(vm.readInt(t.error),1224n);equal(call(t,'CloseHandle',[h]),1n);equal(vm.readInt(second,2),0x6261n);equal(call(t,'UnmapViewOfFile',[first]),1n);equal(call(t,'UnmapViewOfFile',[second]),1n);assert.throws(()=>vm.readInt(first),/memory access/);const writable=open(t,undefined,0xc0000000n,2n);assert.notEqual(writable,t.invalid);equal(call(t,'GetFileSize',[writable,0n]),0n);equal(call(t,'CloseHandle',[writable]),1n);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=open(t),sa=0x20300n,width=mode/8;vm.write(sa,new Uint8Array(24));equal(section(t,h,0n,sa),0n);equal(vm.readInt(t.error),87n);vm.write(sa,encode(BigInt(mode===32?12:24),4));vm.write(sa+BigInt(mode===32?4:8),encode(1n,width));equal(section(t,h,0n,sa),0n);equal(vm.readInt(t.error),50n);vm.write(sa+BigInt(mode===32?4:8),encode(0n,width));vm.write(sa+BigInt(mode===32?8:16),encode(1n,4));const s=section(t,h,16n,sa);equal(s,0x180000n);equal(call(t,'GetHandleInformation',[s,0x20200n]),1n);equal(vm.readInt(0x20200n),1n);
 const cross=0x21ffdn;frame(t,'DuplicateHandle',[t.invalid,s,t.invalid,cross,0n,0n,3n]);equal(core.wire_protect(t.handle,0x22000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,0x22000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);const alias=vm.readInt(cross,width);equal(alias,0x180001n);equal(map(t,s),0n);equal(vm.readInt(t.error),6n);equal(call(t,'SetHandleInformation',[alias,2n,2n]),1n);equal(call(t,'CloseHandle',[alias]),0n);equal(vm.readInt(t.error),6n);equal(call(t,'SetHandleInformation',[alias,2n,0n]),1n);equal(call(t,'DuplicateHandle',[t.invalid,alias,t.invalid,t.sp,4n,1n,0n]),1n);const another=vm.readInt(t.sp,width);equal(vm.ip,vm.entryPoints.returned);equal(call(t,'DuplicateHandle',[t.invalid,alias,t.invalid,0x20200n,2n,0n,0n]),0n);equal(vm.readInt(t.error),50n);
 const base=map(t,another);equal(vm.readInt(base),0x7b8n);equal(call(t,'CloseHandle',[alias]),1n);equal(call(t,'CloseHandle',[another]),1n);equal(call(t,'CloseHandle',[h]),1n);equal(vm.readInt(base),0x7b8n);equal(call(t,'UnmapViewOfFile',[base]),1n);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=open(t),s=section(t,h);const missing=mode===64?s+(1n<<32n):0x123456n;
 for(const [api,args,error] of [['CreateFileMappingW',[t.invalid,0n,2n,0n,0n,0n],50n],['CreateFileMappingW',[h,0n,4n,0n,0n,0n],50n],['CreateFileMappingW',[h,0n,2n,0n,0n,1n],50n],['MapViewOfFile',[missing,4n,0n,0n,0n],6n],['MapViewOfFile',[s,2n,0n,0n,0n],50n],['MapViewOfFile',[s,4n,0n,1n,0n],1132n],['MapViewOfFile',[s,4n,1n,0n,0n],87n],['MapViewOfFileEx',[s,4n,0n,0n,1n,0x16000001n],1132n],['MapViewOfFileEx',[s,4n,0n,0n,1n,0x20000n],487n],['MapViewOfFile',[s,4n,0n,65536n,6n],87n]]){equal(call(t,api,args),0n);equal(vm.readInt(t.error),error);}
 equal(section(t,h,65542n),0n);equal(vm.readInt(t.error),5n);equal(section(t,open(t,'C:/data/empty.bin')),0n);equal(vm.readInt(t.error),1006n);
 frame(t,'MapViewOfFile',[s,4n,0n,0n,0n]);const page=t.sp&~4095n;equal(core.wire_protect(t.handle,page,4096n,1),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);const base=vm.register(0);assert.ok(base);frame(t,'UnmapViewOfFile',[base]);equal(core.wire_protect(t.handle,page,4096n,1),0);const unmap=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),unmap);equal(vm.readInt(base),0x7b8n);equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);assert.throws(()=>vm.readInt(base),/memory access/);vm.dispose();
}

// Lost backing must be detected before WriteFile commits payload or cursor.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=open(t),s=section(t,h),first=map(t,s),base=map(t,s);equal(core.wire_unmap(t.handle,base,17n*4096n),0);vm.write(0x21000n,[99]);equal(call(t,'WriteFile',[h,0x21000n,1n,0x20200n,0n]),0n);equal(vm.readInt(t.error),487n);equal(vm.readInt(0x20200n),0n);equal(vm.readInt(first,1),0xb8n);equal(call(t,'SetFilePointerEx',[h,0n,0x20200n,1n]),1n);equal(vm.readInt(0x20200n,8),0n);equal(call(t,'ReadFile',[h,0x21000n,1n,0x20200n,0n]),1n);equal(vm.readInt(0x21000n,1),0xb8n);equal(map(t,s,0n,0n,base),base);equal(call(t,'UnmapViewOfFile',[base]),1n);equal(call(t,'UnmapViewOfFile',[first]),1n);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode,true),{vm}=t,ip=vm.ip,regs=Array.from({length:mode===32?8:16},(_,r)=>vm.register(r)),h=open(t),s=section(t,h),base=map(t,s);for(let r=0;r<regs.length;r++)vm.register(r,regs[r]);vm.ip=ip;await assert.rejects(vm.run(1000n),/attach returned FALSE/);assert.throws(()=>vm.readInt(base),/memory access/);equal(call(t,'CloseHandle',[s]),0n);equal(vm.readInt(t.error),6n);equal(call(t,'CloseHandle',[h]),0n);equal(vm.readInt(t.error),6n);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=open(t),handles=[];for(let n=0;n<1024;n++){const s=section(t,h,1n);assert.ok(s);handles.push(s);}equal(section(t,h),0n);equal(vm.readInt(t.error),8n);for(const s of handles)equal(call(t,'CloseHandle',[s]),1n);const s=section(t,h,1n),views=[];for(let n=0;n<1024;n++){const base=map(t,s);assert.ok(base);views.push(base);}equal(map(t,s),0n);equal(vm.readInt(t.error),8n);for(const base of views)equal(call(t,'UnmapViewOfFile',[base]),1n);const base=map(t,s);equal(base,views[0]);equal(call(t,'UnmapViewOfFile',[base]),1n);equal(call(t,'CloseHandle',[s]),1n);vm.dispose();
}

console.log(`${checks} file section/view/coherence/alias/fault/lifetime checks passed`);
