import assert from 'node:assert/strict';
import {WasmMachine, Exit} from '../web/runtime.mjs';
import {core, stateLayout, encode} from './x87-harness.mjs';
import {fixture} from './pe-fixture.mjs';

let checks=0;
const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
const wide=s=>Uint8Array.from([...(s+'\0')].flatMap(c=>[c.charCodeAt(0)&255,c.charCodeAt(0)>>8]));
const park=0x2f000n, source=0x20000n, command=0x20400n, startup=0x22000n, output=0x22800n;
const load=(handle,address,size=4)=>BigInt.asUintN(64,core.wire_load(handle,address,size,1));
const text=(handle,address)=>{let s='';for(let n=0;n<1024;n++){const c=Number(load(handle,address+BigInt(n*2),2));if(!c)return s;s+=String.fromCharCode(c);}throw new Error('unterminated test string');};

function program(mode,child=false){
 const f=fixture(mode,{tls:child});
 f.relocations.splice(0,f.relocations.length,...f.relocations.filter(r=>r>=0x1100));
 f.bytes.fill(0x90,f.offset(0x1000),f.offset(0x1100));
 if(!child){f.put(f.dirs+8,0);f.put(f.dirs+12,0);f.bytes.set([0x31,0xc0,0xc3],f.offset(0x1000));f.rebuildRelocations();return f;}
 f.bytes.fill(0,f.offset(0x2040),f.offset(0x20c0));f.rput(0x2000,0x2040);f.rput(0x2010,0x2080);
 for(const [n,name] of ['GetCurrentProcessId','GetCurrentThreadId','GetStartupInfoW','GetEnvironmentVariableW','GetSystemTimes'].entries()){
  const at=0x2480+n*64;f.rput(0x2040+n*f.ptr,at,f.ptr);f.rput(0x2080+n*f.ptr,at,f.ptr);f.rtext(at+2,name+'\0');
 }
 f.bytes.set(wide('Marker'),f.offset(0x3040));
 let pc=0x1000;
 const emit=bytes=>{f.bytes.set(bytes,f.offset(pc));pc+=bytes.length;};
 const address=(opcode,rva)=>{const at=pc;emit([...opcode,...new Uint8Array(f.ptr)]);f.absolute(at+opcode.length,f.preferred+BigInt(rva));};
 const call=n=>{if(mode===32)address([0xff,0x15],0x2080+n*f.ptr);else{emit([0xff,0x15,...encode(BigInt.asUintN(32,BigInt(0x2080+n*f.ptr-pc-6)),4)]);}};
 const save=rva=>{if(mode===32)address([0xa3],rva);else emit([0x89,0x05,...encode(BigInt.asUintN(32,BigInt(rva-pc-6)),4)]);};
 if(mode===64)emit([0x48,0x83,0xec,40]);
 call(0);save(0x3020);call(1);save(0x3024);
 address(mode===32?[0x68]:[0x48,0xb9],0x3100);call(2);
 if(mode===32){emit([0x6a,64]);address([0x68],0x3200);address([0x68],0x3040);}
 else{address([0x48,0xb9],0x3040);address([0x48,0xba],0x3200);emit([0x41,0xb8,64,0,0,0]);}
 call(3);save(0x3028);
 if(mode===32){for(const rva of[0x3310,0x3308,0x3300])address([0x68],rva);}
 else{address([0x48,0xb9],0x3300);address([0x48,0xba],0x3308);address([0x49,0xb8],0x3310);}
 call(4);emit([0xb8,73,0,0,0]);if(mode===64)emit([0x48,0x83,0xc4,40]);emit([0xc3]);
 f.rebuildRelocations();return f;
}

