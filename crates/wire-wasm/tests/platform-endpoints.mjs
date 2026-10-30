import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,window} from './gdi-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';

const groups=[
 ['crypt32.dll',['CertOpenStore','CertCloseStore','CertAddEncodedCertificateToStore','CertFreeCertificateContext','CertGetCertificateChain','CertFreeCertificateChain','CertVerifyCertificateChainPolicy']],
 ['iphlpapi.dll',['IcmpCreateFile','Icmp6CreateFile','IcmpCloseHandle','IcmpSendEcho','IcmpSendEcho2','IcmpSendEcho2Ex','Icmp6SendEcho2','IcmpParseReplies','Icmp6ParseReplies']],
 ['uiautomationcore.dll',['UiaClientsAreListening','UiaHostProviderFromHwnd','UiaRaiseAutomationEvent','UiaRaiseAutomationPropertyChangedEvent','UiaRaiseNotificationEvent','UiaRaiseStructureChangedEvent','UiaReturnRawElementProvider']],
];
const owner=name=>groups.find(([,names])=>names.includes(name))[0];
const api=(t,name,args=[])=>call(t,name,args,owner(name));
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const mark=t=>t.vm.write(t.error,encode(123n,4));
const error=(t,n)=>equal(t.vm.readInt(t.error),BigInt(n));
const property='UiaRaiseAutomationPropertyChangedEvent';
function propertyFrame(t,{provider=1n,old=0x24000n,next=0x24100n}={}){
 if(t.mode===64)frame(t,property,[provider,30000n,old,next],owner(property));
 else frame(t,property,[provider,30000n,...[old,next].flatMap(p=>[0n,4n,8n,12n].map(o=>t.vm.readInt(p+o)))],owner(property));
}

