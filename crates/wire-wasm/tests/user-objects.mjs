import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,finish,window} from './gdi-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';

const registered=['RegisterClipboardFormatA','RegisterClipboardFormatW','RegisterWindowMessageA','RegisterWindowMessageW'];
const properties=['SetPropA','SetPropW','GetPropA','GetPropW','RemovePropA','RemovePropW'];
const names=[...registered,...properties];
const text=(vm,p,s,wide=false)=>vm.write(p,Buffer.from(s+'\0',wide?'utf16le':'ascii'));
const invoke=async(t,name,args)=>{frame(t,name,args);return finish(t);};

// Both ABIs share the existing process namespace and UTF-16-unit validation.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 for(const name of names){assert.ok(vm.bind('user32.dll',name));for(const dll of ['kernel32.dll','kernelbase.dll'])assert.throws(()=>vm.bind(dll,name),/unknown/);}
 for(const name of ['EnumPropsA','EnumPropsW','EnumPropsExA','EnumPropsExW'])assert.throws(()=>vm.bind('user32.dll',name),/migrated|unknown/);
 vm.flags=0x8d5n;vm.write(t.error,encode(77n,4));
 text(vm,0x22000n,'K',true);equal(call(t,'RegisterClipboardFormatW',[0x22000n]),0xc000n);text(vm,0x22000n,'k');equal(call(t,'RegisterWindowMessageA',[0x22000n]),0xc000n);
 vm.write(0x22000n,[0x93,0xfa,0x96,0x7b,0]);equal(call(t,'RegisterClipboardFormatA',[0x22000n]),0xc001n);text(vm,0x22000n,'日本',true);equal(call(t,'RegisterWindowMessageW',[0x22000n]),0xc001n);
 text(vm,0x22000n,'a'.repeat(255),true);equal(call(t,'RegisterClipboardFormatW',[0x22000n]),0xc002n);equal(vm.readInt(t.error),77n);equal(vm.flags,0x8d5n);
 for(const value of ['', 'a'.repeat(256),'😀'.repeat(128)]){text(vm,0x22000n,value,true);equal(call(t,'RegisterWindowMessageW',[0x22000n]),0n);equal(vm.readInt(t.error),87n);}
 text(vm,0x22000n,'😀'.repeat(127)+'x',true);equal(call(t,'RegisterWindowMessageW',[0x22000n]),0xc003n);
 vm.write(0x22000n,[0,0xd8,0,0]);equal(call(t,'RegisterClipboardFormatW',[0x22000n]),0n);equal(vm.readInt(t.error),1113n);
 const fresh=await machine(mode);text(fresh.vm,0x22000n,'k');equal(call(fresh,'RegisterWindowMessageA',[0x22000n]),0xc000n);fresh.vm.dispose();
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Properties are window-owned, distinguish atoms from text, and retain full pointer values.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,hwnd=await window(t,{brush:0n}),value=mode===32?0xfedcba98n:0xfedcba9876543210n;
 const other=await invoke(t,'CreateWindowExW',[0n,0x20100n,0x20180n,0n,10n,20n,16n,12n,0n,0n,t.image.base,0n]);assert.ok(other);assert.notEqual(other,hwnd);
 const pointer=mode===64?0x123500000000n:0x22000n;if(mode===64)vm.map(pointer,4096n,3);
 text(vm,pointer,'PROP',true);text(vm,pointer+64n,'prop');vm.write(t.error,encode(77n,4));
 equal(call(t,'SetPropW',[hwnd,pointer,value]),1n);equal(call(t,'GetPropA',[hwnd,pointer+64n]),value);equal(call(t,'GetPropW',[other,pointer]),0n);
 equal(call(t,'SetPropA',[hwnd,123n,42n]),1n);text(vm,pointer,'123',true);equal(call(t,'SetPropW',[hwnd,pointer,99n]),1n);equal(call(t,'GetPropW',[hwnd,123n]),42n);text(vm,pointer,'123');equal(call(t,'GetPropA',[hwnd,pointer]),99n);
 equal(call(t,'SetPropA',[hwnd,0xffffn,7n]),1n);equal(call(t,'RemovePropW',[hwnd,0xffffn]),7n);equal(call(t,'GetPropA',[hwnd,0xffffn]),0n);
 text(vm,pointer,'',true);equal(call(t,'SetPropW',[hwnd,pointer,8n]),1n);equal(call(t,'RemovePropA',[hwnd,pointer+1n]),8n);
 equal(call(t,'SetPropW',[hwnd,123n,0n]),1n);equal(call(t,'RemovePropA',[hwnd,123n]),0n);equal(call(t,'RemovePropW',[hwnd,pointer]),0n);equal(vm.readInt(t.error),77n);
 text(vm,pointer,'K',true);equal(call(t,'SetPropW',[hwnd,pointer,value]),1n);text(vm,pointer,'k');equal(call(t,'RemovePropA',[hwnd,pointer]),value);
 for(const[name,args,error]of [['GetPropW',[0n,0n],1400n],['SetPropA',[hwnd,0n,1n],87n],['RemovePropW',[123n,0n],1400n]]){equal(call(t,name,args),0n);equal(vm.readInt(t.error),error);}
 if(mode===64){equal(call(t,'SetPropW',[hwnd&0xffffffffn,123n,9n]),0n);equal(vm.readInt(t.error),1400n);}
 equal(await invoke(t,'DestroyWindow',[hwnd]),1n);equal(call(t,'GetPropW',[hwnd,pointer]),0n);equal(vm.readInt(t.error),1400n);equal(call(t,'IsWindow',[other]),1n);
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Properties remain available during WM_NCDESTROY, then disappear with the window.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,hwnd=await window(t,{brush:0n}),get=vm.bind('user32.dll','GetPropW'),value=mode===32?0x76543210n:0xfedcba9876543210n;
 equal(call(t,'SetPropW',[hwnd,123n,value]),1n);
 const code=mode===32?[0x81,0x7c,0x24,8,0x82,0,0,0,0x75,18,0x6a,123,0xff,0x74,0x24,8,0xb8,...encode(get,4),0xff,0xd0,0xa3,...encode(0x23000n,4),0x31,0xc0,0xc2,16,0]:[0x81,0xfa,0x82,0,0,0,0x75,34,0x48,0x83,0xec,0x28,0xba,123,0,0,0,0x48,0xb8,...encode(get,8),0xff,0xd0,0x49,0xbb,...encode(0x23000n,8),0x49,0x89,0x03,0x48,0x83,0xc4,0x28,0x31,0xc0,0xc3];
 // Branch directly to the shared zero-return epilogue for other destroy notifications.
 code[mode===32?9:7]=code.length-(mode===32?5:3)-(mode===32?10:8);
 vm.write(0x30000n,code);equal(call(t,mode===32?'SetWindowLongW':'SetWindowLongPtrW',[hwnd,BigInt.asUintN(32,-4n),0x30000n]),vm.bind('user32.dll','DefWindowProcW'));
 equal(await invoke(t,'DestroyWindow',[hwnd]),1n);equal(vm.readInt(0x23000n,mode/8),value);equal(call(t,'GetPropW',[hwnd,123n]),0n);equal(vm.readInt(t.error),1400n);vm.dispose();
}

