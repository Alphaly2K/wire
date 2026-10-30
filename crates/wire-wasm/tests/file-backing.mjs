import assert from 'node:assert/strict';
import {WasmMachine} from '../web/runtime.mjs';
import {applicationFixture} from './application-fixture.mjs';
import {moduleFixture} from './module-fixture.mjs';
import {delayFixture} from './delay-fixture.mjs';
import {dynamicFixture,trace} from './dynamic-fixture.mjs';
import {fileFixture,filePosition,fileMapOffset} from './file-fixture.mjs';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';

const pattern=(offset,length)=>Uint8Array.from({length},(_,n)=>Number((offset+BigInt(n))*37n&255n));
const text=(vm,at,value)=>vm.write(at,new TextEncoder().encode(value+'\0'));
const api=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const seek=(t,h,position)=>api(t,'SetFilePointerEx',t.mode===32?[h,position&0xffffffffn,position>>32n,0n,0n]:[h,position,0n,0n]);
async function io(t,name,args=[]){frame(t,name,args,'kernel32.dll');equal((await t.vm.run(1n)).reason,Exit.Budget);return t.vm.register(0);}
const read=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const wide=value=>Uint8Array.from([...value+'\0'].flatMap(c=>[c.charCodeAt(0)&255,c.charCodeAt(0)>>8]));
async function returned(vm){for(let n=0;n<1000;n++){const result=await vm.run(1000n);if(result.reason===Exit.Returned)return vm.register(0);equal(result.reason,Exit.Budget);}throw Error('file continuation did not return');}

for(const mode of[32,64]){
 const t=await machine(mode,{wallClock:()=>123456789n}),{vm}=t,requests=[],size=1n<<40n,before=core.memory.buffer.byteLength;
 vm.supplyFileSource('C:\\large.dat',size,async(offset,length)=>{requests.push([offset,length]);return pattern(offset,length);});
 equal(vm.fileStats(),{cachedBytes:0,ownedBytes:0,sources:1});assert.ok(core.memory.buffer.byteLength<=before+2*1024*1024);
 text(vm,0x20000n,'C:\\large.dat');const h=api(t,'CreateFileA',[0x20000n,0xc0000000n,7n,0n,3n,0n,0n]);assert.notEqual(h,(1n<<BigInt(mode))-1n);
 equal(api(t,'GetFileSizeEx',[h,0x21000n]),1n);equal(vm.readInt(0x21000n,8),size);equal(requests,[]);
 const high=(1n<<36n)+19n;equal(seek(t,h,high),1n);
 frame(t,'ReadFile',[h,0x22000n,33n,0x23000n,0n],'kernel32.dll');const state=snapshot(t),work=vm.work,retired=vm.retired;
 equal(vm.service().reason,Exit.FileInput);equal(snapshot(t),state);equal(vm.work,work);equal(vm.retired,retired);
 assert.throws(()=>{vm.ip=0n;},/pending file input/);vm.write(t.sp,encode(0xdeadn,mode/8));
 equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(0x23000n),33n);equal(read(vm,0x22000n,33),pattern(high,33));equal(requests.length,1);equal(vm.fileStats().cachedBytes,65536);
 const copied='C:\\copy.dat';text(vm,0x20100n,copied);equal(api(t,'CopyFileA',[0x20000n,0x20100n,0n]),1n);
 const copy=api(t,'CreateFileA',[0x20100n,0xc0000000n,7n,0n,3n,0n,0n]);equal(seek(t,copy,high),1n);
 vm.write(0x24000n,[9,7,5]);equal(await io(t,'WriteFile',[copy,0x24000n,3n,0x23000n,0n]),1n);equal(vm.fileStats().ownedBytes,65536);
 equal(seek(t,copy,high),1n);equal(await io(t,'ReadFile',[copy,0x22000n,4n,0x23000n,0n]),1n);equal(read(vm,0x22000n,4),Uint8Array.from([9,7,5,pattern(high+3n,1)[0]]));
 equal(seek(t,h,high),1n);equal(await io(t,'ReadFile',[h,0x22000n,4n,0x23000n,0n]),1n);equal(read(vm,0x22000n,4),pattern(high,4));
 const section=api(t,'CreateFileMappingA',[h,0n,2n,0n,0n,0n]),offset=(1n<<36n)+65536n*7n;
 const view=await io(t,'MapViewOfFile',[section,4n,offset>>32n,offset&0xffffffffn,64n]);assert.ok(view);equal(read(vm,view,64),pattern(offset,64));
 equal(seek(t,h,offset+3n),1n);vm.write(0x24000n,[1,2,3]);equal(await io(t,'WriteFile',[h,0x24000n,3n,0x23000n,0n]),1n);equal(read(vm,view+3n,3),new Uint8Array([1,2,3]));
 equal(api(t,'UnmapViewOfFile',[view]),1n);equal(api(t,'CloseHandle',[section]),1n);
 equal(seek(t,h,96n),1n);equal(api(t,'SetEndOfFile',[h]),1n);equal(seek(t,h,size),1n);equal(api(t,'SetEndOfFile',[h]),1n);
 const fetched=requests.length;equal(seek(t,h,high),1n);equal(await io(t,'ReadFile',[h,0x22000n,17n,0x23000n,0n]),1n);equal(read(vm,0x22000n,17),new Uint8Array(17));equal(requests.length,fetched);
 equal(api(t,'CloseHandle',[h]),1n);equal(api(t,'CloseHandle',[copy]),1n);equal(api(t,'DeleteFileA',[0x20100n]),1n);equal(vm.fileStats().ownedBytes,0);vm.dispose();
}

