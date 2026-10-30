import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';

const api=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const packed=fields=>fields.flatMap(v=>Array.from(encode(BigInt(v),2)));
const utf16=s=>Array.from({length:s.length},(_,i)=>s.charCodeAt(i)).flatMap(v=>Array.from(encode(BigInt(v),2)));
const bytes=(vm,p,n)=>Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const text=(vm,p,s,wide=true)=>vm.write(p,wide?utf16(s+'\0'):Array.from(Buffer.from(s+'\0','latin1')));
const input=0x22000n,picture=0x23000n,out=0x24000n,locale=0x26000n;
const point=[2024,2,65535,29,13,5,7,999];
const names=['GetDateFormatA','GetDateFormatW','GetDateFormatEx','GetTimeFormatA','GetTimeFormatW','GetTimeFormatEx'];

function expected(t,name,args,value){
 const narrow=name.endsWith('A'),data=narrow?Array.from(Buffer.from(value+'\0','latin1')):utf16(value+'\0');
 const required=BigInt(data.length/(narrow?1:2));
 equal(api(t,name,[...args.slice(0,4),0xdead0000n,0n,...args.slice(6)]),required);
 t.vm.write(out,new Uint8Array(data.length+4).fill(0xa5));
 equal(api(t,name,args),required);equal(bytes(t.vm,out,data.length),data);equal(bytes(t.vm,out+BigInt(data.length),4),[165,165,165,165]);
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 vm.write(input,packed(point));vm.write(t.error,encode(123n,4));
 vm.flags=0x8d5n;vm.mxcsr=0x5f81;vm.xmm(3,new Uint8Array(16).fill(0x79));const saved=fp(t);
 for(const [id,label,short,long,month,monthday,time]of [
  [0x409,'en-US','2/29/2024','Thursday, February 29, 2024','February 2024','February 29','1:05:07 PM'],
  [9,'en','2/29/2024','Thursday, February 29, 2024','February 2024','February 29','1:05:07 PM'],
  [0x411,'ja-JP','2024/02/29','2024年2月29日','2024年2月','2月29日','13:05:07'],
  [0x11,'ja','2024/02/29','2024年2月29日','2024年2月','2月29日','13:05:07'],
  [0x7f,'','02/29/2024','Thursday, 29 February 2024','2024 February','February 29','13:05:07'],
 ]){
  text(vm,locale,label);
  for(const suffix of ['A','W','Ex']){
   const idArg=suffix==='Ex'?locale:BigInt(id);
   for(const[flags,value]of [[0,short],[1,short],[2,long],[8,month],[0x80,monthday]]){
    if(suffix==='A'&&(id===0x411||id===0x11)&&flags!==0&&flags!==1)continue;
    expected(t,'GetDateFormat'+suffix,[idArg,BigInt(flags),input,0n,out,100n,0n],value);
   }
   expected(t,'GetTimeFormat'+suffix,[idArg,0n,input,0n,out,100n],time);
  }
 }
 text(vm,locale,'en-US');
 const times=['1:05:07 PM','1 PM','1:05 PM','1 PM','1:05:07','1','1:05','1','13:05:07 PM','13 PM','13:05 PM','13 PM','13:05:07','13','13:05','13'];
 for(let flags=0;flags<16;flags++)for(const suffix of ['A','W','Ex']){
  text(vm,picture,'hh:mm:ss tt',suffix!=='A');
  expected(t,'GetTimeFormat'+suffix,[suffix==='Ex'?locale:0x409n,BigInt(flags),input,0n,out,100n],times[flags]);
  expected(t,'GetTimeFormat'+suffix,[suffix==='Ex'?locale:0x409n,BigInt(flags),input,picture,out,100n],times[flags].replace(/^(1|13)(?=:| |$)/,x=>x.padStart(2,'0')));
 }
 for(const [date,pattern,value,flags=0]of [
  [true,"y yy yyy yyyy M MM MMM MMMM d dd ddd dddd",'24 24 2024 2024 2 02 Feb February 29 29 Thu Thursday'],
  [true,"'year' yyyy '' 'M/d' M/d", "year 2024 ' M/d 2/29"],
  [true,"'unterminated yyyy",'unterminated yyyy'],
  [true,'', ''],[true,'\ud800 yyyy \udfff','\ud800 2024 \udfff'],
  [false,'hhh:mmm:sss ttt','01:05:07 PM'],
  [false,"h 'm:s' tt",'1 m:s PM',3],
  [false,'tt--h:mm:ss','1:05:07',4],
  [false,'H tt:ss','13 PM',2],
  [false,'h:mm:ss tt','13:05:07 PM',8],
 ]){
  text(vm,picture,pattern);expected(t,date?'GetDateFormatW':'GetTimeFormatW',[0x409n,BigInt(flags),input,picture,out,100n],value);
 }
 // Date validation ignores the supplied weekday and all time fields.
 vm.write(input,packed([2024,2,65535,29,65535,65535,65535,65535]));
 text(vm,picture,'dddd yyyy-MM-dd');expected(t,'GetDateFormatW',[0x409n,0n,input,picture,out,100n],'Thursday 2024-02-29');
 for(const [fields,value]of [[[1601,1,9,1,0,0,0,0],'Monday 1601-01-01'],[[9999,12,9,31,0,0,0,0],'Friday 9999-12-31']]){vm.write(input,packed(fields));expected(t,'GetDateFormatW',[0x409n,0n,input,picture,out,100n],value);}
 vm.write(input,packed([65535,65535,65535,65535,0,0,0,0]));expected(t,'GetTimeFormatW',[0x409n,0n,input,0n,out,100n],'12:00:00 AM');
 vm.write(input,packed(point));
 for(const defaultId of [0n,0x400n,0x800n])expected(t,'GetDateFormatW',[defaultId,0n,input,0n,out,100n],'2024/02/29');
 for(const defaultName of [0n,mode===32?0xffffffffn:0xffffffffffffffffn])expected(t,'GetDateFormatEx',[defaultName,0n,input,0n,out,100n,0n],'2024/02/29');
 expected(t,'GetDateFormatW',[0x409n,0x40n,input,0n,out,100n],'2/29/2024');
 equal(vm.readInt(t.error),123n);equal(vm.flags,0x8d5n);equal(vm.mxcsr,0x5f81);equal(vm.xmm(3),new Uint8Array(16).fill(0x79));equal(fp(t),saved);
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.write(input,packed(point));text(vm,picture,'yyyy-MM-dd');
 for(const [name,args,code]of [
  ['GetDateFormatW',[0x409n,0n,input,0n,out,0xffffffffn],87n],
  ['GetDateFormatW',[0x409n,0n,input,0n,0n,10n],87n],
  ['GetDateFormatEx',[0n,0n,input,0n,out,100n,1n],87n],
  ['GetDateFormatW',[0xdeadn,0n,input,0n,out,100n],87n],
  ['GetDateFormatW',[0x409n,3n,0xdead0000n,0n,out,100n],1004n],
  ['GetDateFormatW',[0x409n,0x81n,input,0n,out,100n],1004n],
  ['GetDateFormatW',[0x409n,0x30n,input,0n,out,100n],1004n],
  ['GetDateFormatW',[0x409n,0x50n,input,0n,out,100n],1004n],
  ['GetDateFormatW',[0x409n,4n,input,0n,out,100n],50n],
  ['GetDateFormatW',[0x409n,0x10n,input,0n,out,100n],50n],
  ['GetDateFormatW',[0x409n,1n,input,picture,out,100n],1004n],
  ['GetTimeFormatW',[0x409n,0x10n,input,0n,out,100n],1004n],
  ['GetTimeFormatW',[0x409n,0x80000000n,input,picture,out,100n],1004n],
 ]){vm.write(out,new Uint8Array(16).fill(0xa5));equal(api(t,name,args),0n);equal(vm.readInt(t.error),code);equal(bytes(vm,out,16),new Array(16).fill(0xa5));}
 for(const fields of [[1600,12,0,31,0,0,0,0],[10000,1,0,1,0,0,0,0],[1900,2,0,29,0,0,0,0],[2024,0,0,1,0,0,0,0],[2024,13,0,1,0,0,0,0],[2024,4,0,31,0,0,0,0]]){vm.write(input,packed(fields));equal(api(t,'GetDateFormatW',[0x409n,0n,input,0n,out,100n]),0n);equal(vm.readInt(t.error),87n);}
 for(const at of [4,5,6,7]){const fields=point.slice();fields[at]=[24,60,60,1000][at-4];vm.write(input,packed(fields));equal(api(t,'GetTimeFormatW',[0x409n,0n,input,0n,out,100n]),0n);equal(vm.readInt(t.error),87n);}
 vm.write(input,packed(point));text(vm,picture,'g');equal(api(t,'GetDateFormatW',[0x409n,0n,input,picture,out,100n]),0n);equal(vm.readInt(t.error),50n);
 for(const name of names){const ex=name.endsWith('Ex');vm.write(out,new Uint8Array(32).fill(0xa5));equal(api(t,name,[ex?0n:0x409n,0n,input,0n,out,1n,0n]),0n);equal(vm.readInt(t.error),122n);equal(bytes(vm,out,32),new Array(32).fill(0xa5));equal(vm.bind('kernelbase.dll',name),vm.bind('kernel32.dll',name));assert.throws(()=>vm.bind('user32.dll',name),/unknown/);}
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// ANSI input pictures and output use the locale code page, or the explicit ACP.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.write(input,packed(point));
 vm.write(picture,[0x80,32,100,100,0]);equal(api(t,'GetDateFormatA',[0x409n,0n,input,picture,out,100n]),5n);equal(bytes(vm,out,5),[0x80,32,50,57,0]);
 vm.write(picture,[0x82,0xa0,32,100,100,0]);equal(api(t,'GetDateFormatA',[0x409n,0x40000000n,input,picture,out,100n]),6n);equal(bytes(vm,out,6),[0x82,0xa0,32,50,57,0]);
 equal(api(t,'GetDateFormatA',[0x411n,2n,input,0n,0n,0n]),14n);equal(api(t,'GetDateFormatA',[0x411n,2n,input,0n,out,14n]),14n);equal(bytes(vm,out,14),[50,48,50,52,0x94,0x4e,50,0x8c,0x8e,50,57,0x93,0xfa,0]);
 if(mode===64){const high=api(t,'VirtualAlloc',[0n,4096n,0x103000n,4n]);assert.ok(high>0xffffffffn);vm.write(high,packed(point));text(vm,high+32n,'yyyy');equal(api(t,'GetDateFormatW',[0x100000409n,0x100000000n,high,high+32n,high+64n,0x100000010n]),5n);equal(bytes(vm,high+64n,10),utf16('2024\0'));equal(api(t,'VirtualFree',[high,0n,0x8000n]),1n);}
 vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,cross=0x24ffcn;vm.write(input,packed(point));text(vm,picture,'yyyy-MM-dd');
 for(const name of names){
  vm.write(cross,new Uint8Array(32).fill(0xa5));frame(t,name,[name.endsWith('Ex')?0n:0x409n,0n,input,0n,cross,100n,0n],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(vm,cross,4),[165,165,165,165]);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);
 }
 frame(t,'GetDateFormatW',[0x409n,0n,input,picture,out,100n],'kernel32.dll');equal(core.wire_protect(t.handle,input,4096n,1),0);let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,input,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);
 text(vm,cross,'yyyy');frame(t,'GetDateFormatW',[0x409n,0n,input,cross,out,100n],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,1),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);
 frame(t,'GetTimeFormatW',[0x409n,0x10n,input,0n,out,100n],'kernel32.dll');equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(t.error),1004n);
 text(vm,picture,'yyyy-MM-dd');equal(api(t,'GetDateFormatW',[0x409n,0n,input,picture,picture,100n]),11n);equal(bytes(vm,picture,22),utf16('2024-02-29\0'));
 equal(api(t,'GetDateFormatW',[0x409n,0n,input,0n,t.sp,100n]),10n);equal(vm.ip,vm.entryPoints.returned);equal(bytes(vm,t.sp,20),utf16('2/29/2024\0'));
 equal(api(t,'GetDateFormatW',[0x409n,0n,input,0n,input,100n]),10n);equal(bytes(vm,input,20),utf16('2/29/2024\0'));
 equal(t.time.wallCalls,0);vm.dispose();
}

