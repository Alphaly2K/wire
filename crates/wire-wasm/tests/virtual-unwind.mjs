import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,finish} from './gdi-harness.mjs';
import {dynamicFixture} from './dynamic-fixture.mjs';
const native=(t,name,args=[])=>call(t,name,args,'ntdll.dll');
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,width=mode/8,p=0x22000n,base=t.image.base;
 vm.write(t.error,encode(123n,4));
 for(const pc of [base,base+0x1000n,base+t.image.size-1n]){
  equal(native(t,'RtlPcToFileHeader',[pc,p]),base);equal(vm.readInt(p,width),base);
 }
 for(const pc of [0n,base-1n,base+t.image.size,0x30000n,vm.bind('kernel32.dll','GetLastError')]){
  equal(native(t,'RtlPcToFileHeader',[pc,p]),0n);equal(vm.readInt(p,width),0n);
 }
 equal(native(t,'RtlPcToFileHeader',[base,t.sp]),base);equal(vm.ip,vm.entryPoints.returned);
 const f=dynamicFixture(mode,{entry:false});vm.supplyModule('header.dll',f.bytes);
 vm.write(0x20000n,new TextEncoder().encode('header.dll\0'));
 frame(t,'LoadLibraryA',[0x20000n],'kernel32.dll');await finish(t);const dll=vm.register(0);assert.ok(dll);
 equal(native(t,'RtlPcToFileHeader',[dll+0x1000n,p]),dll);equal(vm.readInt(p,width),dll);
 frame(t,'FreeLibrary',[dll],'kernel32.dll');await finish(t);
 equal(native(t,'RtlPcToFileHeader',[dll+0x1000n,p]),0n);equal(vm.readInt(p,width),0n);
 const cross=0x24fffn;vm.write(cross,encode(0x55555555n,width));
 frame(t,'RtlPcToFileHeader',[base,cross],'ntdll.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);
 const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(cross,1),0x55n);
 equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(cross,width),base);
 equal(vm.readInt(t.error),123n);equal(t.time.calls,0);equal(t.time.wallCalls,0);
 for(const dll of ['kernel32.dll','kernelbase.dll'])equal(vm.bind(dll,'RtlPcToFileHeader'),vm.bind('ntdll.dll','RtlPcToFileHeader'));
 if(mode===32)assert.throws(()=>vm.bind('ntdll.dll','RtlVirtualUnwind'));
 vm.dispose();
}

const context=0x22000n,entry=0x27000n,info=0x28000n,code=0x30000n,stack=0x41000n;
const data=0x35000n,establisher=0x36000n,pointers=0x37000n,sentinel=0x5555555555555555n;
const put=(vm,p,value,width=8)=>vm.write(p,encode(value,width));
function seed(t,sp=stack,pc=code+0x20n){
 const {vm}=t;vm.write(context,new Uint8Array(1232).fill(0xa5));
 put(vm,context+48n,0n,4);put(vm,context+152n,sp);put(vm,context+248n,pc);
 vm.write(pointers,new Uint8Array(256).fill(0x55));put(vm,data,sentinel);put(vm,establisher,sentinel);
 put(vm,t.error,123n,4);
}
function metadata(t,codes,{flags=1,prolog=5,fp=0}={}){
 const {vm}=t;vm.write(code,new Uint8Array(512).fill(0x90));
 vm.write(entry,[...encode(code,4),...encode(code+0x100n,4),...encode(info,4)]);
 vm.write(info,[flags,prolog,codes.length/2,fp,...codes]);
 const tail=info+4n+BigInt(Math.ceil(codes.length/4)*4);
 put(vm,tail,code+0x180n,4);put(vm,tail+4n,0x12345678n,4);return tail+4n;
}
const args=(kind=1n,options={})=>[kind,options.base??0n,options.pc??code+0x20n,options.entry??entry,options.context??context,options.data??data,options.frame??establisher,options.pointers??pointers];
const unwind=(t,kind=1n,options={})=>native(t,'RtlVirtualUnwind',args(kind,options));

// Public unwinding accepts synthetic, unaligned stacks and opaque CONTEXT fields.
{
 const t=await machine(64),{vm}=t;
 for(const kind of [0n,1n,2n]){
  seed(t,stack+3n);put(vm,stack+3n,0xfedcba9876543210n);
  equal(unwind(t,kind,{entry:0n}),0n);equal(vm.readInt(context+248n,8),0xfedcba9876543210n);
  equal(vm.readInt(context+152n,8),stack+11n);equal(vm.readInt(establisher,8),stack+3n);
  equal(vm.readInt(data,8),kind===0n?sentinel:0n);equal(bytes(vm,pointers,256),new Uint8Array(256).fill(0x55));
  equal(vm.readInt(context+48n),0n);equal(vm.readInt(context+52n),0xa5a5a5a5n);
 }
 seed(t);put(vm,stack,73n);equal(unwind(t,0n,{entry:0n,data:0n,pointers:0xffffffffffffffffn}),0n);
 for(const dll of ['kernel32.dll','kernelbase.dll'])equal(vm.bind(dll,'RtlVirtualUnwind'),vm.bind('ntdll.dll','RtlVirtualUnwind'));
 equal(vm.readInt(t.error),123n);equal(t.time.calls,0);vm.dispose();
}