// Missing backends must not publish objects, trusted status, callbacks or replies.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,hwnd=await window(t),ptr=mode/8;
 const clock=t.time.calls,wall=t.time.wallCalls;
 vm.write(0x24000n,new Uint8Array(512).fill(0xa5));
 mark(t);equal(api(t,'CertOpenStore',[1n,1n,0n,0n,1n]),0n);error(t,50);
 equal(api(t,'CertAddEncodedCertificateToStore',[0n,1n,1n,100n,1n,0x24000n]),0n);error(t,50);
 equal(api(t,'CertVerifyCertificateChainPolicy',[1n,1n,1n,0x24000n]),0n);error(t,50);
 equal(bytes(vm,0x24000n,512),new Uint8Array(512).fill(0xa5));
 equal(api(t,'CertGetCertificateChain',[0n,1n,0n,0n,1n,0n,0n,0x24000n]),0n);error(t,50);equal(vm.readInt(0x24000n,ptr),0n);
 equal(api(t,'CertGetCertificateChain',[0n,1n,0n,0n,1n,0n,0n,0n]),0n);error(t,87);
 for(const [name,result]of [['CertCloseStore',1n],['CertFreeCertificateContext',1n],['CertFreeCertificateChain',0n]]){mark(t);equal(api(t,name,[0n,0n]),result);error(t,123);}
 for(const h of [1n,...(mode===64?[0x100000000n]:[])]){equal(api(t,'CertCloseStore',[h,0n]),0n);error(t,6);}
 for(const name of ['CertFreeCertificateContext','CertFreeCertificateChain']){
  frame(t,name,[1n],owner(name));const before=snapshot(t),work=vm.work;
  assert.throws(()=>vm.service(),/owned certificate context/);equal(snapshot(t),before);equal(vm.work,work);
 }
 for(const name of ['IcmpCreateFile','Icmp6CreateFile']){equal(api(t,name),BigInt.asUintN(mode,-1n));error(t,50);}
 for(const [name,count]of [['IcmpCloseHandle',1],['IcmpSendEcho',8],['IcmpSendEcho2',11],['IcmpSendEcho2Ex',12],['Icmp6SendEcho2',12]]){
  for(const handle of [0n,1n,BigInt.asUintN(mode,-1n)]){
   equal(api(t,name,[handle,...Array(count-1).fill(1n)]),0n);error(t,6);equal(vm.register(4),t.sp+BigInt(mode===32?4+count*4:8));
  }
 }
 for(const name of ['IcmpParseReplies','Icmp6ParseReplies']){equal(api(t,name,[1n,0xffffffffn]),0n);error(t,50);}
 mark(t);equal(api(t,'UiaClientsAreListening'),0n);equal(api(t,'UiaReturnRawElementProvider',[hwnd,1n,1n,1n]),0n);error(t,123);
 for(const [h,result]of [[hwnd,0x80004001n],[0n,0x80070006n],...(mode===64?[[hwnd&0xffffffffn,0x80070006n]]:[])]){
  vm.write(0x24000n,new Uint8Array(ptr).fill(0xa5));equal(api(t,'UiaHostProviderFromHwnd',[h,0x24000n]),result);equal(vm.readInt(0x24000n,ptr),0n);error(t,123);
 }
 equal(api(t,'UiaHostProviderFromHwnd',[hwnd,0n]),0x80004003n);error(t,123);
 for(const [name,tail]of [['UiaRaiseAutomationEvent',[20000n]],['UiaRaiseNotificationEvent',[0n,0n,1n,1n]],['UiaRaiseStructureChangedEvent',[0n,1n,0xffffffffn]]]){
  for(const provider of [0n,1n]){equal(api(t,name,[provider,...tail]),provider===0n?0x80070057n:0x80004001n);error(t,123);}
 }
 vm.write(0x24000n,new Uint8Array(24).fill(0xa5));vm.write(0x24100n,new Uint8Array(24).fill(0x5a));
 for(const provider of [0n,1n]){
  propertyFrame(t,{provider});equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),provider===0n?0x80070057n:0x80004001n);equal(vm.register(4),t.sp+BigInt(mode===32?44:8));error(t,123);
 }
 equal(bytes(vm,0x24000n,24),new Uint8Array(24).fill(0xa5));equal(bytes(vm,0x24100n,24),new Uint8Array(24).fill(0x5a));
 // Full-width pointer outputs may cover the saved return slot.
 frame(t,'CertGetCertificateChain',[0n,1n,0n,0n,1n,0n,0n,t.sp],owner('CertGetCertificateChain'));
 equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp,ptr),0n);error(t,50);
 frame(t,'UiaHostProviderFromHwnd',[hwnd,t.sp],owner('UiaHostProviderFromHwnd'));
 equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp,ptr),0n);equal(vm.register(0),0x80004001n);
 equal(t.time.calls,clock);equal(t.time.wallCalls,wall);
 for(const[dll,names]of groups)for(const name of names){assert.ok(vm.bind(dll,name));assert.throws(()=>vm.bind('user32.dll',name),/unknown|migrated/);}
 vm.dispose();
}

// Output, TEB and return-frame failures retain all earlier state and allow retry.
for(const mode of [32,64])for(const operation of ['chain-output','chain-error','chain-return','uia-output','variant-source']){
 const t=await machine(mode),{vm}=t,ptr=mode/8;
 vm.write(0x24ffen,new Uint8Array(32).fill(0xa5));mark(t);
 let page=0x25000n,protection=2;
 if(operation.startsWith('chain')){
  frame(t,'CertGetCertificateChain',[0n,1n,0n,0n,1n,0n,0n,0x24ffen],owner('CertGetCertificateChain'));
  if(operation==='chain-error'){page=t.image.teb;protection=2;}
  if(operation==='chain-return'){page=t.sp&~4095n;protection=1;}
 }else if(operation==='uia-output')frame(t,'UiaHostProviderFromHwnd',[0n,0x24ffen],owner('UiaHostProviderFromHwnd'));
 else{
  vm.write(0x24000n,new Uint8Array(24));vm.write(0x24100n,new Uint8Array(24));
  if(mode===32){t.sp=0x24ff0n;propertyFrame(t);}
  else{vm.write(0x24ff8n,new Uint8Array(24));propertyFrame(t,{old:0x24ff8n});}
  protection=1;
 }
 equal(core.wire_protect(t.handle,page,4096n,protection),0);const before=snapshot(t),work=vm.work,retired=vm.retired;
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.work,work);equal(vm.retired,retired);
 if(operation!=='variant-source')equal(vm.readInt(0x24ffen,2),0xa5a5n);
 equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);
 if(operation.startsWith('chain')){equal(vm.register(0),0n);equal(vm.readInt(0x24ffen,ptr),0n);error(t,50);}
 else{equal(vm.register(0),operation==='uia-output'?0x80070006n:0x80004001n);error(t,123);}
 vm.dispose();
}

