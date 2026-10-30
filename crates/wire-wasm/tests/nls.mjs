import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';
const api=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const utf16=s=>Array.from({length:s.length},(_,i)=>Array.from(encode(BigInt(s.charCodeAt(i)),2))).flat();
const bytes=(vm,p,n)=>Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const string=(vm,p,s)=>vm.write(p,utf16(s+'\0'));
const scalarApis=['GetACP','GetOEMCP','GetConsoleCP','GetConsoleOutputCP'];
const localeApis=['GetUserDefaultLCID','GetSystemDefaultLCID','GetThreadLocale','GetUserDefaultLangID','GetSystemDefaultLangID'];
const names=[...scalarApis,...localeApis,'GetCPInfo','GetCPInfoExA','GetCPInfoExW','IsValidCodePage','IsDBCSLeadByte','IsDBCSLeadByteEx','MultiByteToWideChar','WideCharToMultiByte','GetStringTypeW','GetUserDefaultLocaleName','GetSystemDefaultLocaleName','GetLocaleInfoA','GetLocaleInfoW','GetLocaleInfoEx','LocaleNameToLCID','LCIDToLocaleName','IsValidLocaleName','LCMapStringW','LCMapStringEx'];

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,src=0x22000n,dst=0x23000n,used=0x24000n;
 vm.flags=0x8d5n;vm.xmm(3,new Uint8Array(16).fill(0xa5));const fpu=fp(t);vm.write(t.error,encode(123n,4));
 for(const name of scalarApis)equal(api(t,name),932n);for(const name of localeApis)equal(api(t,name),0x411n);
 for(const [cp,input,expected]of [[932,[0x82,0xa0,0x82,0xa2,0],utf16('あい\0')],[1252,[0x80,0xe9,0],utf16('€é\0')],[65001,[0xf0,0x9f,0x98,0x80,0],utf16('😀\0')]]){
  vm.write(src,input);equal(api(t,'MultiByteToWideChar',[BigInt(cp),8n,src,0xffffffffn,0n,0n]),BigInt(expected.length/2));equal(api(t,'MultiByteToWideChar',[BigInt(cp),8n,src,0xffffffffn,dst,100n]),BigInt(expected.length/2));equal(bytes(vm,dst,expected.length),expected);
  equal(api(t,'WideCharToMultiByte',[BigInt(cp),0n,dst,BigInt(expected.length/2),src,100n,0n,cp===65001?0n:used]),BigInt(input.length));equal(bytes(vm,src,input.length),input);if(cp!==65001)equal(vm.readInt(used),0n);
 }
 equal(vm.readInt(t.error),123n);
 vm.write(src,[65,0,66]);equal(api(t,'MultiByteToWideChar',[65001n,0n,src,3n,dst,100n]),3n);equal(bytes(vm,dst,6),utf16('A\0B'));
 vm.write(src,[0xc0,0xaf]);vm.write(dst,new Uint8Array(8).fill(0xa5));equal(api(t,'MultiByteToWideChar',[65001n,8n,src,2n,dst,100n]),0n);equal(vm.readInt(t.error),1113n);equal(bytes(vm,dst,8),new Array(8).fill(0xa5));equal(api(t,'MultiByteToWideChar',[65001n,0n,src,2n,dst,100n]),2n);equal(bytes(vm,dst,4),utf16('\ufffd\ufffd'));
 vm.write(src,encode(0xd800n,2));equal(api(t,'WideCharToMultiByte',[65001n,0x80n,src,1n,dst,100n,0n,0n]),0n);equal(vm.readInt(t.error),1113n);equal(api(t,'WideCharToMultiByte',[65001n,0n,src,1n,dst,100n,0n,0n]),3n);equal(bytes(vm,dst,3),[0xef,0xbf,0xbd]);
 for(const [name,args,code]of [
  ['MultiByteToWideChar',[65001n,1n,0xdead0000n,1n,dst,1n],1004n],
  ['MultiByteToWideChar',[932n,2n,0xdead0000n,1n,dst,1n],50n],
  ['MultiByteToWideChar',[932n,0n,src,0n,dst,1n],87n],
  ['MultiByteToWideChar',[932n,0n,src,0xfffffffen,dst,1n],87n],
  ['MultiByteToWideChar',[932n,0n,0xdead0000n,1n,dst,0xffffffffn],87n],
  ['WideCharToMultiByte',[65001n,0n,0xdead0000n,1n,dst,1n,used,0n],87n],
  ['WideCharToMultiByte',[65001n,0n,0xdead0000n,1n,dst,1n,0n,used],87n],
  ['WideCharToMultiByte',[1252n,0n,src,1n,src,10n,0n,0n],87n],
 ]){equal(api(t,name,args),0n);equal(vm.readInt(t.error),code);}
 string(vm,src,'😀');vm.write(0x25000n,[33]);equal(api(t,'WideCharToMultiByte',[1252n,0n,src,2n,dst,100n,0x25000n,used]),1n);equal(bytes(vm,dst,1),[33]);equal(vm.readInt(used),1n);
 string(vm,src,'€');equal(api(t,'WideCharToMultiByte',[1252n,0x400n,src,1n,dst,100n,0n,used]),1n);equal(bytes(vm,dst,1),[0x80]);equal(vm.readInt(used),0n);
 string(vm,src,'\u301c');equal(api(t,'WideCharToMultiByte',[932n,0x400n,src,1n,dst,100n,0n,used]),1n);equal(bytes(vm,dst,1),[63]);equal(vm.readInt(used),1n);
 string(vm,src,'AbZ');equal(api(t,'LCMapStringW',[0x411n,0x100n,src,0xffffffffn,dst,100n]),4n);equal(bytes(vm,dst,8),utf16('abz\0'));string(vm,0x25000n,'en-US');equal(api(t,'LCMapStringEx',[0x25000n,0x200n,src,3n,dst,100n,0n,0n,0n]),3n);equal(bytes(vm,dst,6),utf16('ABZ'));
 string(vm,src,'Aa0 \t');equal(api(t,'GetStringTypeW',[1n,src,5n,dst]),1n);const types=[0n,1n,2n,3n,4n].map(n=>vm.readInt(dst+n*2n,2));equal(types.map((n,i)=>n&[1n,2n,4n,8n,0x20n][i]),[1n,2n,4n,8n,0x20n]);
 equal(vm.flags,0x8d5n);equal(vm.xmm(3),new Uint8Array(16).fill(0xa5));equal(fp(t),fpu);equal(t.time.calls,0);vm.dispose();
}

