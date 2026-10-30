// Generated legacy SSE inputs, independent integer/lane references, and real
// core Wasm memory. No game, native executor, or external test data is used.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { WasmMachine, Exit } from '../web/runtime.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const build = spawnSync('cargo', ['build', '--quiet', '--offline', '--locked', '--lib', '--manifest-path', 'crates/wire-wasm/Cargo.toml', '--target', 'wasm32-unknown-unknown', '--target-dir', 'target/wasm-rewrite'], { cwd: root, encoding: 'utf8' });
assert.equal(build.status, 0, build.stderr);
const { instance } = await WebAssembly.instantiate(readFileSync(resolve(root, 'target/wasm-rewrite/wasm32-unknown-unknown/debug/wire_wasm.wasm')));
const core = instance.exports, base = 0x10000n, data = 0x20000n;
const layout = Array.from(new Uint32Array(core.memory.buffer,core.wire_state_layout(),11));
let checks = 0;
function program(bytes, mode = 64) {
  const calls = { resolve: 0, load: 0, store: 0, store128: 0, link: 0, compile: 0, begin: 0 };
  let handle, ticket;
  const imports = { ...core,
    wire_create(...args) { handle = core.wire_create(...args); return handle; },
    wire_ticket(...args) { ticket = core.wire_ticket(...args); return ticket; },
  };
  const observation = { onLink: null, onMemory: null };
  for (const kind of Object.keys(calls)) imports[`wire_${kind}`] = (...args) => {
    calls[kind]++;
    if (kind === 'link') observation.onLink?.();
    if (['resolve', 'load', 'store', 'store128'].includes(kind)) observation.onMemory?.();
    return core[`wire_${kind}`](...args);
  };
  const vm = new WasmMachine(imports, { mode, ip: base });
  vm.map(base, 4096n, 7); vm.write(base, [...bytes, 0xcc]);
  return { vm, calls, observation, get handle() { return handle; }, get ticket() { return ticket; } };
}
const mask = bits => (1n << BigInt(bits)) - 1n;
const encode = (value, size = 16) => Uint8Array.from({ length: size }, (_, index) => Number((value >> BigInt(index * 8)) & 255n));
const decode = bytes => [...bytes].reduceRight((value, byte) => (value << 8n) | BigInt(byte), 0n);
const lanes = (bytes, bits) => Array.from({ length: 128 / bits }, (_, index) => (decode(bytes) >> BigInt(index * bits)) & mask(bits));
const pack = (values, bits) => encode(values.reduceRight((value, lane) => (value << BigInt(bits)) | (lane & mask(bits)), 0n));
const signed = (value, bits) => BigInt.asIntN(bits, value);
const clamp = (value, min, max) => value < min ? min : value > max ? max : value;
const equal = (actual, expected, message) => { assert.deepEqual([...actual], [...expected], message); checks++; };
const samples = [Array(16).fill(0), Array(16).fill(255), Array.from({ length: 16 }, (_, n) => n * 17), Array.from({ length: 16 }, (_, n) => (n * 61 + 0x80) & 255), [0, 0x80, 0xff, 0x7f, 0x55, 0xaa, 1, 0, 0x80, 0xff, 0, 0x80, 0xfe, 0xff, 0xff, 0x7f]];

const binary = [
  ['and',128,0xdb],['andnot',128,0xdf],['or',128,0xeb],['xor',128,0xef],
  ['add',8,0xfc],['add',16,0xfd],['add',32,0xfe],['add',64,0xd4],
  ['sub',8,0xf8],['sub',16,0xf9],['sub',32,0xfa],['sub',64,0xfb],
  ['adds',8,0xec],['adds',16,0xed],['addu',8,0xdc],['addu',16,0xdd],
  ['subs',8,0xe8],['subs',16,0xe9],['subu',8,0xd8],['subu',16,0xd9],
  ['eq',8,0x74],['eq',16,0x75],['eq',32,0x76],['eq',64,0x29,true],
  ['gt',8,0x64],['gt',16,0x65],['gt',32,0x66],['gt',64,0x37,true],
  ['mins',8,0x38,true],['minu',8,0xda],['maxs',8,0x3c,true],['maxu',8,0xde],
  ['mins',16,0xea],['minu',16,0x3a,true],['maxs',16,0xee],['maxu',16,0x3e,true],
  ['mins',32,0x39,true],['minu',32,0x3b,true],['maxs',32,0x3d,true],['maxu',32,0x3f,true],
  ['avg',8,0xe0],['avg',16,0xe3],['mul',16,0xd5],['mul',32,0x40,true],
  ['packs',16,0x63],['packu',16,0x67],['packs',32,0x6b],['packu',32,0x2b,true],
];
function reference(kind, bits, left, right) {
  const a = lanes(left, bits), b = lanes(right, bits), width = mask(bits);
  if (kind.startsWith('pack')) {
    const out = bits / 2, min = kind === 'packu' ? 0n : -(1n << BigInt(out - 1));
    const max = kind === 'packu' ? mask(out) : mask(out - 1);
    return pack([...a, ...b].map(value => clamp(signed(value, bits), min, max)), out);
  }
  return pack(a.map((x, index) => {
    const y = b[index], sx = signed(x, bits), sy = signed(y, bits);
    switch (kind) {
      case 'and': return x & y; case 'andnot': return ~x & y; case 'or': return x | y; case 'xor': return x ^ y;
      case 'add': return x + y; case 'sub': return x - y; case 'mul': return x * y; case 'avg': return (x + y + 1n) >> 1n;
      case 'eq': return x === y ? width : 0n; case 'gt': return sx > sy ? width : 0n;
      case 'mins': return sx < sy ? x : y; case 'minu': return x < y ? x : y;
      case 'maxs': return sx > sy ? x : y; case 'maxu': return x > y ? x : y;
      case 'adds': case 'subs': return clamp(kind === 'adds' ? sx + sy : sx - sy, -(1n << BigInt(bits - 1)), mask(bits - 1));
      case 'addu': case 'subu': return clamp(kind === 'addu' ? x + y : x - y, 0n, width);
      default: throw new Error(kind);
    }
  }), bits);
}
for (const mode of [32, 64]) for (const [kind, bits, opcode, extended] of binary) for (const memory of [false, true]) {
  const instruction = [0x66, 0x0f, ...(extended ? [0x38] : []), opcode, memory ? 0x03 : 0xc1];
  const test = program([0x04, 1, ...instruction], mode), { vm } = test;
  if (memory) vm.map(data, 4096n);
  for (let sample = 0; sample < samples.length; sample++) {
    const left = samples[sample], right = samples[(sample + 2) % samples.length];
    vm.ip = base; vm.register(0, 127n); vm.register(3, data); vm.flags = 0x602n;
    vm.xmm(0, left); vm.xmm(1, right); vm.xmm(mode === 32 ? 7 : 15, samples[3]);
    if (memory) vm.write(data, right);
    const before = vm.work;
    assert.equal((await vm.run(2n)).reason, Exit.Budget);
    equal(vm.xmm(0), reference(kind, bits, left, right), `${kind}/${bits}/${mode}/${memory}`);
    equal(vm.xmm(1), right); equal(vm.xmm(mode === 32 ? 7 : 15), samples[3]);
    assert.equal(vm.flags, 0xe92n); assert.equal(vm.work - before, 2n);
  }
  assert.equal(test.calls.resolve, memory ? 1 : 0); assert.equal(test.calls.load, 0); assert.equal(test.calls.store128, 0);
  vm.dispose();
}

