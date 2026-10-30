import assert from 'node:assert/strict';
import {WasmMachine,Exit} from '../web/runtime.mjs';
import {core,stateLayout,encode} from './x87-harness.mjs';
import {delayFixture} from './delay-fixture.mjs';
import {moduleFixture} from './module-fixture.mjs';
import {dynamicFixture} from './dynamic-fixture.mjs';
import {fixture} from './pe-fixture.mjs';
let checks=0;const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
const wide=s=>Uint8Array.from([...s.split('').flatMap(c=>[c.charCodeAt(0)&255,c.charCodeAt(0)>>8]),0,0]);
const snapshot=t=>new Uint8Array(core.memory.buffer,core.wire_state(t.handle),stateLayout[0]).slice();
const read=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
async function machine(mode,reject=false){
 let handle;const wall={calls:0};const vm=new WasmMachine({...core,wire_create(...a){handle=core.wire_create(...a);return handle;}},{mode,clock(){throw new Error('unexpected elapsed clock');},wallClock(){wall.calls++;return 123n;}});
 vm.supplyFile('C:/sink.bin',[]);let main;if(reject){vm.supplyModule('bad.dll',dynamicFixture(mode,{id:2,attach:false}).bytes);main=moduleFixture(mode,{dependency:'bad.dll'});}else{main=delayFixture(mode);main.put(main.dirs+13*8,0);main.put(main.dirs+13*8+4,0);}const image=vm.load(main.bytes);if(!reject)equal((await vm.run(100n)).reason,Exit.Returned);vm.map(0x20000n,0x20000n,7);
 return {vm,handle,mode,image,wall,sp:image.sp-2048n,error:image.teb+BigInt(mode===32?0x34:0x68),invalid:(1n<<BigInt(mode))-1n};
}
function frame(t,name,args=[]){const {vm,sp,mode}=t;vm.ip=vm.bind('kernel32.dll',name);vm.register(4,sp);vm.write(sp,encode(vm.entryPoints.returned,mode/8));for(let n=0;n<args.length;n++)if(mode===32)vm.write(sp+4n+BigInt(n*4),encode(BigInt.asUintN(32,args[n]),4));else if(n<4)vm.register([1,2,8,9][n],BigInt.asUintN(64,args[n]));else vm.write(sp+40n+BigInt(n-4)*8n,encode(BigInt.asUintN(64,args[n]),8));}
function call(t,name,args=[]){const work=t.vm.work,retired=t.vm.retired;frame(t,name,args);equal(t.vm.service().reason,Exit.Dispatch);equal(t.vm.work,work+1n);equal(t.vm.retired,retired);return t.vm.register(0);}
const transfer=(t,name,h,count,p=0x21000n,out=0x20100n)=>call(t,name,[h,p,count,out,0n]);
function pipe(t,size=0n,sa=0n,out=0x20200n){equal(call(t,'CreatePipe',[out,out+BigInt(t.mode/8),sa,size]),1n);return [t.vm.readInt(out,t.mode/8),t.vm.readInt(out+BigInt(t.mode/8),t.mode/8)];}
function copy(t,h,desired=0n,options=2n,out=0x20300n){equal(call(t,'DuplicateHandle',[t.invalid,h,t.invalid,out,desired,0n,options]),1n);return t.vm.readInt(out,t.mode/8);}