// Input failures preserve the saved frame; retry supplies the same immutable range.
for(const mode of[32,64]){
 const t=await machine(mode),{vm}=t;let bad=true,requests=0;
 vm.supplyFileSource('C:\\input',100000n,async(offset,length)=>{requests++;return bad?new Uint8Array():pattern(offset,length);});
 text(vm,0x20000n,'C:\\input');const h=api(t,'CreateFileA',[0x20000n,0x80000000n,7n,0n,3n,0n,0n]);
 frame(t,'ReadFile',[h,0x22000n,4n,0x23000n,0n],'kernel32.dll');const before=snapshot(t);await assert.rejects(vm.run(1n),/byte count/);equal(snapshot(t),before);bad=false;
 equal((await vm.run(1n)).reason,Exit.Budget);equal(read(vm,0x22000n,4),pattern(0n,4));equal(requests,2);vm.dispose();
}

// Static dependency input is fetched before publishing the initial image.
for(const mode of[32,64]){
 const {program,library}=applicationFixture(mode),vm=new WasmMachine(core,{mode}),requests=[];
 vm.supplyDirectory('C:\\empty\\nested');vm.supplyFileSource('C:\\library.dll',BigInt(library.bytes.length),async(offset,length)=>{requests.push([offset,length]);return library.bytes.slice(Number(offset),Number(offset)+length);});
 assert.throws(()=>vm.load(program.bytes),error=>error.fileInput);equal(vm.image,null);equal(vm.work,0n);
 await vm.loadAsync(program.bytes);equal(requests.length,1);equal((await vm.run(1000n)).reason,Exit.Returned);equal(vm.register(0),37n);equal(vm.fileStats().ownedBytes,0);vm.dispose();
}

// Dynamic loading uses the same suspended file input path and saved return.
for(const mode of[32,64]){
 const t=await machine(mode),{vm}=t,f=moduleFixture(mode,{dll:true,tls:true});let requests=0;
 vm.supplyFileSource('C:\\dynamic.dll',BigInt(f.bytes.length),async(offset,length)=>{requests++;return f.bytes.slice(Number(offset),Number(offset)+length);});text(vm,0x20000n,'dynamic.dll');
 frame(t,'LoadLibraryA',[0x20000n],'kernel32.dll');let result=await vm.run(1000n);equal(result.reason,Exit.Returned);assert.ok(vm.register(0));equal(requests,1);vm.dispose();
}

