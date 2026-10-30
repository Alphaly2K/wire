import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,finish} from './gdi-harness.mjs';
import {moduleFixture} from './module-fixture.mjs';
import {fp} from './x87-harness.mjs';

const names=['EnumProcessModules','EnumProcessModulesEx','GetModuleInformation','GetModuleBaseNameA','GetModuleBaseNameW','GetModuleFileNameExA','GetModuleFileNameExW'];
const dll=name=>name.startsWith('K32')?'kernel32.dll':'psapi.dll';
const api=(t,name,args)=>call(t,name,args,dll(name));
const kernel=(t,name,args)=>call(t,name,args,'kernel32.dll');
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const text=(vm,p,s)=>vm.write(p,Buffer.from(s+'\0','utf16le'));
function enumeration(t,name='K32EnumProcessModules',filter=0n){
 equal(api(t,name,[-1n,0x24000n,512n,0x23000n,filter]),1n);const count=Number(t.vm.readInt(0x23000n))/ (t.mode/8);
 return Array.from({length:count},(_,n)=>t.vm.readInt(0x24000n+BigInt(n*t.mode/8),t.mode/8));
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,width=mode/8,out=0x24000n,needed=0x23000n,path='C:\\plugins\\leaf日.dll';
 for(const name of names)for(const prefix of ['', 'K32']){
  const entry=prefix+name;assert.ok(vm.bind(dll(entry),entry));
  for(const wrong of ['user32.dll',prefix?'psapi.dll':'kernel32.dll'])assert.throws(()=>vm.bind(wrong,entry),/unknown/);
 }
 const before=enumeration(t);equal(before[0],t.image.base);
 const leaf=moduleFixture(mode,{dll:true,tls:true});vm.supplyFile(path,leaf.bytes);text(vm,0x22000n,path);
 frame(t,'LoadLibraryW',[0x22000n],'kernel32.dll');await finish(t);const base=vm.register(0);assert.ok(base);
 const handles=enumeration(t);equal(handles.length,before.length+1);assert.ok(handles.includes(base));equal(handles[0],t.image.base);
 vm.flags=0x8d5n;vm.xmm(3,new Uint8Array(16).fill(0x7b));const fpu=fp(t);vm.write(t.error,encode(123n,4));
 for(const prefix of ['', 'K32']){
  const name=prefix+'EnumProcessModules';equal(enumeration(t,name),handles);
  vm.write(out,new Uint8Array(64).fill(0xa5));equal(api(t,name,[-1n,out,BigInt(width+width-1),needed]),1n);equal(vm.readInt(out,width),t.image.base);equal(vm.readInt(out+BigInt(width),width),BigInt('0x'+'a5'.repeat(width)));equal(vm.readInt(needed),BigInt(handles.length*width));
  equal(api(t,name,[-1n,0xdeadn,0n,needed]),1n);equal(vm.readInt(needed),BigInt(handles.length*width));
  for(const filter of [0n,1n,2n,3n])equal(enumeration(t,prefix+'EnumProcessModulesEx',filter),filter===(mode===32?2n:1n)?[]:handles);
  equal(enumeration(t,prefix+'EnumProcessModulesEx',0x100000003n),handles);
  for(const module of [t.image.base,base]){
   vm.write(out,new Uint8Array(40).fill(0xa5));equal(api(t,prefix+'GetModuleInformation',[-1n,module,out,40n]),1n);
   equal(vm.readInt(out,width),module);equal(vm.readInt(out+BigInt(width)),BigInt(leaf.view.getUint32(leaf.opt+56,true)));
   equal(vm.readInt(out+BigInt(mode===32?8:16),width),module+0x1000n);equal(vm.readInt(out+BigInt(mode===32?12:24)),0xa5a5a5a5n);
  }
  for(const [query,expected] of [['GetModuleBaseNameA',Buffer.from([108,101,97,102,0x93,0xfa,46,100,108,108,0])],['GetModuleBaseNameW',Buffer.from('leaf日.dll\0','utf16le')],['GetModuleFileNameExA',Buffer.from([...Buffer.from('C:\\plugins\\leaf'),0x93,0xfa,...Buffer.from('.dll\0')])],['GetModuleFileNameExW',Buffer.from(path+'\0','utf16le')]]){
   const unit=query.endsWith('W')?2:1,units=expected.length/unit;
   for(const capacity of [1,4,units-1,units,units+5]){
    vm.write(out,new Uint8Array(256).fill(0xa5));const count=Math.min(capacity,units),want=Uint8Array.from(expected.subarray(0,count*unit));if(count<units)want.fill(0,(count-1)*unit);
    equal(api(t,prefix+query,[-1n,base,out,BigInt(capacity)]),BigInt(count<units?count:units-1));equal(bytes(vm,out,count*unit),new Uint8Array(want));equal(vm.readInt(out+BigInt(count*unit),1),0xa5n);
   }
  }
 }
 equal(vm.readInt(t.error),123n);equal(vm.flags,0x8d5n);equal(vm.xmm(3),new Uint8Array(16).fill(0x7b));equal(fp(t),fpu);
 text(vm,0x22000n,'ntdll.dll');const provider=kernel(t,'GetModuleHandleW',[0x22000n]);assert.ok(handles.includes(provider));
 equal(api(t,'K32GetModuleInformation',[-1n,provider,out,24n]),0n);equal(vm.readInt(t.error),50n);
 equal(api(t,'K32GetModuleFileNameExW',[-1n,provider,out,64n]),BigInt('C:\\Windows\\System32\\ntdll.dll'.length));
 equal(api(t,'K32GetModuleBaseNameW',[-1n,0n,out,64n]),11n);equal(Buffer.from(bytes(vm,out,24)).toString('utf16le'),'program.exe\0');
 const pid=kernel(t,'GetCurrentProcessId',[]),full=kernel(t,'OpenProcess',[0x410n,0n,pid]),limited=kernel(t,'OpenProcess',[0x1000n,0n,pid]);
 equal(api(t,'K32EnumProcessModules',[full,out,512n,needed]),1n);
 equal(api(t,'K32EnumProcessModules',[limited,out,512n,needed]),0n);equal(vm.readInt(t.error),5n);
 equal(api(t,'K32GetModuleFileNameExW',[limited,0n,out,64n]),mode===64?14n:0n);
 if(mode===64){const high=0x123400000000n;vm.map(high,4096n,3);equal(api(t,'K32GetModuleInformation',[-1n,base,high,24n]),1n);equal(vm.readInt(high,8),base);}
 for(const [name,args,error] of [['K32EnumProcessModules',[0xdeadn,0xdeadn,64n,0xdeadn],6n],['K32EnumProcessModulesEx',[-1n,out,64n,needed,4n],87n],['K32GetModuleInformation',[-1n,base,out,BigInt(mode===32?11:23)],122n],['K32GetModuleBaseNameW',[-1n,base,0xdeadn,0n],122n],['K32GetModuleInformation',[-1n,0n,out,24n],6n]]){
  equal(api(t,name,args),0n);equal(vm.readInt(t.error),error);
 }
 frame(t,'FreeLibrary',[base],'kernel32.dll');await finish(t);equal(vm.register(0),1n);equal(enumeration(t),before);equal(api(t,'K32GetModuleInformation',[-1n,base,out,24n]),0n);equal(vm.readInt(t.error),6n);
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Multi-output calls preflight every byte and preserve saved returns and inputs.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,out=0x24ffen,needed=0x23000n;
 for(const [name,args] of [['K32EnumProcessModules',[-1n,out,512n,needed]],['K32GetModuleInformation',[-1n,t.image.base,out,24n]],['K32GetModuleFileNameExW',[-1n,0n,out,64n]]]){
  vm.write(out,new Uint8Array(64).fill(0xa5));vm.write(needed,encode(0x1234n,4));frame(t,name,args,dll(name));const before=snapshot(t);
  equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(out,2),0xa5a5n);equal(vm.readInt(needed),0x1234n);
  equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);
 }
 equal(api(t,'K32EnumProcessModules',[-1n,0x24000n,512n,t.sp]),1n);equal(vm.ip,vm.entryPoints.returned);
 equal(api(t,'K32EnumProcessModules',[-1n,0x24000n,512n,0x24000n]),1n);equal(vm.readInt(0x24000n),BigInt(3*mode/8));
 frame(t,'K32EnumProcessModules',[0xdeadn,0xdeadn,512n,0xdeadn],dll('K32EnumProcessModules'));const before=snapshot(t);
 equal(core.wire_protect(t.handle,t.error&~4095n,4096n,2),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,t.error&~4095n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(t.error),6n);
 vm.dispose();
}
console.log(`${checks} module enumeration/info/name/ABI/rights/lifetime/fault checks passed`);