async function machine(mode,childMode){
 const handles=[];
 const vm=new WasmMachine({...core,wire_create(...args){const h=core.wire_create(...args);handles.push(h);return h;}},
  {mode,clock:()=>0n,wallClock:()=>0n,cpuTimes:()=>[11n,22n,33n],random(){throw new Error('unexpected random source');}});
 vm.supplyFile('C:\\child.exe',program(childMode,true).bytes);
 const image=vm.load(program(mode).bytes);equal((await vm.run(100n)).reason,Exit.Returned);
 vm.map(0x20000n,0x30000n,7);vm.write(park,[0xeb,0xfe]);
 vm.write(source,wide('C:\\child.exe'));vm.write(command,wide('"custom argv0" --flag'));
 return {vm,mode,handles,image,sp:image.sp-1024n,error:image.teb+BigInt(mode===32?0x34:0x68)};
}
function frame(t,name,args=[],target=park){
 const {vm,sp,mode}=t;vm.ip=vm.bind('kernel32.dll',name);vm.register(4,sp);vm.write(sp,encode(target,mode/8));
 for(let n=0;n<args.length;n++)if(mode===32)vm.write(sp+4n+BigInt(n*4),encode(BigInt.asUintN(32,args[n]),4));
 else if(n<4)vm.register([1,2,8,9][n],args[n]);else vm.write(sp+40n+BigInt(n-4)*8n,encode(args[n],8));
}
function call(t,name,args=[]){const before=t.vm.work;frame(t,name,args);equal(t.vm.service().reason,Exit.Dispatch);equal(t.vm.work,before+1n);return t.vm.register(0);}
function inputs(t,flags=0n,environment=0n){
 const {vm,mode}=t,width=mode/8;vm.write(startup,new Uint8Array(104));vm.write(startup,encode(BigInt(mode===32?68:104),4));
 const scalar=mode===32?16n:32n;vm.write(startup+scalar,encode(111n,4));vm.write(startup+scalar+28n,encode(1n,4));
 vm.write(startup+scalar+32n,encode(5n,2));vm.write(0x23000n,wide('Child Window'));
 vm.write(startup+BigInt(width*3),encode(0x23000n,width));
 return [source,command,0n,0n,0n,flags,environment,0n,startup,output];
}

for(const mode of [32,64])for(const childMode of [32,64]){
 const t=await machine(mode,childMode),{vm}=t;
 vm.write(0x24000n,wide('Marker'));vm.write(0x24200n,wide('parent'));
 equal(call(t,'SetEnvironmentVariableW',[0x24000n,0x24200n]),1n);
 const suspended=mode===childMode,explicit=mode===64;
 if(explicit)vm.write(0x25000n,wide('Marker=child\0'));
 vm.write(t.error,encode(123n,4));const args=inputs(t,(suspended?4n:0n)|(explicit?0x400n:0n),explicit?0x25000n:0n);
 equal(call(t,'CreateProcessW',args),1n);equal(vm.readInt(t.error),123n);
 equal(vm.processes().length,2);equal(vm.processes()[1].id,2);equal(vm.processes()[1].mode,childMode);equal(vm.processes()[1].work,0n);
 const width=mode/8,process=vm.readInt(output,width),thread=vm.readInt(output+BigInt(width),width);
 equal(vm.readInt(output+BigInt(width*2)),2n);equal(vm.readInt(output+BigInt(width*2+4)),65537n);
 equal(call(t,'GetProcessId',[process]),2n);equal(call(t,'GetThreadId',[thread]),65537n);
 equal(call(t,'GetExitCodeProcess',[process,0x26000n]),1n);equal(vm.readInt(0x26000n),259n);
 if(suspended){vm.ip=park;equal((await vm.run(100n)).reason,Exit.Budget);equal(vm.processes()[1].work,0n);equal(call(t,'ResumeThread',[thread]),1n);}
 frame(t,'WaitForSingleObject',[process,0xffffffffn]);equal(vm.service().reason,Exit.Waiting);
 const before=vm.totalWork,waitingWork=vm.work;
 for(let n=0;n<1000;n++){const result=await vm.run(1n);equal(result.reason,Exit.Budget);if(vm.processes()[1].exitCode!==null)break;assert.ok(n<999);}
 assert.ok(vm.totalWork>before);equal(vm.processes()[1].exitCode,73);
 if(vm.work===waitingWork)equal((await vm.run(1n)).reason,Exit.Budget);
 equal(vm.work,waitingWork+1n);equal(vm.register(0),0n);equal(vm.ip,park);
 equal(vm.register(4),t.sp+BigInt(mode===32?12:8));
 const child=t.handles[1],info=Array.from(new BigUint64Array(core.memory.buffer,core.wire_image_info(child),10));
 equal(load(child,info[0]+0x3020n),2n);equal(load(child,info[0]+0x3024n),65537n);
 equal([0x3300n,0x3308n,0x3310n].map(o=>load(child,info[0]+o,8)),[11n,22n,33n]);
 equal(load(child,info[0]+0x3008n),4n);equal(text(child,info[0]+0x3200n),explicit?'child':'parent');
 equal(load(child,info[0]+0x3100n),BigInt(childMode===32?68:104));
 equal(load(child,info[0]+BigInt(childMode===32?0x3110:0x3120)),111n);
 const title=load(child,info[0]+0x3100n+BigInt(childMode/8*3),childMode/8);
 equal(text(child,title),'Child Window');equal(text(child,info[7]),'"custom argv0" --flag');
 if(mode===childMode)equal(vm.readInt(t.image.base+0x3020n),0n,'child memory is distinct from the same parent guest address');
 equal(call(t,'GetExitCodeProcess',[process,0x26000n]),1n);equal(vm.readInt(0x26000n),73n);
 equal(call(t,'GetExitCodeThread',[thread,0x26000n]),1n);equal(vm.readInt(0x26000n),73n);
 equal(call(t,'WaitForSingleObject',[thread,0n]),0n);
 const count=vm.totalWork;vm.dispose();assert.throws(()=>vm.totalWork,/disposed/);assert.ok(count>0n);
}

