import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';
const names=['SHGetFolderPathA','SHGetFolderPathW','SHGetSpecialFolderPathA','SHGetSpecialFolderPathW'];
const folders=[[5n,'C:\\Users\\wire\\Documents'],[0x1an,'C:\\Users\\wire\\AppData\\Roaming'],[0x1cn,'C:\\Users\\wire\\AppData\\Local'],[0x24n,'C:\\Windows'],[0x25n,'C:\\Windows\\System32'],[0x28n,'C:\\Users\\wire']];
const output=0x24000n;
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const api=(t,name,id,{p=output,create=0n,flags=0n,token=0n}={})=>call(t,name,name.includes('Special')?[0n,p,id,create]:[0n,id,token,flags,p],'shell32.dll');
const result=(name,code=0n)=>code?name.includes('Special')?0n:0x80070000n|code:name.includes('Special')?1n:0n;
for(const mode of [32,64]){
 const t=await machine(mode);for(const name of names){assert.ok(t.vm.bind('shell32.dll',name));for(const dll of ['kernel32.dll','user32.dll'])assert.throws(()=>t.vm.bind(dll,name),/migrated|unknown/);}t.vm.dispose();
}

// Both old folder families resolve the configured virtual profile, with ANSI/Wide outputs and no allocation.
for(const mode of [32,64])for(const name of names){
 const t=await machine(mode),{vm}=t,wide=name.endsWith('W');vm.write(t.error,encode(77n,4));vm.flags=0x8d5n;vm.xmm(2,new Uint8Array(16).fill(0x72));
 for(const[id,path]of folders){const expected=new Uint8Array(Buffer.from(path+'\0',wide?'utf16le':'ascii'));vm.write(output,new Uint8Array(260*2).fill(0xa5));equal(api(t,name,id|0x4000n),result(name));equal(bytes(vm,output,expected.length),expected);equal(vm.readInt(output+BigInt(expected.length),1),0xa5n);equal(api(t,name,id|0x4000n,{p:0n}),result(name));equal(vm.readInt(t.error),77n);}
 equal(api(t,name,0x4005n,{flags:1n}),result(name));equal(api(t,name,0x4005n,{p:t.sp}),result(name));equal(vm.ip,vm.entryPoints.returned);equal(vm.flags,0x8d5n);equal(vm.xmm(2),new Uint8Array(16).fill(0x72));equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Default lookup verifies VFS directories; CREATE publishes the same path queried through the known-folder API.
for(const mode of [32,64])for(const name of names){
 const t=await machine(mode),{vm}=t,path=folders[0][1],wide=name.endsWith('W');vm.write(t.error,encode(77n,4));vm.write(output,new Uint8Array(512).fill(0xa5));equal(api(t,name,5n),result(name,3n));equal(vm.readInt(output,wide?2:1),0n);equal(vm.readInt(output+BigInt(wide?2:1),1),0xa5n);equal(vm.readInt(t.error),name.includes('Special')?3n:77n);
 equal(api(t,name,0x8005n,{create:1n}),result(name));vm.write(0x23000n,Buffer.from(path+'\0','utf16le'));equal(call(t,'GetFileAttributesW',[0x23000n],'kernel32.dll')&0x10n,0x10n);equal(api(t,name,5n),result(name));
 vm.write(0x22000n,Buffer.from('d09ad3fd8f23af46adb46c85480369c7','hex'));equal(call(t,'SHGetKnownFolderPath',[0x22000n,0n,0n,0x22100n],'shell32.dll'),0n);const p=vm.readInt(0x22100n,mode/8);equal(bytes(vm,p,(path.length+1)*2),new Uint8Array(Buffer.from(path+'\0','utf16le')));call(t,'CoTaskMemFree',[p],'ole32.dll');vm.dispose();
}

// Unsupported IDs/tokens and malformed flags return explicit failures, clear only the terminator, and retain HRESULT LastError.
for(const mode of [32,64])for(const name of names){
 const t=await machine(mode),{vm}=t,wide=name.endsWith('W');for(const[id,options,code]of [[0x4000n,{},50n],[0x14005n,{},87n],...(!name.includes('Special')?[[0x4005n,{token:1n},50n],[0x4005n,{flags:2n},87n]]:[])]){vm.write(t.error,encode(77n,4));vm.write(output,new Uint8Array(16).fill(0xa5));equal(api(t,name,id,options),result(name,code));equal(vm.readInt(output,wide?2:1),0n);equal(vm.readInt(output+BigInt(wide?2:1),1),0xa5n);equal(vm.readInt(t.error),name.includes('Special')?code:77n);}vm.dispose();
}

// Full output and initial frame faults precede directory creation; BOOL error output and terminator commit atomically.
for(const mode of [32,64])for(const name of names)for(const kind of ['output','return','error']){
 if(kind==='error'&&!name.includes('Special'))continue;
 const t=await machine(mode),{vm}=t,p=kind==='output'?0x24ffen:output,id=kind==='error'?5n:0x8005n,args=name.includes('Special')?[0n,p,id,0n]:[0n,id,0n,0n,p];vm.write(p,new Uint8Array(128).fill(0xa5));frame(t,name,args,'shell32.dll');const page=kind==='output'?0x25000n:kind==='return'?t.sp&~4095n:t.image.teb;equal(core.wire_protect(t.handle,page,4096n,kind==='return'?1:2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(p,name.endsWith('W')?2:1),name.endsWith('W')?0xa5a5n:0xa5n);equal(core.wire_protect(t.handle,page,4096n,4),0);vm.write(0x23000n,Buffer.from(folders[0][1]+'\0','utf16le'));equal(call(t,'GetFileAttributesW',[0x23000n],'kernel32.dll'),0xffffffffn);frame(t,name,args,'shell32.dll');equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),result(name,kind==='error'?3n:0n));equal(call(t,'GetFileAttributesW',[0x23000n],'kernel32.dll'),kind==='error'?0xffffffffn:0x10n);vm.dispose();
}