for(const mode of [32,64]){
 const t=await machine(mode,true),{vm}=t,ip=vm.ip,regs=Array.from({length:mode===32?8:16},(_,r)=>vm.register(r)),[r,w]=pipe(t),alias=copy(t,0x101n);equal(call(t,'SetStdHandle',[0xfffffff5n,alias]),1n);vm.write(0x21000n,[111,107]);equal(transfer(t,'WriteFile',alias,2n),1n);vm.writeInput([1,2,3]);for(let n=0;n<regs.length;n++)vm.register(n,regs[n]);vm.ip=ip;await assert.rejects(vm.run(1000n),/attach returned FALSE/);equal(vm.readOutput(),Uint8Array.from([111,107]));for(const h of [r,w,alias,0x100n]){equal(call(t,'GetFileType',[h]),0n);equal(vm.readInt(t.error),6n);}equal(call(t,'GetStdHandle',[0xfffffff5n]),alias);assert.throws(()=>vm.writeInput([4]),/error 87/);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,ptr=mode/8,params=vm.readInt(t.image.peb+BigInt(mode===32?0x10:0x20),ptr),std=params+BigInt(mode===32?0x18:0x20);
 for(const [slot,h] of [[0xfffffff6n,0x100n],[0xfffffff5n,0x101n],[0xfffffff4n,0x102n]]){equal(call(t,'GetStdHandle',[slot|0xabcd000000000000n]),h);equal(call(t,'GetFileType',[h]),3n);}equal([0,1,2].map(n=>vm.readInt(std+BigInt(n*ptr),ptr)),[0x100n,0x101n,0x102n]);
 vm.write(0x21000n,[65,0,255]);equal(transfer(t,'WriteFile',0x101n,3n),1n);equal(vm.readInt(0x20100n),3n);equal(transfer(t,'WriteFile',0x102n,3n,0x21000n,0n),1n);equal(vm.readOutput(1,0),new Uint8Array());equal(vm.readOutput(1,2),Uint8Array.from([65,0]));equal(vm.readOutput(1),Uint8Array.from([255]));equal(vm.readOutput(2),Uint8Array.from([65,0,255]));equal(vm.readOutput(1),new Uint8Array());
 const value=mode===32?0x12345678n:0xabcdef0123456789n;equal(call(t,'SetStdHandle',[0xfffffff5n,value]),1n);equal(call(t,'GetStdHandle',[0xfffffff5n]),value);equal(vm.readInt(std+BigInt(ptr),ptr),value);equal(call(t,'GetFileType',[value]),0n);equal(vm.readInt(t.error),6n);equal(call(t,'GetStdHandle',[0n]),t.invalid);equal(vm.readInt(t.error),87n);equal(call(t,'SetStdHandle',[0n,1n]),0n);
 const alias=copy(t,0x101n);equal(call(t,'CloseHandle',[0x101n]),1n);equal(call(t,'SetStdHandle',[0xfffffff5n,alias]),1n);equal(transfer(t,'WriteFile',alias,3n),1n);equal(call(t,'GetHandleInformation',[alias,0x20300n]),1n);equal(vm.readInt(0x20300n),0n);equal(call(t,'SetHandleInformation',[alias,2n,2n]),1n);equal(call(t,'CloseHandle',[alias]),0n);equal(vm.readInt(t.error),6n);equal(call(t,'SetHandleInformation',[alias,2n,0n]),1n);equal(call(t,'CloseHandle',[alias]),1n);equal(call(t,'GetStdHandle',[0xfffffff5n]),alias);equal(call(t,'GetFileType',[alias]),0n);equal(vm.readOutput(),Uint8Array.from([65,0,255]));
 equal(core.wire_protect(t.handle,0x22000n,4096n,0x104),0);equal(call(t,'GetConsoleMode',[0x100n,0x22000n]),0n);equal(vm.readInt(t.error),6n);assert.throws(()=>vm.readInt(0x22000n),/memory access/);equal(call(t,'WriteConsoleW',[0x102n,t.invalid,4n,0x20100n,0n]),0n);equal(t.wall.calls,0);
 if(mode===32)equal(call(t,'SetHandleCount',[0xabcdef89n]),0xabcdef89n);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.write(0x20100n,encode(0xa5a5a5a5n,4));frame(t,'ReadFile',[0x100n,0x21000n,3n,0x20100n,0n]);const before=snapshot(t);equal(vm.service().reason,Exit.Waiting);equal(snapshot(t),before);equal((await vm.run(0n)).reason,Exit.Budget);equal(snapshot(t),before);const paused=await vm.run(100n);equal(paused.reason,Exit.Waiting);equal(paused.work,0n);equal(snapshot(t),before);equal(vm.readInt(0x20100n),0xa5a5a5a5n);
 const bytes=Uint8Array.from([1,2,3,4]);vm.writeInput(bytes);bytes.fill(0);equal((await vm.run(1n)).reason,Exit.Budget);equal(read(vm,0x21000n,3),Uint8Array.from([1,2,3]));equal(vm.readInt(0x20100n),3n);equal(transfer(t,'ReadFile',0x100n,3n),1n);equal(vm.readInt(0x20100n),1n);equal(vm.readInt(0x21000n,1),4n);vm.writeInput([],{end:true});equal(transfer(t,'ReadFile',0x100n,1n),1n);equal(vm.readInt(0x20100n),0n);equal(transfer(t,'ReadFile',0x100n,0n,t.invalid),1n);assert.throws(()=>vm.writeInput([5]),/error 87/);equal(t.wall.calls,0);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,params=vm.readInt(t.image.peb+BigInt(mode===32?0x10:0x20),mode/8);frame(t,'SetStdHandle',[0xfffffff5n,0x102n]);equal(core.wire_protect(t.handle,params&~4095n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,params&~4095n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(call(t,'GetStdHandle',[0xfffffff5n]),0x102n);
 if(mode===64){vm.write(t.image.peb+0x20n,encode(t.invalid,8));frame(t,'SetStdHandle',[0xfffffff5n,0x101n]);const invalid=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),invalid);vm.write(t.image.peb+0x20n,encode(params,8));equal(call(t,'GetStdHandle',[0xfffffff5n]),0x102n);}
 vm.write(0x20000n,wide('C:/sink.bin'));const file=call(t,'CreateFileW',[0x20000n,0xc0000000n,7n,0n,3n,0x80n,0n]);equal(call(t,'SetStdHandle',[0xfffffff5n,file]),1n);equal(call(t,'GetStdHandle',[0xfffffff5n]),file);vm.write(0x21000n,[9]);equal(transfer(t,'WriteFile',file,1n),1n);equal(vm.readOutput(),new Uint8Array());equal(t.wall.calls,1);equal(call(t,'CloseHandle',[file]),1n);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,sa=0x20400n;vm.write(sa,new Uint8Array(24));vm.write(sa,encode(BigInt(mode===32?12:24),4));vm.write(sa+BigInt(mode===32?8:16),encode(1n,4));const [reader,writer]=pipe(t,0n,sa,t.sp);equal(vm.ip,vm.entryPoints.returned);for(const h of [reader,writer]){equal(call(t,'GetHandleInformation',[h,0x20300n]),1n);equal(vm.readInt(0x20300n),1n);equal(call(t,'GetFileType',[h]),3n);}
 vm.write(0x21000n,[97,98,99]);equal(transfer(t,'WriteFile',writer,3n),1n);equal(transfer(t,'ReadFile',reader,2n),1n);equal(read(vm,0x21000n,2),Uint8Array.from([97,98]));const alias=copy(t,writer,0n,3n);equal(call(t,'GetFileType',[writer]),0n);vm.write(0x21000n,[100]);equal(transfer(t,'WriteFile',alias,1n),1n);equal(transfer(t,'ReadFile',reader,9n),1n);equal(vm.readInt(0x20100n),2n);equal(read(vm,0x21000n,2),Uint8Array.from([99,100]));equal(call(t,'CloseHandle',[alias]),1n);equal(transfer(t,'ReadFile',reader,1n),0n);equal(vm.readInt(t.error),109n);equal(vm.readInt(0x20100n),0n);equal(transfer(t,'ReadFile',reader,0n,t.invalid),1n);equal(call(t,'CloseHandle',[reader]),1n);
 const [r,w]=pipe(t),metadata=copy(t,r,0n,0n);equal(call(t,'CloseHandle',[r]),1n);equal(transfer(t,'WriteFile',w,1n),1n);equal(transfer(t,'ReadFile',metadata,1n),0n);equal(vm.readInt(t.error),5n);equal(call(t,'CloseHandle',[metadata]),1n);equal(transfer(t,'WriteFile',w,1n),0n);equal(vm.readInt(t.error),109n);equal(call(t,'CloseHandle',[w]),1n);equal(t.wall.calls,0);vm.dispose();
}

