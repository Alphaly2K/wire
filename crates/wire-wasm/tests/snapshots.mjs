import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,finish} from './gdi-harness.mjs';
const api=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const put=(vm,p,v,width=4)=>vm.write(p,encode(v,width));
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,p=0x22000n,width=mode/8,invalid=(1n<<BigInt(mode))-1n;
 const sizeA=mode===32?296:304,sizeW=mode===32?556:568,count=mode===32?20n:28n,name=mode===32?36n:44n;
 for(const flags of [0n,1n,8n,16n,0x80000000n]){equal(api(t,'CreateToolhelp32Snapshot',[flags,0n]),invalid);equal(vm.readInt(t.error),50n);}
 vm.write(0x30000n,[0xb8,37,0,0,0,...(mode===32?[0xc2,4,0]:[0xc3])]);
 const worker=api(t,'CreateThread',[0n,65536n,0x30000n,0n,4n,0n]);equal(api(t,'SetThreadPriority',[worker,0xfffffffen]),1n);
 const h=api(t,'CreateToolhelp32Snapshot',[0x180000006n,0xffffffffn]);assert.ok(h!==invalid);
 const process=api(t,'OpenProcess',[0x400n,0n,1n]);assert.ok(new Set([h,worker,process]).size===3);equal(api(t,'CloseHandle',[process]),1n);
 equal(api(t,'SetThreadPriority',[worker,2n]),1n);const late=api(t,'CreateThread',[0n,65536n,0x30000n,0n,4n,0n]);
 put(vm,t.error,123n);equal(api(t,'GetHandleInformation',[h,p]),1n);equal(vm.readInt(p),1n);
 vm.write(p,new Uint8Array(sizeW+16).fill(0xa5));put(vm,p,BigInt(sizeW+16));equal(api(t,'Process32FirstW',[h,p]),1n);
 equal(vm.readInt(p),BigInt(sizeW));equal(vm.readInt(p+8n),1n);equal(vm.readInt(p+count),2n);equal(vm.readInt(p+count+8n),8n);equal(vm.readInt(p+name,2),BigInt('p'.charCodeAt(0)));equal(vm.readInt(p+BigInt(sizeW)),0xa5a5a5a5n);
 put(vm,p,BigInt(sizeA));equal(api(t,'Process32Next',[h,p]),0n);equal(vm.readInt(t.error),18n);equal(api(t,'Process32First',[h,p]),1n);equal(vm.readInt(p+name,1),112n);
 put(vm,p,BigInt(sizeW));equal(api(t,'Process32NextW',[h,p]),0n);equal(vm.readInt(t.error),18n);
 put(vm,p,27n);equal(api(t,'Thread32First',[h,p]),0n);equal(vm.readInt(t.error),24n);
 put(vm,p,28n);equal(api(t,'Thread32Next',[h,p]),1n);equal(vm.readInt(p+8n),1n);equal(vm.readInt(p+12n),1n);equal(vm.readInt(p+16n),8n);
 const cross=0x24ff0n;put(vm,cross,28n);vm.write(cross+4n,new Uint8Array(24).fill(0x55));frame(t,'Thread32Next',[h,cross],'kernel32.dll');
 equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(cross+4n),0x55555555n);
 equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(cross+8n),2n);equal(vm.readInt(cross+16n),6n);
 put(vm,p,28n);equal(api(t,'Thread32Next',[h,p]),0n);equal(vm.readInt(t.error),18n);put(vm,t.sp-8n,28n);equal(api(t,'Thread32First',[h,t.sp-8n]),1n);equal(vm.ip,vm.entryPoints.returned);
 put(vm,p,28n);equal(api(t,'Thread32First',[h,p]),1n);
 equal(api(t,'ResumeThread',[worker]),1n);frame(t,'WaitForSingleObject',[worker,0xffffffffn],'kernel32.dll');await finish(t);
 const after=api(t,'CreateToolhelp32Snapshot',[6n,0n]);put(vm,p,BigInt(sizeA));equal(api(t,'Process32First',[after,p]),1n);equal(vm.readInt(p+count),2n);
 put(vm,p,28n);equal(api(t,'Thread32First',[after,p]),1n);equal(api(t,'Thread32Next',[after,p]),1n);equal(vm.readInt(p+8n),3n);
 equal(api(t,'Thread32Next',[h,p]),1n);equal(vm.readInt(p+8n),2n);equal(vm.readInt(p+16n),6n);
 if(mode===64){equal(api(t,'Thread32First',[h|(1n<<32n),0xdead0000n]),0n);equal(vm.readInt(t.error),6n);}
 put(vm,p,0x5555n,width);equal(api(t,'DuplicateHandle',[invalid,h,invalid,p,0n,0n,2n]),0n);equal(vm.readInt(t.error),50n);equal(vm.readInt(p,width),0x5555n);
 equal(api(t,'SetHandleInformation',[h,2n,2n]),1n);equal(api(t,'CloseHandle',[h]),0n);equal(vm.readInt(t.error),5n);
 equal(api(t,'SetHandleInformation',[h,2n,0n]),1n);equal(api(t,'CloseHandle',[h]),1n);equal(api(t,'Thread32First',[h,0xdead0000n]),0n);equal(vm.readInt(t.error),6n);
 equal(api(t,'CloseHandle',[after]),1n);
 for(const kind of [2n,4n]){const only=api(t,'CreateToolhelp32Snapshot',[kind,0n]);put(vm,p,kind===2n?28n:BigInt(sizeA));equal(api(t,kind===2n?'Thread32First':'Process32First',[only,p]),0n);equal(vm.readInt(t.error),18n);equal(api(t,'CloseHandle',[only]),1n);}
 const handles=[];for(let n=0;n<128;n++){const value=api(t,'CreateToolhelp32Snapshot',[2n,0n]);assert.ok(value!==invalid);handles.push(value);}
 equal(api(t,'CreateToolhelp32Snapshot',[2n,0n]),invalid);equal(vm.readInt(t.error),8n);equal(api(t,'CreateToolhelp32Snapshot',[8n,0n]),invalid);equal(vm.readInt(t.error),50n);
 for(const handle of handles)equal(api(t,'CloseHandle',[handle]),1n);
 const next=api(t,'CreateToolhelp32Snapshot',[2n,0n]);assert.ok(next>handles.at(-1));equal(api(t,'CloseHandle',[next]),1n);
 for(const name of ['CreateToolhelp32Snapshot','Process32First','Process32NextW','Thread32First','Thread32Next'])equal(vm.bind('kernelbase.dll',name),vm.bind('kernel32.dll',name));
 assert.throws(()=>vm.bind('kernel32.dll','Process32FirstA'));equal(api(t,'CloseHandle',[worker]),1n);equal(api(t,'CloseHandle',[late]),1n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}
console.log(`${checks} Toolhelp layouts/captured identities/priorities/cursors/flags/limits/faults checks passed`);