for(const mode of [32,64]){
 const t=await machine(mode,mode),{vm}=t,args=inputs(t);
 vm.write(startup+BigInt(mode/8*2),encode(source,mode/8));
 equal(call(t,'CreateProcessW',args),0n);equal(vm.readInt(t.error),50n);equal(vm.processes().length,1);
 vm.write(startup+BigInt(mode/8*2),encode(0n,mode/8));
 for(const [kind,error] of [['image',193n],['dependency',126n],['export',127n],['ordinal',127n]]){
  const f=fixture(mode,{tls:false});
  if(kind==='image')f.bytes[0]=0;
  else if(kind==='dependency')f.rtext(0x20c0,'absent.dll\0');
  else if(kind==='export')f.rtext(0x20e2,'MissingFixtureExport\0');
  else{f.rput(0x2080,(1n<<BigInt(mode-1))|23n,mode/8);f.rput(0x20a0,(1n<<BigInt(mode-1))|23n,mode/8);}
  const path=`C:\\bad-${kind}.exe`;vm.supplyFile(path,f.bytes);vm.write(source,wide(path));
  equal(call(t,'CreateProcessW',args),0n);equal(vm.readInt(t.error),error);equal(vm.processes().length,1);
 }
 vm.write(source,wide('C:\\child.exe'));
 frame(t,'CreateProcessW',args);const saved=new Uint8Array(core.memory.buffer,core.wire_state(t.handles[0]),stateLayout[0]).slice();
 equal(core.wire_protect(t.handles[0],output&~4095n,4096n,2),0);
 equal(vm.service().reason,Exit.Fault);equal(new Uint8Array(core.memory.buffer,core.wire_state(t.handles[0]),stateLayout[0]),saved);
 equal(vm.processes().length,1);equal(core.wire_child_mode(t.handles[0]),0);
 equal(core.wire_protect(t.handles[0],output&~4095n,4096n,4),0);
 equal(vm.service().reason,Exit.Dispatch);equal(vm.processes().length,2);equal(vm.processes()[1].id,2);
 const process=vm.readInt(output,mode/8);equal(call(t,'TerminateProcess',[process,91n]),1n);
 vm.ip=park;equal((await vm.run(100n)).reason,Exit.Budget);vm.ip=park;equal((await vm.run(100n)).reason,Exit.Budget);
 equal(vm.processes()[1].exitCode,91);equal(vm.processes()[1].retired,0n);
 equal(call(t,'GetExitCodeProcess',[process,0x26000n]),1n);equal(vm.readInt(0x26000n),91n);
 vm.dispose();
}

