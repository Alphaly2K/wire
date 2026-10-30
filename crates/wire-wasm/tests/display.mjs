import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,finish,window,rectangle,readRect} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';
const api=(t,name,args=[])=>call(t,name,args);
const text=(vm,p,value,wide=true)=>vm.write(p,wide?Array.from(value+'\0',c=>[c.charCodeAt(0)&255,c.charCodeAt(0)>>8]).flat():new TextEncoder().encode(value+'\0'));
const string=(vm,p,wide=true)=>{let value='';for(let n=0;n<128;n++){const unit=Number(vm.readInt(p+BigInt(n*(wide?2:1)),wide?2:1));if(!unit)return value;value+=String.fromCharCode(unit);}throw new Error('unterminated display string');};
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const packed=(x,y)=>BigInt.asUintN(32,BigInt(x))|(BigInt.asUintN(32,BigInt(y))<<32n);
function fromPoint(t,x,y,flags){
 if(t.mode!==32)return api(t,'MonitorFromPoint',[packed(x,y),flags]);
 frame(t,'MonitorFromPoint',[0n,0n,flags]);t.vm.write(t.sp+4n,encode(packed(x,y),8));
 const work=t.vm.work,retired=t.vm.retired;equal(t.vm.service().reason,Exit.Dispatch);equal(t.vm.work,work+1n);equal(t.vm.retired,retired);equal(t.vm.register(4),t.sp+16n);return t.vm.register(0);
}
function callback(t,{result=1n,at=0x30000n}={}){
 const {mode,vm}=t,b=[],emit=(...x)=>b.push(...x),value=(x,n)=>emit(...encode(BigInt(x),n));
 const store=(p,width=mode/8)=>{if(mode===32){emit(0xa3);value(p,4);}else{emit(0x49,0xbb);value(p,8);emit(width===8?0x49:0x41,0x89,0x03);}};
 for(let i=0;i<4;i++){if(mode===32)emit(0x8b,0x44,0x24,(i+1)*4);else emit(...[[0x48,0x89,0xc8],[0x48,0x89,0xd0],[0x4c,0x89,0xc0],[0x4c,0x89,0xc8]][i]);store(0x23000n+BigInt(i*8));}
 if(mode===32)emit(0x8b,0x54,0x24,12);
 for(let i=0;i<4;i++){if(mode===32)emit(0x8b,0x42,i*4);else emit(0x41,0x8b,0x40,i*4);store(0x23100n+BigInt(i*4),4);}
 if(mode===32){emit(0xff,0x05);value(0x23020n,4);}else{emit(0x49,0xbb);value(0x23020n,8);emit(0x41,0xff,0x03);}
 emit(0x66,0x0f,0xef,0xdb,...(mode===32?[0xb8]:[0x48,0xb8]));value(result,mode/8);emit(...(mode===32?[0xc2,16,0]:[0xc3]));vm.write(at,b);return at;
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.write(t.error,encode(123n,4));
 const monitor=fromPoint(t,0,0,0n);assert.ok(monitor);if(mode===64)assert.ok(monitor>0xffffffffn);
 for(const [x,y,inside] of [[1023,767,true],[1024,767,false],[0,768,false],[-1,0,false],[0,-1,false],[-2147483648,2147483647,false]])for(const flags of [0n,1n,2n])equal(fromPoint(t,x,y,flags),inside||flags?monitor:0n);
 for(const [rect,inside] of [[[0,0,10,20],true],[[-100,-100,1,1],true],[[1024,0,1100,20],false],[[2,3,2,5],false],[[10,10,0,0],false]]){
  rectangle(t,0x22000n,rect);for(const flags of [0n,1n,2n])equal(api(t,'MonitorFromRect',[0x22000n,flags]),inside||flags?monitor:0n);
 }
 equal(api(t,'MonitorFromRect',[0xdeadn,3n]),0n);equal(vm.readInt(t.error),87n);
 for(const wide of [false,true])for(const size of [40,wide?104:72]){
  vm.write(0x24000n,new Uint8Array(size+4).fill(0xa5));vm.write(0x24000n,encode(BigInt(size),4));
  equal(api(t,wide?'GetMonitorInfoW':'GetMonitorInfoA',[monitor,0x24000n]),1n);
  equal(vm.readInt(0x24000n),BigInt(size));equal(readRect(t,0x24004n),[0,0,1024,768]);equal(readRect(t,0x24014n),[0,0,1024,768]);equal(vm.readInt(0x24024n),1n);equal(vm.readInt(0x24000n+BigInt(size)),0xa5a5a5a5n);
  if(size!==40)equal(string(vm,0x24028n,wide),'\\\\.\\DISPLAY1');
 }
 equal(api(t,'GetMonitorInfoW',[monitor+(mode===64?(1n<<32n):16n),0xdeadn]),0n);equal(vm.readInt(t.error),1461n);
 for(const [index,expected] of [[0,1024],[1,768],[4,23],[32,4],[76,0],[77,0],[78,1024],[79,768],[80,1],[9999,0]])equal(api(t,'GetSystemMetrics',[BigInt(index)]),BigInt(expected));
 equal(api(t,'GetDoubleClickTime'),500n);
 for(const name of ['GetSystemMetrics','MonitorFromPoint','MonitorFromRect','MonitorFromWindow','GetMonitorInfoA','GetMonitorInfoW','EnumDisplayDevicesA','EnumDisplayDevicesW','EnumDisplaySettingsA','EnumDisplaySettingsW','EnumDisplaySettingsExA','EnumDisplaySettingsExW','EnumDisplayMonitors','GetDoubleClickTime'])for(const dll of ['kernel32.dll','kernelbase.dll','gdi32.dll'])assert.throws(()=>vm.bind(dll,name),/unknown/);
 const hwnd=await window(t);equal(api(t,'MonitorFromWindow',[hwnd,0n]),monitor);equal(api(t,'MonitorFromWindow',[0n,0n]),0n);equal(api(t,'MonitorFromWindow',[0n,1n]),monitor);equal(api(t,'MonitorFromWindow',[hwnd+1n,2n]),0n);equal(vm.readInt(t.error),1400n);
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

for(const mode of [32,64])for(const wide of [false,true]){
 const t=await machine(mode),{vm}=t,suffix=wide?'W':'A',deviceSize=wide?840:424,modeSize=wide?220:156,sizeOffset=wide?68:36,bitsOffset=wide?168:104;
 vm.write(t.error,encode(123n,4));
 for(const device of [null,'\\\\.\\DISPLAY1']){
  vm.write(0x24000n,new Uint8Array(deviceSize+4).fill(0xa5));vm.write(0x24000n,encode(BigInt(deviceSize),4));if(device)text(vm,0x22000n,device,wide);
  equal(api(t,'EnumDisplayDevices'+suffix,[device?0x22000n:0n,0n,0x24000n,0n]),1n);
  equal(string(vm,0x24004n,wide),device?'\\\\.\\DISPLAY1\\Monitor0':'\\\\.\\DISPLAY1');equal(vm.readInt(0x24000n+BigInt(wide?324:164)),device?3n:5n);equal(vm.readInt(0x24000n+BigInt(deviceSize)),0xa5a5a5a5n);
 }
 const old=bytes(vm,0x24000n,deviceSize);equal(api(t,'EnumDisplayDevices'+suffix,[0n,1n,0x24000n,0n]),0n);equal(bytes(vm,0x24000n,deviceSize),old);equal(vm.readInt(t.error),123n);
 equal(api(t,'EnumDisplayDevices'+suffix,[0n,0n,0x24000n,1n]),0n);equal(vm.readInt(t.error),50n);equal(bytes(vm,0x24000n,deviceSize),old);
 for(const size of [modeSize-32,modeSize])for(const index of [0n,0xffffffffn,0xfffffffen])for(const flags of [0n,2n,4n,6n]){
  vm.write(0x24000n,new Uint8Array(modeSize+4).fill(0xa5));vm.write(0x24000n+BigInt(sizeOffset),encode(BigInt(size),2));
  equal(api(t,'EnumDisplaySettingsEx'+suffix,[0n,index,0x24000n,flags]),1n);
  equal(vm.readInt(0x24000n+BigInt(sizeOffset),2),BigInt(size));equal(vm.readInt(0x24000n+BigInt(sizeOffset+4)),0x5c0020n);
  for(const [n,expected] of [32n,1024n,768n,0n,60n].entries())equal(vm.readInt(0x24000n+BigInt(bitsOffset+n*4)),expected);
  equal(vm.readInt(0x24000n+BigInt(size)),0xa5a5a5a5n);
 }
 vm.write(0x24000n+BigInt(sizeOffset),encode(BigInt(modeSize),2));equal(api(t,'EnumDisplaySettings'+suffix,[0n,0n,0x24000n]),1n);
 equal(api(t,'EnumDisplaySettingsEx'+suffix,[0n,0n,0x24000n,1n]),0n);equal(vm.readInt(t.error),87n);
 vm.write(t.error,encode(123n,4));const before=bytes(vm,0x24000n,modeSize);equal(api(t,'EnumDisplaySettings'+suffix,[0n,99n,0x24000n]),0n);equal(bytes(vm,0x24000n,modeSize),before);equal(vm.readInt(t.error),123n);
 for(const [name,args] of [['GetDisplayConfigBufferSizes',[2n,0xdeadn,0xbeefn]],['QueryDisplayConfig',[2n,0xdeadn,0xbeefn,0xdeadn,0xbeefn,0n]],['DisplayConfigGetDeviceInfo',[0xdeadn]]]){equal(api(t,name,args),50n);equal(vm.readInt(t.error),123n);}
 equal(t.time.calls,0);vm.dispose();
}

// Monitor callbacks receive a transient clipped RECT and preserve the suspended CPU.
for(const mode of [32,64])for(const result of mode===64?[0n,2n,0x100000000n]:[0n,2n]){
 const t=await machine(mode),{vm}=t,proc=callback(t,{result}),param=mode===32?0x87654321n:0xfedcba9876543210n;
 rectangle(t,0x22000n,[-10,20,1100,90]);vm.flags=0x8d5n;vm.mxcsr=0x5f81;vm.xmm(3,new Uint8Array(16).fill(0x7b));const fpu=fp(t);vm.write(t.error,encode(123n,4));
 frame(t,'EnumDisplayMonitors',[0n,0x22000n,proc,param]);const work=vm.work,retired=vm.retired;
 equal((await vm.run(0n)).reason,Exit.Budget);equal(vm.work,work);equal(vm.service().reason,Exit.Dispatch);equal(vm.work,work+1n);equal(vm.retired,retired);
 equal(await finish(t,1n),result&0xffffffffn?1n:0n);equal(vm.work-work,vm.retired-retired+1n);
 equal(vm.readInt(0x23000n,mode/8),fromPoint(t,0,0,0n));equal(vm.readInt(0x23008n,mode/8),0n);equal(vm.readInt(0x23018n,mode/8),param);equal(vm.readInt(0x23020n),1n);equal(readRect(t,0x23100n),[0,20,1024,90]);
 equal(vm.readInt(t.error),123n);equal(vm.flags,0x8d5n);equal(vm.xmm(3),new Uint8Array(16).fill(0x7b));equal(vm.mxcsr,0x5f81);equal(fp(t),fpu);equal(t.time.calls,0);vm.dispose();
}

// Both the RECT and callback frame are checked before either is written.
for(const mode of [32,64])for(const page of [0x24000n,0x25000n]){
 const t=await machine(mode),{vm}=t,proc=callback(t);t.sp=mode===32?0x25010n:0x25020n;
 vm.write(0x24fc0n,new Uint8Array(96).fill(0xa5));frame(t,'EnumDisplayMonitors',[0n,0n,proc,0n]);
 const buffer=t.sp-16n,old=bytes(vm,buffer,16);equal(core.wire_protect(t.handle,page,4096n,2),0);const before=snapshot(t);
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(vm,buffer,16),old);equal(vm.readInt(0x23020n),0n);equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(await finish(t,1n),1n);equal(vm.readInt(0x23020n),1n);vm.dispose();
}
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 frame(t,'EnumDisplayMonitors',[0n,0n,0xdead0000n,0n]);equal(vm.service().reason,Exit.Dispatch);equal((await vm.run(1n)).reason,Exit.Fault);equal(vm.ip,0xdead0000n);
 vm.map(0xdead0000n,4096n,7);callback(t,{at:0xdead0000n});equal(await finish(t,1n),1n);equal(vm.readInt(0x23020n),1n);
 rectangle(t,0x22000n,[2000,2000,3000,3000]);equal(api(t,'EnumDisplayMonitors',[0n,0x22000n,0xdeadbeefn,0n]),1n);equal(vm.readInt(0x23020n),1n);
 equal(api(t,'EnumDisplayMonitors',[1n,0xdeadbeefn,0xdeadbeefn,0n]),0n);equal(vm.readInt(t.error),50n);equal(api(t,'EnumDisplayMonitors',[0n,0n,0n,0n]),0n);equal(vm.readInt(t.error),87n);
 for(const [name,size,args] of [['GetMonitorInfoW',104,[fromPoint(t,0,0,0n),0x24ff8n]],['EnumDisplayDevicesW',840,[0n,0n,0x24ff8n,0n]],['EnumDisplaySettingsW',220,[0n,0n,0x24ff8n]]]){
  vm.write(0x24ff8n,new Uint8Array(size).fill(0xa5));vm.write(0x24ff8n+BigInt(name==='EnumDisplaySettingsW'?68:0),encode(BigInt(size),name==='EnumDisplaySettingsW'?2:4));
  frame(t,name,args);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t),prefix=bytes(vm,0x24ff8n,8);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(vm,0x24ff8n,8),prefix);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);
 }
 vm.dispose();
}

// Scalar display outputs use the saved return even when records overlap the original stack.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,monitor=fromPoint(t,0,0,0n),out=t.sp-32n;
 vm.write(out,encode(40n,4));equal(api(t,'GetMonitorInfoW',[monitor,out]),1n);equal(vm.ip,vm.entryPoints.returned);equal(readRect(t,out+20n),[0,0,1024,768]);
 if(mode===64){
  const high=0x123400000000n;vm.map(high,4096n,3);vm.write(high,encode(104n,4));equal(api(t,'GetMonitorInfoW',[monitor,high]),1n);equal(string(vm,high+40n),'\\\\.\\DISPLAY1');
  equal(fromPoint(t,0,0,0xabcdef0000000000n),monitor);
 }
 vm.dispose();
}
console.log(`${checks} virtual display/mode/monitor/packed-POINT/ABI/callback/fault checks passed`);
