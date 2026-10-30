import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';

const folders=[
 ['f1b32785-6fba-4fcf-9d55-7b8e7f157091','C:\\Users\\wire\\AppData\\Local'],
 ['3eb685db-65f9-4cf6-a03a-e3ef65729f3d','C:\\Users\\wire\\AppData\\Roaming'],
 ['5e6c858f-0e22-4760-9afe-ea3317b67173','C:\\Users\\wire'],
 ['a520a1a4-1780-4ff6-bd18-167343c5af16','C:\\Users\\wire\\AppData\\LocalLow'],
 ['fdd39ad0-238f-46af-adb4-6c85480369c7','C:\\Users\\wire\\Documents'],
];
function guid(text){const parts=text.split('-'),bytes=[];for(let n=0;n<3;n++)bytes.push(...encode(BigInt('0x'+parts[n]),n===0?4:2));return Uint8Array.from([...bytes,...Buffer.from(parts[3]+parts[4],'hex')]);}
function text(vm,p){let s='';for(let n=0;n<260;n++){const c=Number(vm.readInt(p+BigInt(n*2),2));if(!c)return s;s+=String.fromCharCode(c);}throw new Error('unterminated folder result');}
const api=(t,args)=>call(t,'SHGetKnownFolderPath',args,'shell32.dll');
const free=(t,p)=>call(t,'CoTaskMemFree',[p],'ole32.dll');

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,id=0x22000n,out=0x22100n;
 vm.write(t.error,encode(123n,4));vm.flags=0x8d5n;vm.xmm(3,new Uint8Array(16).fill(0x7b));const fpu=fp(t);
 for(const [index,[key,path]] of folders.entries()){
  vm.write(id,guid(key));vm.write(out,new Uint8Array(16).fill(0xa5));
  equal(api(t,[id,0n,0n,out]),index<3?0n:0x80070003n);
  let p=vm.readInt(out,mode/8);
  equal(vm.readInt(out+BigInt(mode/8),4),0xa5a5a5a5n);
  if(index<3){equal(text(vm,p),path);equal(free(t,p),0n);}else equal(p,0n);
  equal(api(t,[id,0x4000n,0n,out]),0n);p=vm.readInt(out,mode/8);equal(text(vm,p),path);free(t,p);
  if(index>=3){
   vm.write(0x23000n,Buffer.from(path+'\0','utf16le'));
   equal(call(t,'GetFileAttributesW',[0x23000n],'kernel32.dll'),0xffffffffn);vm.write(t.error,encode(123n,4));
   equal(api(t,[id,0x8000n,0n,out]),0n);p=vm.readInt(out,mode/8);equal(text(vm,p),path);free(t,p);
   equal(call(t,'GetFileAttributesW',[0x23000n],'kernel32.dll')&0x10n,0x10n);
  }
 }
 vm.write(id,guid(folders[0][0]));
 for(const [flags,token] of [[1n,0n],[0n,1n],...(mode===64?[[0n,0x100000000n]]:[])]){
  vm.write(out,new Uint8Array(8).fill(0xa5));equal(api(t,[id,flags,token,out]),0x80070032n);equal(vm.readInt(out,mode/8),0n);
 }
 vm.write(id,new Uint8Array(16));equal(api(t,[id,0n,0n,out]),0x80070032n);equal(vm.readInt(out,mode/8),0n);
 vm.write(id,guid(folders[0][0]));
 // Copy-in precedes publishing the pointer, including an input/output alias.
 equal(api(t,[id,0x100004000n,0n,id]),0n);let p=vm.readInt(id,mode/8);equal(text(vm,p),folders[0][1]);free(t,p);
 vm.write(id,guid(folders[0][0]));equal(api(t,[id,0n,0n,t.sp]),0n);p=vm.readInt(t.sp,mode/8);equal(vm.ip,vm.entryPoints.returned);equal(text(vm,p),folders[0][1]);free(t,p);
 if(mode===64){const high=0x123400000000n;vm.map(high,4096n,3);equal(api(t,[id,0n,0n,high]),0n);p=vm.readInt(high,8);equal(text(vm,p),folders[0][1]);free(t,p);}
 for(const dll of ['kernel32.dll','kernelbase.dll'])assert.throws(()=>vm.bind(dll,'SHGetKnownFolderPath'),/unknown/);
 equal(vm.readInt(t.error),123n);equal(vm.flags,0x8d5n);equal(vm.xmm(3),new Uint8Array(16).fill(0x7b));equal(fp(t),fpu);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// An output fault must precede directory creation and task allocation.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,id=0x22000n,out=0x24ffen;
 const expected=call(t,'CoTaskMemAlloc',[128n],'ole32.dll');free(t,expected);
 vm.write(id,guid(folders[3][0]));vm.write(out,[0x7b,0x6c]);frame(t,'SHGetKnownFolderPath',[id,0x8000n,0n,out],'shell32.dll');
 equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(out,2),0x6c7bn);
 vm.write(0x23000n,Buffer.from(folders[3][1]+'\0','utf16le'));
 equal(call(t,'GetFileAttributesW',[0x23000n],'kernel32.dll'),0xffffffffn);
 const available=call(t,'CoTaskMemAlloc',[128n],'ole32.dll');equal(available,expected);free(t,available);
 frame(t,'SHGetKnownFolderPath',[id,0x8000n,0n,out],'shell32.dll');
 equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);let p=vm.readInt(out,mode/8);equal(text(vm,p),folders[3][1]);free(t,p);
 // A GUID read fault must not overwrite the saved output pointer.
 vm.write(0x22100n,encode(0x5a5an,mode/8));frame(t,'SHGetKnownFolderPath',[0xdead0000n,0n,0n,0x22100n],'shell32.dll');const bad=snapshot(t);
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),bad);equal(vm.readInt(0x22100n,mode/8),0x5a5an);
 vm.dispose();
}
console.log(`${checks} known-folder/profile/task/VFS/ABI/fault checks passed`);