// Guest validation precedes host input, including fixed mappings and return frames.
for(const mode of[32,64]){
 const t=await machine(mode),{vm}=t;let requests=0;
 vm.supplyFileSource('C:\\unread',100000n,async()=>{requests++;throw Error('unexpected input');});text(vm,0x20000n,'C:\\unread');
 const h=api(t,'CreateFileA',[0x20000n,0x80000000n,7n,0n,3n,0n,0n]);
 for(const kind of['data','count','return']){
  frame(t,'ReadFile',[h,kind==='data'?0xdead0000n:0x22000n,4n,kind==='count'?0xdead0000n:0x23000n,0n],'kernel32.dll');
  if(kind==='return')equal(core.wire_protect(t.handle,t.sp&~4095n,4096n,1),0);
  const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(requests,0);
  if(kind==='return')equal(core.wire_protect(t.handle,t.sp&~4095n,4096n,4),0);
 }
 const section=api(t,'CreateFileMappingA',[h,0n,2n,0n,0n,0n]);
 equal(api(t,'MapViewOfFileEx',[section,4n,0n,0n,64n,0x20000n]),0n);equal(vm.readInt(t.error),487n);equal(requests,0);
 equal(api(t,'GetFileSizeEx',[h,0x23000n]),1n);equal(vm.readInt(0x23000n,8),100000n);vm.dispose();
}

// Awaiting a range locks the family; disposal never supplies data to a released VM.
for(const mode of[32,64])for(const dispose of[false,true]){
 const t=await machine(mode),{vm}=t;let release,started;const waiting=new Promise(resolve=>started=resolve);
 vm.supplyFileSource('C:\\waiting',65536n,(offset,length)=>{started();return new Promise(resolve=>release=()=>resolve(pattern(offset,length)));});
 text(vm,0x20000n,'C:\\waiting');const h=api(t,'CreateFileA',[0x20000n,0x80000000n,7n,0n,3n,0n,0n]);
 frame(t,'ReadFile',[h,0x22000n,4n,0x23000n,0n],'kernel32.dll');const before=snapshot(t),running=vm.run(1n);await waiting;
 await assert.rejects(vm.run(1n),/already running/);assert.throws(()=>vm.supplyFile('C:\\extra',[]),/already running/);assert.throws(()=>vm.supplyFileSource('C:\\extra',0n,()=>[]),/already running/);
 assert.throws(()=>vm.register(0,9n),/pending file input/);equal(snapshot(t),before);
 if(dispose){vm.dispose();release();await assert.rejects(running,/disposed/);equal(core.wire_state(t.handle),0);}
 else{release();equal((await running).reason,Exit.Budget);equal(read(vm,0x22000n,4),pattern(0n,4));vm.dispose();}
}

// Version parsing resumes after input and retains its Windows error protocol.
for(const mode of[32,64]){
 const t=await machine(mode),{vm}=t,f=moduleFixture(mode,{dll:true});let requests=0;
 vm.supplyFileSource('C:\\version.dll',BigInt(f.bytes.length),async(offset,length)=>{requests++;return f.bytes.slice(Number(offset),Number(offset)+length);});text(vm,0x20000n,'C:\\version.dll');
 vm.write(0x23000n,encode(123n,4));frame(t,'GetFileVersionInfoSizeA',[0x20000n,0x23000n],'version.dll');
 const before=snapshot(t);equal(vm.service().reason,Exit.FileInput);equal(snapshot(t),before);equal(vm.readInt(0x23000n),123n);
 equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.register(0),0n);equal(vm.readInt(t.error),1813n);equal(vm.readInt(0x23000n),0n);equal(requests,1);vm.dispose();
}

