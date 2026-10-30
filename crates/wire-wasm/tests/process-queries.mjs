import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
const api=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const open=(t,access,inherit=0n,pid=1n)=>api(t,'OpenProcess',[access,inherit,pid]);
const duplicate=(t,source,access,{owner=(1n<<BigInt(t.mode))-1n,flags=0n,inherit=0n}={})=>{
 equal(api(t,'DuplicateHandle',[owner,source,owner,0x24000n,access,inherit,flags]),1n);
 return t.vm.readInt(0x24000n,t.mode/8);
};

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,current=(1n<<BigInt(mode))-1n,thread=current-1n;
 equal(api(t,'GetCurrentProcessId'),1n);equal(vm.readInt(t.image.teb+BigInt(mode===32?0x20:0x40),mode/8),1n);
 equal(open(t,0x400n,0n,0n),0n);equal(vm.readInt(t.error),87n);equal(open(t,0x400n,0n,4242n),0n);equal(vm.readInt(t.error),87n);
 equal(open(t,0x80000000n),0n);equal(vm.readInt(t.error),50n);
 const query=open(t,0x400n,1n),limited=open(t,0x1000n),empty=open(t,0n,mode===64?1n<<32n:0n),setting=open(t,0x200n),all=open(t,0x1fffffn);
 assert.ok(query&&limited&&empty&&setting&&all);
 vm.write(t.error,encode(123n,4));equal(api(t,'GetProcessId',[query]),1n);equal(api(t,'GetProcessId',[limited]),1n);equal(api(t,'GetProcessId',[current]),1n);equal(vm.readInt(t.error),123n);
 equal(api(t,'GetExitCodeProcess',[limited,0x22000n]),1n);equal(vm.readInt(0x22000n),259n);equal(api(t,'GetProcessAffinityMask',[limited,0x22000n,0x22010n]),1n);equal(vm.readInt(0x22000n,mode/8),1n);equal(vm.readInt(0x22010n,mode/8),1n);
 if(mode===64){equal(api(t,'GetProcessId',[query|(1n<<32n)]),0n);equal(vm.readInt(t.error),6n);}
 equal(api(t,'GetHandleInformation',[empty,0x22000n]),1n);equal(vm.readInt(0x22000n),0n);equal(api(t,'GetProcessId',[empty]),0n);equal(vm.readInt(t.error),5n);equal(api(t,'GetExitCodeProcess',[empty,0xdead0000n]),0n);equal(vm.readInt(t.error),5n);equal(api(t,'GetProcessId',[thread]),0n);equal(vm.readInt(t.error),6n);
 equal(api(t,'SetProcessAffinityMask',[setting,1n]),1n);equal(api(t,'SetProcessAffinityMask',[query,1n]),0n);equal(vm.readInt(t.error),5n);for(const mask of [0n,2n,mode===64?0x100000001n:3n]){equal(api(t,'SetProcessAffinityMask',[setting,mask]),0n);equal(vm.readInt(t.error),87n);}
 equal(api(t,'GetHandleInformation',[query,0x22000n]),1n);equal(vm.readInt(0x22000n),1n);equal(api(t,'SetHandleInformation',[query,2n,2n]),1n);equal(api(t,'CloseHandle',[query]),0n);equal(vm.readInt(t.error),6n);equal(api(t,'GetProcessId',[query]),1n);equal(api(t,'SetHandleInformation',[query,2n,0n]),1n);
 const narrowed=duplicate(t,all,0x1000n,{inherit:1n});equal(api(t,'GetProcessId',[narrowed]),1n);equal(api(t,'DuplicateHandle',[current,narrowed,current,0x24000n,0x400n,0n,0n]),0n);equal(vm.readInt(t.error),5n);
 equal(api(t,'DuplicateHandle',[query,all,current,0x24000n,0n,0n,2n]),0n);equal(vm.readInt(t.error),5n);equal(api(t,'DuplicateHandle',[current,all,query,0x24000n,0n,0n,2n]),0n);equal(vm.readInt(t.error),5n);
 const pseudo=duplicate(t,current,0n,{owner:all,flags:2n});equal(api(t,'GetProcessId',[pseudo]),1n);equal(api(t,'GetHandleInformation',[pseudo,0x22000n]),1n);equal(vm.readInt(0x22000n),0n);
 const moved=duplicate(t,narrowed,0n,{owner:all,flags:3n});equal(api(t,'GetProcessId',[narrowed]),0n);equal(vm.readInt(t.error),6n);equal(api(t,'GetProcessId',[moved]),1n);
 // Capture the return frame before output overwrites its saved return address.
 equal(api(t,'DuplicateHandle',[current,current,current,t.sp,0x1000n,0n,0n]),1n);const overlap=vm.readInt(t.sp,mode/8);equal(vm.ip,vm.entryPoints.returned);equal(api(t,'GetProcessId',[overlap]),1n);
 const operation=open(t,8n),writer=open(t,0x20n);const page=api(t,'VirtualAllocEx',[operation,0n,4096n,0x3000n,4n]);assert.ok(page);equal(api(t,'VirtualQueryEx',[query,page,0x25000n,48n]),mode===32?28n:48n);equal(api(t,'VirtualQueryEx',[limited,page,0xdead0000n,48n]),0n);equal(vm.readInt(t.error),5n);
 equal(api(t,'VirtualAllocEx',[query,0n,4096n,0x3000n,4n]),0n);equal(vm.readInt(t.error),5n);equal(api(t,'VirtualProtectEx',[operation,page,4096n,0x20n,0x22000n]),1n);equal(vm.readInt(0x22000n),4n);equal(api(t,'FlushInstructionCache',[writer,page,4096n]),1n);equal(api(t,'FlushInstructionCache',[query,page,4096n]),0n);equal(vm.readInt(t.error),5n);equal(api(t,'VirtualFreeEx',[operation,page,0n,0x8000n]),1n);
 equal(api(t,'SetThreadIdealProcessor',[thread,0n]),0n);equal(api(t,'SetThreadIdealProcessor',[thread,BigInt(mode)]),0n);for(const processor of [1n,0xffffffffn]){equal(api(t,'SetThreadIdealProcessor',[thread,processor]),0xffffffffn);equal(vm.readInt(t.error),87n);}
 const threadQuery=api(t,'OpenThread',[0x40n,0n,1n]);equal(api(t,'SetThreadIdealProcessor',[threadQuery,0n]),0xffffffffn);equal(vm.readInt(t.error),5n);equal(api(t,'SetThreadIdealProcessor',[query,0n]),0xffffffffn);equal(vm.readInt(t.error),6n);
 for(const name of ['GetActiveProcessorCount','GetMaximumProcessorCount']){for(const group of [0n,0xffffn,0x10000n])equal(api(t,name,[group]),1n);equal(api(t,name,[1n]),0n);equal(vm.readInt(t.error),87n);}
 equal(api(t,'GetActiveProcessorGroupCount'),1n);equal(api(t,'GetMaximumProcessorGroupCount'),1n);vm.write(t.error,encode(123n,4));equal(api(t,'GetCurrentPackageId',[0xdead0000n,0xdead1000n]),15700n);equal(vm.readInt(t.error),123n);
 vm.write(0x22000n,encode(0xa5n,mode/8));frame(t,'GetProcessAffinityMask',[all,0x22000n,0x24ffen],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(0x22000n,mode/8),0xa5n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),1n);
 frame(t,'DuplicateHandle',[current,all,current,0x25000n,0n,0n,3n],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(api(t,'GetProcessId',[all]),1n);
 for(const name of ['OpenProcess','GetProcessId','GetExitCodeProcess','GetProcessAffinityMask','SetProcessAffinityMask','SetThreadIdealProcessor','GetActiveProcessorCount','GetMaximumProcessorCount','GetActiveProcessorGroupCount','GetMaximumProcessorGroupCount','GetCurrentPackageId']){equal(vm.bind('kernelbase.dll',name),vm.bind('kernel32.dll',name));assert.throws(()=>vm.bind('user32.dll',name),/unknown/);}
 for(const h of [query,limited,empty,setting,all,pseudo,moved,overlap,operation,writer,threadQuery])equal(api(t,'CloseHandle',[h]),1n);equal(api(t,'GetProcessId',[all]),0n);equal(vm.readInt(t.error),6n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

for(const mode of [32,64]){
 let ticks=10n;const t=await machine(mode,{clock:()=>ticks}),{vm}=t,current=(1n<<BigInt(mode))-1n,p=open(t,0x100000n),alias=duplicate(t,p,0n,{flags:2n}),query=open(t,0x400n);
 equal(api(t,'WaitForSingleObject',[p,0n]),258n);equal(api(t,'WaitForSingleObject',[current,0n]),258n);equal(api(t,'WaitForSingleObject',[query,0n]),0xffffffffn);equal(vm.readInt(t.error),5n);
 const event=api(t,'CreateEventW',[0n,0n,1n,0n]);vm.write(0x22000n,[...encode(p,mode/8),...encode(event,mode/8)]);
 equal(api(t,'WaitForMultipleObjects',[2n,0x22000n,1n,0n]),258n);equal(api(t,'WaitForSingleObject',[event,0n]),0n);equal(api(t,'SetEvent',[event]),1n);equal(api(t,'WaitForMultipleObjects',[2n,0x22000n,0n,0n]),1n);
 vm.write(0x22000n,[...encode(p,mode/8),...encode(alias,mode/8)]);equal(api(t,'WaitForMultipleObjects',[2n,0x22000n,1n,0n]),0xffffffffn);equal(vm.readInt(t.error),87n);
 frame(t,'WaitForSingleObject',[p,7n],'kernel32.dll');let before=snapshot(t);equal(vm.service().reason,Exit.Waiting);equal(snapshot(t),before);ticks+=70000n;equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),258n);
 for(const h of [p,alias,query,event])equal(api(t,'CloseHandle',[h]),1n);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,current=(1n<<BigInt(mode))-1n,handles=[];
 for(let n=0;n<1024;n++){const h=open(t,0n);assert.ok(h);handles.push(h);}
 equal(open(t,0n),0n);equal(vm.readInt(t.error),8n);equal(api(t,'DuplicateHandle',[current,current,current,0x24000n,0n,0n,2n]),0n);equal(vm.readInt(t.error),8n);equal(api(t,'CloseHandle',[handles.pop()]),1n);const replacement=open(t,0n);assert.ok(replacement>handles.at(-1));for(const h of [...handles,replacement])equal(api(t,'CloseHandle',[h]),1n);vm.dispose();
}
console.log(`${checks} process-handle/rights/query/affinity/wait/VMM/ABI/fault checks passed`);