for (const [bits, kind, opcode, immediateGroup] of [[16,'left',0xf1,6],[32,'left',0xf2,6],[64,'left',0xf3,6],[16,'right',0xd1,2],[32,'right',0xd2,2],[64,'right',0xd3,2],[16,'signed',0xe1,4],[32,'signed',0xe2,4]]) {
  for (const immediate of [false, true]) for (const count of [0n, 1n, 15n, 16n, 31n, 32n, 63n, 64n, 255n, ...(immediate ? [] : [0x100000000n, mask(64)])]) {
    const instruction = immediate ? [0x66, 0x0f, bits === 16 ? 0x71 : bits === 32 ? 0x72 : 0x73, 0xc0 | (immediateGroup << 3), Number(count)] : [0x66, 0x0f, opcode, 0xc1];
    const test = program(instruction), { vm } = test;
    vm.xmm(0, samples[4]); vm.xmm(1, encode(count | (mask(64) << 64n)));
    assert.equal((await vm.run(1n)).reason, Exit.Budget);
    const result = lanes(samples[4], bits).map(value => count >= BigInt(bits) ? kind === 'signed' && signed(value, bits) < 0 ? mask(bits) : 0n : kind === 'left' ? value << count : kind === 'signed' ? signed(value, bits) >> count : value >> count);
    equal(vm.xmm(0), pack(result, bits)); assert.equal(vm.flags, 0x202n); vm.dispose();
  }
}

for (const left of [false, true]) for (const count of [0,1,15,16,17,255]) {
  const { vm } = program([0x66, 0x0f, 0x73, left ? 0xf8 : 0xd8, count]); vm.xmm(0, samples[2]);
  assert.equal((await vm.run(1n)).reason, Exit.Budget);
  equal(vm.xmm(0), Array.from({ length: 16 }, (_, n) => { const index = left ? n - count : n + count; return index >= 0 && index < 16 ? samples[2][index] : 0; })); vm.dispose();
}

for (const [prefix, opcode, size, high] of [[0x66,0x60,1,false],[0x66,0x68,1,true],[0x66,0x61,2,false],[0x66,0x69,2,true],[0x66,0x62,4,false],[0x66,0x6a,4,true],[0x66,0x6c,8,false],[0x66,0x6d,8,true],[0,0x14,4,false],[0,0x15,4,true],[0x66,0x14,8,false],[0x66,0x15,8,true]]) {
  const { vm } = program([...(prefix ? [prefix] : []),0x0f,opcode,0xc1]); vm.xmm(0,samples[2]);vm.xmm(1,samples[3]);
  assert.equal((await vm.run(1n)).reason,Exit.Budget);
  const a=lanes(samples[2],size*8),b=lanes(samples[3],size*8),start=high ? a.length/2 : 0;
  equal(vm.xmm(0),pack(a.slice(start,start+a.length/2).flatMap((value,index)=>[value,b[start+index]]),size*8));vm.dispose();
}
for (const [kind,prefix,opcode] of [['d',0x66,0x70],['hw',0xf3,0x70],['lw',0xf2,0x70],['ps',0,0xc6],['pd',0x66,0xc6]]) for (const immediate of [0,1,0x1b,0xe4,255]) {
  const {vm}=program([...(prefix?[prefix]:[]),0x0f,opcode,0xc1,immediate]);vm.xmm(0,samples[2]);vm.xmm(1,samples[3]);
  assert.equal((await vm.run(1n)).reason,Exit.Budget);
  const bits=kind==='pd'?64:['hw','lw'].includes(kind)?16:32,a=lanes(samples[2],bits),b=lanes(samples[3],bits);
  const result=kind==='d'?b.map((_,n)=>b[(immediate>>(2*n))&3]):kind==='ps'?a.map((_,n)=>(n<2?a:b)[(immediate>>(2*n))&3]):kind==='pd'?[a[immediate&1],b[(immediate>>1)&1]]:b.map((value,n)=>((kind==='hw')===(n>=4))?b[(kind==='hw'?4:0)+((immediate>>(2*(n%4)))&3)]:value);
  equal(vm.xmm(0),pack(result,bits));vm.dispose();
}
for (let control=0;control<256;control+=16) {
  const {vm}=program([0x66,0x0f,0x38,0,0xc1]);const indices=Array.from({length:16},(_,n)=>control+n);vm.xmm(0,samples[2]);vm.xmm(1,indices);
  assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.xmm(0),indices.map(value=>value&128?0:samples[2][value&15]));vm.dispose();
}