function ioProgram(mode,file,excluded,parentHandle){
 const f=program(mode);f.relocations.length=0;f.bytes.fill(0x90,f.offset(0x1000),f.offset(0x1400));
 f.put(f.dirs+8,0x2000);f.put(f.dirs+12,40);f.bytes.fill(0,f.offset(0x2040),f.offset(0x20c0));
 f.rput(0x2000,0x2040);f.rput(0x2010,0x2080);
 for(const [n,name] of ['GetStdHandle','WriteFile','ReadFile','GetHandleInformation','CloseHandle','GetStartupInfoW','GetProcessId'].entries()){
  const rva=0x2480+n*48;f.rput(0x2040+n*f.ptr,rva,f.ptr);f.rput(0x2080+n*f.ptr,rva,f.ptr);f.rtext(rva+2,name+'\0');
 }
 f.bytes.set([88,89],f.offset(0x3040));
 let pc=0x1000;const emit=code=>{f.bytes.set(code,f.offset(pc));pc+=code.length;};
 const absolute=(opcode,rva)=>{const at=pc;emit([...opcode,...new Uint8Array(f.ptr)]);f.absolute(at+opcode.length,f.preferred+BigInt(rva));};
 const relative=(opcode,rva)=>emit([...opcode,...encode(BigInt.asUintN(32,BigInt(rva-pc-opcode.length-4)),4)]);
 const save=rva=>mode===32?absolute([0xa3],rva):relative([0x48,0x89,0x05],rva);
 const invoke=(n,args)=>{
  if(mode===32)for(const arg of args.toReversed()){
   if(arg.saved)absolute([0xff,0x35],arg.saved);else if(arg.rva)absolute([0x68],arg.rva);else emit([0x68,...encode(arg,4)]);
  }else for(const [n,arg] of args.entries()){
   const opcode=n<4?[[0x48,0xb9],[0x48,0xba],[0x49,0xb8],[0x49,0xb9]][n]:[0x48,0xb8];
   if(arg.saved){assert.equal(n,0);relative([0x48,0x8b,0x0d],arg.saved);}
   else if(arg.rva)absolute(opcode,arg.rva);else emit([...opcode,...encode(arg,8)]);
   if(n>=4)emit([0x48,0x89,0x44,0x24,32+(n-4)*8]);
  }
  if(mode===32)absolute([0xff,0x15],0x2080+n*f.ptr);else relative([0xff,0x15],0x2080+n*f.ptr);
 };
 if(mode===64)emit([0x48,0x83,0xec,56]);
 invoke(0,[0xfffffff5n]);save(0x3020);
 invoke(1,[{saved:0x3020},{rva:0x3040},2n,{rva:0x3200},0n]);
 invoke(0,[0xfffffff6n]);save(0x3028);
 invoke(2,[{saved:0x3028},{rva:0x3100},1n,{rva:0x3200},0n]);
 invoke(3,[excluded,{rva:0x3200}]);save(0x3030);
 invoke(2,[file,{rva:0x3101},1n,{rva:0x3200},0n]);
 invoke(5,[{rva:0x3300}]);invoke(6,[parentHandle]);save(0x3038);invoke(4,[{saved:0x3020}]);
 emit([0xb8,73,0,0,0]);if(mode===64)emit([0x48,0x83,0xc4,56]);emit([0xc3]);
 f.rebuildRelocations();return f.bytes;
}

