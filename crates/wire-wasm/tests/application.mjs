import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {WasmApplication,Exit,validateInstallation,executableMode} from '../web/application.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {applicationFixture} from './application-fixture.mjs';
import {fixture} from './pe-fixture.mjs';
import {readApplicationBytes} from '../web/platform.mjs';
import {core as built} from './x87-harness.mjs';

const core=new WebAssembly.Module(readFileSync(new URL('../../../target/wasm-rewrite/wasm32-unknown-unknown/debug/wire_wasm.wasm',import.meta.url)));
const container=()=>({ownerDocument:{},appendChild(){throw Error('unexpected window');}});
let checks=0;
const equal=(a,b)=>{assert.deepEqual(a,b);checks++;};
const encode=(value,width)=>Uint8Array.from({length:width},(_,n)=>Number((value>>BigInt(n*8))&255n));

// An output configured on an unstarted root follows every parent/child ABI pairing.
for(const mode of[32,64])for(const childMode of[32,64]){
  const {manifest,files}=applicationFixture(mode),requests=[],closed=[],compiler={close(){throw Error('borrowed compiler closed');}};
  let fail=true;
  const factory=kind=>{requests.push(kind);if(fail){fail=false;throw Error('output setup failed');}};
  const old=globalThis.WireWebGPU;
  globalThis.WireWebGPU={async create(){factory('graphics');return{profile(){const b=new Uint8Array(73),v=new DataView(b.buffer);v.setUint32(0,1,true);v.setUint32(8,1,true);b[72]=65;return b;},dispatch(){throw Error('unexpected GPU work');},submit(){},close(){closed.push('graphics');}};}};
  let app;
  try{
    app=await WasmApplication.load({core,manifest,read:async url=>files.get(url),container:container(),graphics:{shaderCompiler:compiler},audio:{createSink(){factory('audio');return{write(){return true;},close(){closed.push('audio');}};}}});
    const vm=app.machine;equal(requests,[]);equal((await vm.run(1000n)).reason,Exit.Returned);equal(requests,[]);
    assert.throws(()=>vm.configureOutputs({}),/unstarted/);
    const f=fixture(childMode,{tls:false});f.relocations.splice(0);
    if(childMode===32){
      f.rtext(0x20c0,'dsound.dll\0');f.rtext(0x20e2,'DirectSoundCreate\0');
      f.bytes.set([0x6a,0,0x68,0,0,0,0,0x6a,0,0xff,0x15,0,0,0,0,0xc3],f.offset(0x1000));f.absolute(0x1003,f.preferred+0x3000n);f.absolute(0x100b,f.preferred+0x20a0n);
    }else{
      f.rtext(0x20c0,'dxgi.dll\0');f.rtext(0x20e2,'CreateDXGIFactory1\0');
      f.bytes.set([0x78,0xae,0x0a,0x77,0x6f,0xf2,0xba,0x4d,0xa8,0x29,0x25,0x3c,0x83,0xd1,0xb3,0x87],f.offset(0x3000));
      f.bytes.set([0x48,0x83,0xec,40,0x48,0x8d,0x0d,...encode(0x3000n-0x100bn,4),0x48,0x8d,0x15,...encode(0x3020n-0x1012n,4),0xff,0x15,...encode(0x20a0n-0x1018n,4),0x48,0x83,0xc4,40,0xc3],f.offset(0x1000));
    }
    f.rebuildRelocations();vm.supplyFile('C:\\child.exe',f.bytes);vm.map(0x20000n,0x10000n,7);
    vm.write(0x20000n,Uint8Array.from([...'C:\\child.exe\0'].flatMap(c=>[c.charCodeAt(0),0])));vm.write(0x21000n,encode(BigInt(mode===32?68:104),104));
    const service=(name,args)=>{const sp=vm.image.sp-2048n;vm.ip=vm.bind('kernel32.dll',name);vm.register(4,sp);vm.write(sp,encode(vm.entryPoints.returned,mode/8));for(const[n,a]of args.entries())if(mode===32)vm.write(sp+4n+BigInt(n*4),encode(a,4));else if(n<4)vm.register([1,2,8,9][n],a);else vm.write(sp+40n+BigInt((n-4)*8),encode(a,8));return vm.service();};
    equal(service('CreateProcessW',[0x20000n,0n,0n,0n,0n,0n,0n,0n,0x21000n,0x22000n]).reason,Exit.Dispatch);equal(vm.register(0),1n);
    equal(service('TerminateProcess',[(1n<<BigInt(mode))-1n,19n]).reason,Exit.ProcessExited);
    await assert.rejects(vm.run(1000n),/output setup failed/);equal(vm.processes()[1].exitCode,null);
    equal((await app.step(1000n)).reason,Exit.ProcessExited);equal(vm.processes()[1].exitCode,0);equal(vm.processExitCode,19);
    const kind=childMode===32?'audio':'graphics';equal(requests,[kind,kind]);equal(closed,[kind]);
  }finally{app?.close();globalThis.WireWebGPU=old;}
}