for (const [prefix,opcode,bits] of [[0x66,0xd7,8],[0,0x50,32],[0x66,0x50,64]]) {
  const {vm}=program([...(prefix?[prefix]:[]),0x0f,opcode,0xc1]);vm.xmm(1,samples[4]);vm.register(0,mask(64));
  assert.equal((await vm.run(1n)).reason,Exit.Budget);
  assert.equal(vm.register(0),lanes(samples[4],bits).reduce((result,lane,n)=>result|((lane>>BigInt(bits-1))<<BigInt(n)),0n));vm.dispose();checks++;
}
const read128 = (handle, address) => encode(BigInt.asUintN(64, core.wire_load(handle,address,8,1)) | (BigInt.asUintN(64, core.wire_load(handle,address+8n,8,1)) << 64n));
for (const mode of [32,64]) for (const [prefix,load,store,aligned] of [[0,0x10,0x11,false],[0,0x28,0x29,true],[0x66,0x10,0x11,false],[0x66,0x28,0x29,true],[0xf3,0x6f,0x7f,false],[0x66,0x6f,0x7f,true]]) {
  for (const write of [false,true]) for (const offset of aligned?[0]:[0,1,4088]) {
    const test=program([...(prefix?[prefix]:[]),0x0f,write?store:load,write?0x0b:0x03],mode),{vm}=test;
    vm.map(data,8192n);const address=data+BigInt(offset);vm.write(address,samples[2]);vm.register(3,address);vm.xmm(0,samples[3]);vm.xmm(1,samples[4]);
    assert.equal((await vm.run(1n)).reason,Exit.Budget);
    equal(write?read128(test.handle,address):vm.xmm(0),write?samples[4]:samples[2]);
    assert.equal(test.calls.resolve,offset===4088?0:1);assert.equal(test.calls.store128,write&&offset===4088?1:0);assert.equal(test.calls.load,!write&&offset===4088?2:0);
    assert.equal(vm.flags,0x202n);vm.dispose();
  }
  if(aligned) {
    const test=program([...(prefix?[prefix]:[]),0x0f,load,0x03],mode),{vm}=test;
    vm.map(data,4096n);vm.register(3,data+1n);vm.xmm(0,samples[2]);assert.equal(core.wire_protect(test.handle,data,4096n,0x104),0);
    assert.equal((await vm.run(1n)).reason,Exit.Exception);assert.equal(core.wire_exception(test.handle),14);assert.equal(vm.retired,0n);equal(vm.xmm(0),samples[2]);assert.equal(test.calls.resolve,0);
    vm.register(3,data);assert.equal((await vm.run(1n)).reason,Exit.Fault);assert.equal(core.wire_fault_kind(test.handle),4,'alignment did not consume the guard');vm.dispose();
  }
}
for (const write of [false,true]) for (const reason of ['guard','cross-page']) {
  const test=program([0x04,1,0x66,0x0f,0xef,0xd2,0xf3,0x0f,write?0x7f:0x6f,write?0x03:0x03]),{vm}=test;
  vm.map(data,8192n);const address=data+(reason==='guard'?0n:4088n),page=reason==='guard'?data:data+4096n;
  vm.write(address,samples[3]);vm.register(0,127n);vm.register(3,address);vm.xmm(0,samples[2]);vm.xmm(2,samples[4]);
  assert.equal(core.wire_protect(test.handle,page,4096n,reason==='guard'?0x104:write?2:1),0);
  assert.equal((await vm.run(10n)).reason,Exit.Fault);equal(vm.xmm(0),samples[2]);equal(vm.xmm(2),Array(16).fill(0));
  assert.equal(vm.work,2n);assert.equal(vm.retired,2n);assert.equal(vm.ip,base+6n);assert.equal(vm.flags,0xa92n);
  assert.equal(core.wire_protect(test.handle,page,4096n,4),0);equal(read128(test.handle,address),samples[3],'a failed vector store cannot leave one half written');
  assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(write?read128(test.handle,address):vm.xmm(0),write?samples[2]:samples[3]);assert.equal(vm.retired,3n);vm.dispose();
}