for(const mode of [32,64]){
 let ticks=BigInt(Date.UTC(2024,1,29,13,5,7,999))*10000n+116444736000000000n,fail=false;
 const t=await machine(mode,{wallClock(){if(fail)throw new Error('NLS wall clock unavailable');return ticks;}}),{vm}=t;
 for(const name of names){const samples=t.time.wallCalls;equal(api(t,name,[name.endsWith('Ex')?0n:0x409n,0n,0n,0n,out,100n,0n]),name.startsWith('GetDate')?name.endsWith('Ex')?11n:10n:name.endsWith('Ex')?9n:11n);equal(t.time.wallCalls,samples+1);const value=name.startsWith('GetDate')?name.endsWith('Ex')?'2024/02/29':'2/29/2024':name.endsWith('Ex')?'13:05:07':'1:05:07 PM';equal(bytes(vm,out,value.length*(name.endsWith('A')?1:2)),name.endsWith('A')?Array.from(Buffer.from(value)):utf16(value));}
 frame(t,'GetTimeFormatW',[0x409n,0n,0n,0n,out,100n],'kernel32.dll');fail=true;const before=snapshot(t);assert.throws(()=>vm.service(),/NLS wall clock unavailable/);equal(snapshot(t),before);fail=false;equal(vm.service().reason,Exit.Dispatch);
 ticks=1n<<63n;frame(t,'GetDateFormatW',[0x409n,0n,0n,0n,out,100n],'kernel32.dll');const invalid=snapshot(t);assert.throws(()=>vm.service(),/wall clock is outside/);equal(snapshot(t),invalid);equal(t.time.calls,0);vm.dispose();
}
console.log(`${checks} NLS calendar picture/flags/locale/ANSI/UTF-16/ABI/clock/transaction checks passed`);
