import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,finish} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';
import {fixture} from './pe-fixture.mjs';
import {WasmMachine} from '../web/runtime.mjs';
const names=['InitSecurityInterfaceA','InitSecurityInterfaceW','EnumerateSecurityPackagesA','EnumerateSecurityPackagesW','QuerySecurityPackageInfoA','QuerySecurityPackageInfoW','AcquireCredentialsHandleA','AcquireCredentialsHandleW','InitializeSecurityContextA','InitializeSecurityContextW','AddCredentialsA','AddCredentialsW','AcceptSecurityContext','FreeContextBuffer','FreeCredentialsHandle','DeleteSecurityContext','ImpersonateSecurityContext','RevertSecurityContext','QueryCredentialsAttributesA','QueryCredentialsAttributesW','QueryContextAttributesA','QueryContextAttributesW','ApplyControlToken','CompleteAuthToken','QuerySecurityContextToken','MakeSignature','VerifySignature','EncryptMessage','DecryptMessage','ExportSecurityContext','ImportSecurityContextA','ImportSecurityContextW','SetContextAttributesA','SetContextAttributesW'];
const slots=['EnumerateSecurityPackages','QueryCredentialsAttributes','AcquireCredentialsHandle','FreeCredentialsHandle',null,'InitializeSecurityContext','AcceptSecurityContext','CompleteAuthToken','DeleteSecurityContext','ApplyControlToken','QueryContextAttributes','ImpersonateSecurityContext','RevertSecurityContext','MakeSignature','VerifySignature','FreeContextBuffer','QuerySecurityPackageInfo',null,null,'ExportSecurityContext','ImportSecurityContext','AddCredentials',null,'QuerySecurityContextToken','EncryptMessage','DecryptMessage','SetContextAttributes'];
const variants=new Set([0,1,2,5,10,16,20,21,26]);
const api=(t,name,args=[])=>call(t,name,args,'secur32.dll');
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,width=mode/8,out=0x24000n,used=0x23000n;
 for(const name of names){assert.ok(vm.bind('secur32.dll',name));for(const dll of ['kernel32.dll','kernelbase.dll','user32.dll'])assert.throws(()=>vm.bind(dll,name),/unknown/);}
 vm.flags=0x8d5n;vm.xmm(3,new Uint8Array(16).fill(0x7b));const fpu=fp(t);vm.write(t.error,encode(123n,4));
 const tables=[];
 for(const suffix of ['A','W']){
  const table=api(t,'InitSecurityInterface'+suffix);assert.ok(table);tables.push(table);equal(api(t,'InitSecurityInterface'+suffix),table);equal(vm.readInt(table),1n);
  for(const [slot,name]of slots.entries())equal(vm.readInt(table+BigInt((slot+1)*width),width),name?vm.bind('secur32.dll',name+(variants.has(slot)?suffix:'')):0n);
  assert.throws(()=>vm.write(table,[0]),/Protection/);equal(api(t,'FreeContextBuffer',[table]),0x80090301n);equal(vm.readInt(table),1n);
  vm.write(out,new Uint8Array(16).fill(0xa5));vm.write(used,encode(99n,4));equal(api(t,'EnumerateSecurityPackages'+suffix,[used,out]),0n);equal(vm.readInt(used),0n);equal(vm.readInt(out,width),0n);equal(vm.readInt(out+BigInt(width),1),0xa5n);
  vm.write(0x22000n,Buffer.from('Unavailable\0',suffix==='W'?'utf16le':'ascii'));
  for(const [name,args]of [['QuerySecurityPackageInfo',[0x22000n,out]],['AcquireCredentialsHandle',[1n,0x22000n,0n,1n,1n,1n,1n,out,out+16n]],['ImportSecurityContext',[0x22000n,1n,1n,out]]]){vm.write(out,encode(0xfeedn,width));equal(api(t,name+suffix,args),0x80090305n);equal(vm.readInt(out,width),0xfeedn);args[name==='AcquireCredentialsHandle'?1:0]=0n;equal(api(t,name+suffix,args),0x8009035dn);}
 }
 assert.notEqual(tables[0],tables[1]);equal(api(t,'FreeContextBuffer',[0n]),0n);
 for(const [suffix,bytes]of [['A',[0x81,0]],['W',[0,0xd8,0,0]]]){vm.write(0x22000n,bytes);frame(t,'QuerySecurityPackageInfo'+suffix,[0x22000n,out],'secur32.dll');const before=snapshot(t);assert.throws(()=>vm.service(),/invalid security package name/);equal(snapshot(t),before);}
 for(const name of names.filter(n=>!/^InitSecurityInterface|^EnumerateSecurityPackages|^QuerySecurityPackageInfo|^AcquireCredentialsHandle|^ImportSecurityContext/.test(n)))equal(api(t,name,Array(12).fill(1n)),0x80090301n);
 equal(api(t,'EnumerateSecurityPackagesA',[out,out]),0n);equal(vm.readInt(out,width),0n);equal(api(t,'EnumerateSecurityPackagesA',[used,t.sp]),0n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp,width),0n);
 if(mode===64){const high=0x123400000000n;vm.map(high,4096n,3);equal(api(t,'EnumerateSecurityPackagesW',[high,high+8n]),0n);equal(vm.readInt(high),0n);equal(vm.readInt(high+8n,8),0n);}
 equal(vm.readInt(t.error),123n);equal(vm.flags,0x8d5n);equal(vm.xmm(3),new Uint8Array(16).fill(0x7b));equal(fp(t),fpu);equal(t.time.calls,0);equal(t.time.wallCalls,0);
 // Invoke the table's enumeration pointer through actual guest CALL and stdcall completion.
 const target=vm.readInt(tables[0]+BigInt(width),width),code=mode===32?[0x68,...encode(out,4),0x68,...encode(used,4),0xb8,...encode(target,4),0xff,0xd0,0xc3]:[0x48,0x83,0xec,40,0x48,0xb9,...encode(used,8),0x48,0xba,...encode(out,8),0x48,0xb8,...encode(target,8),0xff,0xd0,0x48,0x83,0xc4,40,0xc3];vm.write(0x30000n,code);vm.ip=0x30000n;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,width));await finish(t,1n);equal(vm.register(0),0n);equal(vm.register(4),t.sp+BigInt(width));
 vm.dispose();
}
for(const mode of [32,64])for(const which of ['count','pointer']){
 const t=await machine(mode),{vm}=t,a=which==='count'?0x24ffen:0x23000n,b=which==='pointer'?0x24ffen:0x24000n;vm.write(a,encode(0xfeedn,4));vm.write(b,encode(0xdeadn,mode/8));frame(t,'EnumerateSecurityPackagesA',[a,b],'secur32.dll');const before=snapshot(t);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(a,2),0xfeedn);equal(vm.readInt(b,2),0xdeadn);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(a),0n);equal(vm.readInt(b,mode/8),0n);vm.dispose();
}
for(const mode of [32,64]){
 const f=fixture(mode,{tls:false});f.rtext(0x20c0,'secur32.dll\0');f.rtext(0x20e2,'InitSecurityInterfaceW\0');f.bytes[f.offset(0x1000)]=0xc3;const vm=new WasmMachine(core,{mode}),image=vm.load(f.bytes);equal(vm.readInt(image.base+0x20a0n,mode/8),vm.bind('secur32.dll','InitSecurityInterfaceW'));equal((await vm.run(100n)).reason,Exit.Returned);vm.dispose();
}
console.log(`${checks} SSPI table/ABI/empty-profile/indirect-call/fault checks passed`);