for(const mode of [32,64])for(const childMode of [32,64])for(const explicit of [false,true])for(const pipe of [false,true]){
 const t=await machine(mode,childMode),{vm}=t,width=mode/8,ptr=BigInt(width);
 const makePipe=()=>{equal(call(t,'CreatePipe',[0x27000n,0x27010n,0n,0n]),1n);return [vm.readInt(0x27000n,width),vm.readInt(0x27010n,width)];};
 const inherit=h=>equal(call(t,'SetHandleInformation',[h,1n,1n]),1n);
 const alias=h=>{equal(call(t,'DuplicateHandle',[t.mode===32?0xffffffffn:0xffffffffffffffffn,h,t.mode===32?0xffffffffn:0xffffffffffffffffn,0x27020n,0n,1n,2n]),1n);return vm.readInt(0x27020n,width);};
 let stdin,stdout,reader,writer;
 if(pipe){[reader,stdout]=makePipe();[stdin,writer]=makePipe();inherit(stdin);inherit(stdout);vm.write(0x27100n,[7]);equal(call(t,'WriteFile',[writer,0x27100n,1n,0x27110n,0n]),1n);}
 else{stdin=alias(0x100n);stdout=alias(0x101n);vm.writeInput([7,8]);}
 const [,excluded]=makePipe();inherit(excluded);
 vm.supplyFile('C:\\input.dat',[10,11,12]);vm.write(0x27200n,wide('C:\\input.dat'));
 const file=call(t,'CreateFileW',[0x27200n,0x80000000n,7n,0n,3n,0n,0n]);assert.ok(file!==BigInt.asUintN(mode,-1n));inherit(file);
 equal(call(t,'ReadFile',[file,0x27100n,1n,0x27110n,0n]),1n);equal(vm.readInt(0x27100n,1),10n);
 const parentHandle=call(t,'OpenProcess',[0x400n,1n,1n]);assert.ok(parentHandle);
 vm.supplyFile('C:\\io-child.exe',ioProgram(childMode,file,excluded,parentHandle));vm.write(source,wide('C:\\io-child.exe'));
 const args=inputs(t,0x20n|(explicit?0x80000n:0n));args[4]=1n;
 vm.write(startup+BigInt(mode===32?44:60),encode(0x100n,4));
 const stdAt=mode===32?56n:80n;for(const [n,h] of [stdin,stdout,stdout].entries())vm.write(startup+stdAt+BigInt(n)*ptr,encode(h,width));
 if(explicit){
  const list=0x28000n,values=0x28200n;vm.write(0x28100n,encode(4096n,width));
  equal(call(t,'InitializeProcThreadAttributeList',[list,1n,0n,0x28100n]),1n);
  for(const [n,h] of [stdin,stdout,file,parentHandle].entries())vm.write(values+BigInt(n)*ptr,encode(h,width));
  equal(call(t,'UpdateProcThreadAttribute',[list,0n,0x20002n,values,4n*ptr,0n,0n]),1n);
  const base=mode===32?68n:104n;vm.write(startup,encode(base+ptr,4));vm.write(startup+base,encode(list,width));
 }
 if(mode===64)args[5]|=1n<<32n;
 equal(call(t,'CreateProcessW',args),1n);equal(vm.processes().length,2);
 if(explicit){call(t,'DeleteProcThreadAttributeList',[0x28000n]);vm.write(0x28200n,new Uint8Array(width*4));}
 equal(call(t,'CloseHandle',[stdout]),1n);
 const process=vm.readInt(output,width);frame(t,'WaitForSingleObject',[process,0xffffffffn]);
 equal((await vm.run(10000n)).reason,Exit.Budget);equal(vm.processes()[1].exitCode,73);
 const child=t.handles[1],info=Array.from(new BigUint64Array(core.memory.buffer,core.wire_image_info(child),10));
 equal(load(child,info[0]+0x3020n,childMode/8),stdout);equal(load(child,info[0]+0x3028n,childMode/8),stdin);
 equal(load(child,info[0]+0x3038n),1n);equal(load(child,info[0]+0x3030n),explicit?0n:1n);equal(load(child,info[0]+0x3100n,1),7n);equal(load(child,info[0]+0x3101n,1),11n);
 const childStd=info[0]+BigInt(childMode===32?0x3338:0x3350);
 equal(load(child,childStd,childMode/8),stdin);equal(load(child,childStd+BigInt(childMode/8),childMode/8),stdout);
 equal(call(t,'ReadFile',[file,0x27100n,1n,0x27110n,0n]),1n);equal(vm.readInt(0x27100n,1),12n);
 if(pipe){equal(call(t,'ReadFile',[reader,0x27100n,8n,0x27110n,0n]),1n);equal(vm.readInt(0x27110n),2n);equal(vm.readInt(0x27100n,2),0x5958n);equal(call(t,'ReadFile',[reader,0x27100n,1n,0x27110n,0n]),0n);equal(vm.readInt(t.error),109n);}
 else{equal(vm.readOutput(),new Uint8Array([88,89]));equal(call(t,'ReadFile',[stdin,0x27100n,1n,0x27110n,0n]),1n);equal(vm.readInt(0x27100n,1),8n);}
 vm.dispose();
}