// Every output is preflighted before endpoint allocation or queue consumption.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,cross=0x21ffdn;vm.write(0x20200n,encode(0xa5a5a5a5n,4));frame(t,'CreatePipe',[0x20200n,cross,0n,0n]);equal(core.wire_protect(t.handle,0x22000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(0x20200n),0xa5a5a5a5n);equal(core.wire_protect(t.handle,0x22000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);const r=vm.readInt(0x20200n,mode/8),w=vm.readInt(cross,mode/8);equal([r,w],[0x10000n,0x10001n]);vm.write(0x21000n,[11,12,13]);equal(transfer(t,'WriteFile',w,3n),1n);frame(t,'ReadFile',[r,cross,4n,0x20100n,0n]);equal(core.wire_protect(t.handle,0x22000n,4096n,2),0);const reading=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),reading);equal(core.wire_protect(t.handle,0x22000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(read(vm,cross,3),Uint8Array.from([11,12,13]));
 frame(t,'DuplicateHandle',[t.invalid,w,t.invalid,cross,0n,0n,3n]);equal(core.wire_protect(t.handle,0x22000n,4096n,2),0);const alias=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),alias);equal(core.wire_protect(t.handle,0x22000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);const copied=vm.readInt(cross,mode/8);equal(copied,0x10002n);equal(call(t,'CloseHandle',[copied]),1n);equal(call(t,'CloseHandle',[r]),1n);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,[r,w]=pipe(t);vm.write(0x21000n,new Uint8Array(4096).fill(7));equal(transfer(t,'WriteFile',w,4096n),1n);frame(t,'WriteFile',[w,0x21000n,1n,0x20100n,0n]);const before=snapshot(t);equal(vm.service().reason,Exit.Waiting);equal(snapshot(t),before);equal(transfer(t,'ReadFile',r,4096n,0x23000n),1n);equal(vm.readInt(0x23000n,1),7n);equal(transfer(t,'WriteFile',w,1n),1n);equal(transfer(t,'WriteFile',w,4097n),0n);equal(vm.readInt(t.error),8n);for(const h of [r,w])equal(call(t,'CloseHandle',[h]),1n);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,base=0x1000000n,capacity=8*1024*1024;vm.map(base,BigInt(capacity),3);equal(transfer(t,'WriteFile',0x101n,BigInt(capacity),base),1n);vm.write(0x21000n,[99]);frame(t,'WriteFile',[0x101n,0x21000n,1n,0x20100n,0n]);const before=snapshot(t);equal(vm.service().reason,Exit.Waiting);equal(snapshot(t),before);equal(vm.readOutput(1,1),Uint8Array.from([0]));equal(vm.service().reason,Exit.Dispatch);let total=0,last;for(let n=0;n<8;n++){const chunk=vm.readOutput(1,1024*1024);total+=chunk.length;last=chunk.at(-1);}equal(total,capacity);equal(last,99);equal(vm.readOutput(),new Uint8Array());equal(t.wall.calls,0);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,code=0x30000n;vm.write(code,[0xb8,1,0,0,0,0xc3]);vm.ip=code;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,mode/8));equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),1n);vm.writeInput([0xb8,9,0,0,0,0xc3],{end:true});equal(transfer(t,'ReadFile',0x100n,6n,code),1n);vm.ip=code;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,mode/8));equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),9n);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,capacity=8*1024*1024,base=0x1000000n;vm.map(base,BigInt(capacity),3);vm.writeInput(new Uint8Array(capacity).fill(11));assert.throws(()=>vm.writeInput([12],{end:true}),/error 8/);equal(transfer(t,'ReadFile',0x100n,BigInt(capacity),base),1n);equal(vm.readInt(base+BigInt(capacity-1),1),11n);frame(t,'ReadFile',[0x100n,0x21000n,1n,0x20100n,0n]);equal(vm.service().reason,Exit.Waiting);vm.writeInput([],{end:true});equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(0x20100n),0n);
 frame(t,'WriteFile',[0x101n,t.invalid,0n,0n,0n]);const page=t.sp&~4095n;equal(core.wire_protect(t.handle,page,4096n,0x104),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.service().reason,Exit.Dispatch);equal(vm.readOutput(),new Uint8Array());equal(t.wall.calls,0);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,handles=[];for(let n=0;n<128;n++)handles.push(copy(t,0x101n));equal(call(t,'DuplicateHandle',[t.invalid,0x101n,t.invalid,0x20300n,0n,0n,2n]),0n);equal(vm.readInt(t.error),8n);for(const h of handles)equal(call(t,'CloseHandle',[h]),1n);const restored=copy(t,0x101n);assert.ok(!handles.includes(restored));equal(call(t,'CloseHandle',[restored]),1n);
 const large=[];for(let n=0;n<4;n++)large.push(pipe(t,16n*1024n*1024n));equal(call(t,'CreatePipe',[0x20200n,0x20208n,0n,16n*1024n*1024n]),0n);equal(vm.readInt(t.error),8n);for(const pair of large)for(const h of pair)equal(call(t,'CloseHandle',[h]),1n);const list=[];for(let n=0;n<1024;n++)list.push(pipe(t));equal(call(t,'CreatePipe',[0x20200n,0x20208n,0n,0n]),0n);equal(vm.readInt(t.error),8n);for(const pair of list)for(const h of pair)equal(call(t,'CloseHandle',[h]),1n);for(const h of pipe(t))equal(call(t,'CloseHandle',[h]),1n);vm.dispose();
}

