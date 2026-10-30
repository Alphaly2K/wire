import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';
const api=(t,name,args=[])=>call(t,name,args,'advapi32.dll');
const kernel=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const out=0x23000n,data=0x24000n,used=0x22000n;
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const current=t=>(1n<<BigInt(t.mode))-1n;
function open(t,access=8n,output=out,process=current(t)){equal(api(t,'OpenProcessToken',[process,access,output]),1n);return t.vm.readInt(output,t.mode/8);}
const info=(t,h,c,p=data,n=64n,r=used)=>api(t,'GetTokenInformation',[h,BigInt(c),p,n,r]);

// Tokens encode both ABI layouts and use only virtual user and integrity identities.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 vm.write(0x20000n,new TextEncoder().encode('advapi32.dll\0'));const provider=kernel(t,'LoadLibraryA',[0x20000n]);assert.ok(provider);
 for(const name of ['OpenProcessToken','OpenThreadToken','GetTokenInformation','GetSidSubAuthority','GetSidSubAuthorityCount','GetSidIdentifierAuthority','IsValidSid','GetLengthSid','CopySid','EqualSid','GetSidLengthRequired','InitializeSid']){
  vm.write(0x20000n,new TextEncoder().encode(name+'\0'));equal(kernel(t,'GetProcAddress',[provider,0x20000n]),vm.bind('advapi32.dll',name));
 }
 vm.write(t.error,encode(123n,4));vm.flags=0x8d5n;vm.mxcsr=0x1fa0;vm.xmm(7,new Uint8Array(16).fill(0xa5));const fpu=fp(t),h=open(t);
 equal(h,mode===32?0xa0000000n:0x7ffd05000000n);
 for(const [c,value] of [[8,1n],[18,1n],[20,0n],[29,0n]]){
  vm.write(data,new Uint8Array(8).fill(0xa5));equal(info(t,h,c),1n);equal(vm.readInt(used),4n);equal(vm.readInt(data),value);equal(vm.readInt(data+4n),0xa5a5a5a5n);
 }
 for(const [c,authority,parts,attributes] of [[1,5,[21,1,1,1,1000],0],[25,16,[8192],0x60]]){
  equal(info(t,h,c),1n);const p=vm.readInt(data,mode/8),offset=mode/4;
  equal(p,data+BigInt(offset));equal(vm.readInt(data+BigInt(mode/8)),BigInt(attributes));equal(vm.readInt(used),BigInt(offset+8+parts.length*4));
  equal(bytes(vm,p,8+parts.length*4),Uint8Array.from([1,parts.length,0,0,0,0,0,authority,...parts.flatMap(n=>Array.from(encode(BigInt(n),4)))]));
  equal(api(t,'IsValidSid',[p]),1n);equal(api(t,'GetSidSubAuthorityCount',[p]),p+1n);equal(api(t,'GetSidSubAuthority',[p,BigInt(parts.length-1)]),p+8n+BigInt(parts.length-1)*4n);
 }
 equal(kernel(t,'GetHandleInformation',[h,used]),1n);equal(vm.readInt(used),0n);equal(kernel(t,'SetHandleInformation',[h,3n,3n]),1n);equal(kernel(t,'GetHandleInformation',[h,used]),1n);equal(vm.readInt(used),3n);
 equal(kernel(t,'CloseHandle',[h]),0n);equal(vm.readInt(t.error),6n);equal(kernel(t,'SetHandleInformation',[h,2n,0n]),1n);equal(kernel(t,'CloseHandle',[h]),1n);equal(info(t,h,20),0n);equal(vm.readInt(t.error),6n);
 vm.write(t.error,encode(123n,4));equal(vm.flags,0x8d5n);equal(vm.mxcsr,0x1fa0);equal(vm.xmm(7),new Uint8Array(16).fill(0xa5));equal(fp(t),fpu);equal(t.time.calls,0);equal(t.time.wallCalls,0);
 for(const name of ['OpenProcessToken','OpenThreadToken','GetTokenInformation']){
  const address=vm.bind('advapi32.dll',name);assert.ok(address);for(const dll of ['kernel32.dll','kernelbase.dll','user32.dll'])assert.throws(()=>vm.bind(dll,name),/unknown/);
 }
 vm.dispose();
}