// Fixed-frame saves precede SET_FPREG and must survive a dynamic RSP adjustment.
{
 const t=await machine(64),{vm}=t;
 const payload=metadata(t,[20,0x68,2,0,15,0x34,2,0,10,3,5,0x72,1,0x50],{flags:9,prolog:20,fp:0x25});
 for(const kind of [0n,1n,2n,3n,0x100000001n]){
  seed(t,stack-0x800n);put(vm,context+160n,stack+32n);put(vm,stack+16n,0x123456789abcdef0n);
  vm.write(stack+32n,new Uint8Array(16).fill(0x6a));put(vm,stack+64n,0xabcdefn);put(vm,stack+72n,0x13000n);
  const old=bytes(vm,context,1232);equal(unwind(t,kind),kind&1n?code+0x180n:0n);
  equal(vm.readInt(context+144n,8),0x123456789abcdef0n);equal(vm.readInt(context+160n,8),0xabcdefn);
  equal(vm.readInt(context+152n,8),stack+80n);equal(vm.readInt(context+248n,8),0x13000n);
  equal(vm.readInt(pointers+128n+3n*8n,8),stack+16n);equal(vm.readInt(pointers+128n+5n*8n,8),stack+64n);
  equal(vm.readInt(pointers+6n*8n,8),stack+32n);equal(vm.readInt(pointers+128n+4n*8n,8),sentinel);
  equal(vm.readInt(establisher,8),stack);equal(vm.readInt(data,8),kind&1n?payload:sentinel);
  for(const [at,length] of [[0,120],[256,160],[672,560]])equal(bytes(vm,context+BigInt(at),length),old.slice(at,at+length));
  equal(bytes(vm,context+512n,16),new Uint8Array(16).fill(0x6a));
 }
 equal(vm.readInt(t.error),123n);equal(t.time.calls,0);vm.dispose();
}

// Only completed prologue operations and remaining epilogue pops have origins.
{
 const t=await machine(64),{vm}=t;
 const payload=metadata(t,[5,0x42,1,0x30],{flags:9});
 vm.write(code+0x50n,[0x48,0x83,0xc4,40,0x5b,0xc3]);
 for(const [offset,sp,restored,handler] of [[0,stack,false,false],[1,stack-8n,true,false],[5,stack-48n,true,true],[0x50,stack-48n,true,false],[0x54,stack-8n,true,false],[0x55,stack,false,false]]){
  seed(t,sp,code+BigInt(offset));put(vm,stack-8n,73n);put(vm,stack,0x13000n);
  equal(unwind(t,1n,{pc:code+BigInt(offset)}),handler?code+0x180n:0n);
  equal(vm.readInt(context+144n,8),restored?73n:0xa5a5a5a5a5a5a5a5n);
  equal(vm.readInt(context+152n,8),stack+8n);equal(vm.readInt(context+248n,8),0x13000n);
  equal(vm.readInt(pointers+152n,8),restored?stack-8n:sentinel);equal(vm.readInt(data,8),handler?payload:sentinel);
 }
 metadata(t,[],{flags:9,prolog:0});vm.write(code+0x20n,[0xc2,16,0]);seed(t);put(vm,stack,0x13000n);
 equal(unwind(t),code+0x180n);equal(vm.readInt(context+152n,8),stack+8n);
 metadata(t,[4,3,1,0x50],{prolog:4,fp:5});vm.write(code+0x50n,[0x5d,0xc3]);seed(t);put(vm,context+160n,stack-16n);put(vm,stack,73n);put(vm,stack+8n,0x13000n);
 equal(unwind(t,0n,{pc:code+0x50n}),0n);equal(vm.readInt(establisher,8),stack+8n);equal(vm.readInt(pointers+168n,8),stack);
 vm.dispose();
}