for (const [prefix,opcode,high,registerForm] of [[0,0x12,false,false],[0,0x16,true,false],[0x66,0x12,false,false],[0x66,0x16,true,false],[0,0x12,false,true],[0,0x16,true,true]]) {
  const test=program([...(prefix?[prefix]:[]),0x0f,opcode,registerForm?0xc1:0x03]),{vm}=test;
  vm.map(data,4096n);vm.write(data,samples[3]);vm.register(3,data);vm.xmm(0,samples[2]);vm.xmm(1,samples[3]);
  assert.equal((await vm.run(1n)).reason,Exit.Budget);
  const source=samples[3].slice(registerForm&&!high?8:0,registerForm&&!high?16:8),expected=[...samples[2]];expected.splice(high?8:0,8,...source);equal(vm.xmm(0),expected);vm.dispose();
}
for (const size of [4,8]) for (const memory of [false,true]) for (const write of [false,true]) {
  const test=program([size===4?0xf3:0xf2,0x0f,write?0x11:0x10,memory?(write?0x0b:0x03):(write?0xc8:0xc1)]),{vm}=test;
  vm.map(data,4096n);vm.write(data,write?samples[2]:samples[3]);vm.register(3,data);vm.xmm(0,samples[2]);vm.xmm(1,samples[3]);
  assert.equal((await vm.run(1n)).reason,Exit.Budget);
  if(memory&&write) {const expected=[...samples[2]];expected.splice(0,size,...samples[3].slice(0,size));equal(read128(test.handle,data),expected);}
  else {const expected=memory?Array(16).fill(0):[...samples[2]];expected.splice(0,size,...samples[3].slice(0,size));equal(vm.xmm(0),expected);}
  vm.dispose();
}
for (const size of [4,8]) for (const toXmm of [false,true]) {
  const {vm}=program([0x66,...(size===8?[0x48]:[]),0x0f,toXmm?0x6e:0x7e,toXmm?0xc0:0xc8]);
  vm.register(0,0xfedcba9876543210n);vm.xmm(0,samples[2]);vm.xmm(1,samples[3]);assert.equal((await vm.run(1n)).reason,Exit.Budget);
  if(toXmm) equal(vm.xmm(0),encode(0xfedcba9876543210n&mask(size*8)));else assert.equal(vm.register(0),decode(samples[3].slice(0,size)));
  vm.dispose();checks++;
}
const narrow=program([0xf3,0x0f,0x7e,0xc1]);narrow.vm.xmm(0,samples[2]);narrow.vm.xmm(1,samples[3]);assert.equal((await narrow.vm.run(1n)).reason,Exit.Budget);equal(narrow.vm.xmm(0),[...samples[3].slice(0,8),...Array(8).fill(0)]);narrow.vm.dispose();

for(const pd of [false,true]) for(const [kind,opcode] of [['and',0x54],['andnot',0x55],['or',0x56],['xor',0x57]]) {
  const test=program([0x04,1,...(pd?[0x66]:[]),0x0f,opcode,0xc1]),{vm}=test;
  const a=encode(0x7f8000017ff0000000000001n),b=encode(mask(128)^0x8000000080000000n);
  vm.xmm(0,a);vm.xmm(1,b);vm.register(0,127n);
  new DataView(core.memory.buffer).setUint32(core.wire_state(test.handle)+layout[10],0x5f80,true);
  assert.equal((await vm.run(2n)).reason,Exit.Budget);equal(vm.xmm(0),reference(kind,128,a,b));assert.equal(vm.flags,0xa92n);
  assert.equal(new DataView(core.memory.buffer).getUint32(core.wire_state(test.handle)+layout[10],true),0x5f80,'bit operations do not interpret NaNs or accrue FP exceptions');vm.dispose();
}
for(const pd of [false,true]) for(const high of [false,true]) {
  const test=program([...(pd?[0x66]:[]),0x0f,high?0x17:0x13,0x0b]),{vm}=test;
  vm.map(data,4096n);vm.write(data,samples[2]);vm.register(3,data);vm.xmm(1,samples[3]);
  assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(read128(test.handle,data),[...samples[3].slice(high?8:0,high?16:8),...samples[2].slice(8)]);vm.dispose();
}
for(const size of [4,8]) {
  const test=program([size===4?0x66:0xf3,0x0f,size===4?0x6e:0x7e,0x03]),{vm}=test;
  vm.map(data,4096n);vm.write(data+4096n-BigInt(size),samples[3].slice(0,size));vm.register(3,data+4096n-BigInt(size));vm.xmm(0,samples[2]);
  assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.xmm(0),[...samples[3].slice(0,size),...Array(16-size).fill(0)],'narrow XMM loads access only their operand width');vm.dispose();
}

const loop=program([0x66,0x0f,0xfc,0xc1,0xff,0xe2]);
loop.vm.map(data,4096n,7);loop.vm.write(data,[0x83,0xe9,1,0x74,2,0xff,0xe3,0xcc]);
loop.vm.map(0x30000n,4096n,7);loop.vm.write(0x30000n,[0x66,0x0f,0xef,0xd0,0x66,0x44,0x0f,0xd4,0xf8,0xff,0xe6]);
loop.vm.register(2,data);loop.vm.register(3,0x30000n);loop.vm.register(6,base);loop.vm.register(1,3n);loop.vm.xmm(1,Array(16).fill(1));
assert.equal((await loop.vm.run(100n)).reason,Exit.Unsupported);
const installed=loop.vm.stats();
for (const count of [12,2]) {
  loop.vm.ip=base;loop.vm.register(1,BigInt(count));loop.vm.xmm(0,Array(16).fill(7));loop.vm.xmm(1,Array(16).fill(1));loop.vm.xmm(2,Array(16).fill(0));loop.vm.xmm(15,Array(16).fill(0));
  loop.calls.link=0;loop.calls.begin=0;
  loop.observation.onLink=()=>{equal(loop.vm.xmm(0),Array(16).fill(7),'XMM backing remains at entry while resident modules execute');equal(loop.vm.xmm(2),Array(16).fill(0));equal(loop.vm.xmm(15),Array(16).fill(0));};
  assert.equal((await loop.vm.run(1000n)).reason,Exit.Unsupported);
  equal(loop.vm.xmm(0),Array(16).fill(7+count));
  let xor=0,total=0n;for(let n=1;n<count;n++){xor^=7+n;total=(total+decode(Array(8).fill(7+n)))&mask(64);}
  equal(loop.vm.xmm(2),Array(16).fill(xor));equal(loop.vm.xmm(15),encode(total|(total<<64n)));
  assert.equal(loop.calls.begin,1);assert.deepEqual(loop.vm.stats(),installed);assert.equal(loop.calls.link,3,'integer and SIMD modules share one resident run');
}
loop.vm.dispose();