for(const mode of [32,64]){
 const t=await machine(mode,mode),{vm}=t,width=mode/8,base=mode===32?68n:104n,list=0x28000n,values=0x29000n;
 equal(call(t,'CreatePipe',[0x27000n,0x27010n,0n,0n]),1n);const writer=vm.readInt(0x27010n,width);
 const args=inputs(t,0x80024n);args[4]=1n;vm.write(startup,encode(base+BigInt(width),4));vm.write(startup+base,encode(list,width));
 const reset=()=>{vm.write(0x28100n,encode(4096n,width));equal(call(t,'InitializeProcThreadAttributeList',[list,1n,0n,0x28100n]),1n);};
 reset();equal(call(t,'UpdateProcThreadAttribute',[list,0n,0x20002n,values,BigInt(width),0n,0n]),1n);
 vm.write(values,encode(writer,width));equal(call(t,'CreateProcessW',args),0n);equal(vm.readInt(t.error),87n);equal(vm.processes().length,1);
 equal(call(t,'SetHandleInformation',[writer,1n,1n]),1n);
 vm.write(values,encode(BigInt.asUintN(mode,-1n),width));equal(call(t,'CreateProcessW',args),0n);equal(vm.readInt(t.error),87n);
 reset();vm.write(values,encode(writer,width));vm.write(values+BigInt(width),encode(writer,width));
 equal(call(t,'UpdateProcThreadAttribute',[list,0n,0x20002n,values,BigInt(width*2),0n,0n]),1n);
 equal(call(t,'CreateProcessW',args),0n);equal(vm.readInt(t.error),87n);
 reset();equal(call(t,'UpdateProcThreadAttribute',[list,0n,0x20007n,values,8n,0n,0n]),1n);
 equal(call(t,'CreateProcessW',args),0n);equal(vm.readInt(t.error),50n);equal(vm.processes().length,1);
 reset();equal(call(t,'UpdateProcThreadAttribute',[list,0n,0x20002n,values,BigInt(width),0n,0n]),1n);
 const event=call(t,'CreateEventW',[0n,0n,0n,0n]);assert.ok(event);equal(call(t,'SetHandleInformation',[event,1n,1n]),1n);
 const all=[...args];all[5]=0x24n;vm.write(startup,encode(base,4));
 equal(call(t,'CreateProcessW',all),0n);equal(vm.readInt(t.error),50n);equal(vm.processes().length,1);
 vm.write(startup,encode(base+BigInt(width),4));
 frame(t,'CreateProcessW',args);equal(core.wire_protect(t.handles[0],values,4096n,1),0);
 const saved=new Uint8Array(core.memory.buffer,core.wire_state(t.handles[0]),stateLayout[0]).slice();
 equal(vm.service().reason,Exit.Fault);equal(new Uint8Array(core.memory.buffer,core.wire_state(t.handles[0]),stateLayout[0]),saved);equal(vm.processes().length,1);
 equal(core.wire_protect(t.handles[0],values,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),1n);equal(vm.processes()[1].id,2);
 const process=vm.readInt(output,width);equal(call(t,'TerminateProcess',[process,0n]),1n);vm.ip=park;equal((await vm.run(100n)).reason,Exit.Budget);
 vm.dispose();
}


for(const mode of [32,64])for(const force of [false,true]){
 const t=await machine(mode,mode),{vm}=t;
 equal(call(t,'CreateProcessW',inputs(t)),1n);
 if(force){frame(t,'TerminateProcess',[BigInt.asUintN(mode,-1n),91n]);equal(vm.service().reason,Exit.ProcessExited);}
 else frame(t,'ExitProcess',[91n]);
 const result=await vm.run(10000n);equal(result.reason,Exit.ProcessExited);equal(result.exitCode,91);
 equal(vm.processes().map(p=>p.exitCode),[91,73]);assert.ok(vm.processes()[1].retired>0n);
 vm.dispose();
}

console.log(`${checks} process creation/inheritance/independent VM/environment/startup/wait/suspend/exit checks passed`);
