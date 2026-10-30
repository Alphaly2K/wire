import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve,dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { WasmMachine } from '../web/runtime.mjs';
import { mask } from './x87-reference.mjs';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../../..');
const build=spawnSync('cargo',['build','--quiet','--locked','--offline','--lib','--manifest-path','crates/wire-wasm/Cargo.toml','--target','wasm32-unknown-unknown','--target-dir','target/wasm-rewrite'],{cwd:root,encoding:'utf8'});assert.equal(build.status,0,build.stderr);
const {instance}=await WebAssembly.instantiate(readFileSync(resolve(root,'target/wasm-rewrite/wasm32-unknown-unknown/debug/wire_wasm.wasm'))),core=instance.exports;
const layout=Array.from(new Uint32Array(core.memory.buffer,core.wire_fpu_layout(),7)),stateLayout=Array.from(new Uint32Array(core.memory.buffer,core.wire_state_layout(),11)),base=0x10000n,data=0x20000n;
const encode=(v,size=16)=>Uint8Array.from({length:size},(_,n)=>Number((v>>BigInt(n*8))&255n));
function program(code,mode=64,options={}){let handle;const calls={numeric:0,resolve:0,load:0,store:0,fp_area:0,fp_commit:0,link:0,begin:0},watch={onLink:null,onArea:null};const imports={...core,wire_create(...args){handle=core.wire_create(...args);return handle;}};
 for(const name of Object.keys(core).filter(n=>n.startsWith('wire_ext_')||n.startsWith('wire_crc')))imports[name]=(...args)=>{calls.numeric++;const p=core.wire_state(handle),before=new Uint8Array(core.memory.buffer,p,stateLayout[0]).slice(),value=core[name](...args);assert.deepEqual(new Uint8Array(core.memory.buffer,p,stateLayout[0]),before,'numerical functions do not transact CPU backing');return value;};
 for(const name of ['resolve','load','store','fp_area','fp_commit','link','begin'])imports[`wire_${name}`]=(...args)=>{calls[name]++;if(name==='link')watch.onLink?.();const image=name==='fp_area'||name==='fp_commit',p=image?core.wire_state(handle):0,before=image?new Uint8Array(core.memory.buffer,p,stateLayout[0]).slice():null,result=core[`wire_${name}`](...args);if(image)assert.deepEqual(new Uint8Array(core.memory.buffer,p,stateLayout[0]),before,'image transfers do not transact CPU backing');if(name==='fp_area')watch.onArea?.(result,...args);return result;};const vm=new WasmMachine(imports,{mode,ip:base,...options});vm.map(base,4096n,7);vm.write(base,[...code,0xcc]);return {vm,calls,watch,get handle(){return handle;}};}
function fp(test,value){const p=core.wire_state(test.handle),v=new DataView(core.memory.buffer);if(value){core.wire_cancel_repeat(test.handle);for(const [name,n] of [['cw',0],['sw',1],['tags',2],['opcode',6]])v.setUint16(p+layout[n],value[name],true);for(const [name,n] of [['ip',4],['dp',5]])v.setBigUint64(p+layout[n],value[name],true);for(let n=0;n<8;n++){v.setBigUint64(p+layout[3]+n*16,value.regs[n]&mask(64),true);v.setBigUint64(p+layout[3]+n*16+8,value.regs[n]>>64n,true);}}
 return {cw:v.getUint16(p+layout[0],true),sw:v.getUint16(p+layout[1],true),tags:v.getUint16(p+layout[2],true),regs:Array.from({length:8},(_,n)=>v.getBigUint64(p+layout[3]+n*16,true)|(v.getBigUint64(p+layout[3]+n*16+8,true)<<64n)),ip:v.getBigUint64(p+layout[4],true),dp:v.getBigUint64(p+layout[5],true),opcode:v.getUint16(p+layout[6],true)};}

export {core,layout,stateLayout,base,data,encode,program,fp};
