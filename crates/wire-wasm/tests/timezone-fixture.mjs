import {fixture} from './pe-fixture.mjs';

// Read the same zone through Windows calendar, FILETIME, and locale APIs.
export function timezoneFixture(mode){
 const f=fixture(mode,{tls:false}),names=['GetSystemTime','GetLocalTime','GetTimeZoneInformation','GetSystemTimePreciseAsFileTime','FileTimeToLocalFileTime','GetDateFormatW','GetTimeFormatW','SystemTimeToTzSpecificLocalTime','TzSpecificLocalTimeToSystemTime'];
 f.relocations.splice(0);f.bytes.fill(0,f.offset(0x2040),f.offset(0x2400));f.rput(0x2000,0x2040);f.rput(0x2010,0x20a0);f.rput(0x200c,0x2160);f.rtext(0x2160,'kernel32.dll\0');
 let name=0x2180;for(const[n,value]of names.entries()){f.rput(0x2040+n*f.ptr,name,f.ptr);f.rput(0x20a0+n*f.ptr,name,f.ptr);f.rtext(name+2,value+'\0');name+=value.length+3;}
 let pc=0x1000;
 const emit=(...code)=>{f.bytes.set(code,f.offset(pc));pc+=code.length;};
 const encode=(value,width)=>Array.from({length:width},(_,n)=>Number(BigInt.asUintN(width*8,BigInt(value))>>BigInt(n*8)&255n));
 const absolute=(prefix,rva)=>{emit(...prefix);f.absolute(pc,f.preferred+BigInt(rva));pc+=f.ptr;};
 const relative=(prefix,rva)=>{emit(...prefix);emit(...encode(rva-pc-4,4));};
 const pointer=rva=>({rva});
 const call=(name,args)=>{
  if(mode===32){
   for(const a of args.toReversed())if(a.rva!==undefined)absolute([0x68],a.rva);else emit(0x68,...encode(a,4));
   absolute([0xff,0x15],0x20a0+names.indexOf(name)*f.ptr);
  }else{
   for(const[n,a]of args.entries()){
    const reg=n<4?[1,2,8,9][n]:0,prefix=reg<8?0x48:0x49;
    if(a.rva!==undefined)absolute([prefix,0xb8+(reg&7)],a.rva);else emit(prefix,0xb8+(reg&7),...encode(a,8));
    if(n>=4)emit(0x48,0x89,0x44,0x24,32+(n-4)*8);
   }
   relative([0xff,0x15],0x20a0+names.indexOf(name)*f.ptr);
  }
 };
 if(mode===64)emit(0x48,0x83,0xec,56);
 call('GetSystemTime',[pointer(0x3000)]);call('GetLocalTime',[pointer(0x3010)]);call('GetTimeZoneInformation',[pointer(0x3020)]);
 if(mode===32)absolute([0xa3],0x30d0);else relative([0x89,0x05],0x30d0);
 call('GetSystemTimePreciseAsFileTime',[pointer(0x30e0)]);call('FileTimeToLocalFileTime',[pointer(0x30e0),pointer(0x30e8)]);
 call('GetDateFormatW',[0x7fn,0n,0n,0n,pointer(0x3100),32n]);call('GetTimeFormatW',[0x7fn,0n,0n,0n,pointer(0x3140),32n]);
 call('SystemTimeToTzSpecificLocalTime',[0n,pointer(0x3000),pointer(0x3180)]);call('TzSpecificLocalTimeToSystemTime',[0n,pointer(0x3180),pointer(0x3190)]);
 if(mode===32)absolute([0x0f,0xb7,0x05],0x3018);else relative([0x0f,0xb7,0x05],0x3018);
 if(mode===64)emit(0x48,0x83,0xc4,56);emit(0xc3);
 if(pc>0x1400)throw Error('timezone fixture code capacity exceeded');f.rebuildRelocations();return f;
}