// Real process/thread query handles obey their access rights; non-impersonating threads have no token.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 for(const access of [0x400n,0x1000n]){const process=kernel(t,'OpenProcess',[access,0n,1n]),h=open(t,8n,out,process);equal(info(t,h,20),1n);equal(kernel(t,'CloseHandle',[h]),1n);equal(kernel(t,'CloseHandle',[process]),1n);}
 const limited=kernel(t,'OpenProcess',[0x100000n,0n,1n]);
 for(const [h,access,error] of [[0n,8n,6n],[limited,8n,5n],[current(t),0x20n,50n]]){
  vm.write(out,encode(0xfeedn,mode/8));equal(api(t,'OpenProcessToken',[h,access,out]),0n);equal(vm.readInt(t.error),error);equal(vm.readInt(out,mode/8),0xfeedn);
 }
 const noquery=open(t,0n);equal(info(t,noquery,0xffffffff,0xdead0000n,0n,0xdead0000n),0n);equal(vm.readInt(t.error),5n);
 const h=open(t);vm.write(used,encode(0xfeedn,4));equal(info(t,h,2,0xdead0000n,0n),0n);equal(vm.readInt(t.error),50n);equal(vm.readInt(used),0xfeedn);
 vm.write(data,new Uint8Array(64).fill(0xa5));equal(info(t,h,1,data,0n),0n);equal(vm.readInt(t.error),122n);equal(vm.readInt(used),mode===32?36n:44n);equal(bytes(vm,data,64),new Uint8Array(64).fill(0xa5));
 for(const self of [0n,1n]){equal(api(t,'OpenThreadToken',[current(t)-1n,8n,self,0xdead0000n]),0n);equal(vm.readInt(t.error),1008n);}
 equal(api(t,'OpenThreadToken',[0n,8n,0n,0xdead0000n]),0n);equal(vm.readInt(t.error),6n);
 for(const [access,error] of [[0x40n,1008n],[0x800n,1008n],[0n,5n]]){
  const thread=kernel(t,'OpenThread',[access,0n,1n]);assert.ok(thread);equal(api(t,'OpenThreadToken',[thread,8n,0n,0xdead0000n]),0n);equal(vm.readInt(t.error),error);equal(kernel(t,'CloseHandle',[thread]),1n);
 }
 if(mode===64){equal(info(t,h&0xffffffffn,20),0n);equal(vm.readInt(t.error),6n);equal(open(t,0xabcdef0000000008n),h+1n);}
 vm.dispose();
}

// DuplicateHandle keeps token rights, handle flags, protected-close behavior and process ownership.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=open(t),process=current(t),dup=(access,inherit,options=0n,output=out)=>kernel(t,'DuplicateHandle',[process,h,process,output,access,inherit,options]);
 equal(dup(0n,1n,2n),1n);const alias=vm.readInt(out,mode/8);assert.notEqual(alias,h);equal(info(t,alias,20),1n);equal(kernel(t,'GetHandleInformation',[alias,used]),1n);equal(vm.readInt(used),1n);
 equal(dup(0n,0n),1n);const reduced=vm.readInt(out,mode/8);equal(info(t,reduced,20),0n);equal(vm.readInt(t.error),5n);
 vm.write(out,encode(0xfeedn,mode/8));equal(dup(0x20n,0n),0n);equal(vm.readInt(t.error),5n);equal(vm.readInt(out,mode/8),0xfeedn);
 equal(kernel(t,'SetHandleInformation',[h,2n,2n]),1n);equal(dup(0n,0n,3n),0n);equal(vm.readInt(t.error),6n);equal(info(t,h,20),1n);
 equal(kernel(t,'SetHandleInformation',[h,2n,0n]),1n);equal(dup(0n,0n,3n,t.sp),1n);equal(vm.ip,vm.entryPoints.returned);const moved=vm.readInt(t.sp,mode/8);equal(info(t,h,20),0n);equal(vm.readInt(t.error),6n);equal(info(t,moved,20),1n);equal(info(t,alias,20),1n);
 for(const token of [moved,alias,reduced])equal(kernel(t,'CloseHandle',[token]),1n);
 const other=await machine(mode);equal(info(other,moved,20),0n);equal(other.vm.readInt(other.error),6n);other.vm.dispose();vm.dispose();
}