// Chained and indirect entries retain the origins from every metadata segment.
{
 const t=await machine(64),{vm}=t;
 metadata(t,[5,0x68,4,0,3,0xc4,2,0],{flags:0x21});
 vm.write(info+12n,[...encode(code,4),...encode(code+0x100n,4),...encode(info+0x50n,4)]);
 vm.write(info+0x50n,[9,5,3,0,5,1,32,0,1,0x30,0,0,...encode(code+0x180n,4)]);
 put(vm,stack+16n,0x12345678n);vm.write(stack+64n,new Uint8Array(16).fill(0x6a));put(vm,stack+256n,73n);put(vm,stack+264n,0x13000n);
 for(const indirect of [false,true]){
  if(indirect){put(vm,entry+8n,info+0x81n,4);vm.write(info+0x80n,[...encode(code,4),...encode(code+0x100n,4),...encode(info,4)]);}
  seed(t);equal(unwind(t),code+0x180n);equal(vm.readInt(context+144n,8),73n);equal(vm.readInt(context+216n,8),0x12345678n);
  equal(vm.readInt(context+152n,8),stack+272n);equal(vm.readInt(pointers+152n,8),stack+256n);equal(vm.readInt(pointers+224n,8),stack+16n);equal(vm.readInt(pointers+48n,8),stack+64n);
 }
 vm.dispose();
}

// Machine frames supply RIP/RSP without converting FLAGS or selectors for resume.
{
 const t=await machine(64),{vm}=t;
 for(const error of [false,true]){
  metadata(t,[0,error?0x1a:0x0a],{prolog:0});seed(t);const at=stack+(error?8n:0n);
  put(vm,at,0xfedcba9876543210n);put(vm,at+16n,0x247n);put(vm,at+24n,stack+0x800n);
  equal(unwind(t),0n);equal(vm.readInt(context+248n,8),0xfedcba9876543210n);equal(vm.readInt(context+152n,8),stack+0x800n);
  equal(vm.readInt(context+68n),0xa5a5a5a5n);equal(vm.readInt(context+56n,2),0xa5a5n);equal(bytes(vm,pointers,256),new Uint8Array(256).fill(0x55));
 }
 vm.dispose();
}

// Every output is preflighted before committing CONTEXT or another output.
for(const fault of ['context','frame','data','pointers','stack','metadata','guard']){
 const t=await machine(64),{vm}=t;
 metadata(t,[5,0x30],{flags:9});seed(t);put(vm,stack,73n);put(vm,stack+8n,0x13000n);
 const page={context,frame:establisher,data,pointers,stack,metadata:info,guard:pointers}[fault];
 const old=bytes(vm,context,1232);frame(t,'RtlVirtualUnwind',args(),'ntdll.dll');
 const before=snapshot(t);equal(core.wire_protect(t.handle,page,4096n,fault==='guard'?0x104:fault==='stack'||fault==='metadata'?1:2),0);
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(vm,context,1232),old);
 equal(vm.readInt(establisher,8),sentinel);equal(vm.readInt(data,8),sentinel);
 equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);
 equal(vm.readInt(context+144n,8),73n);equal(vm.readInt(context+248n,8),0x13000n);equal(vm.readInt(t.error),123n);vm.dispose();
}
{
 const t=await machine(64),{vm}=t;metadata(t,[5,0x30]);seed(t);put(vm,stack,73n);put(vm,stack+8n,0x13000n);
 equal(unwind(t,0n,{frame:t.sp,data:0n,pointers:0n}),0n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp,8),stack);
 seed(t);frame(t,'RtlVirtualUnwind',args(0n,{entry:0xfffffffffffffffcn}),'ntdll.dll');let before=snapshot(t);
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(establisher,8),sentinel);
 vm.write(info,[3]);frame(t,'RtlVirtualUnwind',args(),'ntdll.dll');before=snapshot(t);
 assert.throws(()=>vm.service(),/unwind|metadata/);equal(snapshot(t),before);equal(vm.readInt(establisher,8),sentinel);vm.dispose();
}
{
 const t=await machine(64),{vm}=t,base=0x100000000n;vm.map(base,0x10000n,7);
 vm.write(base+0x3000n,new Uint8Array(256).fill(0x90));
 vm.write(entry,[...encode(0x3000n,4),...encode(0x3100n,4),...encode(0x400n,4)]);
 vm.write(base+0x400n,[1,6,9,0,6,0x79,0x10,0x10,0,0,4,0xd5,0,0x10,0,0,2,0x11,8,0x20,0,0]);
 seed(t,0x40000n);put(vm,0x41000n,0x123456789abcdef0n);vm.write(0x41010n,new Uint8Array(16).fill(0x7a));put(vm,0x42008n,0x13000n);
 equal(unwind(t,0n,{base,pc:base+0x3020n}),0n);equal(vm.readInt(context+224n,8),0x123456789abcdef0n);
 equal(vm.readInt(context+152n,8),0x42010n);equal(vm.readInt(context+248n,8),0x13000n);
 equal(vm.readInt(pointers+232n,8),0x41000n);equal(vm.readInt(pointers+56n,8),0x41010n);
 equal(bytes(vm,context+528n,16),new Uint8Array(16).fill(0x7a));vm.dispose();
}
console.log(`${checks} mapped-header/supplied-CONTEXT/unwind-origins/handler/prologue/epilogue/chain/machine-frame/atomic-fault checks passed`);
