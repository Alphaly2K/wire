import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';
const names=['wsprintfA','wsprintfW','wvsprintfA','wvsprintfW'];
const spec=0x22000n,out=0x24000n,list=0x26000n;
const text=(vm,p,s,wide=false)=>vm.write(p,Buffer.from(s+'\0',wide?'utf16le':'ascii'));
const bytes=(vm,p,n)=>Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const expected=(s,wide)=>Array.from(Buffer.from(s+'\0',wide?'utf16le':'ascii'));
const render=(t,name,format,args,{destination=out,varargs=list}={})=>{
 const wide=name.endsWith('W'),v=name.startsWith('wv');text(t.vm,spec,format,wide);
 if(v){args.forEach((a,n)=>t.vm.write(varargs+BigInt(n*t.mode/8),encode(a,t.mode/8)));return call(t,name,[destination,spec,varargs]);}
 return call(t,name,[destination,spec,...args]);
};

// Fixed ABI parameters and lazy varargs use distinct PE32/AMD64 layouts.
for(const mode of [32,64])for(const name of names){
 const t=await machine(mode),{vm}=t,wide=name.endsWith('W'),v=name.startsWith('wv');
 for(const dll of ['kernel32.dll','kernelbase.dll'])assert.throws(()=>vm.bind(dll,name),/unknown/);assert.ok(vm.bind('user32.dll',name));
 vm.write(t.error,encode(123n,4));vm.flags=0x8d7n;vm.mxcsr=0x5f81;vm.xmm(3,new Uint8Array(16).fill(0x72));
 const args=[0x12n,0xffffffd6n,...(mode===32?[1n,1n]:[0x100000001n]),0x20000n];text(vm,0x20000n,'abc',wide);
 const result='00000012|-42|4294967297|abc|%';equal(render(t,name,'%08x|%d|%I64u|%s|%%',args),BigInt(result.length));equal(bytes(vm,out,expected(result,wide).length),expected(result,wide));
 equal(vm.ip,vm.entryPoints.returned);equal(vm.register(4),t.sp+BigInt(mode===32?(v?16:4):8));equal(vm.readInt(t.error),123n);equal(vm.flags,0x8d7n);equal(vm.mxcsr,0x5f81);equal(vm.xmm(3),new Uint8Array(16).fill(0x72));equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Integer widths, Windows padding, unknown specifiers and pointer rendering are format-controlled.
for(const mode of [32,64])for(const name of ['wsprintfA','wvsprintfW']){
 const t=await machine(mode),{vm}=t,wide=name.endsWith('W'),ptr=mode===32?'00000123':'1234567800000123';
 const wanted='-0007|0x00002a|9   |00003|'+ptr+'|f|%';equal(render(t,name,'%05d|%#06x|%-4u|%.5u|%p|%f|%%',[0xfffffff9n,42n,9n,3n,mode===32?0x123n:0x1234567800000123n]),BigInt(wanted.length));equal(bytes(vm,out,expected(wanted,wide).length),expected(wanted,wide));
 const args=mode===32?[0xffffffffn,0xffffffffn,0xffffffffn,0x89abcdefn,0xffffffffn]:[0xffffffffffffffffn,0xffffffffffffffffn,0x1234567889abcdefn,0xabcdef00ffffffffn];
 const result=mode===32?'-1|4294967295|89abcdef|4294967295':'-1|18446744073709551615|89abcdef|4294967295';
 equal(render(t,name,'%I64d|%Iu|%I32x|%lu',args),BigInt(result.length));equal(bytes(vm,out,expected(result,wide).length),expected(result,wide));
 vm.dispose();
}

// A/W specifiers preserve CP932 conversion, precision and null strings.
for(const mode of [32,64])for(const name of names){
 const t=await machine(mode),{vm}=t,wide=name.endsWith('W');vm.write(0x20000n,[0x83,0x5c,0x83,0x74,0x83,0x67,0]);text(vm,0x21000n,'ソフト',true);
 const args=wide?[0x21000n,0x20000n,0x20000n,0x21000n,0n]:[0x20000n,0x21000n,0x21000n,0x20000n,0n];
 const result='ソフト|ソフト|ソフト|ソフト|(null)';equal(render(t,name,'%s|%S|%'+(wide?'hs':'ls')+'|%'+(wide?'ls':'hs')+'|%s',args),BigInt(wide?result.length:34));
 const converted=wide?Array.from(Buffer.from(result+'\0','utf16le')):[...Array.from({length:4},()=>[0x83,0x5c,0x83,0x74,0x83,0x67,124]).flat(),...Buffer.from('(null)\0')];equal(bytes(vm,out,converted.length),converted);
 const short=wide?'ソ|ab':'a|ab';text(vm,0x21000n,wide?'ソフト':'abcd',wide);text(vm,0x20000n,'abcd',!wide);equal(render(t,name,'%.1s|%.2S',[0x21000n,0x20000n]),BigInt(short.length));equal(bytes(vm,out,expected(short,wide).length),expected(short,wide));vm.dispose();
}

// Literals and unsupported conversions never dereference unused arguments.
for(const mode of [32,64])for(const name of names){
 const t=await machine(mode),{vm}=t,wide=name.endsWith('W'),v=name.startsWith('wv');text(vm,spec,'ok|%f|%%',wide);
 if(v)frame(t,name,[out,spec,0x60000n]);else{
  if(mode===32)t.sp=0x4fff4n;
  else t.sp=0x4ffd8n;
  frame(t,name,[out,spec]);
 }
 equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),6n);equal(bytes(vm,out,expected('ok|f|%',wide).length),expected('ok|f|%',wide));vm.dispose();
}