const step=program([0x66,0x0f,0xfc,0xc1,0x83,0xe9,1,0x75,0xf7]);step.vm.xmm(1,Array(16).fill(1));step.vm.register(1,7n);
for(let n=0;n<21;n++){assert.equal((await step.vm.run(1n)).reason,Exit.Budget);equal(step.vm.xmm(0),Array(16).fill(Math.floor(n/3)+1));}
assert.equal(step.calls.compile,1);assert.equal(step.vm.retired,21n);step.vm.dispose();

const owners=[program([0x66,0x0f,0xef,0xc1]),program([0x66,0x0f,0xef,0xc1])];
owners.forEach(({vm},n)=>{vm.xmm(0,Array(16).fill(n+1));vm.xmm(1,Array(16).fill(0xaa));});
await Promise.all(owners.map(({vm})=>vm.run(10n)));owners.forEach(({vm},n)=>{equal(vm.xmm(0),Array(16).fill((n+1)^0xaa));vm.dispose();});

const source=program([0x66,0x0f,0xfc,0xc1,0xf3,0x0f,0x7f,0x03]);source.vm.register(3,base+0x100n);source.vm.xmm(0,samples[2]);source.vm.xmm(1,Array(16).fill(1));
assert.equal((await source.vm.run(2n)).reason,Exit.Budget);assert.equal(source.vm.retired,2n);equal(read128(source.handle,base+0x100n),samples[2].map(value=>(value+1)&255));equal(source.vm.xmm(0),samples[2].map(value=>(value+1)&255));assert.equal(source.calls.store128,1);assert.equal(core.wire_ready(source.handle,source.ticket),0);source.vm.dispose();

