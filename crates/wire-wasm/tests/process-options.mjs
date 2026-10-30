import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
const api=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const init=(t,p,count=4n,size=0x21000n,flags=0n)=>api(t,'InitializeProcThreadAttributeList',[p,count,flags,size]);
const update=(t,p,key=0x20002n,value=0x23000n,bytes=BigInt(t.mode/8),flags=0n,previous=0n,returned=0n)=>api(t,'UpdateProcThreadAttribute',[p,flags,key,value,bytes,previous,returned]);

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 vm.write(t.error,encode(123n,4));equal(api(t,'GetErrorMode'),0n);equal(api(t,'SetErrorMode',[0x8003n]),0n);equal(api(t,'GetErrorMode'),0x8003n);equal(vm.readInt(t.error),123n);
 equal(api(t,'GetThreadErrorMode'),0n);equal(api(t,'SetThreadErrorMode',[2n,0x22000n]),1n);equal(vm.readInt(0x22000n),0n);equal(api(t,'SetErrorMode',[1n]),0x8003n);equal(api(t,'GetThreadErrorMode'),2n);
 equal(api(t,'SetErrorMode',[4n]),1n);equal(api(t,'SetErrorMode',[0n]),4n);equal(api(t,'GetErrorMode'),4n);equal(api(t,'SetErrorMode',[0x400n]),0n);equal(vm.readInt(t.error),87n);equal(api(t,'GetErrorMode'),4n);
 vm.write(0x22000n,encode(0xa5n,4));equal(api(t,'SetThreadErrorMode',[4n,0x22000n]),0n);equal(vm.readInt(t.error),87n);equal(vm.readInt(0x22000n),0xa5n);equal(api(t,'GetThreadErrorMode'),2n);
 frame(t,'SetThreadErrorMode',[0x8001n,0x24ffen],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(api(t,'GetThreadErrorMode'),2n);
 if(mode===64){equal(api(t,'SetErrorMode',[(1n<<32n)|1n]),4n);equal(api(t,'GetErrorMode'),5n);equal(api(t,'SetThreadErrorMode',[(1n<<32n)|2n,0n]),1n);}
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,width=mode/8,list=0x22000n;
 vm.map(0x80000n,4096n,2);equal(init(t,0n,4n,0x80000n),0n);equal(vm.readInt(t.error),122n);equal(core.wire_protect(t.handle,0x80000n,4096n,4),0);const needed=vm.readInt(0x80000n,width);assert.ok(needed>0n&&needed<4096n);
 vm.write(0x21000n,encode(needed-1n,width));vm.write(list,new Uint8Array(Number(needed)+8).fill(0xa5));equal(init(t,list),0n);equal(vm.readInt(0x21000n,width),needed);equal(vm.readInt(list),0xa5a5a5a5n);
 vm.write(0x21000n,encode(4096n,width));vm.write(t.error,encode(123n,4));equal(init(t,list),1n);equal(vm.readInt(0x21000n,width),needed);equal(vm.readInt(list+needed),0xa5a5a5a5n);equal(vm.readInt(t.error),123n);
 // Values remain caller-owned; registration records the address without dereferencing it.
 equal(update(t,list,0x20002n,0xdead0000n),1n);equal(update(t,list),0n);equal(vm.readInt(t.error),698n);equal(update(t,list,0x20007n,0x23000n,8n),1n);
 for(const [key,bytes,error] of [[0x20000n,1n,24n],[0x30003n,1n,24n],[0x30005n,8n,24n],[0x20007n,3n,24n],[0x2000en,2n,24n],[0x20019n,4n,24n],[0xdeadn,8n,50n]]){equal(update(t,list,key,0x23000n,bytes),0n);equal(vm.readInt(t.error),error);}
 equal(update(t,list,0x20000n,0x23000n,BigInt(width),1n),0n);equal(vm.readInt(t.error),87n);equal(update(t,list,0x20000n,0x23000n,BigInt(width),0n,1n),0n);equal(vm.readInt(t.error),87n);equal(update(t,list,0x20000n,0x23000n,BigInt(width),0n,0n,1n),0n);equal(vm.readInt(t.error),87n);
 equal(update(t,list,0x20000n),1n);equal(update(t,list,0x30005n,0x23000n,4n),1n);equal(update(t,list,0x20019n,0x23000n,2n),0n);equal(vm.readInt(t.error),31n);
 vm.write(t.error,encode(123n,4));api(t,'DeleteProcThreadAttributeList',[list]);equal(vm.readInt(t.error),123n);equal(update(t,list),0n);equal(vm.readInt(t.error),87n);equal(init(t,list),1n);
 if(mode===64){equal(update(t,list,0x100020002n),0n);equal(vm.readInt(t.error),50n);equal(update(t,list,0x20002n,0xdead0000n,(1n<<32n)+8n),1n);}
 equal(init(t,0n,33n),0n);equal(vm.readInt(t.error),50n);equal(init(t,0n,4n,0x21000n,1n),0n);equal(vm.readInt(t.error),87n);
 for(const [key,length] of [[0x2000dn,BigInt(width)],[0x30003n,mode===32?12n:16n],[0x2000en,4n],[0x20019n,2n]]){equal(init(t,list),1n);equal(update(t,list,key,0x23000n,length),1n);}
 equal(init(t,0n,0n),0n);const empty=vm.readInt(0x21000n,width);assert.ok(empty<needed);equal(init(t,list,0n),1n);equal(update(t,list),0n);equal(vm.readInt(t.error),31n);
 for(const name of ['GetErrorMode','SetErrorMode','InitializeProcThreadAttributeList','UpdateProcThreadAttribute','DeleteProcThreadAttributeList']){equal(vm.bind('kernelbase.dll',name),vm.bind('kernel32.dll',name));assert.throws(()=>vm.bind('ntdll.dll',name),/unknown/);}
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,width=mode/8,list=0x24ff0n;
 equal(init(t,0n),0n);const needed=vm.readInt(0x21000n,width);vm.write(0x21000n,encode(needed,width));vm.write(list,new Uint8Array(Number(needed)).fill(0xa5));
 frame(t,'InitializeProcThreadAttributeList',[list,4n,0n,0x21000n],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(list),0xa5a5a5a5n);equal(vm.readInt(0x21000n,width),needed);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),1n);
 frame(t,'UpdateProcThreadAttribute',[list,0n,0x20002n,0xdead0000n,BigInt(width),0n,0n],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),1n);equal(update(t,list),0n);equal(vm.readInt(t.error),698n);
 vm.write(0x21000n,encode(7n,width));frame(t,'InitializeProcThreadAttributeList',[0n,4n,0n,0x21000n],'kernel32.dll');equal(core.wire_protect(t.handle,t.error&~4095n,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(0x21000n,width),7n);equal(core.wire_protect(t.handle,t.error&~4095n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(0x21000n,width),needed);
 equal(init(t,0n,4n,t.sp),0n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp,width),needed);vm.dispose();
}
console.log(`${checks} process/thread-error-mode/attribute-list/ABI/pointer-lifetime/fault checks passed`);
