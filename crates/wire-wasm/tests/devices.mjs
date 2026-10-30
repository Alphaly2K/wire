import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,window,invoke} from './gdi-harness.mjs';
const hid=['HidD_GetHidGuid','HidD_FreePreparsedData','HidD_GetAttributes','HidD_GetPreparsedData','HidD_GetManufacturerString','HidD_GetProductString','HidD_GetSerialNumberString','HidP_GetCaps','HidP_MaxDataListLength','HidP_GetButtonCaps','HidP_GetValueCaps','HidP_GetData','HidP_SetUsageValue','HidP_SetUsages'];
const setup=['SetupDiGetClassDevsA','SetupDiGetClassDevsW','SetupDiDestroyDeviceInfoList','SetupDiEnumDeviceInfo','SetupDiEnumDeviceInterfaces','SetupDiGetDeviceInterfaceDetailA','SetupDiGetDeviceInterfaceDetailW'];
const notifications=['RegisterDeviceNotificationA','RegisterDeviceNotificationW','UnregisterDeviceNotification'];
const guid=0x23000n,out=0x24000n;
const api=(t,name,args=[])=>call(t,name,args,hid.includes(name)?'hid.dll':setup.includes(name)?'setupapi.dll':'user32.dll');
const t=await machine(64),{vm}=t,hwnd=await window(t),other=await machine(64);
for(const [dll,names]of [['hid.dll',hid],['setupapi.dll',setup],['user32.dll',notifications]])for(const name of names){assert.ok(vm.bind(dll,name));for(const wrong of ['kernel32.dll','kernelbase.dll',...(dll==='user32.dll'?['hid.dll']:['user32.dll'])])assert.throws(()=>vm.bind(wrong,name),/unknown/);assert.throws(()=>other.vm.bind('kernel32.dll',name),/unknown/);}
vm.flags=0x8d5n;vm.write(t.error,encode(123n,4));equal(api(t,hid[0],[guid]),0n);equal(Array.from({length:16},(_,i)=>Number(vm.readInt(guid+BigInt(i),1))),[0xb2,0x55,0x1e,0x4d,0x6f,0xf1,0xcf,0x11,0x88,0xcb,0,0x11,0x11,0,0,0x30]);equal(vm.readInt(t.error),123n);
equal(api(t,hid[1],[0n]),1n);equal(api(t,hid[1],[1n]),0n);equal(vm.readInt(t.error),87n);
for(const name of hid.slice(2,7)){vm.write(out,new Uint8Array(32).fill(0xa5));equal(api(t,name,[1n,out,32n]),0n);equal(vm.readInt(t.error),6n);equal(vm.readInt(out,8),0xa5a5a5a5a5a5a5a5n);}
vm.write(t.error,encode(123n,4));for(const name of hid.slice(7))equal(api(t,name,Array(8).fill(1n)),name===hid[8]?0n:0xc0110001n);equal(vm.readInt(t.error),123n);
const sets=[];
for(const suffix of ['A','W']){
 const set=api(t,'SetupDiGetClassDevs'+suffix,[guid,0n,0n,0xabcdef0100000012n]);assert.ok(set>0xffffffffn&&set!==0xffffffffffffffffn);sets.push(set);
 vm.write(out,new Uint8Array(32).fill(0xa5));vm.write(out,encode(32n,4));equal(api(t,'SetupDiEnumDeviceInfo',[set,0n,out]),0n);equal(vm.readInt(t.error),259n);equal(vm.readInt(out+24n,8),0xa5a5a5a5a5a5a5a5n);
 equal(api(t,'SetupDiEnumDeviceInterfaces',[set,0n,guid,0n,out]),0n);equal(vm.readInt(t.error),259n);equal(vm.readInt(out+24n,8),0n);
 vm.write(out+64n,encode(0xfeedn,4));equal(api(t,'SetupDiGetDeviceInterfaceDetail'+suffix,[set,out,1n,0n,out+64n,1n]),0n);equal(vm.readInt(t.error),87n);equal(vm.readInt(out+64n),0xfeedn);
}
assert.ok(sets[1]>sets[0]);
equal(api(other,'SetupDiDestroyDeviceInfoList',[sets[0]]),0n);equal(other.vm.readInt(other.error),6n);
for(const [args,error]of [[[0n,0n,0n,2n],87n],[[guid,0n,0n,1n],50n],[[guid,0n,0n,32n],87n]]){equal(api(t,'SetupDiGetClassDevsW',args),0xffffffffffffffffn);equal(vm.readInt(t.error),error);}
const high=0x123400000000n;vm.map(high,4096n,3);vm.write(high,encode(32n,4));equal(api(t,'SetupDiEnumDeviceInfo',[sets[0],0n,high]),0n);equal(vm.readInt(t.error),259n);
// No filter failure or failed copyout may consume a set/subscription or write a prefix.
for(const [name,args,dll,page]of [['SetupDiDestroyDeviceInfoList',[sets[0]],'setupapi.dll',t.error&~4095n],['HidD_GetHidGuid',[0x24ffen],'hid.dll',0x25000n],['SetupDiEnumDeviceInterfaces',[sets[0],0n,guid,0n,0x24ffen],'setupapi.dll',0x25000n]]){
 vm.write(0x24ffen,new Uint8Array(32).fill(0xa5));if(name==='SetupDiEnumDeviceInterfaces')vm.write(0x24ffen,encode(32n,4));frame(t,name,args,dll);equal(core.wire_protect(t.handle,page,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(0x24ffen,2),name==='SetupDiEnumDeviceInterfaces'?32n:0xa5a5n);equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);
 if(name==='SetupDiDestroyDeviceInfoList'){equal(vm.register(0),1n);equal(vm.readInt(t.error),0n);equal(api(t,name,[sets[0]]),0n);equal(vm.readInt(t.error),6n);}
}
for(const name of notifications.slice(0,2)){
 vm.write(out,new Uint8Array(28));vm.write(out,encode(28n,4));vm.write(out+4n,encode(5n,4));vm.write(out+12n,Array.from({length:16},(_,i)=>Number(vm.readInt(guid+BigInt(i),1))));const token=api(t,name,[hwnd,out,0n]);assert.ok(token>0xffffffffn);equal(api(t,'UnregisterDeviceNotification',[token]),1n);equal(api(t,'UnregisterDeviceNotification',[token]),0n);equal(vm.readInt(t.error),6n);
 for(const [args,error]of [[[hwnd,1n,2n],87n],[[hwnd,1n,1n],50n],[[1n,1n,0n],87n]]){equal(api(t,name,args),0n);equal(vm.readInt(t.error),error);}
}
const token=api(t,'RegisterDeviceNotificationW',[hwnd,0n,4n]);assert.ok(token);equal(await invoke(t,'DestroyWindow',[hwnd]),1n);equal(api(t,'UnregisterDeviceNotification',[token]),0n);equal(vm.readInt(t.error),6n);
equal(vm.flags,0x8d5n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();other.vm.dispose();
const pe32=await machine(32);for(const name of [...hid,...setup,...notifications])assert.throws(()=>pe32.vm.bind(hid.includes(name)?'hid.dll':setup.includes(name)?'setupapi.dll':'user32.dll',name),/unknown|not implemented/);pe32.vm.dispose();
console.log(`${checks} device ownership/HID absence/notification lifetime/provider/fault checks passed`);