// Peek observes the shared byte queue without draining it, including aliases and half-close.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,[r,w]=pipe(t),out=0x20400n,data=0x21000n;
 const peek=(h=r,p=data,n=2n,outputs=[out,out+4n,out+8n])=>call(t,'PeekNamedPipe',[h,p,n,...outputs]);
 for(const name of ['PeekNamedPipe','CancelIo','CancelIoEx']){
  equal(vm.bind('kernelbase.dll',name),vm.bind('kernel32.dll',name));
  assert.throws(()=>vm.bind('user32.dll',name),/unknown|migrated/);
 }
 vm.write(t.error,encode(321n,4));equal(peek(),1n);equal(read(vm,out,12),new Uint8Array(12));equal(vm.readInt(t.error),321n);
 vm.write(data,[11,12,13,14,15]);equal(transfer(t,'WriteFile',w,5n),1n);
 for(const h of [r,copy(t,r)]){equal(peek(h),1n);equal(read(vm,data,2),Uint8Array.from([11,12]));equal([0n,4n,8n].map(n=>vm.readInt(out+n)),[2n,5n,0n]);}
 equal(peek(r,0n,0xffffffffn),1n);equal([0n,4n,8n].map(n=>vm.readInt(out+n)),[0n,5n,0n]);
 equal(peek(r,t.invalid,0n,[0n,0n,0n]),1n);
 equal(peek(r,data,0xffffffff00000003n),1n);equal(vm.readInt(out),3n);equal(read(vm,data,3),Uint8Array.from([11,12,13]));
 equal(peek(r,data,0xffffffffn),1n);equal(vm.readInt(out),5n);
 const noRead=copy(t,r,0n,0n);for(const [h,error]of [[w,5n],[noRead,5n],[0xbadn,6n],...(mode===64?[[r|0x100000000n,6n]]:[])]){
  vm.write(out,new Uint8Array(12).fill(0xa5));equal(peek(h,t.invalid,2n),0n);equal(vm.readInt(t.error),error);equal(read(vm,out,12),new Uint8Array(12).fill(0xa5));
 }
 vm.write(0x20000n,wide('C:/sink.bin'));const file=call(t,'CreateFileW',[0x20000n,0x80000000n,7n,0n,3n,0n,0n]);equal(peek(file),0n);equal(vm.readInt(t.error),6n);
 for(const h of [r,w,noRead,file,0x100n,0x101n]){
  vm.write(t.error,encode(765n,4));equal(call(t,'CancelIo',[h]),1n);equal(vm.readInt(t.error),765n);
  for(const pointer of [0n,t.invalid]){equal(call(t,'CancelIoEx',[h,pointer]),0n);equal(vm.readInt(t.error),1168n);}
 }
 for(const name of ['CancelIo','CancelIoEx']){equal(call(t,name,[0xbadn,t.invalid]),0n);equal(vm.readInt(t.error),6n);}
 equal(call(t,'CloseHandle',[file]),1n);equal(call(t,'CancelIo',[file]),0n);equal(vm.readInt(t.error),6n);
 // Overlapping results follow buffer, copied count, available count, then message remainder.
 equal(peek(r,data,5n,[data,data,data]),1n);equal(vm.readInt(data),0n);equal(vm.readInt(data+4n,1),15n);
 equal(peek(r,t.sp,5n,[out,out+4n,out+8n]),1n);equal(vm.ip,vm.entryPoints.returned);equal(read(vm,t.sp,5),Uint8Array.from([11,12,13,14,15]));
 equal(call(t,'CloseHandle',[w]),1n);equal(peek(r),1n);equal(vm.readInt(out+4n),5n);equal(transfer(t,'ReadFile',r,9n),1n);equal(read(vm,data,5),Uint8Array.from([11,12,13,14,15]));equal(peek(r),0n);equal(vm.readInt(t.error),109n);
 const [pending,writer]=pipe(t);frame(t,'ReadFile',[pending,data,1n,out,0n]);equal(vm.service().reason,Exit.Waiting);equal(call(t,'CancelIo',[pending]),1n);equal(call(t,'CancelIoEx',[pending,0n]),0n);equal(vm.readInt(t.error),1168n);frame(t,'ReadFile',[pending,data,1n,out,0n]);equal(vm.service().reason,Exit.Waiting);equal(transfer(t,'WriteFile',writer,1n),1n);equal(transfer(t,'ReadFile',pending,1n),1n);
 equal(peek(0x100n),1n);equal(vm.readInt(out+4n),0n);vm.writeInput([21,22,23]);equal(peek(0x100n),1n);equal(read(vm,data,2),Uint8Array.from([21,22]));equal(vm.readInt(out+4n),3n);equal(transfer(t,'ReadFile',0x100n,3n),1n);equal(read(vm,data,3),Uint8Array.from([21,22,23]));vm.writeInput([],{end:true});equal(peek(0x100n),0n);equal(vm.readInt(t.error),109n);equal(peek(0x101n),0n);equal(vm.readInt(t.error),5n);equal(t.wall.calls,0);vm.dispose();
}