// Zero budget preserves the vector bank at the shared resident boundary.
const isolated=program([0x66,0x0f,0xef,0xc1]);isolated.vm.xmm(0,samples[2]);isolated.vm.xmm(1,samples[3]);
assert.equal((await isolated.vm.run(0n)).reason,Exit.Budget);equal(isolated.vm.xmm(0),samples[2]);
assert.equal((await isolated.vm.run(1n)).reason,Exit.Budget);equal(isolated.vm.xmm(0),reference('xor',128,samples[2],samples[3]));isolated.vm.dispose();
const advanced = [
  ['mulhi',16,0xe5,false],['mulhiu',16,0xe4,false],['mulq',32,0x28,true],['mulqu',32,0xf4,false],
  ['madd',16,0xf5,false],['maddubs',8,0x04,true],['mulhrs',16,0x0b,true],['sad',8,0xf6,false],
  ['hadd',16,0x01,true],['hadd',32,0x02,true],['hadds',16,0x03,true],
  ['hsub',16,0x05,true],['hsub',32,0x06,true],['hsubs',16,0x07,true],
  ['sign',8,0x08,true],['sign',16,0x09,true],['sign',32,0x0a,true],
  ['abs',8,0x1c,true],['abs',16,0x1d,true],['abs',32,0x1e,true],['minpos',16,0x41,true],
];
const extendedSamples=[...samples,pack(Array(8).fill(0x8000n),16),pack([0x7fffn,0x7fffn,0x8000n,0x8000n,1n,0n,1n,0n],16),pack([mask(32),mask(31),1n<<31n,0n],32)];
function advancedReference(kind,bits,left,right){
  const a=lanes(left,bits),b=lanes(right,bits);
  if(kind==='abs')return pack(b.map(value=>{const x=signed(value,bits);return x<0n?-x:x;}),bits);
  if(kind==='sign')return pack(a.map((value,n)=>signed(b[n],bits)===0n?0n:signed(b[n],bits)<0n?-value:value),bits);
  if(kind==='mulhi'||kind==='mulhiu')return pack(a.map((value,n)=>((kind==='mulhi'?signed(value,bits):value)*(kind==='mulhi'?signed(b[n],bits):b[n]))>>16n),16);
  if(kind==='mulhrs')return pack(a.map((value,n)=>(signed(value,16)*signed(b[n],16)+0x4000n)>>15n),16);
  if(kind==='mulq'||kind==='mulqu')return pack([0,2].map(n=>(kind==='mulq'?signed(a[n],32):a[n])*(kind==='mulq'?signed(b[n],32):b[n])),64);
  if(kind==='madd')return pack(Array.from({length:4},(_,n)=>signed(a[2*n],16)*signed(b[2*n],16)+signed(a[2*n+1],16)*signed(b[2*n+1],16)),32);
  if(kind==='maddubs')return pack(Array.from({length:8},(_,n)=>clamp(a[2*n]*signed(b[2*n],8)+a[2*n+1]*signed(b[2*n+1],8),-32768n,32767n)),16);
  if(kind==='sad')return pack([0,8].map(start=>a.slice(start,start+8).reduce((sum,value,n)=>sum+(value>b[start+n]?value-b[start+n]:b[start+n]-value),0n)),64);
  if(kind==='minpos'){const min=b.reduce((x,y)=>x<y?x:y);return pack([min,BigInt(b.indexOf(min))],16);}
  if(kind.startsWith('h'))return pack([a,b].flatMap(values=>Array.from({length:values.length/2},(_,n)=>{
    const x=signed(values[2*n],bits),y=signed(values[2*n+1],bits),value=kind.startsWith('hsub')?x-y:x+y;
    return kind.endsWith('s')?clamp(value,-(1n<<BigInt(bits-1)),mask(bits-1)):value;
  })),bits);
  throw new Error(kind);
}
for(const mode of [32,64])for(const [kind,bits,opcode,extended] of advanced)for(const memory of [false,true]){
  const test=program([0x04,1,0x66,0x0f,...(extended?[0x38]:[]),opcode,memory?0x03:0xc1],mode),{vm}=test;
  if(memory)vm.map(data,4096n);
  for(let n=0;n<extendedSamples.length;n++){
    const a=extendedSamples[n],b=extendedSamples[(n+3)%extendedSamples.length];vm.ip=base;vm.register(0,127n);vm.register(3,data);vm.flags=0x602n;vm.mxcsr=0x5f9f;vm.xmm(0,a);vm.xmm(1,b);if(memory)vm.write(data,b);
    assert.equal((await vm.run(2n)).reason,Exit.Budget,`${kind}/${bits}/${mode}/${memory}`);equal(vm.xmm(0),advancedReference(kind,bits,a,b),`${kind}/${bits}/${n}`);equal(vm.xmm(1),b);assert.equal(vm.flags,0xe92n);assert.equal(vm.mxcsr,0x5f9f);
  }
  assert.equal(test.calls.resolve,memory?1:0);assert.equal(test.calls.load,0);vm.dispose();
  if(!memory){
    const {vm}=program([0x66,0x0f,...(extended?[0x38]:[]),opcode,0xc0],mode);vm.xmm(0,extendedSamples[5]);assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.xmm(0),advancedReference(kind,bits,extendedSamples[5],extendedSamples[5]),'source and destination overlap');vm.dispose();
  }
}
for(const signedExtension of [false,true])for(const [from,to,number] of [[8,16,0],[8,32,1],[8,64,2],[16,32,3],[16,64,4],[32,64,5]])for(const memory of [false,true]){
  const test=program([0x66,0x0f,0x38,(signedExtension?0x20:0x30)+number,memory?0x03:0xc1]),{vm}=test,width=16*from/to;
  if(memory)vm.map(data,4096n);const address=data+4096n-BigInt(width);vm.register(3,address);
  for(const bytes of extendedSamples){vm.ip=base;vm.xmm(0,samples[2]);vm.xmm(1,bytes);vm.mxcsr=0xffff;if(memory)vm.write(address,bytes.slice(0,width));assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.xmm(0),pack(lanes(bytes,from).slice(0,128/to).map(value=>signedExtension?signed(value,from):value),to));assert.equal(vm.mxcsr,0xffff);}
  assert.equal(test.calls.resolve,memory?1:0);assert.equal(test.calls.load,0);vm.dispose();
}
for(const [bits,opcode] of [[8,0x10],[32,0x14],[64,0x15]])for(const memory of [false,true])for(const alias of [false,true]){
  const test=program([0x66,0x0f,0x38,opcode,memory?(alias?0x03:0x13):(alias?0xc3:0xd3)]),{vm}=test;
  if(memory)vm.map(data,4096n);vm.register(3,data);
  for(const control of extendedSamples){
    const a=alias?control:samples[2],b=samples[3];vm.ip=base;vm.xmm(0,control);vm.xmm(2,a);vm.xmm(3,b);if(memory)vm.write(data,b);
    assert.equal((await vm.run(1n)).reason,Exit.Budget);const result=lanes(a,bits).map((value,n)=>lanes(control,bits)[n]>>(BigInt(bits)-1n)?lanes(b,bits)[n]:value);equal(vm.xmm(alias?0:2),pack(result,bits));if(!alias)equal(vm.xmm(0),control,'implicit XMM0 remains read-only');
  }
  vm.dispose();
}
for(const [size,opcode] of [[2,0x0e],[4,0x0c],[8,0x0d]])for(const immediate of [0,1,0x1b,0xe4,255]){
  const {vm}=program([0x66,0x0f,0x3a,opcode,0xc1,immediate]);vm.xmm(0,samples[2]);vm.xmm(1,samples[3]);assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.xmm(0),samples[2].map((value,n)=>immediate&(1<<Math.floor(n/size))?samples[3][n]:value));vm.dispose();
}
for(const immediate of [0,1,15,16,17,31,32,33,255])for(const memory of [false,true]){
  const {vm}=program([0x66,0x0f,0x3a,0x0f,memory?0x03:0xc1,immediate]);vm.map(data,4096n);vm.write(data,samples[3]);vm.register(3,data);vm.xmm(0,samples[2]);vm.xmm(1,samples[3]);assert.equal((await vm.run(1n)).reason,Exit.Budget);const joined=[...samples[3],...samples[2]];equal(vm.xmm(0),Array.from({length:16},(_,n)=>joined[n+immediate]??0));vm.dispose();
}
for(const [size,insert,extract] of [[1,0x20,0x14],[2,0xc4,0x15],[4,0x22,0x16],[8,0x22,0x16]])for(const memory of [false,true])for(const immediate of [0,1,7,15,255]){
  const rex=size===8?[0x48]:[],insertCode=[0x66,...rex,0x0f,...(size===2?[]:[0x3a]),insert,memory?0x03:0xc1,immediate];
  const test=program(insertCode),{vm}=test,value=0xfedcba9876543210n;vm.register(1,value);vm.xmm(0,samples[2]);const address=data+4096n-BigInt(size);
  if(memory){vm.map(data,4096n);vm.write(address,encode(value,size));vm.register(3,address);}
  assert.equal((await vm.run(1n)).reason,Exit.Budget);const expected=[...samples[2]],at=(immediate&(16/size-1))*size;expected.splice(at,size,...encode(value,size));equal(vm.xmm(0),expected);vm.dispose();
  const out=program([0x66,...rex,0x0f,0x3a,extract,memory?0x03:0xc1,immediate]);out.vm.xmm(0,samples[3]);out.vm.register(1,mask(64));
  if(memory){out.vm.map(data,4096n);out.vm.write(data,samples[2]);out.vm.register(3,address);}
  assert.equal((await out.vm.run(1n)).reason,Exit.Budget);assert.equal(memory?out.vm.readInt(address,size):out.vm.register(1),decode(samples[3].slice(at,at+size)));checks++;out.vm.dispose();
}
// Legacy PEXTRW and EXTRACTPS have distinct encodings and the same unsigned
// destination rule, including clearing the high half of a 32-bit GPR write.
for(const [prefix,opcode,size] of [[[],0xc5,2],[[0x3a],0x17,4]]){
  const {vm}=program([0x66,0x0f,...prefix,opcode,0xc1,255]);vm.xmm(0,samples[2]);vm.xmm(1,samples[3]);vm.register(prefix.length?1:0,mask(64));assert.equal((await vm.run(1n)).reason,Exit.Budget);assert.equal(vm.register(prefix.length?1:0),decode((prefix.length?samples[2]:samples[3]).slice(16-size)));checks++;vm.dispose();
}
for(const memory of [false,true]){
  const test=program(Array.from({length:256},(_,immediate)=>[0x66,0x0f,0x3a,0x21,memory?0x03:0xc1,immediate]).flat()),{vm}=test;
  if(memory){vm.map(data,4096n);vm.register(3,data+4092n);vm.write(data+4092n,samples[3].slice(0,4));}
  for(let immediate=0;immediate<256;immediate++){
    vm.ip=base+BigInt(6*immediate);vm.xmm(0,samples[2]);vm.xmm(1,samples[3]);assert.equal((await vm.run(1n)).reason,Exit.Budget);const expected=[...samples[2]],from=memory?0:(immediate>>6)*4;expected.splice(((immediate>>4)&3)*4,4,...samples[3].slice(from,from+4));for(let n=0;n<16;n++)if(immediate&(1<<Math.floor(n/4)))expected[n]=0;equal(vm.xmm(0),expected);
  }
  assert.equal(test.calls.compile,1);vm.dispose();
}
for(const immediate of [0,1,2,3,4,5,6,7,255])for(const memory of [false,true]){
  const test=program([0x66,0x0f,0x3a,0x42,memory?0x03:0xc1,immediate]),{vm}=test;if(memory)vm.map(data,4096n);vm.register(3,data);
  for(let n=0;n<extendedSamples.length;n++){const a=extendedSamples[n],b=extendedSamples[(n+1)%extendedSamples.length];vm.ip=base;vm.xmm(0,a);vm.xmm(1,b);if(memory)vm.write(data,b);assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.xmm(0),pack(Array.from({length:8},(_,lane)=>Array.from({length:4},(_,k)=>Math.abs(a[lane+(immediate&4)+k]-b[(immediate&3)*4+k])).reduce((sum,x)=>sum+BigInt(x),0n)),16));}vm.dispose();
}
for(const [prefix,opcode,kind] of [[0xf2,0x12,'double'],[0xf3,0x12,'low'],[0xf3,0x16,'high']])for(const memory of [false,true]){
  const {vm}=program([prefix,0x0f,opcode,memory?0x03:0xc1]);vm.xmm(0,samples[2]);vm.xmm(1,samples[3]);if(memory){vm.map(data,4096n);const address=kind==='double'?data+4088n:data;vm.register(3,address);vm.write(address,samples[3].slice(0,kind==='double'?8:16));}
  assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.xmm(0),Array.from({length:16},(_,n)=>samples[3][kind==='double'?n%8:Math.floor(n/8)*8+(kind==='high'?4:0)+n%4]));vm.dispose();
}
for(const memory of [false,true])for(const [a,b] of [[0n,0n],[0n,1n],[1n,1n],[1n,3n],[0x80000000000000000000000000000000n,0x80000000000000000000000000000000n]]){
  const test=program([0x04,1,0x66,0x0f,0x38,0x17,memory?0x03:0xc1,0x0f,0x90,0xc0,0x0f,0x92,0xc1,0x0f,0x94,0xc2,0x0f,0x9a,0xc7]),{vm}=test;
  vm.xmm(0,encode(a));vm.xmm(1,encode(b));vm.register(0,127n);vm.flags=0x602n;vm.mxcsr=0x9fc0;if(memory){vm.map(data,4096n);vm.write(data,encode(b));vm.register(3,data);}
  const flags=((a&b)===0n?64n:0n)|((~a&b&mask(128))===0n?1n:0n);assert.equal((await vm.run(6n)).reason,Exit.Budget);assert.equal(vm.flags,0x602n|flags);assert.equal(vm.register(0)&255n,0n);assert.equal(vm.register(1)&255n,flags&1n);assert.equal(vm.register(2)&255n,flags>>6n);assert.equal((vm.register(3)>>8n)&255n,0n);assert.equal(vm.mxcsr,0x9fc0);equal(vm.xmm(0),encode(a));vm.dispose();
}

