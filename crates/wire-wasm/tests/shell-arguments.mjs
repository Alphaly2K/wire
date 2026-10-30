import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
const wide=s=>Array.from({length:s.length+1},(_,n)=>[...encode(BigInt(n<s.length?s.charCodeAt(n):0),2)]).flat();
const local=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const argv=(t,input,count)=>call(t,'CommandLineToArgvW',[input,count],'shell32.dll');
function verify(t,pointer,expected){
 const {vm,mode}=t,width=mode/8,size=(expected.length+1)*width+expected.reduce((sum,arg)=>sum+(arg.length+1)*2,0);
 let offset=(expected.length+1)*width;
 for(const [n,arg] of expected.entries()){
  const p=vm.readInt(pointer+BigInt(n*width),width);equal(p,pointer+BigInt(offset));
  for(let i=0;i<=arg.length;i++)equal(vm.readInt(p+BigInt(i*2),2),BigInt(i<arg.length?arg.charCodeAt(i):0));
  offset+=(arg.length+1)*2;
 }
 equal(vm.readInt(pointer+BigInt(expected.length*width),width),0n);
 equal(local(t,'LocalSize',[pointer]),BigInt(size));equal(local(t,'LocalFree',[pointer]),0n);
}
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,input=0x22000n,count=0x24000n;
 vm.flags=0x8d7n;vm.mxcsr=0x5f81;vm.xmm(3,new Uint8Array(16).fill(0x79));
 const cases=[
  ['', ['C:\\program.exe']],['program',['program']],['  program arg',['','program','arg']],['\t',['']],
  ['"a b" c',['a b','c']],['program "" tail\\',['program','','tail\\']],
  ['program "a b',['program','a b']],['program a"b c"d',['program','ab cd']],
  ['program a""b',['program','ab']],['program a"""b',['program','a"b']],
  ['program "a""b"',['program','a"b']],['"a b"c',['a b','c']],
  ['program 日本語 😀',['program','日本語','😀']],['program \ud800 \udfff',['program','\ud800','\udfff']],
 ];
 for(let n=0;n<=10;n++)cases.push(['program '+'\\'.repeat(n)+'"x y"',n%2?['program','\\'.repeat((n-1)/2)+'"x','y']:['program','\\'.repeat(n/2)+'x y']]);
 for(const [line,expected] of cases){
  vm.write(input,wide(line));vm.write(t.error,encode(123n,4));const pointer=argv(t,input,count);assert.ok(pointer);equal(vm.readInt(count),BigInt(expected.length));verify(t,pointer,expected);equal(vm.readInt(t.error),123n);
 }
 vm.write(input,wide('program alias'));const pointer=argv(t,input,input);equal(vm.readInt(input),2n);verify(t,pointer,['program','alias']);
 vm.write(input,wide('program frame'));const alias=argv(t,input,t.sp);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp),2n);verify(t,alias,['program','frame']);
 vm.write(0x30000n,new Uint8Array(65536).fill(65));vm.write(count,encode(0xfeedn,4));equal(argv(t,0x30000n,count),0n);equal(vm.readInt(t.error),206n);equal(vm.readInt(count),0xfeedn);
 for(const dll of ['kernel32.dll','kernelbase.dll','ntdll.dll'])assert.throws(()=>vm.bind(dll,'CommandLineToArgvW'),/unknown/);
 equal(vm.flags,0x8d7n);equal(vm.mxcsr,0x5f81);equal(vm.xmm(3),new Uint8Array(16).fill(0x79));equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,input=0x22000n,count=0x24ffen;
 const spare=local(t,'LocalAlloc',[0n,256n]);equal(local(t,'LocalFree',[spare]),0n);
 vm.write(input,wide('program fault'));vm.write(count,encode(0xfeedn,4));
 frame(t,'CommandLineToArgvW',[input,count],'shell32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);let before=snapshot(t);
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(count),0xfeedn);
 equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),spare);verify(t,spare,['program','fault']);
 frame(t,'CommandLineToArgvW',[input,count],'shell32.dll');equal(core.wire_protect(t.handle,input,4096n,1),0);before=snapshot(t);
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,input,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),spare);verify(t,spare,['program','fault']);
 vm.write(count,encode(0xfeedn,4));frame(t,'CommandLineToArgvW',[input,count],'shell32.dll');equal(core.wire_protect(t.handle,spare,4096n,0x104),0);before=snapshot(t);
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(count),0xfeedn);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),spare);verify(t,spare,['program','fault']);
 if(mode===64){const high=local(t,'VirtualAlloc',[0n,4096n,0x103000n,4n]);assert.ok(high>0xffffffffn);vm.write(high,wide('program high'));verify(t,argv(t,high,high+512n),['program','high']);equal(vm.readInt(high+512n),2n);equal(local(t,'VirtualFree',[high,0n,0x8000n]),1n);}
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}
console.log(`${checks} shell argv/UTF-16/quoting/local allocation/ABI/fault checks passed`);
