import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,call} from './gdi-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {moduleFixture} from './module-fixture.mjs';
const api=(t,name,args=[])=>call(t,name,args,'ole32.dll');
const auto=(t,name,args=[])=>call(t,name,args,'oleaut32.dll');
const kernel=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const wide=(t,p,s)=>t.vm.write(p,Buffer.from(s+'\0','utf16le'));
const guid=(t,p,fields)=>t.vm.write(p,fields.flatMap(n=>Array.from(encode(BigInt(n),4))));
const clsid=[0x4590f811,0x11d01d3a,0xaa001f89,0x242e4b00];
const iids=[[0xdc12a687,0x11cf737f,0xaa004d88,0x242e4b00],[0x9556dc99,0x11cf828c,0xaa007ea3,0xc7403200],[0x027947e1,0x11ced731,0x000057a3,0x01000000],[0xdc12a681,0x11cf737f,0xaa004d88,0x242e4b00]];
const bytes=(t,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(t.vm.readInt(p+BigInt(i),1)));
const method=(t,p,slot)=>t.vm.readInt(t.vm.readInt(p)+BigInt(slot*4));
function frameAt(t,pc,args){const{vm,sp}=t;vm.ip=pc;vm.register(4,sp);vm.write(sp,encode(vm.entryPoints.returned,4));for(const[n,value]of args.entries())vm.write(sp+4n+BigInt(n*4),encode(value,4));}
function invoke(t,p,slot,args=[]){const{vm}=t;frameAt(t,method(t,p,slot),[p,...args]);const work=vm.work,retired=vm.retired;equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);equal(vm.register(4),t.sp+8n+BigInt(args.length*4));equal(vm.work,work+1n);equal(vm.retired,retired);return vm.register(0);}
function locator(t){guid(t,0x20000n,clsid);guid(t,0x20020n,iids[0]);equal(api(t,'CoCreateInstance',[0x20000n,0n,1n,0x20020n,0x20100n]),0n);return t.vm.readInt(0x20100n);}
function service(t,p){wide(t,0x20200n,'\\\\.\\ROOT\\CIMV2');equal(invoke(t,p,3,[0x20200n,0n,0n,0n,0n,0n,0n,0x20104n]),0n);return t.vm.readInt(0x20104n);}
function query(t,p){wide(t,0x20300n,'WQL');wide(t,0x20400n,'SELECT * FROM Win32_OperatingSystem');equal(invoke(t,p,20,[0x20300n,0x20400n,0x30n,0n,0x20108n]),0n);return t.vm.readInt(0x20108n);}
function next(t,p,count=1n){const result=invoke(t,p,4,[0xffffffffn,count,0x2010cn,0x20110n]);return [result,t.vm.readInt(0x2010cn),t.vm.readInt(0x20110n)];}
async function setup(){const t=await machine(32);api(t,'CoInitialize',[0n]);const l=locator(t),s=service(t,l),e=query(t,s);return{...t,l,s,e};}