// Every guest output is validated before conversion bytes or BOOL metadata publish.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,src=0x22000n,dst=0x23000n,used=0x24ffen;
 string(vm,src,'😀');vm.write(dst,[0xa5]);vm.write(used,new Uint8Array(4).fill(0xa5));frame(t,'WideCharToMultiByte',[1252n,0n,src,2n,dst,10n,0n,used],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(vm,dst,1),[0xa5]);equal(bytes(vm,used,2),[0xa5,0xa5]);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(bytes(vm,dst,1),[63]);equal(vm.readInt(used),1n);
 equal(api(t,'WideCharToMultiByte',[1252n,0n,src,2n,dst,10n,0n,dst]),1n);equal(vm.readInt(dst),1n);
 vm.write(src,[0x82,0xa0,0]);vm.write(used,[0xa5,0xa5]);frame(t,'MultiByteToWideChar',[932n,0n,src,3n,used,2n],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(vm,used,2),[0xa5,0xa5]);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(used,2),0x3042n);
 frame(t,'MultiByteToWideChar',[65001n,1n,0xdead0000n,1n,dst,1n],'kernel32.dll');equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0n);equal(vm.readInt(t.error),1004n);
 vm.write(src,[65,66]);equal(api(t,'MultiByteToWideChar',[65001n,0n,src,2n,t.sp,2n]),2n);equal(vm.ip,vm.entryPoints.returned);equal(bytes(vm,t.sp,4),utf16('AB'));
 vm.write(used,[0x82,0xa0,0]);equal(core.wire_protect(t.handle,0x25000n,4096n,1),0);equal(api(t,'MultiByteToWideChar',[932n,0n,used,2n,dst,10n]),1n);equal(vm.readInt(dst,2),0x3042n);frame(t,'MultiByteToWideChar',[932n,0n,used,3n,dst,10n],'kernel32.dll');before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),2n);
 for(const name of ['GetCPInfo','GetCPInfoExA','GetCPInfoExW']){const pointer=0x24ff0n;vm.write(pointer,new Uint8Array(16).fill(0xa5));frame(t,name,name==='GetCPInfo'?[932n,pointer]:[932n,0n,pointer],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(vm,pointer,16),new Array(16).fill(0xa5));equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),1n);}
 equal(t.time.calls,0);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,out=0x23000n,input=0x22000n;
 for(const [cp,max]of [[932,2],[1252,1],[65001,4],[0,2],[1,2],[3,2]]){
  equal(api(t,'GetCPInfo',[BigInt(cp),out]),1n);equal(vm.readInt(out),BigInt(max));equal(vm.readInt(out+4n,1),63n);equal(bytes(vm,out+6n,4),max===2?[0x81,0x9f,0xe0,0xfc]:[0,0,0,0]);
  for(const suffix of ['A','W']){equal(api(t,'GetCPInfoEx'+suffix,[BigInt(cp),0n,out]),1n);equal(vm.readInt(out+20n),BigInt(cp<=3?932:cp));}
 }
 for(let b=0;b<256;b++){const expected=b>=0x81&&b<=0x9f||b>=0xe0&&b<=0xfc;equal(api(t,'IsDBCSLeadByte',[BigInt(b)]),BigInt(expected));equal(api(t,'IsDBCSLeadByteEx',[932n,BigInt(b)]),BigInt(expected));equal(api(t,'IsDBCSLeadByteEx',[1252n,BigInt(b)]),0n);}
 for(const [cp,valid]of [[932,1],[1252,1],[65001,1],[0,0],[1,0],[3,0],[42,0]])equal(api(t,'IsValidCodePage',[BigInt(cp)]),BigInt(valid));
 string(vm,input,'en');equal(api(t,'LocaleNameToLCID',[input,0n]),0x409n);equal(api(t,'LocaleNameToLCID',[input,0x8000000n]),9n);equal(api(t,'LCIDToLocaleName',[9n,out,100n,0n]),6n);equal(bytes(vm,out,12),utf16('en-US\0'));equal(api(t,'LCIDToLocaleName',[9n,out,100n,0x8000000n]),3n);equal(bytes(vm,out,6),utf16('en\0'));
 for(const name of ['GetUserDefaultLocaleName','GetSystemDefaultLocaleName']){equal(api(t,name,[0n,0n]),0n);equal(vm.readInt(t.error),122n);equal(api(t,name,[out,0xffffffffn]),0n);equal(vm.readInt(t.error),122n);equal(api(t,name,[out,100n]),6n);equal(bytes(vm,out,12),utf16('ja-JP\0'));}
 equal(api(t,'LCIDToLocaleName',[0x411n,0n,0n,0n]),6n);equal(api(t,'GetLocaleInfoW',[0x409n,0x20001004n,out,2n]),2n);equal(vm.readInt(out),1252n);equal(api(t,'GetLocaleInfoA',[0x409n,0x20001004n,out,4n]),4n);equal(vm.readInt(out),1252n);
 string(vm,input,'ja-JP');equal(api(t,'GetLocaleInfoEx',[input,0x5cn,0n,0n]),6n);equal(api(t,'GetLocaleInfoEx',[input,0x5cn,out,6n]),6n);equal(bytes(vm,out,12),utf16('ja-JP\0'));equal(api(t,'GetLocaleInfoA',[0x409n,0x59n,out,10n]),3n);equal(bytes(vm,out,3),[101,110,0]);
 string(vm,input,'zz-ZZ');vm.write(t.error,encode(123n,4));equal(api(t,'IsValidLocaleName',[input]),0n);equal(vm.readInt(t.error),123n);equal(api(t,'LocaleNameToLCID',[input,0n]),0n);equal(vm.readInt(t.error),87n);
 for(const name of names){equal(vm.bind('kernelbase.dll',name),vm.bind('kernel32.dll',name));assert.throws(()=>vm.bind('user32.dll',name),/unknown/);}equal(t.time.calls,0);vm.dispose();
}
console.log(`${checks} NLS encoding/locale/ABI/output/fault checks passed (${names.length} services)`);