// Delay imports and late forwards can request multiple external dependencies.
for(const mode of[32,64]){
 const main=delayFixture(mode),vm=new WasmMachine(core,{mode}),requests=[];
 const leaf=dynamicFixture(mode,{id:1,forward:'target.#17',tls:true}),target=dynamicFixture(mode,{id:2,seed:73,tls:true});
 for(const[name,f]of[['leaf',leaf],['target',target]])vm.supplyFileSource(`C:\\${name}.dll`,BigInt(f.bytes.length),async(offset,length)=>{requests.push(name);return f.bytes.slice(Number(offset),Number(offset)+length);});
 const image=vm.load(main.bytes);equal((await vm.run(1000n)).reason,Exit.Returned);vm.map(0x20000n,0x10000n,7);
 equal(requests,[]);vm.ip=vm.readInt(image.base+0x3120n,mode/8);vm.register(4,image.sp-256n);vm.write(image.sp-256n,encode(vm.entryPoints.returned,mode/8));
 equal(await returned(vm),73n);equal(requests,['leaf','target']);equal(vm.readInt(trace+16n),1n);equal(vm.readInt(trace+32n),1n);vm.dispose();
}

// CreateProcess stages an external executable and shares its source with children.
for(const mode of[32,64])for(const childMode of[32,64]){
 const t=await machine(mode,{wallClock:()=>123n}),{vm}=t,f=fileFixture(childMode,{mapping:false}),ranges=[];let requests=0;
 vm.supplyFileSource('C:\\large.dat',1n<<40n,async(offset,length)=>{ranges.push([offset,length]);return pattern(offset,length);});
 vm.supplyFileSource('C:\\child.exe',BigInt(f.bytes.length),async(offset,length)=>{requests++;return f.bytes.slice(Number(offset),Number(offset)+length);});vm.write(0x20000n,wide('C:\\child.exe'));
 const startup=0x24000n,output=0x25000n;vm.write(startup,new Uint8Array(104));vm.write(startup,encode(BigInt(mode===32?68:104),4));
 frame(t,'CreateProcessW',[0x20000n,0n,0n,0n,0n,4n,0n,0n,startup,output],'kernel32.dll');const before=snapshot(t);
 equal(vm.service().reason,Exit.FileInput);equal(snapshot(t),before);equal(vm.processes().length,1);
 equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.register(0),1n);equal(vm.processes().length,2);equal(vm.processes()[1].mode,childMode);equal(requests,1);
 const thread=vm.readInt(output+BigInt(mode/8),mode/8);
 equal(api(t,'CreateProcessW',[0x20000n,0n,0n,0n,0n,4n,0n,0n,startup,output]),1n);equal(vm.processes().length,3);equal(requests,1);
 equal(api(t,'ResumeThread',[thread]),1n);
 for(let n=0;n<100 && vm.processes()[1].exitCode===null;n++){const result=await vm.run(1000n);assert.ok([Exit.Dispatch,Exit.Budget,Exit.Waiting].includes(result.reason),JSON.stringify(result,(_,value)=>typeof value==='bigint'?value.toString():value));}
 equal(vm.processes()[1].exitCode,9);equal(ranges,[[filePosition&~65535n,65536]]);
 vm.dispose();
}

// Both JITs execute the complete file program with independent expected bytes.
for(const mode of[32,64]){
 const vm=new WasmMachine(core,{mode}),size=1n<<40n;vm.supplyFileSource('C:\\large.dat',size,async(offset,length)=>pattern(offset,length));
 const image=vm.load(fileFixture(mode).bytes);equal((await vm.run(2000n)).reason,Exit.Returned);equal(vm.register(0),9n);
 equal(vm.readInt(image.base+0x3000n,8),size);equal(vm.readInt(image.base+0x3008n),33n);equal(vm.readInt(image.base+0x300cn),3n);equal(vm.readInt(image.base+0x3010n),4n);
 equal(read(vm,image.base+0x3020n,33),pattern(filePosition,33));equal(read(vm,image.base+0x3050n,4),new Uint8Array([9,7,5,pattern(filePosition+3n,1)[0]]));equal(read(vm,image.base+0x3080n,64),pattern(fileMapOffset,64));vm.dispose();
}
console.log(`${checks} sparse input/read/write/mapping/loader/ABI/transaction checks passed`);