// A generated PE binds the complete groups in formal mode and calls both ABIs.
for(const mode of [32,64]){
 const all=[...groups,['kernel32.dll',['GetLastError']]],f=fixture(mode,{tls:false}),slots=new Map();
 f.bytes.fill(0,f.offset(0x2000),f.offset(0x2600));f.put(f.dirs+12,(all.length+1)*20);let at=0x2000+(all.length+1)*20;
 for(const[index,[dll,names]]of all.entries()){
  const desc=0x2000+index*20;f.rput(desc+12,at);f.rtext(at,dll+'\0');at+=dll.length+1;at=Math.ceil(at/f.ptr)*f.ptr;
  const lookup=at;at+=(names.length+1)*f.ptr;const iat=at;at+=(names.length+1)*f.ptr;f.rput(desc,lookup);f.rput(desc+16,iat);
  for(const[i,name]of names.entries()){f.rput(lookup+i*f.ptr,at,f.ptr);f.rput(iat+i*f.ptr,at,f.ptr);f.rtext(at+2,name+'\0');at+=name.length+3;slots.set(name,{dll,rva:iat+i*f.ptr});}
 }
 assert.ok(at<0x2600);const code=[];f.relocations.length=0;
 const invoke=name=>{const rva=BigInt(slots.get(name).rva);code.push(0xff,0x15);if(mode===32){f.relocations.push(0x1000+code.length);code.push(...encode(f.preferred+rva,4));}else code.push(...encode(rva-BigInt(0x1000+code.length+4),4));};
 if(mode===32){for(let n=0;n<8;n++)code.push(0x6a,0);code.push(0x68,...encode(30000n,4),0x6a,1);}
 else{
  code.push(0x48,0x83,0xec,40,0x48,0xc7,0xc1,1,0,0,0,0xba,...encode(30000n,4));
  for(const [reg,rva]of [[0xb8,0x3000n],[0xb9,0x3020n]]){code.push(0x49,reg);f.relocations.push(0x1000+code.length);code.push(...encode(f.preferred+rva,8));}
 }
 invoke(property);code.push(0x3d,...encode(0x80004001n,4),0x75,0);const branch=code.length-1;
 invoke('IcmpCreateFile');invoke('GetLastError');if(mode===64)code.push(0x48,0x83,0xc4,40);code.push(0xc3);
 const failed=code.length;code.push(0xb8,...encode(0xdeadn,4));if(mode===64)code.push(0x48,0x83,0xc4,40);code.push(0xc3);code[branch]=failed-branch-1;
 f.bytes.set(code,f.offset(0x1000));f.rebuildRelocations();
 const vm=new WasmMachine(core,{mode,clock(){throw Error('unexpected clock');}}),image=vm.load(f.bytes);
 for(const[name,{dll,rva}]of slots)equal(vm.readInt(image.base+BigInt(rva),f.ptr),vm.bind(dll,name));
 equal((await vm.run(200n)).reason,Exit.Returned);equal(vm.register(0),50n);assert.ok(vm.retired>0n);vm.dispose();
}
console.log(`${checks} optional certificate/ICMP/UIA ABI, failure, ownership and formal-import checks passed`);