for(const mode of[32,64]){
  const {manifest,files}=applicationFixture(mode);
  equal(executableMode(files.get('/app')),mode);
  const app=await WasmApplication.load({core,manifest,read:async url=>files.get(url),container:container()});
  const vm=app.machine,base=vm.image.base,dll=vm.moduleBase('library.dll');
  assert.throws(()=>vm.finishProcessEntry(),/has not returned/);
  let result;
  for(let n=0;n<500;n++){result=await app.step(7n);if(result.reason===Exit.ProcessExited)break;equal([Exit.Budget,Exit.Dispatch].includes(result.reason),true);}
  equal(result.reason,Exit.ProcessExited);equal(result.exitCode,37);
  equal(vm.readInt(base+0x3188n),0x10n);equal(vm.readInt(base+0x3180n),1n);
  equal(vm.readInt(dll+0x3030n),2n);equal(vm.readInt(dll+0x3020n),4n);
  equal(vm.readInt(dll+0x3048n),0n);equal(vm.readInt(dll+0x3050n,mode/8),1n);
  equal(vm.readInt(dll+0x3068n),0n);equal(vm.readInt(dll+0x3070n,mode/8),0n);
  vm.finishProcessEntry();vm.finishProcessEntry();equal((await app.step(10n)).exitCode,37);equal(vm.readInt(base+0x3180n),1n);equal(vm.readInt(dll+0x3030n),2n);
  equal(vm.stats(),{modules:0,chargedBytes:0,instances:0});app.present();app.close();app.close();
  await assert.rejects(app.step(1n),/closed/);assert.throws(()=>app.present(),/closed/);assert.throws(()=>vm.processes(),/disposed/);
}

// Invalid installations fail before fetching or creating a VM.
for(const change of[
  m=>m.files.push({...m.program}),m=>m.files[0].path='c:\\APP.EXE',m=>m.files[0].path='C:\\empty',
  m=>m.directories.push('c:\\EMPTY'),m=>m.directories.push('C:\\library.dll\\child'),
  m=>m.files[0].path='C:\\..\\escape',m=>m.files[0].path='C:\\alias. ',m=>m.files[0].path='C:\\con.txt',
  m=>m.files[0].path='C:\\quote"',m=>m.files[0].size=256*1024*1024,m=>m.command='app.exe',
  m=>{m.files[0].path='C:\\ΟΣ';m.files.push({path:'C:\\ος',url:'/same',size:0});},m=>m.files[0].path='C:\\\ud800',
  m=>m.input={url:'/input',size:8*1024*1024+1},m=>m.directories=Array.from({length:4097},(_,n)=>'C:\\d'+n),
  m=>m.timeZone=new Array(171).fill(0),m=>m.timeZone=new Array(172).fill(-1),m=>m.timeZone=new Array(172).fill(256),m=>m.timeZone=new Uint8Array(172),
  m=>m.timeZone=new Array(172),
]){
  const {manifest}=applicationFixture(64);change(manifest);let reads=0;
  await assert.rejects(WasmApplication.load({core,manifest,container:container(),read(){reads++;}}));equal(reads,0);
}

