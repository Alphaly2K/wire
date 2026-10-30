import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';

const kernel=['AttachConsole','SetConsoleCtrlHandler','ReadConsoleA','ReadConsoleW','GetDiskFreeSpaceExA','GetDiskFreeSpaceExW','GetOverlappedResult','GetOverlappedResultEx','CompareStringA','CompareStringW','CompareStringEx'];
const shell=['ShellExecuteA','ShellExecuteW','SHFileOperationA','SHFileOperationW'];
const api=(t,name,args=[])=>call(t,name,args,shell.includes(name)?'shell32.dll':'kernel32.dll');
const mark=t=>t.vm.write(t.error,encode(123n,4));
const error=(t,n)=>equal(t.vm.readInt(t.error),BigInt(n));
const fill=(t,p=0x24200n,n=32)=>t.vm.write(p,new Uint8Array(n).fill(0xa5));
const bytes=(t,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(t.vm.readInt(p+BigInt(i),1)));
function record(t,status,count,p=0x24000n){t.vm.write(p,encode(BigInt(status),t.mode/8));t.vm.write(p+BigInt(t.mode/8),encode(count,t.mode/8));return p;}
const args=(p,out=0x24200n,wait=0n)=>[1n,p,out,wait,1n];

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,clock=t.time.calls,wall=t.time.wallCalls;
 for(const name of [...kernel,...shell])assert.ok(vm.bind(shell.includes(name)?'shell32.dll':'kernel32.dll',name));
 // Finished IOSB queries use no live handle, event, APC or extra record fields.
 const tail=mode===32?0x24ff8n:0x24ff0n;
 const statuses=[[0,0],[0x80000005,234],[0xc0000008,6],[0xc000000d,87],[0xc0000011,38],[0xc0000022,5],[0xc000007f,112],[0xc00000b0,233],[0xc000014b,109],[0xc0000120,995],[0xc0070000,0],[0xc007006d,109]];
 for(const name of ['GetOverlappedResult','GetOverlappedResultEx'])for(const[status,code]of statuses){
  mark(t);fill(t);record(t,status,0xfedcba9876543210n,tail);equal(core.wire_protect(t.handle,0x25000n,4096n,1),0);
  equal(api(t,name,args(tail)),status===0?1n:0n);equal(vm.readInt(0x24200n),0x76543210n);error(t,status===0?123:code);
  equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);
 }
 for(const name of ['GetOverlappedResult','GetOverlappedResultEx'])for(const wait of [0n,1n,0xffffffffn]){
  vm.write(0x24ffcn,encode(0x103n,4));equal(core.wire_protect(t.handle,0x25000n,4096n,1),0);
  equal(api(t,name,args(0x24ffcn,1n,wait)),0n);error(t,wait===0n?996:50);
  equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);
 }
 fill(t);record(t,0xc1234567,100n);equal(api(t,'GetOverlappedResult',args(0x24000n)),0n);error(t,50);equal(bytes(t,0x24200n,32),new Uint8Array(32).fill(0xa5));
 // Output aliases are resolved from captured status, byte count and caller return.
 for(const out of [0x24000n,0x24000n+BigInt(mode/8),t.sp]){
  record(t,0,0x12345678n);frame(t,'GetOverlappedResult',args(0x24000n,out),'kernel32.dll');equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(out),0x12345678n);
 }
 record(t,0,99n);equal(core.wire_protect(t.handle,t.image.teb,4096n,1),0);equal(api(t,'GetOverlappedResult',args(0x24000n)),1n);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);
 // Unsupported OS services retain their caller data and publish no successful operation.
 for(const [name,a,code]of [['AttachConsole',[0xffffffffn],50],['SetConsoleCtrlHandler',[1n,1n],50],['SetConsoleCtrlHandler',[0n,1n],50],['ReadConsoleA',[0x100n,1n,99n,1n,1n],6],['ReadConsoleW',[0x100n,1n,99n,1n,1n],6]]){equal(api(t,name,a),0n);error(t,code);}
 for(const name of ['GetDiskFreeSpaceExA','GetDiskFreeSpaceExW']){
  fill(t);equal(api(t,name,[0n,0x24200n,0x24208n,0x24210n]),0n);error(t,50);equal(bytes(t,0x24200n,32),new Uint8Array(32).fill(0xa5));
  vm.write(0x24000n,Buffer.from('C:\\absent\0',name.endsWith('W')?'utf16le':'ascii'));equal(api(t,name,[0x24000n,1n,1n,1n]),0n);error(t,3);
 }
 for(const name of shell){mark(t);fill(t);equal(api(t,name,name.startsWith('SH')?[0x24200n]:Array(6).fill(1n)),name.startsWith('SH')?120n:31n);error(t,name.startsWith('SH')?123:1155);equal(bytes(t,0x24200n,32),new Uint8Array(32).fill(0xa5));}
 vm.write(0x24000n,Buffer.from('en-US\0','utf16le'));
 for(const name of ['CompareStringA','CompareStringW','CompareStringEx']){
  const locale=name.endsWith('Ex')?0x24000n:0x409n;
  for(const [flags,left,code]of [[0n,1n,50],[1n,1n,50],[0x80000000n,1n,1004],[0n,0n,87]]){equal(api(t,name,[locale,flags,left,0xffffffffn,1n,0xffffffffn,1n,1n,1n]),0n);error(t,code);}
 }
 equal(t.time.calls,clock);equal(t.time.wallCalls,wall);vm.dispose();
}

