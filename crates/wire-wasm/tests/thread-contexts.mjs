import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
const api=(t,name,args=[],dll='kernel32.dll')=>call(t,name,args,dll);
const flagOffset=mode=>mode===32?0n:48n;
const architecture=mode=>mode===32?0x10000n:0x100000n;
const raxOffset=mode=>mode===32?176n:120n;
const ipOffset=mode=>mode===32?184n:248n;
const spOffset=mode=>mode===32?196n:152n;
const context=(t,p,flags=3n,fill=0xa5)=>{t.vm.write(p,new Uint8Array(t.mode===32?716:1232).fill(fill));t.vm.write(p+flagOffset(t.mode),encode(architecture(t.mode)|flags,4));};

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,p=0x22000n,width=mode/8,current=(1n<<BigInt(mode))-2n,full=mode===32?0x2fn:15n;
 vm.write(0x30000n,[0xb8,37,0,0,0,...(mode===32?[0xc2,4,0]:[0xc3])]);const h=api(t,'CreateThread',[0n,65536n,0x30000n,0x1234n,4n,0x21000n]);
 context(t,p,full);equal(api(t,'GetThreadContext',[h,p]),1n);equal(vm.readInt(p+ipOffset(mode),width),0x30000n);const stack=vm.readInt(p+spOffset(mode),width);assert.ok(stack);equal(vm.readInt(p+(mode===32?172n:128n),width),mode===32?0n:0x1234n);
 context(t,p,1n);equal(api(t,'GetThreadContext',[h,p]),1n);equal(vm.readInt(p+raxOffset(mode),width),(1n<<BigInt(mode))-1n & 0xa5a5a5a5a5a5a5a5n);equal(vm.readInt(p+ipOffset(mode),width),0x30000n);
 context(t,p,full);equal(api(t,'GetThreadContext',[h,p]),1n);vm.write(p+flagOffset(mode),encode(architecture(mode)|2n,4));vm.write(p+raxOffset(mode),encode(0xabcdef01n,width));equal(api(t,'SetThreadContext',[h,p]),1n);context(t,p,full);equal(api(t,'GetThreadContext',[h,p]),1n);equal(vm.readInt(p+raxOffset(mode),width),0xabcdef01n);equal(vm.readInt(p+spOffset(mode),width),stack);
 vm.write(p+flagOffset(mode),encode(architecture(mode)|1n,4));vm.write(p+BigInt(mode===32?192:68),encode(0xffffffffn,4));equal(api(t,'SetThreadContext',[h,p]),1n);context(t,p,full);equal(api(t,'GetThreadContext',[h,p]),1n);const flags=vm.readInt(p+BigInt(mode===32?192:68));equal(flags&0x3000n,0n);equal(flags&0x200n,0x200n);vm.write(p+flagOffset(mode),encode(architecture(mode)|1n,4));vm.write(p+BigInt(mode===32?192:68),encode(0x202n,4));equal(api(t,'SetThreadContext',[h,p]),1n);
 context(t,p,full);equal(api(t,'GetThreadContext',[h,p]),1n);const mxcsr=mode===32?228n:52n;vm.write(p+flagOffset(mode),encode(architecture(mode)|(mode===32?0x20n:8n),4));vm.write(p+mxcsr,encode(0x10000n,4));if(mode===64)vm.write(p+280n,encode(0x10000n,4));equal(api(t,'SetThreadContext',[h,p]),0n);equal(vm.readInt(t.error),87n);context(t,p,full);equal(api(t,'GetThreadContext',[h,p]),1n);equal(vm.readInt(p+mxcsr),0x1f80n);
 for(const name of ['GetThreadContext','SetThreadContext']){context(t,p,0x10n);equal(api(t,name,[h,p]),0n);equal(vm.readInt(t.error),50n);vm.write(p+flagOffset(mode),encode(0n,4));equal(api(t,name,[h,p]),0n);equal(vm.readInt(t.error),87n);}
 const onlyGet=api(t,'OpenThread',[8n,0n,2n]),onlySet=api(t,'OpenThread',[16n,0n,2n]);equal(api(t,'SetThreadContext',[onlyGet,0xdead0000n]),0n);equal(vm.readInt(t.error),5n);equal(api(t,'GetThreadContext',[onlySet,0xdead0000n]),0n);equal(vm.readInt(t.error),5n);equal(api(t,'DuplicateHandle',[(1n<<BigInt(mode))-1n,onlyGet,(1n<<BigInt(mode))-1n,0x21008n,16n,0n,0n]),0n);equal(vm.readInt(t.error),5n);
 context(t,p,3n);equal(api(t,'GetThreadContext',[current,p]),1n);equal(vm.readInt(p+ipOffset(mode),width),vm.entryPoints.returned);equal(api(t,'SetThreadContext',[current,p]),0n);equal(vm.readInt(t.error),170n);
 const running=api(t,'CreateThread',[0n,65536n,0x30000n,0n,0n,0n]);equal(api(t,'GetThreadContext',[running,p]),0n);equal(vm.readInt(t.error),170n);equal(api(t,'SuspendThread',[running]),0n);equal(api(t,'GetThreadContext',[running,p]),1n);
 if(mode===64){equal(api(t,'GetThreadContext',[h,p+8n]),0n);equal(vm.readInt(t.error),998n);context(t,p,1n);equal(api(t,'GetThreadContext',[h,p]),1n);vm.write(p+ipOffset(mode),encode(1n<<48n,8));equal(api(t,'SetThreadContext',[h,p]),0n);equal(vm.readInt(t.error),87n);}
 const cross=0x24ff0n;context(t,cross,full);frame(t,'GetThreadContext',[h,cross],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(cross+BigInt(mode===32?8:0)),0xa5a5a5a5n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);
 context(t,cross,full);equal(api(t,'GetThreadContext',[h,cross]),1n);vm.write(cross+raxOffset(mode),encode(7n,width));frame(t,'SetThreadContext',[h,cross],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,1),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);context(t,p,full);equal(api(t,'GetThreadContext',[h,p]),1n);equal(vm.readInt(p+raxOffset(mode),width),0xabcdef01n);
 equal(api(t,'ResumeThread',[h]),1n);frame(t,'WaitForSingleObject',[h,0xffffffffn],'kernel32.dll');equal((await vm.run(1000n)).reason,Exit.Returned);equal(api(t,'GetThreadContext',[h,p]),0n);equal(vm.readInt(t.error),6n);for(const handle of [h,onlyGet,onlySet,running])equal(api(t,'CloseHandle',[handle]),1n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,p=0x22000n,width=mode/8;
 context(t,p,0n);frame(t,'RtlCaptureContext',[p],'ntdll.dll');const old=mode===32?0x12345678n:0x123456789abcdef0n;vm.register(0,old);vm.register(3,0x1234n);vm.flags=0xad7n;vm.xmm(0,new Uint8Array(16).fill(91));vm.mxcsr=0x5f80;vm.write(t.error,encode(123n,4));equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),old);equal(vm.readInt(p+raxOffset(mode),width),old);equal(vm.readInt(p+ipOffset(mode),width),vm.entryPoints.returned);equal(vm.readInt(p+spOffset(mode),width),t.sp+8n);equal(vm.readInt(p+flagOffset(mode)),mode===32?0x10007n:0x10000fn);equal(vm.readInt(p+BigInt(mode===32?164:144),width),0x1234n);equal(vm.readInt(t.error),123n);
 if(mode===64){equal(vm.readInt(p+52n),0x5f80n);equal(vm.readInt(p+416n,8),0x5b5b5b5b5b5b5b5bn);frame(t,'RtlCaptureContext',[p+8n],'ntdll.dll');const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);}
 else equal(vm.readInt(p+204n),0xa5a5a5a5n);
 for(const dll of ['kernel32.dll','kernelbase.dll'])equal(vm.bind(dll,'RtlCaptureContext'),vm.bind('ntdll.dll','RtlCaptureContext'));for(const name of ['GetThreadContext','SetThreadContext'])equal(vm.bind('kernelbase.dll',name),vm.bind('kernel32.dll',name));equal(t.time.calls,0);vm.dispose();
}
console.log(`${checks} selective-thread-context/rights/suspend/capture/FP/atomic-fault checks passed`);