// A late output fault cannot publish an earlier buffer/count or consume queue data.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,[r,w]=pipe(t),cross=0x21ffen,buffer=0x23000n,out=0x20400n;
 vm.write(0x21000n,[31,32,33]);equal(transfer(t,'WriteFile',w,3n),1n);
 for(const index of [1,3,4,5]){
  vm.write(buffer,new Uint8Array(8).fill(0xa5));vm.write(out,new Uint8Array(12).fill(0x5a));
  const args=[r,buffer,3n,out,out+4n,out+8n];args[index]=cross;frame(t,'PeekNamedPipe',args);equal(core.wire_protect(t.handle,0x22000n,4096n,2),0);
  const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(read(vm,buffer,8),new Uint8Array(8).fill(0xa5));equal(read(vm,out,12),new Uint8Array(12).fill(0x5a));
  equal(core.wire_protect(t.handle,0x22000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),1n);
 }
 frame(t,'PeekNamedPipe',[r,buffer,3n,out,out+4n,out+8n]);const page=t.sp&~4095n;equal(core.wire_protect(t.handle,page,4096n,0x104),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.service().reason,Exit.Dispatch);equal(read(vm,buffer,3),Uint8Array.from([31,32,33]));
 for(const name of ['PeekNamedPipe','CancelIo','CancelIoEx']){
  frame(t,name,name==='PeekNamedPipe'?[w,buffer,3n,out,out+4n,out+8n]:[0xbadn,t.invalid]);vm.write(out,new Uint8Array(12).fill(0x5a));equal(core.wire_protect(t.handle,t.error&~4095n,4096n,2),0);const invalid=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),invalid);equal(read(vm,out,12),new Uint8Array(12).fill(0x5a));equal(core.wire_protect(t.handle,t.error&~4095n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0n);
 }
 if(mode===64){const high=0x100020000n;vm.map(high,4096n,3);equal(call(t,'PeekNamedPipe',[r,high,3n,high+8n,high+12n,high+16n]),1n);equal(read(vm,high,3),Uint8Array.from([31,32,33]));equal([8n,12n,16n].map(n=>vm.readInt(high+n)),[3n,3n,0n]);frame(t,'PeekNamedPipe',[r,high+32n,3n,high+4094n,out,0n]);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(read(vm,high+32n,3),new Uint8Array(3));}
 equal(transfer(t,'ReadFile',r,3n),1n);equal(read(vm,0x21000n,3),Uint8Array.from([31,32,33]));equal(t.wall.calls,0);vm.dispose();
}