// Format/source/varargs/output/return faults cannot commit a partial output or caller state.
for(const mode of [32,64])for(const kind of ['format','string','argument','wideArgument','output','return']){
 const t=await machine(mode),{vm}=t;vm.write(out,new Uint8Array(32).fill(0xa5));text(vm,spec,kind==='wideArgument'?'%I64u':'%u:%s');text(vm,0x20000n,'tail');
 let page=0x25000n,protection=2,destination=out;
 if(kind==='output')destination=0x24ffcn;
 if(kind==='wideArgument'){vm.write(0x4fffcn,encode(7n,4));frame(t,'wvsprintfA',[destination,spec,0x4fffcn]);protection=null;}
 else if(kind==='argument'){frame(t,'wvsprintfA',[destination,spec,0x50000n]);protection=null;}
 else frame(t,'wsprintfA',[destination,kind==='format'?0x50000n:spec,7n,kind==='string'?0x50000n:0x20000n]);
 if(kind==='format'||kind==='string')protection=null;
 if(kind==='return'){page=t.sp&~4095n;protection=1;}
 if(protection!==null)equal(core.wire_protect(t.handle,page,4096n,protection),0);
 const before=snapshot(t),output=bytes(vm,out,32),prefix=bytes(vm,0x24ffcn,4);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(vm,out,32),output);equal(bytes(vm,0x24ffcn,4),prefix);
 if(protection===null){vm.map(0x50000n,4096n,7);if(kind==='argument'){vm.write(0x50000n,encode(7n,mode/8));vm.write(0x50000n+BigInt(mode/8),encode(0x20000n,mode/8));}else if(kind==='format')text(vm,0x50000n,'%u:%s');else if(kind==='string')text(vm,0x50000n,'tail');else vm.write(0x50000n,encode(1n,4));}
 else equal(core.wire_protect(t.handle,page,4096n,4),0);
 equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),kind==='wideArgument'?10n:6n);vm.dispose();
}

// Snapshotting all inputs protects output aliases; the saved return precedes every write.
for(const mode of [32,64])for(const name of names){
 const t=await machine(mode),{vm}=t,wide=name.endsWith('W'),v=name.startsWith('wv');
 equal(render(t,name,'X',[],{destination:t.sp}),1n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp,wide?2:1),88n);
 equal(render(t,name,'%u-%u',[7n,9n],{destination:spec}),3n);equal(bytes(vm,spec,expected('7-9',wide).length),expected('7-9',wide));
 if(v){equal(render(t,name,'%u-%u',[7n,9n],{destination:list}),3n);equal(bytes(vm,list,expected('7-9',wide).length),expected('7-9',wide));}
 if(mode===64){const high=0x123400000000n;vm.map(high,12288n,7);text(vm,high,'%s:%p',wide);text(vm,high+4096n,'high',wide);const args=[high+4096n,high+4096n];if(v){args.forEach((a,n)=>vm.write(high+8192n+BigInt(n*8),encode(a,8)));equal(call(t,name,[high+8192n,high,high+8192n]),21n);}else equal(call(t,name,[high+8192n,high,...args]),21n);equal(bytes(vm,high+8192n,expected('high:0000123400001000',wide).length),expected('high:0000123400001000',wide));}
 vm.dispose();
}

// The existing bounded formatter writes a terminator and reports its saturation result.
for(const mode of [32,64])for(const name of ['wsprintfA','wvsprintfW']){
 const t=await machine(mode),{vm}=t,wide=name.endsWith('W');for(const n of [1022,1023,1100]){equal(render(t,name,'x'.repeat(n),[]),n>=1023?0xffffffffn:BigInt(n));equal(bytes(vm,out,(Math.min(n,1023)+1)*(wide?2:1)),expected('x'.repeat(Math.min(n,1023)),wide));}vm.dispose();
}

// Static imports and the actual JIT preserve PE32 cdecl vs stdcall and AMD64 register arguments.
for(const mode of [32,64])for(const listCall of [false,true]){
 const f=fixture(mode,{tls:false}),name=listCall?'wvsprintfA':'wsprintfA';f.rtext(0x20c0,'user32.dll\0');f.rtext(0x20e2,name+'\0');f.rtext(0x2500,'%u');f.rput(0x3100,42,f.ptr);
 const code=mode===32?[0x68,...encode(listCall?f.preferred+0x3100n:42n,4),0x68,...encode(f.preferred+0x2500n,4),0x68,...encode(f.preferred+0x3000n,4),0xff,0x15,...encode(f.preferred+0x20a0n,4),...(listCall?[]:[0x83,0xc4,12]),0xc3]:[0x48,0x83,0xec,0x28,0x48,0x8d,0x0d,...encode(BigInt(0x3000-0x100b),4),0x48,0x8d,0x15,...encode(BigInt(0x2500-0x1012),4),...(listCall?[0x4c,0x8d,0x05,...encode(BigInt(0x3100-0x1019),4)]:[0x41,0xb8,...encode(42n,4)]),0xff,0x15,...encode(BigInt(0x20a0-(listCall?0x101f:0x101e)),4),0x48,0x83,0xc4,0x28,0xc3];
 f.bytes.set(code,f.offset(0x1000));f.relocations.splice(0,f.relocations.length,...(mode===32?[...(listCall?[0x1001]:[]),0x1006,0x100b,0x1011]:[]));f.rebuildRelocations();
 const vm=new WasmMachine(core,{mode}),image=vm.load(f.bytes);equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),2n);equal(bytes(vm,image.base+0x3000n,3),[52,50,0]);vm.dispose();
}
console.log(`${checks} Windows printf/varargs/ABI/encoding/JIT/fault checks passed`);