for(const [prefix,opcode,extended,write,unaligned] of [[0xf2,0xf0,false,false,true],[0x66,0x2a,true,false,false],[0x66,0xe7,false,true,false],[0,0x2b,false,true,false],[0x66,0x2b,false,true,false]])for(const offset of unaligned?[0,1,4088]:[0]){
  const test=program([...(prefix?[prefix]:[]),0x0f,...(extended?[0x38]:[]),opcode,write?0x0b:0x03]),{vm}=test;vm.map(data,8192n);vm.write(data+BigInt(offset),samples[3]);vm.register(3,data+BigInt(offset));vm.xmm(0,samples[2]);vm.xmm(1,samples[4]);vm.mxcsr=0xffff;
  assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(write?read128(test.handle,data+BigInt(offset)):vm.xmm(0),write?samples[4]:samples[3]);assert.equal(vm.mxcsr,0xffff);vm.dispose();
}
for(const [code,size,write] of [
  [[0x66,0x0f,0x38,0x20,0x03],8,false],[[0x66,0x0f,0x38,0x22,0x03],2,false],
  [[0x66,0x0f,0x3a,0x20,0x03,5],1,false],[[0x66,0x48,0x0f,0x3a,0x22,0x03,1],8,false],
  [[0x66,0x0f,0x3a,0x21,0x03,0xf1],4,false],
  [[0x66,0x0f,0x3a,0x14,0x03,15],1,true],[[0x66,0x48,0x0f,0x3a,0x16,0x03,1],8,true],
])for(const cross of size===1?[false]:[false,true]){
  const test=program([0x04,1,...code]),{vm}=test;vm.map(data,8192n);const address=data+(cross?4096n-BigInt(size/2):0n),page=cross?data+4096n:data;vm.write(address,samples[3]);vm.register(3,address);vm.register(0,127n);vm.flags=0x602n;vm.mxcsr=0x9fc0;vm.xmm(0,samples[2]);
  assert.equal(core.wire_protect(test.handle,page,4096n,cross?(write?2:1):0x104),0);
  assert.equal((await vm.run(10n)).reason,Exit.Fault);assert.equal(vm.ip,base+2n);assert.equal(vm.work,1n);assert.equal(vm.retired,1n);assert.equal(vm.flags,0xe92n);assert.equal(vm.mxcsr,0x9fc0);equal(vm.xmm(0),samples[2]);
  assert.equal(core.wire_protect(test.handle,page,4096n,4),0);equal(read128(test.handle,address),samples[3],'failed narrow writes leave the whole operand unchanged');assert.equal((await vm.run(1n)).reason,Exit.Budget);vm.dispose();
}
for(const bad of [false,true]){
  const test=program([0x04,1,0x66,0x0f,0x38,0x17,0x03]),{vm}=test;vm.map(data,4096n);vm.write(data,samples[3]);vm.register(3,data+(bad?1n:0n));vm.register(0,127n);vm.flags=0x602n;vm.xmm(0,samples[2]);
  assert.equal(core.wire_protect(test.handle,data,4096n,0x104),0);assert.equal((await vm.run(10n)).reason,bad?Exit.Exception:Exit.Fault);assert.equal(vm.flags,0xe92n);assert.equal(vm.work,1n);equal(vm.xmm(0),samples[2]);
  if(bad){assert.equal(core.wire_exception(test.handle),14);vm.register(3,data);assert.equal((await vm.run(1n)).reason,Exit.Fault);assert.equal(core.wire_fault_kind(test.handle),4,'alignment fault preserves the guard');}vm.dispose();
}
const rewritten=program([0x66,0x0f,0x3a,0x16,0x03,0,0x04,1]);rewritten.vm.register(3,base+6n);rewritten.vm.register(0,123n);rewritten.vm.flags=0x602n;rewritten.vm.xmm(0,encode(0xccn));
assert.equal(core.wire_compile(rewritten.handle),0);const overwrittenTicket=core.wire_ticket(rewritten.handle);assert.equal((await rewritten.vm.run(10n)).reason,Exit.Unsupported);assert.equal(rewritten.vm.ip,base+6n);assert.equal(rewritten.vm.work,1n);assert.equal(rewritten.vm.retired,1n);assert.equal(rewritten.vm.register(0),123n);assert.equal(rewritten.vm.flags,0x602n);assert.equal(core.wire_ready(rewritten.handle,overwrittenTicket),0);equal(rewritten.vm.xmm(0),encode(0xccn));rewritten.vm.dispose();