for(const mode of [32,64])for(const failure of ['status','count','output','error','return']){
 const t=await machine(mode),{vm}=t,p=0x24000n,out=0x24200n;
 fill(t);mark(t);record(t,failure==='error'?0xc0000022:0,99n);
 let page=0x25000n,protection=1,source=p,destination=out;
 if(failure==='status'){source=0x24ffen;vm.write(source,encode(0n,mode/8));}
 if(failure==='count'){source=mode===32?0x24ffcn:0x24ff8n;record(t,0,99n,source);}
 if(failure==='output'){destination=0x24ffen;fill(t,destination,8);protection=2;}
 if(failure==='error'){page=t.image.teb;protection=2;}
 if(failure==='return')page=t.sp&~4095n;
 frame(t,'GetOverlappedResult',args(source,destination),'kernel32.dll');equal(core.wire_protect(t.handle,page,4096n,protection),0);
 const before=snapshot(t),work=vm.work,retired=vm.retired;equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.work,work);equal(vm.retired,retired);equal(bytes(t,out,32),new Uint8Array(32).fill(0xa5));
 equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),failure==='error'?0n:1n);vm.dispose();
}

// Formal imports and a compiled guest query share the same completion path.
for(const mode of [32,64]){
 const f=fixture(mode,{tls:false}),groups=[['kernel32.dll',kernel],['shell32.dll',shell]],slots=[];
 f.bytes.fill(0,f.offset(0x2000),f.offset(0x2600));f.put(f.dirs+12,60);let at=0x203c;
 for(const[index,[dll,names]]of groups.entries()){
  const desc=0x2000+index*20;f.rput(desc+12,at);f.rtext(at,dll+'\0');at+=dll.length+1;at=Math.ceil(at/f.ptr)*f.ptr;
  const lookup=at;at+=(names.length+1)*f.ptr;const iat=at;at+=(names.length+1)*f.ptr;f.rput(desc,lookup);f.rput(desc+16,iat);
  for(const[i,name]of names.entries()){f.rput(lookup+i*f.ptr,at,f.ptr);f.rput(iat+i*f.ptr,at,f.ptr);f.rtext(at+2,name+'\0');at+=name.length+3;slots.push({dll,name,rva:iat+i*f.ptr});}
 }
 assert.ok(at<0x2600);f.relocations.length=0;f.rput(0x3000+f.ptr,0x12345678,f.ptr);const iat=BigInt(slots.find(s=>s.name==='GetOverlappedResult').rva),code=[];
 const absolute=(opcode,rva)=>{code.push(...opcode);f.relocations.push(0x1000+code.length);code.push(...encode(f.preferred+rva,f.ptr));};
 if(mode===32){code.push(0x6a,0);absolute([0x68],0x3040n);absolute([0x68],0x3000n);code.push(0x6a,0xff);absolute([0xff,0x15],iat);absolute([0xa1],0x3040n);code.push(0xc3);}
 else{code.push(0x48,0x83,0xec,40,0x48,0xc7,0xc1,0xff,0xff,0xff,0xff);absolute([0x48,0xba],0x3000n);absolute([0x49,0xb8],0x3040n);code.push(0x45,0x31,0xc9,0xff,0x15,...encode(iat-BigInt(0x1000+code.length+9),4));code.push(0x8b,0x05,...encode(0x3040n-BigInt(0x1000+code.length+6),4),0x48,0x83,0xc4,40,0xc3);}
 f.bytes.set(code,f.offset(0x1000));f.rebuildRelocations();const vm=new WasmMachine(core,{mode,clock(){throw Error('unexpected clock');}}),image=vm.load(f.bytes);
 for(const{dll,name,rva}of slots)equal(vm.readInt(image.base+BigInt(rva),f.ptr),vm.bind(dll,name));
 equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),0x12345678n);assert.ok(vm.retired>0n);vm.dispose();
}
console.log(`${checks} completed IOSB, unavailable OS service, ABI, fault and formal-import checks passed`);