// A source/error/return fault cannot consume a name or change/remove a property.
for(const mode of [32,64])for(const kind of ['name','error','return-register','return-set','return-remove']){
 const t=await machine(mode),{vm}=t,hwnd=await window(t,{brush:0n});equal(call(t,'SetPropW',[hwnd,123n,7n]),1n);text(vm,0x22000n,'first');
 const name=kind==='name'?'SetPropA':kind==='error'?'SetPropW':kind==='return-register'?'RegisterWindowMessageA':kind==='return-set'?'SetPropW':'RemovePropW';
 const args=kind==='name'?[0n,0x60000n,9n]:kind==='error'?[hwnd,0n,9n]:kind==='return-register'?[0x22000n]:kind==='return-set'?[hwnd,123n,9n]:[hwnd,123n];frame(t,name,args);const before=snapshot(t),work=vm.work;
 const page=kind==='error'?t.image.teb:t.sp&~4095n;if(kind!=='name')equal(core.wire_protect(t.handle,page,4096n,kind==='error'?2:1),0);
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.work,work);
 if(kind==='name'){vm.map(0x60000n,4096n,3);text(vm,0x60000n,'valid');}else equal(core.wire_protect(t.handle,page,4096n,4),0);
 equal(vm.service().reason,Exit.Dispatch);equal(vm.work,work+1n);
 if(kind==='name'){equal(vm.register(0),0n);equal(vm.readInt(t.error),1400n);}
 if(kind==='return-register')equal(vm.register(0),0xc000n);
 if(kind==='return-remove')equal(vm.register(0),7n);
 equal(call(t,'GetPropW',[hwnd,123n]),kind==='return-set'?9n:kind==='return-remove'?0n:7n);vm.dispose();
}

// Every implemented export accepts a static import; execution follows the JIT boundary.
for(const mode of [32,64]){
 const f=fixture(mode,{tls:false});f.bytes.fill(0,f.offset(0x2000),f.offset(0x2600));f.rput(0x2000,0x2080);f.rput(0x200c,0x21e0);f.rput(0x2010,0x2100);f.rtext(0x21e0,'user32.dll\0');let at=0x2200;
 for(const[n,name]of names.entries()){f.rput(0x2080+n*f.ptr,at,f.ptr);f.rput(0x2100+n*f.ptr,at,f.ptr);f.rtext(at+2,name+'\0');at+=name.length+3;}
 f.rtext(0x3000,'JIT name\0');
 const code=mode===32?[0x68,...encode(f.preferred+0x3000n,4),0xff,0x15,...encode(f.preferred+0x2100n+BigInt(2*f.ptr),4),0xc3]:[0x48,0x83,0xec,0x28,0x48,0x8d,0x0d,...encode(0x3000n-0x100bn,4),0xff,0x15,...encode(0x2100n+BigInt(2*f.ptr)-0x1011n,4),0x48,0x83,0xc4,0x28,0xc3];
 f.bytes.set(code,f.offset(0x1000));f.relocations.splice(0,f.relocations.length,...(mode===32?[0x1001,0x1007]:[]));f.rebuildRelocations();
 const vm=new WasmMachine(core,{mode,clock(){throw new Error('unexpected clock');}}),image=vm.load(f.bytes);for(const[n,name]of names.entries())equal(vm.readInt(image.base+0x2100n+BigInt(n*f.ptr),f.ptr),vm.bind('user32.dll',name));equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),0xc000n);assert.ok(vm.retired>0n);vm.dispose();
}
console.log(`${checks} USER registered name/property/ABI/lifetime/JIT/fault checks passed`);
