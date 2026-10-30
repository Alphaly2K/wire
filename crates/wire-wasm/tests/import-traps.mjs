import assert from 'node:assert/strict';
import {WasmMachine, Exit} from '../web/runtime.mjs';
import {core, stateLayout, encode} from './x87-harness.mjs';
import {fixture} from './pe-fixture.mjs';
import {moduleFixture} from './module-fixture.mjs';

let checks = 0;
const equal = (actual, expected, label) => { assert.deepEqual(actual, expected, label); checks++; };
function machine(mode) {
  let handle;
  const vm = new WasmMachine({...core, wire_create(...args) {
    handle = core.wire_create(...args); return handle;
  }}, {mode, clock() { throw new Error('unexpected elapsed sample'); },
    wallClock() { throw new Error('unexpected wall sample'); },
    random() { throw new Error('unexpected random sample'); }});
  return {vm, get handle() { return handle; }};
}
const state = t => new Uint8Array(core.memory.buffer, core.wire_state(t.handle), stateLayout[0]).slice();
function input(mode, dll, symbol) {
  const f = fixture(mode);
  f.rtext(0x20c0, `${dll}\0`);
  if (typeof symbol === 'number') {
    for (const rva of [0x2080, 0x20a0]) f.rput(rva, (1n << BigInt(mode-1)) | BigInt(symbol), f.ptr);
  } else f.rtext(0x20e2, `${symbol}\0`);
  return f;
}
function call(t, name, args) {
  const {vm, mode, image} = t, sp = image.sp - 1024n;
  vm.ip = vm.bind('kernel32.dll', name); vm.register(4, sp);
  vm.write(sp, encode(vm.entryPoints.returned, mode/8));
  for (let n = 0; n < args.length; n++) {
    if (mode === 32) vm.write(sp+4n+BigInt(n*4), encode(args[n], 4));
    else vm.register([1, 2, 8, 9][n], args[n]);
  }
  equal(vm.service().reason, Exit.Dispatch);
  return vm.register(0);
}