// A mask produced by one chunk remains resident when another chunk reads it
// implicitly. Neither instruction publishes intermediate XMM backing to JS.
const implicit=program([0x66,0x0f,0x38,0x08,0xc1,0xff,0xe2]);implicit.vm.map(data,4096n,7);implicit.vm.write(data,[0x66,0x0f,0x38,0x10,0xd3,0x83,0xe9,1,0x74,2,0xff,0xe7,0xcc]);
implicit.vm.register(2,data);implicit.vm.register(7,base);implicit.vm.register(1,2n);implicit.vm.xmm(0,samples[2]);implicit.vm.xmm(1,Array(16).fill(255));implicit.vm.xmm(2,samples[3]);implicit.vm.xmm(3,samples[4]);assert.equal((await implicit.vm.run(100n)).reason,Exit.Unsupported);
for(const count of [9,2]){
  implicit.vm.ip=base;implicit.vm.register(1,BigInt(count));implicit.vm.xmm(0,samples[2]);implicit.vm.xmm(2,samples[3]);implicit.calls.begin=0;implicit.calls.link=0;implicit.observation.onLink=()=>{equal(implicit.vm.xmm(0),samples[2]);equal(implicit.vm.xmm(2),samples[3]);};
  assert.equal((await implicit.vm.run(1000n)).reason,Exit.Unsupported);const control=count%2?samples[2].map(value=>(-value)&255):samples[2];equal(implicit.vm.xmm(0),control);let result=[...samples[3]],maskValue=[...samples[2]];for(let n=0;n<count;n++){maskValue=maskValue.map(value=>(-value)&255);result=result.map((value,k)=>maskValue[k]&128?samples[4][k]:value);}equal(implicit.vm.xmm(2),result);assert.equal(implicit.calls.begin,1);assert.equal(implicit.calls.link,2);
}implicit.vm.dispose();

console.log(`${checks} SIMD execution checks passed: packed arithmetic, extension, lane transfers, blends, precise flags/faults, and resident bank isolation`);