// All new exports bind in one PE and a six-argument call crosses the actual JIT/IAT boundary.
for(const mode of [32,64]){
 const names=['PeekNamedPipe','CancelIo','CancelIoEx'],f=fixture(mode,{tls:false});f.bytes.fill(0,f.offset(0x2000),f.offset(0x2600));f.rput(0x2000,0x2080);f.rput(0x200c,0x2180);f.rput(0x2010,0x21a0);f.rtext(0x2180,'kernelbase.dll\0');let at=0x22a0;
 for(const[i,name]of names.entries()){f.rput(0x2080+i*f.ptr,at,f.ptr);f.rput(0x21a0+i*f.ptr,at,f.ptr);f.rtext(at+2,name+'\0');at+=name.length+3;}
 if(mode===32){f.bytes.set([0x6a,0,0x6a,0,0x6a,0,0x6a,0,0x6a,0,0x6a,0,0xff,0x15,...encode(f.preferred+0x21a0n,4),0xc3],f.offset(0x1000));f.relocations.splice(0,f.relocations.length,0x100e);}else{
  const code=[0x48,0x83,0xec,56,0x31,0xc9,0x31,0xd2,0x45,0x31,0xc0,0x45,0x31,0xc9,0x48,0x89,0x4c,0x24,32,0x48,0x89,0x4c,0x24,40,0xff,0x15];code.push(...encode(0x21a0n-0x1000n-BigInt(code.length)-4n,4),0x48,0x83,0xc4,56,0xc3);f.bytes.set(code,f.offset(0x1000));f.relocations.length=0;
 }
 f.rebuildRelocations();const vm=new WasmMachine(core,{mode,clock(){throw new Error('unexpected clock');}}),image=vm.load(f.bytes);for(const[i,name]of names.entries())equal(vm.readInt(image.base+0x21a0n+BigInt(i*f.ptr),f.ptr),vm.bind('kernel32.dll',name));equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),0n);equal(vm.readInt(image.teb+BigInt(mode===32?0x34:0x68)),6n);assert.ok(vm.retired>0n);vm.dispose();
}
console.log(`${checks} standard streams/pipe aliases/peek/cancellation/waiting/fault/bounds checks passed`);