for (const mode of [32, 64]) {
  for (const [dll, symbol] of [
    ['kernel32.dll', 'MissingFixtureExport'],
    ['kernelbase.dll', 'MissingFixtureExport'],
    ['opengl32.dll', 'wglMakeCurrent'],
    ['ws2_32.dll', 51],
  ]) {
    const f = input(mode, dll, symbol), original = f.bytes.slice(), t = machine(mode), {vm} = t;
    const before = state(t);
    assert.throws(() => vm.load(f.bytes));
    equal(state(t), before); equal(vm.image, null);
    equal(core.wire_import_trap_count(t.handle), 0, 'strict failure publishes no trap');
    const image = vm.load(f.bytes, {diagnosticImports: true}), trap = vm.readInt(image.base+0x20a0n, mode/8);
    equal(image.importTraps, 1); equal(f.bytes, original, 'diagnostic loading leaves the input bytes intact');
    equal(vm.work, 0n); equal(vm.retired, 0n);
    for (let n = 0; n < 100; n++) {
      equal((await vm.run(1n)).reason, Exit.Budget);
      if (vm.ip === trap) break;
      assert.ok(n < 99, 'the original startup reaches the imported call');
    }
    equal(vm.ip, trap); equal(vm.readInt(image.base+0x3008n), 2n, 'both original TLS callbacks run');
    equal(vm.readInt(image.base+0x3010n), 0x11223344n);
    equal(vm.work, vm.retired, 'unimplemented services do not complete synthetic work');
    const saved = state(t), stack = vm.readInt(vm.register(4), mode/8), output = vm.readInt(image.base+0x3000n, 8);
    const expected = `unimplemented diagnostic import: ${dll}!${typeof symbol === 'number' ? '#'+symbol : symbol}`;
    assert.throws(() => vm.service(), error => error.message === expected);
    equal(state(t), saved); equal(vm.readInt(vm.register(4), mode/8), stack); equal(vm.readInt(image.base+0x3000n, 8), output);
    await assert.rejects(vm.run(100n), error => error.message === expected);
    equal(state(t), saved, 'repeated runs stop at the same import without executing its return');
    assert.throws(() => vm.bind(dll, typeof symbol === 'number' ? '#'+symbol : symbol));

    // Diagnostic IAT targets must not advertise implementation through GetProcAddress.
    const text = new TextEncoder(), source = image.base+0x3100n;
    vm.write(source, text.encode(dll+'\0'));
    const provider = call({...t, mode, image}, 'GetModuleHandleA', [source]);
    assert.notEqual(provider, 0n);
    let lookup = typeof symbol === 'number' ? BigInt(symbol) : source;
    if (typeof symbol !== 'number') { vm.write(source, text.encode(symbol+'\0')); lookup = source; }
    equal(call({...t, mode, image}, 'GetProcAddress', [provider, lookup]), 0n);
    equal(image.importTraps, 1);
    vm.dispose();
  }

  for (const dll of ['missing-client.dll', 'C:\\Other\\opengl32.dll']) {
    const t = machine(mode), f = input(mode, dll, 'MissingExport'), before = state(t);
    assert.throws(() => t.vm.load(f.bytes, {diagnosticImports: true}));
    equal(state(t), before); equal(t.vm.image, null); equal(core.wire_import_trap_count(t.handle), 0);
    t.vm.dispose();
  }
  const t = machine(mode), f = fixture(mode, {tls: false});
  assert.throws(() => t.vm.load(f.bytes, {diagnosticImports: 'true'}), TypeError);
  equal(t.vm.image, null); t.vm.dispose();
}
// The explicit diagnostic setting follows later DLL graphs; every original
// TLS/DllMain still runs, and an unimplemented import never completes a call.
for (const mode of [32,64]) for (const ordinal of [false,true]) {
 for (const diagnosticImports of [false,true]) {
  const t=machine(mode),{vm}=t,main=fixture(mode,{tls:false});main.bytes[main.offset(0x1000)]=0xc3;
  const dll=ordinal?'ws2_32.dll':'kernel32.dll',symbol=ordinal?'#17':'MissingFixtureExport';
  const late=moduleFixture(mode,{dll:true,dependency:dll,ordinal,tls:true});
  if(!ordinal)late.rtext(0x20e2,symbol+'\0');
  vm.supplyModule('late.dll',late.bytes);const image=vm.load(main.bytes,{diagnosticImports});
  equal((await vm.run(10n)).reason,Exit.Returned);
  const source=image.base+0x3100n;vm.write(source,new TextEncoder().encode('late.dll\0'));
  const value=call({...t,mode,image},'LoadLibraryA',[source]);
  if(!diagnosticImports){
   equal(value,0n);equal(vm.moduleBase('late.dll'),0n);equal(core.wire_import_trap_count(t.handle),0);
   equal(vm.readInt(image.teb+BigInt(mode===32?0x34:0x68)),127n);
  }else{
   const base=vm.moduleBase('late.dll');assert.notEqual(base,0n);
   const expected=`unimplemented diagnostic import: ${dll}!${symbol}`;
   await assert.rejects(vm.run(1000n),error=>error.message===expected);
   equal(vm.readInt(base+0x3020n),2n,'both original TLS callbacks ran');
   equal(vm.readInt(base+0x3040n,mode/8),base,'original DllMain began');
   equal(core.wire_import_trap_count(t.handle),1);
   const before=state(t),work=vm.work;
   await assert.rejects(vm.run(1000n),error=>error.message===expected);
   equal(state(t),before);equal(vm.work,work);assert.throws(()=>vm.bind(dll,symbol));
  }
  vm.dispose();
 }
 const t=machine(mode),{vm}=t,main=fixture(mode,{tls:false});main.bytes[main.offset(0x1000)]=0xc3;
 vm.supplyModule('late.dll',moduleFixture(mode,{dll:true,dependency:'missing-client.dll'}).bytes);
 const image=vm.load(main.bytes,{diagnosticImports:true});equal((await vm.run(10n)).reason,Exit.Returned);
 vm.write(image.base+0x3100n,new TextEncoder().encode('late.dll\0'));
 equal(call({...t,mode,image},'LoadLibraryA',[image.base+0x3100n]),0n);
 equal(vm.moduleBase('late.dll'),0n);equal(core.wire_import_trap_count(t.handle),0);
 vm.dispose();
}
console.log(`${checks} diagnostic import and strict admission checks passed`);
