import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,finish} from './gdi-harness.mjs';
const api=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
async function output(t,name,p){const {vm}=t,w=vm.work,r=vm.retired;frame(t,name,[p],'kernel32.dll');await finish(t);equal(vm.work,w+1n);equal(vm.retired,r);}
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 vm.write(0x22000n,[65,0xff,0]);vm.write(t.error,encode(123n,4));await output(t,'OutputDebugStringA',0x22000n);equal(vm.readInt(t.error),123n);await output(t,'OutputDebugStringA',0n);equal(vm.readInt(t.error),123n);
 vm.write(0x22000n,[65,0,0xe5,0x65,0x3d,0xd8,0,0xde,0,0]);await output(t,'OutputDebugStringW',0x22000n);equal(vm.readInt(t.error),123n);await output(t,'OutputDebugStringW',0n);equal(vm.readInt(t.error),123n);
 frame(t,'OutputDebugStringA',[0xdead0000n],'kernel32.dll');let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);
 vm.write(0x24ffen,[65,66,67,0]);frame(t,'OutputDebugStringA',[0x24ffen],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,1),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);await finish(t);equal(vm.readInt(t.error),123n);
 const sp=t.sp,guard=(sp&~4095n)-4096n;frame(t,'OutputDebugStringA',[0x22000n],'kernel32.dll');vm.register(4,(sp&~4095n)+8n);vm.write(vm.register(4),encode(vm.entryPoints.returned,mode/8));if(mode===32)vm.write(vm.register(4)+4n,encode(0x22000n,4));equal(core.wire_protect(t.handle,guard,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,guard,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);await finish(t);
 for(const name of ['OutputDebugStringA','OutputDebugStringW']){equal(vm.bind('kernelbase.dll',name),vm.bind('kernel32.dll',name));assert.throws(()=>vm.bind('ntdll.dll',name),/unknown/);}
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}
for(const mode of [32,64])for(const wide of [false,true]){
 const t=await machine(mode),{vm}=t,counter=0x23000n;
 const handler=mode===32?[0xff,0x05,...encode(counter,4),0x31,0xc0,0xc2,4,0]:[0x49,0xbb,...encode(counter,8),0x41,0xff,0x03,0x31,0xc0,0xc3];
 vm.write(0x30000n,handler);const token=api(t,'AddVectoredExceptionHandler',[1n,0x30000n]);vm.write(0x22000n,wide?[65,0,0,0]:[65,0]);
 frame(t,wide?'OutputDebugStringW':'OutputDebugStringA',[0x22000n],'kernel32.dll');equal(vm.service().reason,Exit.Dispatch);
 const pointers=mode===32?vm.readInt(vm.register(4)+4n,4):vm.register(1),record=vm.readInt(pointers,mode/8);equal((await vm.run(2n)).reason,Exit.Budget);equal(vm.readInt(counter),1n);
 const protectedPage=(wide?record:t.error)&~4095n;equal(core.wire_protect(t.handle,protectedPage,4096n,2),0);equal((await vm.run(100n)).reason,Exit.Fault);equal(vm.readInt(counter),1n);equal(core.wire_protect(t.handle,protectedPage,4096n,4),0);await finish(t);equal(vm.readInt(counter),wide?2n:1n);equal(api(t,'RemoveVectoredExceptionHandler',[token]),1n);vm.dispose();
}
console.log(`${checks} debug-output/no-debugger/ANSI-wide/VEH-continuation/fault/ABI checks passed`);