// Download size changes and presenter construction failures release the allocated owner.
const {manifest,files}=applicationFixture(32);
for(const options of[
  {read:async url=>url==='/library'?new Uint8Array():files.get(url),container:container()},
  {read:async url=>files.get(url),container:{}},
  {read:async url=>files.get(url),container:container(),audio:{sink:{}}},
  {read:async url=>files.get(url),container:container(),manifest:{...manifest,timeZone:Array.from({length:172},(_,n)=>n===70?11:0)}},
]){
  const dispose=WasmMachine.prototype.dispose,disposed=[];
  WasmMachine.prototype.dispose=function(){disposed.push(this);return dispose.call(this);};
  try{await assert.rejects(WasmApplication.load({core,manifest,...options}));}
  finally{WasmMachine.prototype.dispose=dispose;}
  equal(disposed.length,1);assert.throws(()=>disposed[0].processes(),/disposed/);
}
const vm=new WasmMachine(built,{mode:32});vm.supplyDirectory('C:\\empty\\leaf');assert.throws(()=>vm.supplyFile('C:\\empty',[1]),/directory/);vm.supplyFile('C:\\file',[1]);assert.throws(()=>vm.supplyDirectory('C:\\file\\leaf'),/directory input/);vm.dispose();
equal(validateInstallation(manifest),files.get('/app').length+files.get('/library').length);
for(const mode of[32,64]){
  const f=applicationFixture(mode),m=f.manifest,reads=[];
  m.files[0]={...m.files[0],external:true,size:String(m.files[0].size)};
  m.files.push({path:'C:\\assets\\large.dat',url:'/large',external:true,size:String(1n<<40n)});
  equal(validateInstallation(m),m.program.size);
  const app=await WasmApplication.load({core,manifest:m,container:container(),read:async(url,range)=>{reads.push([url,range]);const bytes=f.files.get(url);assert.ok(bytes,'unused assets must not be downloaded');return range?bytes.slice(Number(range.offset),Number(range.offset)+range.length):bytes;}});
  equal(reads.map(([url])=>url),['/app','/library']);equal(reads[1][1],{offset:0n,length:f.library.bytes.length,size:BigInt(f.library.bytes.length)});
  equal(app.machine.fileStats(),{cachedBytes:f.library.bytes.length,ownedBytes:0,sources:2});
  let result;for(let n=0;n<500;n++){result=await app.step(100n);if(result.reason===Exit.ProcessExited)break;}
  equal(result.exitCode,37);app.close();
}
for(const bytes of[new Uint8Array(64),new Uint8Array(63)])assert.throws(()=>executableMode(bytes),/PE/);

// Reject incorrect range headers before allocating or reading a response body.
const fetch=globalThis.fetch,range={offset:1n<<36n,length:64,size:1n<<40n};
try{
 for(const[status,header]of[[200,null],[206,'bytes 0-63/64'],[416,null],[206,`bytes ${range.offset}-${range.offset+63n}/${range.size}`]]){
  let bodies=0;globalThis.fetch=async(url,options)=>{
   equal(url,'/external');equal(options.headers.Range,`bytes=${range.offset}-${range.offset+63n}`);
   return{ok:status<400,status,headers:{get:()=>header},arrayBuffer:async()=>{bodies++;return new Uint8Array(64).buffer;}};
  };
  if(status===206 && header?.endsWith('/'+range.size)){equal((await readApplicationBytes('/external',range)).length,64);equal(bodies,1);}
  else{await assert.rejects(readApplicationBytes('/external',range),/无法读取/);equal(bodies,0);}
 }
}finally{globalThis.fetch=fetch;}
console.log(`${checks} application startup/directory/TLS/FLS/detach/manifest checks passed`);