// Activation preserves apartment, class/context/IID rules and the complete saved frame.
{
 const t=await machine(32),{vm}=t;guid(t,0x20000n,clsid);guid(t,0x20020n,iids[0]);const args=[0x20000n,0n,1n,0x20020n,0x20100n];
 equal(api(t,'CoCreateInstance',args),0x800401f0n);equal(vm.readInt(0x20100n),0n);api(t,'CoInitialize',[0n]);vm.write(t.error,encode(123n,4));
 for(const[index,value,result]of[[1,1n,0x80040110n],[2,4n,0x80040154n],[2,0n,0x80070057n],[4,0n,0x80004003n]]){const a=args.slice();a[index]=value;equal(api(t,'CoCreateInstance',a),result);}
 guid(t,0x20020n,[0,0,0,0]);equal(api(t,'CoCreateInstance',args),0x80004002n);guid(t,0x20020n,[0,0,0xc0,0x46000000]);equal(api(t,'CoCreateInstance',args),0n);const p=vm.readInt(0x20100n);equal(invoke(t,p,2),0n);
 guid(t,0x20020n,iids[0]);args[4]=0x20000n;equal(api(t,'CoCreateInstance',args),0n);equal(vm.ip,vm.entryPoints.returned);equal(invoke(t,vm.readInt(0x20000n),2),0n);
 assert.throws(()=>vm.bind('wire.wmi','IWbemLocator.AddRef'),/unknown/);equal(vm.readInt(t.error),123n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
 const q=await machine(64);api(q,'CoInitialize',[0n]);guid(q,0x20000n,clsid);guid(q,0x20020n,iids[0]);equal(api(q,'CoCreateInstance',[0x20000n,0n,1n,0x20020n,0x20100n]),0x80040154n);q.vm.dispose();
}
// Every vtable slot uses the declared stdcall width; unavailable operations return E_NOTIMPL.
{
 const t=await setup(),{vm,l,s,e}=t;const[r,row,n]=next(t,e);equal(r,0n);equal(n,1n);
 const slots=[[[12,4,4,36],new Set([0,1,2,3])],[[12,4,4,24,8,12,24,20,20,20,20,20,20,20,20,20,20,20,20,20,24,24,24,24,32,28],new Set([0,1,2,20,22])],[[12,4,4,4,20,12,8,12],new Set([0,1,2,3,4,6,7])],[[12,4,4,8,24,20,8,20,8,24,4,12,8,12,12,12,12,12,8,20,20,8,8,20,4,12,12],new Set([0,1,2,4])]];
 for(const[k,p]of[l,s,e,row].entries()){
  guid(t,0x20600n,iids[k]);equal(invoke(t,p,0,[0x20600n,0x20120n]),0n);equal(vm.readInt(0x20120n),p);equal(invoke(t,p,2),1n);equal(invoke(t,p,1),2n);equal(invoke(t,p,2),1n);
  const[widths,implemented]=slots[k];for(const[slot,width]of widths.entries())if(!implemented.has(slot)){equal(invoke(t,p,slot,new Array(width/4-1).fill(0n)),0x80004001n);}
 }
 equal(invoke(t,l,0,[0x20600n,0n]),0x80004003n);equal(invoke(t,l,0,[0n,0x20120n]),0x80004003n);
 for(const p of[row,e,s,l])equal(invoke(t,p,2),0n);equal(invoke(t,l,1),0x80004003n);vm.dispose();
}
// Query failures publish null outputs; enumeration snapshots retain independent cursors.
{
 const t=await setup(),{vm,l,s,e}=t;
 wide(t,0x20200n,'root\\missing');equal(invoke(t,l,3,[0x20200n,0n,0n,0n,0n,0n,0n,0x20120n]),0x8004100en);equal(vm.readInt(0x20120n),0n);wide(t,0x20200n,'root');equal(invoke(t,l,3,[0x20200n,1n,0n,0n,0n,0n,0n,0x20120n]),0x8004100cn);
 wide(t,0x20400n,'SELECT * FROM Win32_VideoController');equal(invoke(t,s,20,[0x20300n,0x20400n,0n,0n,0x20120n]),0x80041011n);equal(vm.readInt(0x20120n),0n);wide(t,0x20300n,'SQL');equal(invoke(t,s,20,[0x20300n,0x20400n,0n,0n,0x20120n]),0x80041014n);wide(t,0x20300n,'WQL');wide(t,0x20400n,'select  *\r\n from Win32_OperatingSystem');equal(invoke(t,s,20,[0x20300n,0x20400n,0x40n,0n,0x20120n]),0x80041008n);equal(invoke(t,s,22,[0x20300n,0x20400n,0n,0n,0x20120n]),0x80041011n);
 equal(invoke(t,e,6,[0x20120n]),0n);const clone=vm.readInt(0x20120n);equal(invoke(t,e,7,[0xffffffffn,1n]),0n);equal(next(t,e)[0],1n);equal(invoke(t,e,3),0n);equal(next(t,e,0n)[2],0n);equal(next(t,e,4097n)[0],0x80041008n);
 vm.write(0x20110n,encode(0xa5a5a5a5n,4));const[r,row,n]=next(t,e,2n);equal(r,1n);equal(n,1n);equal(next(t,clone)[0],0n);const other=vm.readInt(0x2010cn);equal(next(t,clone)[2],0n);equal(invoke(t,e,7,[0n,0xffffffffn]),1n);
 for(const p of[row,other,clone,e,s,l])equal(invoke(t,p,2),0n);vm.dispose();
}
// Shared BSTR ownership lets VariantClear and SysFreeString release query values.
{
 const t=await setup(),{vm,l,s,e}=t;const[,row]=next(t,e);const heap=kernel(t,'GetProcessHeap');vm.write(t.error,encode(123n,4));vm.flags=0x8d5n;
 for(const[key,value]of[['__CLASS','Win32_OperatingSystem'],['Caption','Microsoft Windows 7'],['CSDVersion',''],['OSArchitecture','32-bit'],['Version','6.1.7601'],['BuildNumber','7601']]){
  wide(t,0x20600n,key);vm.write(0x20700n,new Uint8Array(16).fill(0xa5));equal(invoke(t,row,4,[0x20600n,0n,0x20700n,0x20720n,0x20724n]),0n);equal(vm.readInt(0x20700n,2),8n);equal(vm.readInt(0x20720n),8n);equal(vm.readInt(0x20724n),0n);const p=vm.readInt(0x20708n);equal(bytes(t,p,value.length*2+2),new Uint8Array(Buffer.from(value+'\0','utf16le')));equal(auto(t,'SysStringLen',[p]),BigInt(value.length));equal(auto(t,'VariantClear',[0x20700n]),0n);equal(vm.readInt(0x20700n,2),0n);equal(vm.readInt(t.error),123n);equal(kernel(t,'HeapSize',[heap,0n,p-4n]),0xffffffffn);vm.write(t.error,encode(123n,4));
 }
 wide(t,0x20600n,'missing');equal(invoke(t,row,4,[0x20600n,0n,0x20700n,0n,0n]),0x80041002n);wide(t,0x20600n,'Version');equal(invoke(t,row,4,[0x20600n,1n,0x20700n,0n,0n]),0x80041008n);
 equal(invoke(t,row,4,[0x20600n,0n,0x20600n,0n,0n]),0n);auto(t,'SysFreeString',[vm.readInt(0x20608n)]);equal(vm.flags,0x8d5n);equal(vm.readInt(t.error),123n);for(const p of[row,e,s,l])invoke(t,p,2);equal(t.time.calls,0);vm.dispose();
}
// Faults precede reference/cursor/allocation changes and preserve all output bytes.
for(const kind of['interface','iid','server','query','next','get','frame']){
 const t=await setup(),{vm,l,s,e}=t;const[,row]=next(t,e);invoke(t,e,3);guid(t,0x20600n,iids[0]);wide(t,0x20800n,'Version');const output=0x24ffen;
 vm.write(0x24ffen,new Uint8Array(24).fill(0xa5));let pc,args;
 if(kind==='interface'){pc=method(t,l,0);args=[l,0x20600n,output];}
 if(kind==='iid'){pc=method(t,l,0);args=[l,0x25000n,0x20120n];}
 if(kind==='server'){pc=method(t,l,3);args=[l,0x20200n,0n,0n,0n,0n,0n,0n,output];}
 if(kind==='query'){pc=method(t,s,20);args=[s,0x20300n,0x25000n,0n,0n,0x20120n];}
 if(kind==='next'){pc=method(t,e,4);args=[e,0n,1n,0x20120n,output];}
 if(kind==='get'){pc=method(t,row,4);args=[row,0x20800n,0n,0x20700n,0x20720n,output];}
 if(kind==='frame'){pc=method(t,l,1);args=[l];}
 frameAt(t,pc,args);const page=kind==='frame'?t.sp&~4095n:0x25000n;equal(core.wire_protect(t.handle,page,4096n,kind==='frame'||kind==='iid'||kind==='query'?1:2),0);const before=snapshot(t),out=bytes(t,0x24ffen,2),oldResult=vm.readInt(0x20120n);
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(t,0x24ffen,2),out);equal(vm.readInt(0x20120n),oldResult);equal(core.wire_protect(t.handle,page,4096n,4),0);
 if(kind==='iid')guid(t,0x25000n,iids[0]);if(kind==='query')wide(t,0x25000n,'SELECT * FROM Win32_OperatingSystem');
 equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);
 if(kind==='interface'||kind==='iid'||kind==='frame')equal(invoke(t,l,2),1n);
 if(kind==='server')invoke(t,vm.readInt(output),2);if(kind==='query')invoke(t,vm.readInt(0x20120n),2);
 if(kind==='next'){equal(vm.readInt(output),1n);equal(next(t,e)[0],1n);invoke(t,vm.readInt(0x20120n),2);}
 if(kind==='get')auto(t,'VariantClear',[0x20700n]);
 for(const p of[row,e,s,l])equal(invoke(t,p,2),0n);vm.dispose();
}
// A stack-alias output returns through the captured address; generated x86 calls use the same gateway.
{
 const t=await setup(),{vm,l,s,e}=t;guid(t,0x20600n,iids[0]);frameAt(t,method(t,l,0),[l,0x20600n,t.sp]);equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp),l);equal(invoke(t,l,2),1n);
 const pc=method(t,l,1);vm.write(0x30000n,[0x68,...encode(l,4),0xb8,...encode(pc,4),0xff,0xd0,0xc3]);frameAt(t,0x30000n,[]);equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),2n);equal(invoke(t,l,2),1n);
 for(const p of[e,s,l])invoke(t,p,2);vm.dispose();
}
// Startup rejection clears WMI metadata alongside its shared process heap backing.
{
 const vm=new WasmMachine(core,{mode:32}),bad=moduleFixture(32,{dll:true,attach:false,tls:false});vm.supplyModule('bad.dll',bad.bytes);const image=vm.load(moduleFixture(32,{dependency:'bad.dll',tls:false}).bytes),t={vm,mode:32,image,sp:image.sp-2048n};vm.map(0x20000n,0x10000n,7);
 const ip=vm.ip,regs=Array.from({length:8},(_,i)=>vm.register(i));api(t,'CoInitialize',[0n]);const p=locator(t);vm.ip=ip;regs.forEach((v,i)=>vm.register(i,v));await assert.rejects(vm.run(10000n),/attach returned FALSE/);assert.throws(()=>vm.readInt(p),/memory access/);vm.dispose();
}
console.log(`${checks} WMI activation/COM slots/query/BSTR/JIT/fault/lifetime checks passed`);