// Static imports use the appropriate pointer width and call-frame convention.
for(const mode of [32,64]){
 const f=fixture(mode,{tls:false}),ptr=mode/8;f.bytes.fill(0,f.offset(0x2000),f.offset(0x2600));f.rput(0x2000,0x2080);f.rput(0x200c,0x2180);f.rput(0x2010,0x2100);f.rtext(0x2180,'shell32.dll\0');let at=0x2200;for(const[i,name]of names.entries()){f.rput(0x2080+i*ptr,at,ptr);f.rput(0x2100+i*ptr,at,ptr);f.rtext(at+2,name+'\0');at+=name.length+3;}
 if(mode===32){f.bytes.set([0x68,...encode(f.preferred+0x3000n,4),0x6a,0,0x6a,0,0x68,...encode(0x4005n,4),0x6a,0,0xff,0x15,...encode(f.preferred+0x2104n,4),0xc3],f.offset(0x1000));f.relocations.splice(0,f.relocations.length,0x1001,0x1012);}
 else{const code=[0x48,0x83,0xec,40,0x31,0xc9,0xba,...encode(0x4005n,4),0x45,0x31,0xc0,0x45,0x31,0xc9,0x48,0x8d,0x05];code.push(...encode(0x3000n-0x1000n-BigInt(code.length)-4n,4),0x48,0x89,0x44,0x24,32,0xff,0x15);code.push(...encode(0x2108n-0x1000n-BigInt(code.length)-4n,4),0x48,0x83,0xc4,40,0xc3);f.bytes.set(code,f.offset(0x1000));f.relocations.splice(0,f.relocations.length);}
 f.rebuildRelocations();const vm=new WasmMachine(core,{mode}),image=vm.load(f.bytes);for(const[i,name]of names.entries())equal(vm.readInt(image.base+0x2100n+BigInt(i*ptr),ptr),vm.bind('shell32.dll',name));equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),0n);equal(bytes(vm,image.base+0x3000n,(folders[0][1].length+1)*2),new Uint8Array(Buffer.from(folders[0][1]+'\0','utf16le')));assert.ok(vm.retired>0n);vm.dispose();
}
{
 const t=await machine(64),high=0x123400000000n;t.vm.map(high,4096n,3);equal(api(t,'SHGetFolderPathW',0x4005n,{p:high}),0n);equal(bytes(t.vm,high,(folders[0][1].length+1)*2),new Uint8Array(Buffer.from(folders[0][1]+'\0','utf16le')));t.vm.dispose();
}
console.log(`${checks} CSIDL/profile/VFS/ANSI/Wide/ABI/JIT/fault checks passed`);