// Query/open output failures preflight all writes and never consume an identity or complete a call.
for(const mode of [32,64])for(const kind of ['open','required','payload','short-error','return','guard']){
 const t=await machine(mode),{vm}=t;let h;
 if(kind!=='open')h=open(t);
 vm.write(used,encode(0xfeedn,4));vm.write(data,new Uint8Array(64).fill(0xa5));vm.write(t.error,encode(123n,4));vm.write(0x24ffcn,new Uint8Array(8).fill(0xa5));
 let page=0x25000n,protection=2;
 if(kind==='open')frame(t,'OpenProcessToken',[current(t),8n,0x24ffen],'advapi32.dll');
 else frame(t,'GetTokenInformation',[h,25n,kind==='payload'?0x24ffcn:data,kind==='short-error'?0n:64n,kind==='required'||kind==='guard'?0x25000n:used],'advapi32.dll');
 if(kind==='short-error')page=t.image.teb;
 if(kind==='return'){page=t.sp&~4095n;protection=1;}
 if(kind==='guard')protection=0x104;
 equal(core.wire_protect(t.handle,page,4096n,protection),0);const before=snapshot(t),work=vm.work;
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.work,work);equal(vm.readInt(used),0xfeedn);equal(bytes(vm,data,64),new Uint8Array(64).fill(0xa5));equal(vm.readInt(t.error),123n);
 equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),kind==='short-error'?0n:1n);
 if(kind==='open')equal(vm.readInt(0x24ffen,mode/8),mode===32?0xa0000000n:0x7ffd05000000n);
 vm.dispose();
}

// Saved frames, aliased size/data and full-width addresses retain their documented copyout order.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=open(t,8n,t.sp);equal(vm.ip,vm.entryPoints.returned);
 equal(info(t,h,20,used,4n,used),1n);equal(vm.readInt(used),4n);
 equal(info(t,h,20,t.sp,4n,used),1n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp),0n);
 if(mode===64){const high=0x123400000000n;vm.map(high,8192n,3);equal(info(t,h,1,high,44n,high+4096n),1n);equal(vm.readInt(high,8),high+16n);equal(vm.readInt(high+4096n),44n);equal(api(t,'IsValidSid',[high+16n]),1n);}
 vm.dispose();
}

// Simultaneous object kinds route flags, duplication and closure to their own owner.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,token=open(t),thread=kernel(t,'OpenThread',[0x40n,0n,1n]);
 assert.notEqual(token,thread,'simultaneous token and thread handles have distinct identities');
 equal(kernel(t,'SetHandleInformation',[token,2n,2n]),1n);equal(kernel(t,'GetHandleInformation',[thread,used]),1n);equal(vm.readInt(used),0n);
 equal(kernel(t,'DuplicateHandle',[current(t),token,current(t),out,0n,0n,2n]),1n);const alias=vm.readInt(out,mode/8);equal(info(t,alias,20),1n);
 equal(kernel(t,'CloseHandle',[token]),0n);equal(vm.readInt(t.error),6n);equal(kernel(t,'GetThreadId',[thread]),1n);
 equal(kernel(t,'SetHandleInformation',[token,2n,0n]),1n);equal(kernel(t,'CloseHandle',[token]),1n);equal(kernel(t,'GetThreadId',[thread]),1n);equal(kernel(t,'CloseHandle',[thread]),1n);equal(info(t,alias,20),1n);vm.dispose();
}

// Live capacity is reclaimed, but closed handle identities remain retired.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,handles=[];for(let n=0;n<128;n++)handles.push(open(t));
 vm.write(out,encode(0xfeedn,mode/8));equal(api(t,'OpenProcessToken',[current(t),8n,out]),0n);equal(vm.readInt(t.error),8n);equal(vm.readInt(out,mode/8),0xfeedn);
 equal(kernel(t,'CloseHandle',[handles[0]]),1n);equal(open(t),handles[127]+1n);equal(info(t,handles[0],20),0n);equal(vm.readInt(t.error),6n);vm.dispose();
}
console.log(`${checks} token ABI/identity/rights/handles/copyout/ownership/capacity/fault checks passed`);
