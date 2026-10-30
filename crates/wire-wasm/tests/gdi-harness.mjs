import assert from 'node:assert/strict';
import {WasmMachine,Exit} from '../web/runtime.mjs';
import {core,stateLayout,encode} from './x87-harness.mjs';
import {fixture} from './pe-fixture.mjs';
export let checks=0;export const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
export const snapshot=t=>new Uint8Array(core.memory.buffer,core.wire_state(t.handle),stateLayout[0]).slice();
export async function machine(mode,{dep=true,clock:readClock=()=>123456789n,wallClock:readWall=()=>{throw new Error('unexpected wall clock');},random,cpuTimes,timeZone}={}){let handle;const time={calls:0,wallCalls:0};const vm=new WasmMachine({...core,wire_create(...a){handle=core.wire_create(...a);return handle;}},{mode,clock(){time.calls++;return readClock();},wallClock(){time.wallCalls++;return readWall();},random,cpuTimes,timeZone});const f=fixture(mode,{tls:false});f.put(f.opt+70,dep?0x140:0x40,2);f.put(f.dirs+8,0);f.put(f.dirs+12,0);f.bytes.fill(0x90,f.offset(0x1000),f.offset(0x1100));f.bytes[f.offset(0x1000)]=0xc3;f.relocations.splice(0,f.relocations.length,...f.relocations.filter(r=>r>=0x1100));f.rebuildRelocations();const image=vm.load(f.bytes);equal((await vm.run(100n)).reason,Exit.Returned);vm.map(0x20000n,0x30000n,7);return{vm,mode,handle,image,time,sp:image.sp-2048n,error:image.teb+BigInt(mode===32?0x34:0x68)};}
export function frame(t,name,args=[],dll='user32.dll'){const {vm,mode,sp}=t;vm.ip=vm.bind(dll,name);vm.register(4,sp);vm.write(sp,encode(vm.entryPoints.returned,mode/8));for(let n=0;n<args.length;n++)if(mode===32)vm.write(sp+4n+BigInt(n*4),encode(args[n],4));else if(n<4)vm.register([1,2,8,9][n],args[n]);else vm.write(sp+40n+BigInt(n-4)*8n,encode(args[n],8));}
export function call(t,name,args=[],dll){const{vm}=t;frame(t,name,args,dll);const w=vm.work,r=vm.retired;equal(vm.service().reason,Exit.Dispatch);equal(vm.work,w+1n);equal(vm.retired,r);return vm.register(0);}
export const gdi=(t,name,args=[])=>call(t,name,args,'gdi32.dll');
export async function finish(t,budget=1000n){for(let n=0;n<10000;n++){const r=await t.vm.run(budget);if(r.reason===Exit.Returned)return t.vm.register(0);equal(r.reason,Exit.Budget);}throw new Error('paint call did not finish');}
export async function invoke(t,name,args=[],budget=1000n){frame(t,name,args);return finish(t,budget);}
const text=(t,p,s)=>t.vm.write(p,Uint8Array.from([...s+'\0'].flatMap(c=>[c.charCodeAt(0)&255,c.charCodeAt(0)>>8])));
export async function window(t,{proc,brush=6n}={}){const{vm,mode}=t;vm.write(0x20000n,new Uint8Array(80));for(const[at,value,size]of[[0,mode===32?48n:80n,4],[8,proc??vm.bind('user32.dll','DefWindowProcW'),mode/8],[mode===32?20:24,t.image.base,mode/8],[mode===32?32:48,brush,mode/8],[mode===32?40:64,0x20100n,mode/8]])vm.write(0x20000n+BigInt(at),encode(value,size));text(t,0x20100n,'Paint');text(t,0x20180n,'Pixels');const atom=call(t,'RegisterClassExW',[0x20000n]);return invoke(t,'CreateWindowExW',[0n,atom,0x20180n,0n,10n,20n,16n,12n,0n,0n,t.image.base,0n]);}
export function rectangle(t,p,r){t.vm.write(p,r.flatMap(n=>Array.from(encode(BigInt.asUintN(32,BigInt(n)),4))));}
export const readRect=(t,p)=>[0n,4n,8n,12n].map(o=>Number(BigInt.asIntN(32,t.vm.readInt(p+o))));
export function trace(t){const{vm,mode}=t;const b=mode===32?[0x8b,0x44,0x24,8,0x83,0xf8,0x14,0x75,6,0xff,0x05,...encode(0x23000n,4),0xb8,...encode(vm.bind('user32.dll','DefWindowProcW'),4),0xff,0xe0]:[0x83,0xfa,0x14,0x75,13,0x49,0xbb,...encode(0x23000n,8),0x41,0xff,0x03,0x48,0xb8,...encode(vm.bind('user32.dll','DefWindowProcW'),8),0xff,0xe0];vm.write(0x30000n,b);return 0x30000n;}
export const color=(pixels,x,y)=>Array.from(pixels.rgba.slice((y*pixels.width+x)*4,(y*pixels.width+x)*4+4));

export {core,encode,Exit};
