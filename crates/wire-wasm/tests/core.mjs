// Executes the compiler itself as Wasm, then installs its output in the same
// engine and linear memory. This checks the live ABI and async source authority.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const build = spawnSync('cargo', ['build', '--quiet', '--offline', '--locked', '--lib', '--manifest-path', 'crates/wire-wasm/Cargo.toml', '--target', 'wasm32-unknown-unknown', '--target-dir', 'target/wasm-rewrite'], { cwd: root, encoding: 'utf8' });
assert.equal(build.status, 0, build.stderr);
const { instance } = await WebAssembly.instantiate(readFileSync(resolve(root, 'target/wasm-rewrite/wasm32-unknown-unknown/debug/wire_wasm.wasm')));
const core = instance.exports;
const driverPointer = core.wire_driver();
const driverBytes = new Uint8Array(core.memory.buffer, driverPointer, core.wire_driver_len()).slice();
const functions = new WebAssembly.Table({ element: 'anyfunc', initial: 1 });
const { instance: driver } = await WebAssembly.instantiate(driverBytes, { core, jit: { functions } });
const layout = Array.from(new Uint32Array(core.memory.buffer, core.wire_state_layout(), 8));
const field = { size: layout[0], regs: layout[1], flags: layout[2], ip: layout[3], work: layout[4], retired: layout[5], fs: layout[6], gs: layout[7] };
const state = handle => core.wire_state(handle);
const read = (handle, offset) => new DataView(core.memory.buffer).getBigUint64(state(handle) + offset, true);
function write(handle, address, bytes) {
  const pointer = core.wire_input(handle, bytes.length);
  assert.notEqual(pointer, 0);
  new Uint8Array(core.memory.buffer, pointer, bytes.length).set(bytes);
  assert.equal(core.wire_write(handle, address), 0);
}
function candidate(handle, imports = core) {
  const status = core.wire_compile(handle);
  const error = new TextDecoder().decode(new Uint8Array(core.memory.buffer, core.wire_error(handle), core.wire_error_len(handle)));
  assert.equal(status, 0, error);
  const ticket = core.wire_ticket(handle);
  assert.notEqual(ticket, 0n);
  assert.equal(core.wire_pin(handle, ticket), 1);
  const bytes = new Uint8Array(core.memory.buffer, core.wire_module(handle, ticket), core.wire_module_len(handle, ticket)).slice();
  return { ticket, pending: WebAssembly.instantiate(bytes, { core: imports, jit: driver.exports }).then(({instance}) => {
    assert.deepEqual(Object.keys(instance.exports), ['chunk']);
    const slot = functions.grow(1);
    functions.set(slot, instance.exports.chunk);
    if (!core.wire_publish(handle, ticket, slot)) functions.set(slot, null);
    return {run(pointer, budget, cache = core.wire_cache(handle), owner = handle) {
      const ready = core.wire_begin(handle, ticket);
      assert.notEqual(ready, 3);
      if (!ready) return 0;
      try { return driver.exports.run(pointer, budget, cache, owner); }
      finally { core.wire_end(handle); }
    }};
  }).finally(() => core.wire_release(handle, ticket)) };
}

const program = [0xb9, 100, 0, 0, 0, 0x31, 0xc0, 0x01, 0xc8, 0x83, 0xe9, 1, 0x75, 0xf9, 0xcc];
assert.equal(core.wire_create(16, 0n, 0n), 0);
for (const mode of [32, 64]) {
  const entry = mode === 32 ? 0x10000n : 0x100010000n;
  const handle = core.wire_create(mode, entry, 0n);
  const pointer = state(handle);
  assert.notEqual(handle, 0);
  assert.equal(core.wire_map(handle, entry, 4096n, 7), 0);
  write(handle, entry, program);
  const first = candidate(handle);
  const duplicate = candidate(handle);
  await first.pending;
  assert.equal(first.ticket, duplicate.ticket, 'same source reuses the immutable module');
  assert.equal(core.wire_ready(handle, first.ticket), 1);
  const installed = await duplicate.pending;
  assert.equal(core.wire_ready(handle, duplicate.ticket), 1);

  // Growing the core and creating another owner cannot move live state or
  // invalidate source that was not changed.
  const other = core.wire_create(mode, entry, 0n);
  assert.equal(core.wire_map(other, entry, 4096n, 7), 0);
  write(other, entry, [0x90, 0xcc]);
  const otherCode = candidate(other);
  await otherCode.pending;
  assert.equal(state(handle), pointer);
  assert.equal(core.wire_ready(handle, duplicate.ticket), 1);
  assert.equal(core.wire_ready(other, duplicate.ticket), 0, 'different owner');
  assert.equal(core.wire_map(handle, entry + 0x10000n, 4096n, 3), 0);
  write(handle, entry + 0x10000n, [1, 2, 3]);
  assert.equal(core.wire_ready(handle, duplicate.ticket), 1, 'unrelated data');

  assert.equal(installed.run(pointer, 303n), 2);
  assert.equal(read(handle, field.regs), 5050n);
  assert.equal(read(handle, field.regs + 8), 0n);
  assert.equal(read(handle, field.work), 302n);
  assert.equal(read(handle, field.retired), 302n);
  assert.equal(read(handle, field.ip), entry + 14n);
  assert.equal(core.wire_ready(handle, duplicate.ticket), 1, 'decoded boundary is a reusable entry');
  assert.equal(installed.run(pointer, 1n), 2);
  assert.equal(read(handle, field.work), 302n);
  new DataView(core.memory.buffer).setBigUint64(pointer + field.ip, entry + 1n, true);
  assert.equal(core.wire_ready(handle, duplicate.ticket), 0, 'instruction interior rejected');
  assert.equal(installed.run(pointer, 1n), 0);
  assert.equal(read(handle, field.work), 302n);

  new DataView(core.memory.buffer).setBigUint64(pointer + field.ip, entry, true);
  const pending = candidate(handle);
  write(handle, entry, program);
  await pending.pending;
  assert.equal(core.wire_ready(handle, pending.ticket), 0, 'source changed during installation');
  const replacement = candidate(handle);
  await replacement.pending;
  assert.equal(core.wire_ready(handle, pending.ticket), 0, 'new proof cannot revive old installation');
  assert.equal(core.wire_ready(handle, replacement.ticket), 1);
  core.wire_destroy(handle);
  const reused = core.wire_create(mode, entry, 0n);
  assert.equal(core.wire_map(reused, entry, 4096n, 7), 0);
  write(reused, entry, program);
  const reusedCode = candidate(reused);
  await reusedCode.pending;
  assert.equal(core.wire_ready(reused, replacement.ticket), 0, 'destroyed owner');
  assert.equal(core.wire_ready(reused, reusedCode.ticket), 1);
  core.wire_destroy(reused);
  core.wire_destroy(other);
}

// Public instruction entries must reject holes and full-width address aliases,
// both within a compact region and across distant source pages.
for (const mode of [32, 64]) for (const distant of [false, true]) {
  const entry = mode === 32 ? 0x10000n : 0x123400010000n;
  const handle = core.wire_create(mode, entry, 0n);
  assert.equal(core.wire_map(handle, entry, 4096n, 7), 0);
  let positions;
  if (distant) {
    const target = entry + 0x40000n;
    assert.equal(core.wire_map(handle, target, 4096n, 7), 0);
    write(handle, entry, [0xe9, 0xfb, 0xff, 3, 0]);
    write(handle, target, [0xb8, 7, 0, 0, 0, 0xcc]);
    positions = new Map([[entry, 1n], [target, 1n], [target + 5n, 0n]]);
  } else {
    write(handle, entry, [0xb8, 0x11, 0x22, 0x33, 0x44, 0x83, 0xc0, 1, 0xeb, 2, 0x90, 0x90, 0x83, 0xc0, 2, 0xcc]);
    positions = new Map([0n, 5n, 8n, 12n, 15n].map(offset => [entry + offset, offset === 15n ? 0n : 1n]));
  }
  const compiled = candidate(handle), installed = await compiled.pending;
  const addresses = [...positions.keys(), ...Array.from({length: 17}, (_, n) => entry + BigInt(n)), entry - 1n];
  if (mode === 64) addresses.push(...[...positions.keys()].map(pc => pc + 0x100000000n));
  for (const pc of new Set(addresses)) {
    set(handle, field.ip, pc);
    const before = read(handle, field.work), delta = positions.get(pc);
    assert.equal(installed.run(state(handle), 1n), delta === undefined ? 0 : delta === 0n ? 2 : 1);
    assert.equal(read(handle, field.work) - before, delta ?? 0n);
    if (delta === undefined) assert.equal(read(handle, field.ip), pc);
  }
  core.wire_destroy(handle);
}

function set(handle, offset, value) { new DataView(core.memory.buffer).setBigUint64(state(handle) + offset, BigInt.asUintN(64, value), true); }
function reg(handle, index, value) { set(handle, field.regs + index * 8, value); }
function map(handle, base, size = 4096n, permissions = 3) { assert.equal(core.wire_map(handle, base, size, permissions), 0); }
function load(handle, address, size = 4) {
  const value = BigInt.asUintN(64, core.wire_load(handle, address, size, 1));
  assert.equal(core.wire_fault_kind(handle), 0);
  return value;
}
async function memoryProgram(mode, bytes) {
  const entry = mode === 32 ? 0x10000n : 0x100010000n;
  const handle = core.wire_create(mode, entry, 0n);
  map(handle, entry, 4096n, 7);
  write(handle, entry, bytes);
  const calls = { resolve: 0, load: 0, store: 0 };
  let wideStores = 0;
  const imports = { ...core };
  for (const operation of Object.keys(calls)) imports[`wire_${operation}`] = (...args) => { calls[operation]++; return core[`wire_${operation}`](...args); };
  imports.wire_store128 = (...args) => { wideStores++; return core.wire_store128(...args); };
  async function install() {
    const compiled = candidate(handle, imports);
    const installed = await compiled.pending;
    assert.equal(core.wire_ready(handle, compiled.ticket), 1);
    return { ticket: compiled.ticket, run: budget => {
      assert.equal(core.wire_enter(handle, compiled.ticket), 1, 'installation authority before run');
      try { return installed.run(state(handle), budget, core.wire_cache(handle), handle); }
      finally { core.wire_release(handle, compiled.ticket); }
    } };
  }
  return { handle, entry, calls, get wideStores() { return wideStores; }, install, reset: () => set(handle, field.ip, entry), destroy: () => core.wire_destroy(handle) };
}

for (const mode of [32, 64]) {
  for (const bits of [8, 16, 32, 64]) {
    if (mode === 32 && bits === 64) continue;
    const prefix = bits === 16 ? [0x66] : bits === 64 ? [0x48] : [];
    const loop = [...prefix, bits === 8 ? 0x8a : 0x8b, 0x03,
      ...prefix, bits === 8 ? 0x80 : 0x83, 0xc0, 3,
      ...prefix, bits === 8 ? 0x88 : 0x89, 0x03, 0x83, 0xe9, 1];
    const vm = await memoryProgram(mode, [0xb9, 12, 0, 0, 0, ...loop, 0x75, 256 - loop.length - 2, 0xcc]);
    const { handle } = vm, data = 0x20000n;
    map(handle, data); write(handle, data, [9, 0, 0, 0, 0, 0, 0, 0]); reg(handle, 3, data);
    const code = await vm.install();
    assert.equal(code.run(1000n), 2);
    assert.equal(load(handle, data, bits / 8), 45n);
    assert.equal(read(handle, field.work), 61n);
    assert.deepEqual(vm.calls, { resolve: 1, load: 0, store: 0 }, `ordinary ${bits}-bit hit accesses`);
    vm.reset();
    assert.equal(code.run(1000n), 2);
    assert.equal(load(handle, data, bits / 8), 81n);
    assert.deepEqual(vm.calls, { resolve: 1, load: 0, store: 0 }, 'translation survives a new invocation');
    vm.destroy();
  }
  const vm = await memoryProgram(mode, [0xb9, 12, 0, 0, 0, 0x83, 0x03, 3, 0x83, 0xe9, 1, 0x75, 0xf8, 0xcc]);
  map(vm.handle, 0x20000n); write(vm.handle, 0x20000n, [9, 0, 0, 0]); reg(vm.handle, 3, 0x20000n);
  assert.equal((await vm.install()).run(1000n), 2);
  assert.equal(load(vm.handle, 0x20000n), 45n);
  assert.equal(read(vm.handle, field.work), 37n);
  assert.deepEqual(vm.calls, { resolve: 1, load: 0, store: 0 }, 'ordinary read-modify-write hits');
  vm.destroy();
}

for (const mode of [32, 64]) {
  // The arithmetic prefix must survive a fault in the following instruction;
  // its pending FLAGS must be exported without committing the failed result.
  for (const reason of ['unmapped', 'protection', 'guard']) {
    const vm = await memoryProgram(mode, [0x04, 1, 0x03, 0x03, 0xcc]);
    const { handle, entry } = vm, data = 0x20000n;
    if (reason !== 'unmapped') {
      map(handle, data); write(handle, data, [7, 0, 0, 0]);
      assert.equal(core.wire_protect(handle, data, 4096n, reason === 'guard' ? 0x104 : 1), 0);
    }
    reg(handle, 0, 0x7fn); reg(handle, 3, data);
    assert.equal((await vm.install()).run(100n), 3, reason);
    assert.equal(core.wire_fault_kind(handle), { unmapped: 2, protection: 3, guard: 4 }[reason]);
    assert.equal(core.wire_fault_address(handle), data);
    assert.equal(core.wire_fault_access(handle), 1);
    assert.equal(read(handle, field.regs), 0x80n);
    assert.equal(read(handle, field.flags), 0xa92n);
    assert.equal(read(handle, field.ip), entry + 2n);
    assert.equal(read(handle, field.work), 1n);
    assert.equal(read(handle, field.retired), 1n);
    if (reason === 'guard') {
      assert.equal((await vm.install()).run(100n), 2);
      assert.equal(read(handle, field.regs), 0x87n);
      assert.equal(read(handle, field.work), 2n);
      assert.equal(read(handle, field.flags), 0x206n);
    }
    vm.destroy();
  }

  const cmov = await memoryProgram(mode, [0x39, 0xc0, 0x0f, 0x45, 0x03, 0xcc]);
  reg(cmov.handle, 0, 123n); reg(cmov.handle, 3, 0x20000n);
  assert.equal((await cmov.install()).run(100n), 3, 'untaken CMOV still reads memory');
  assert.equal(read(cmov.handle, field.regs), 123n);
  assert.equal(read(cmov.handle, field.work), 1n);
  assert.equal(read(cmov.handle, field.ip), cmov.entry + 2n);
  cmov.destroy();

  for (const operation of ['rmw', 'store']) {
    const bytes = operation === 'rmw' ? [0x83, 0x03, 1] : [0x89, 0x03];
    const vm = await memoryProgram(mode, [0x04, 1, ...bytes, 0xcc]);
    const { handle, entry } = vm, data = 0x20ffen;
    map(handle, 0x20000n, 8192n); write(handle, data, [1, 2, 3, 4]);
    assert.equal(core.wire_protect(handle, 0x21000n, 4096n, 2), 0);
    reg(handle, 0, 0x7fn); reg(handle, 3, data);
    assert.equal((await vm.install()).run(100n), 3);
    assert.equal(core.wire_fault_kind(handle), 3);
    assert.equal(core.wire_fault_access(handle), operation === 'rmw' ? 3 : 2);
    assert.equal(read(handle, field.regs), 0x80n);
    assert.equal(read(handle, field.flags), 0xa92n);
    assert.equal(read(handle, field.work), 1n);
    assert.equal(read(handle, field.ip), entry + 2n);
    assert.equal(load(handle, data), 0x04030201n, 'cross-page fault cannot leave a partial store');
    assert.equal(vm.calls.resolve, 0, 'cross-page accesses enter the concrete helper directly');
    vm.destroy();
  }

  const cross = await memoryProgram(mode, [0x83, 0x03, 1, 0xcc]);
  map(cross.handle, 0x20000n, 8192n); write(cross.handle, 0x20ffen, [1, 2, 3, 4]);
  assert.equal(core.wire_protect(cross.handle, 0x21000n, 4096n, 0x104), 0);
  reg(cross.handle, 3, 0x20ffen);
  assert.equal((await cross.install()).run(100n), 3);
  assert.equal(core.wire_fault_kind(cross.handle), 4);
  assert.equal(read(cross.handle, field.work), 0n);
  assert.equal(cross.calls.store, 0);
  assert.equal((await cross.install()).run(100n), 2);
  assert.equal(load(cross.handle, 0x20ffen), 0x04030202n);
  assert.equal(read(cross.handle, field.work), 1n);
  assert.deepEqual(cross.calls, { resolve: 0, load: 2, store: 1 });
  cross.destroy();

  const remap = await memoryProgram(mode, [0x8b, 0x03, 0xcc]);
  map(remap.handle, 0x20000n); write(remap.handle, 0x20000n, [1, 0, 0, 0]); reg(remap.handle, 3, 0x20000n);
  const remappedCode = await remap.install();
  assert.equal(remappedCode.run(100n), 2);
  assert.equal(read(remap.handle, field.regs), 1n);
  remap.reset();
  assert.equal(core.wire_unmap(remap.handle, 0x20000n, 4096n), 0);
  map(remap.handle, 0x20000n); write(remap.handle, 0x20000n, [2, 0, 0, 0]);
  assert.equal(remappedCode.run(100n), 2);
  assert.equal(read(remap.handle, field.regs), 2n, 'same-address remap cannot reuse an old pointer');
  assert.equal(remap.calls.resolve, 2);
  remap.destroy();

  for (const saturated of [false, true]) {
    const vm = await memoryProgram(mode, [0x89, 0x03, 0xcc]);
    map(vm.handle, 0x20000n); reg(vm.handle, 0, 7n); reg(vm.handle, 3, 0x20000n);
    const code = await vm.install();
    if (saturated) new DataView(core.memory.buffer).setBigUint64(core.wire_cache(vm.handle), 0xffffffffffffffffn, true);
    assert.equal(code.run(100n), 2);
    vm.reset(); reg(vm.handle, 0, 8n);
    assert.equal(core.wire_protect(vm.handle, 0x20000n, 4096n, 2), 0);
    assert.equal(code.run(100n), 3);
    assert.equal(core.wire_fault_kind(vm.handle), 3);
    assert.equal(read(vm.handle, field.work), 1n);
    assert.equal(load(vm.handle, 0x20000n), 7n, 'permission changes revoke cached writes, including a saturated epoch');
    assert.equal(vm.calls.resolve, 2);
    vm.destroy();
  }

  const smc = await memoryProgram(mode, [0xc6, 0x03, 0xcc, 0xb8, 7, 0, 0, 0, 0xcc]);
  reg(smc.handle, 3, smc.entry + 3n);
  assert.equal((await smc.install()).run(100n), 4, 'source write exits after the completed store');
  assert.equal(read(smc.handle, field.regs), 0n, 'stale following instruction cannot execute');
  assert.equal(read(smc.handle, field.ip), smc.entry + 3n);
  assert.equal(read(smc.handle, field.work), 1n);
  assert.equal((await smc.install()).run(100n), 2);
  assert.equal(read(smc.handle, field.work), 1n);
  smc.destroy();

  const observed = await memoryProgram(mode, [0x89, 0x03, 0xcc]);
  map(observed.handle, 0x20000n, 4096n, 7); reg(observed.handle, 3, 0x20000n); reg(observed.handle, 0, 0xcccccc90n);
  assert.equal((await observed.install()).run(100n), 2);
  assert.deepEqual(observed.calls, { resolve: 1, load: 0, store: 0 });
  set(observed.handle, field.ip, 0x20000n);
  await observed.install();
  observed.reset(); reg(observed.handle, 0, 0n);
  assert.equal((await observed.install()).run(100n), 2, 'source observation forces a checked store and the resident driver continues the valid caller');
  assert.equal(load(observed.handle, 0x20000n), 0n);
  assert.deepEqual(observed.calls, { resolve: 2, load: 0, store: 1 });
  observed.destroy();
}

const aliases = await memoryProgram(64, [0x8b, 0x03, 0x8b, 0x16, 0x8b, 0x0b, 0x01, 0xd0, 0x01, 0xd1, 0xcc]);
const low = 0x20000n, high = low + (1n << 36n);
map(aliases.handle, low); map(aliases.handle, high);
write(aliases.handle, low, [5, 0, 0, 0]); write(aliases.handle, high, [9, 0, 0, 0]);
reg(aliases.handle, 3, low); reg(aliases.handle, 6, high);
assert.equal((await aliases.install()).run(100n), 2);
assert.equal(read(aliases.handle, field.regs), 14n);
assert.equal(read(aliases.handle, field.regs + 8), 14n);
assert.equal(aliases.calls.resolve, 3, 'colliding translations compare complete 64-bit page tags');
aliases.destroy();

const address16 = await memoryProgram(32, [0x67, 0x8b, 0x00, 0xcc]);
map(address16.handle, 0x1000n); write(address16.handle, 0x1000n, [7, 0, 0, 0]);
reg(address16.handle, 3, 0xffffn); reg(address16.handle, 6, 0x1001n);
assert.equal((await address16.install()).run(100n), 2);
assert.equal(read(address16.handle, field.regs), 7n);
address16.destroy();

const gs = await memoryProgram(64, [0x65, 0x67, 0x8b, 0x04, 0x8b, 0xcc]);
map(gs.handle, high); write(gs.handle, high, [11, 0, 0, 0]);
reg(gs.handle, 3, 0xfffffffcn); reg(gs.handle, 1, 1n); set(gs.handle, field.gs, high);
assert.equal((await gs.install()).run(100n), 2);
assert.equal(read(gs.handle, field.regs), 11n, 'address override wraps before adding GS base');
gs.destroy();

const fs = await memoryProgram(32, [0x64, 0x67, 0x8b, 0x00, 0xcc]);
map(fs.handle, low); write(fs.handle, low + 1n, [13, 0, 0, 0]);
reg(fs.handle, 3, 0xffffn); reg(fs.handle, 6, 2n); set(fs.handle, field.fs, low);
assert.equal((await fs.install()).run(100n), 2);
assert.equal(read(fs.handle, field.regs), 13n);
fs.destroy();

const lea = await memoryProgram(64, [0x64, 0x67, 0x8d, 0x04, 0x8b, 0xcc]);
reg(lea.handle, 3, 0xfffffffcn); reg(lea.handle, 1, 1n); set(lea.handle, field.fs, high);
assert.equal((await lea.install()).run(100n), 2);
assert.equal(read(lea.handle, field.regs), 0n, 'LEA ignores segment base and does not access memory');
assert.deepEqual(lea.calls, { resolve: 0, load: 0, store: 0 });
lea.destroy();

const relative = await memoryProgram(64, [0x8b, 0x05, 10, 0, 0, 0, 0xcc, ...Array(9).fill(0xcc), 17, 0, 0, 0]);
assert.equal((await relative.install()).run(100n), 2);
assert.equal(read(relative.handle, field.regs), 17n, 'RIP-relative address keeps the high PC bits');
relative.destroy();

const negative = await memoryProgram(64, [0x8b, 0x04, 0x25, 0, 0, 0, 0x80, 0xcc]);
map(negative.handle, 0x80000000n); write(negative.handle, 0x80000000n, [23, 0, 0, 0]);
assert.equal((await negative.install()).run(100n), 3);
assert.equal(core.wire_fault_kind(negative.handle), 1, 'a sign-extended displacement cannot alias its low 32 bits');
negative.destroy();

for (const mode of [32, 64]) {
  const width = mode / 8, originalSP = 0x30800n;
  function pointer(value, size = width) { return Array.from({ length: size }, (_, index) => Number((value >> BigInt(index * 8)) & 255n)); }
  async function stackProgram(bytes, sp = originalSP) {
    const vm = await memoryProgram(mode, bytes);
    map(vm.handle, 0x30000n, 8192n);
    reg(vm.handle, 4, sp);
    return vm;
  }
  const program = Array(64).fill(0xcc);
  program.splice(0, 13, 0xb8, 1, 0, 0, 0, 0xe8, 6, 0, 0, 0, 0x83, 0xc0, 3);
  program.splice(16, 13, 0x53, 0xbb, 2, 0, 0, 0, 0xe8, 5, 0, 0, 0, 0x5b, 0xc3);
  program.splice(32, 3, 0x01, 0xd8, 0xc3);
  const calls = await stackProgram(program);
  reg(calls.handle, 3, 0x12345678n);
  const body = await calls.install();
  assert.equal(body.run(100n), 2, 'nested calls and returns stay inside one generated invocation');
  assert.equal(read(calls.handle, field.regs), 6n);
  assert.equal(read(calls.handle, field.regs + 3 * 8), 0x12345678n);
  assert.equal(read(calls.handle, field.regs + 4 * 8), originalSP);
  assert.equal(read(calls.handle, field.work), 10n);
  assert.deepEqual(calls.calls, { resolve: 1, load: 0, store: 0 });
  set(calls.handle, field.ip, calls.entry + 10n);
  assert.equal(body.run(100n), 2, 'same module accepts its return continuation');
  assert.equal(read(calls.handle, field.regs), 9n);
  calls.reset(); set(calls.handle, field.work, 0n); set(calls.handle, field.retired, 0n);
  assert.equal(body.run(7n), 1);
  assert.equal(read(calls.handle, field.ip), calls.entry + 27n);
  assert.equal(read(calls.handle, field.regs + 4 * 8), originalSP - BigInt(2 * width));
  assert.equal(body.run(100n), 2, 'budget resume reuses the installed continuation');
  assert.equal(read(calls.handle, field.regs), 6n);
  assert.equal(read(calls.handle, field.regs + 4 * 8), originalSP);
  assert.equal(read(calls.handle, field.work), 10n);
  assert.deepEqual(calls.calls, { resolve: 1, load: 0, store: 0 });
  calls.destroy();

  const changedProgram = Array(40).fill(0xcc);
  changedProgram.splice(0, 5, 0xe8, 27, 0, 0, 0);
  changedProgram[32] = 0xc3;
  const changedCall = await stackProgram(changedProgram);
  reg(changedCall.handle, 4, changedCall.entry + 16n + BigInt(width));
  const changedCode = await changedCall.install();
  assert.equal(changedCode.run(100n), 4);
  assert.equal(read(changedCall.handle, field.ip), changedCall.entry + 32n, 'a source-changing CALL exits at its target');
  assert.equal(read(changedCall.handle, field.work), 1n);
  assert.equal(read(changedCall.handle, field.regs + 4 * 8), changedCall.entry + 16n);
  assert.equal(core.wire_ready(changedCall.handle, changedCode.ticket), 0);
  assert.equal((await changedCall.install()).run(100n), 0);
  assert.equal(read(changedCall.handle, field.ip), changedCall.entry + 5n);
  assert.equal(read(changedCall.handle, field.work), 2n);
  changedCall.destroy();

  for (const memory of [false, true]) {
    const program = Array(24).fill(0xcc);
    program.splice(0, 9, 0x39, 0xc0, 0x75, 12, 0xff, memory ? 0x13 : 0xd2, 0x83, 0xc0, 3);
    program.splice(16, 4, 0x83, 0xc0, 2, 0xc3);
    const vm = await stackProgram(program);
    reg(vm.handle, 0, 1n);
    if (memory) {
      map(vm.handle, 0x20000n); write(vm.handle, 0x20000n, pointer(vm.entry + 16n)); reg(vm.handle, 3, 0x20000n);
    } else reg(vm.handle, 2, vm.entry + 16n);
    assert.equal((await vm.install()).run(100n), 2, 'indirect call resolves inside the generated CFG');
    assert.equal(read(vm.handle, field.regs), 6n);
    assert.equal(read(vm.handle, field.work), 6n);
    assert.equal(read(vm.handle, field.regs + 4 * 8), originalSP);
    assert.deepEqual(vm.calls, { resolve: memory ? 2 : 1, load: 0, store: 0 });
    vm.destroy();
  }

  const jumpProgram = Array(24).fill(0xcc);
  jumpProgram.splice(0, 6, 0x39, 0xc0, 0x75, 12, 0xff, 0xe2);
  jumpProgram.splice(16, 5, 0xb8, 9, 0, 0, 0);
  const jump = await stackProgram(jumpProgram);
  reg(jump.handle, 2, jump.entry + 16n);
  assert.equal((await jump.install()).run(100n), 2);
  assert.equal(read(jump.handle, field.regs), 9n);
  assert.equal(read(jump.handle, field.work), 4n);
  assert.deepEqual(jump.calls, { resolve: 0, load: 0, store: 0 });
  jump.destroy();

  const outside = await stackProgram([0xff, 0xd2, 0xcc]);
  reg(outside.handle, 2, 0x40000n);
  assert.equal((await outside.install()).run(100n), 0);
  assert.equal(read(outside.handle, field.ip), 0x40000n);
  assert.equal(read(outside.handle, field.work), 1n);
  assert.equal(read(outside.handle, field.regs + 4 * 8), originalSP - BigInt(width));
  assert.equal(load(outside.handle, originalSP - BigInt(width), width), outside.entry + 2n);
  outside.destroy();

  const ret = await stackProgram([0xc2, 16, 0]);
  write(ret.handle, originalSP, pointer(0x40000n));
  assert.equal((await ret.install()).run(100n), 0);
  assert.equal(read(ret.handle, field.ip), 0x40000n);
  assert.equal(read(ret.handle, field.regs + 4 * 8), originalSP + BigInt(width + 16));
  assert.equal(read(ret.handle, field.work), 1n);
  ret.destroy();

  const push = await stackProgram([0x54, 0x58, 0x6a, 0xff, 0x5b, 0xcc]);
  assert.equal((await push.install()).run(100n), 2);
  assert.equal(read(push.handle, field.regs), originalSP, 'PUSH ESP/RSP captures its original value');
  assert.equal(read(push.handle, field.regs + 3 * 8), (1n << BigInt(mode)) - 1n, 'PUSH sign-extends its immediate');
  assert.equal(read(push.handle, field.regs + 4 * 8), originalSP);
  push.destroy();

  const popSP = await stackProgram([0x5c, 0xcc]);
  write(popSP.handle, originalSP, pointer(0x40000n));
  assert.equal((await popSP.install()).run(100n), 2);
  assert.equal(read(popSP.handle, field.regs + 4 * 8), 0x40000n, 'POP ESP/RSP replaces the incremented pointer');
  popSP.destroy();

  const popMemory = await stackProgram([0x8f, 0x04, 0x24, 0xcc]);
  write(popMemory.handle, originalSP, pointer(0x1234n));
  assert.equal((await popMemory.install()).run(100n), 2);
  assert.equal(read(popMemory.handle, field.regs + 4 * 8), originalSP + BigInt(width));
  assert.equal(load(popMemory.handle, originalSP + BigInt(width), width), 0x1234n, 'POP [ESP/RSP] computes its destination after increment');
  popMemory.destroy();

  const popFault = await stackProgram([0x8f, 0x04, 0x24, 0xcc], 0x31000n - BigInt(width));
  write(popFault.handle, 0x31000n - BigInt(width), pointer(0x1234n));
  assert.equal(core.wire_protect(popFault.handle, 0x31000n, 4096n, 2), 0);
  assert.equal((await popFault.install()).run(100n), 3);
  assert.equal(core.wire_fault_access(popFault.handle), 2);
  assert.equal(read(popFault.handle, field.regs + 4 * 8), 0x31000n - BigInt(width));
  assert.equal(read(popFault.handle, field.work), 0n);
  assert.equal(load(popFault.handle, 0x31000n, width), 0n);
  popFault.destroy();

  for (const fault of [false, true]) {
    const leave = await stackProgram([0xc9, 0xcc]);
    const bp = fault ? 0x40000n : 0x30700n;
    reg(leave.handle, 5, bp);
    if (!fault) write(leave.handle, bp, pointer(0x1234n));
    assert.equal((await leave.install()).run(100n), fault ? 3 : 2);
    assert.equal(read(leave.handle, field.regs + 4 * 8), fault ? originalSP : bp + BigInt(width));
    assert.equal(read(leave.handle, field.regs + 5 * 8), fault ? bp : 0x1234n);
    assert.equal(read(leave.handle, field.work), fault ? 0n : 1n);
    leave.destroy();
  }

  const word = await stackProgram([0x66, 0x68, 0x34, 0x12, 0x66, 0x58, 0xcc]);
  reg(word.handle, 0, 0xfedcba9876543210n);
  assert.equal((await word.install()).run(100n), 2);
  assert.equal(read(word.handle, field.regs), 0xfedcba9876541234n);
  assert.equal(read(word.handle, field.regs + 4 * 8), originalSP);
  word.destroy();

  const wordSP = await memoryProgram(mode, [0x66, 0x5c, 0xcc]);
  map(wordSP.handle, 0x3f000n); reg(wordSP.handle, 4, 0x3fffen); write(wordSP.handle, 0x3fffen, [0x34, 0x12]);
  assert.equal((await wordSP.install()).run(100n), 2);
  assert.equal(read(wordSP.handle, field.regs + 4 * 8), 0x41234n, 'POP SP preserves upper bits from the incremented stack pointer');
  wordSP.destroy();

  const wordLeave = await stackProgram([0x66, 0xc9, 0xcc]);
  reg(wordLeave.handle, 5, 0x30700n); write(wordLeave.handle, 0x30700n, [0x34, 0x12]);
  assert.equal((await wordLeave.install()).run(100n), 2);
  assert.equal(read(wordLeave.handle, field.regs + 5 * 8), 0x31234n);
  assert.equal(read(wordLeave.handle, field.regs + 4 * 8), 0x30702n);
  wordLeave.destroy();
}

for (const mode of [32, 64]) {
  for (const [operation, bytes, count, result, flags] of [
    ['adc', [0x11, 0x03], 0n, 0x80000080n, 0x292n],
    ['sbb', [0x19, 0x03], 0n, 0x7fffff80n, 0xa12n],
    ['shl', [0xd3, 0x23], 1n, 0n, 0xa57n],
    ['shr', [0xd3, 0x2b], 1n, 0x40000000n, 0xa16n],
    ['sar', [0xd3, 0x3b], 1n, 0xc0000000n, 0x296n],
    ['zero shift', [0xd3, 0x23], 32n, 0x80000000n, 0xa13n],
  ]) {
    const vm = await memoryProgram(mode, [0x04, 1, ...bytes, 0xcc]);
    map(vm.handle, 0x20000n, 8192n);
    write(vm.handle, 0x20ffen, [0, 0, 0, 0x80]);
    reg(vm.handle, 0, 0xffn); reg(vm.handle, 1, count); reg(vm.handle, 3, 0x20ffen);
    set(vm.handle, field.flags, 0xa02n);
    // The first ADD produces carry and auxiliary carry before the RMW faults.
    assert.equal(core.wire_protect(vm.handle, 0x21000n, 4096n, 2), 0);
    const code = await vm.install();
    assert.equal(code.run(100n), 3, operation);
    assert.equal(read(vm.handle, field.regs), 0n);
    assert.equal(read(vm.handle, field.flags), 0x257n);
    assert.equal(read(vm.handle, field.ip), vm.entry + 2n);
    assert.equal(read(vm.handle, field.work), 1n);
    assert.equal(load(vm.handle, 0x20ffen), 0x80000000n, 'faulting RMW does not partially write');
    assert.equal(core.wire_protect(vm.handle, 0x21000n, 4096n, 4), 0);
    // Resume at the memory instruction with a controlled carry/overflow input.
    set(vm.handle, field.flags, 0xa13n); reg(vm.handle, 0, 0x7fn);
    assert.equal(code.run(100n), 2, operation);
    assert.equal(load(vm.handle, 0x20ffen), result, operation);
    assert.equal(read(vm.handle, field.flags), flags, operation);
    vm.destroy();
  }
}

for (const mode of [32, 64]) {
  for (const [name, bytes, initial, result, flags, accumulator, source] of [
    ['ROL', [0xd3, 0x03], 0x80000000n, 1n, 0xa57n, 0n, 1n],
    ['ROR', [0xd3, 0x0b], 1n, 0x80000000n, 0xa57n, 0n, 1n],
    ['RCL', [0xd3, 0x13], 0n, 1n, 0x256n, 0n, 1n],
    ['RCR', [0xd3, 0x1b], 0n, 0x80000000n, 0xa56n, 0n, 1n],
    ['SHLD', [0x0f, 0xa5, 0x13], 0x80000000n, 0n, 0xa57n, 0n, 1n],
    ['SHRD', [0x0f, 0xad, 0x13], 1n, 0x80000000n, 0xa97n, 0n, 1n],
    ['XADD', [0xf0, 0x0f, 0xc1, 0x13], 0xffffffffn, 0n, 0x257n, 0n, 0xffffffffn],
    ['CMPXCHG match', [0xf0, 0x0f, 0xb1, 0x13], 0n, 1n, 0x246n, 0n, 1n],
    ['CMPXCHG mismatch', [0xf0, 0x0f, 0xb1, 0x13], 1n, 1n, 0x297n, 1n, 1n],
  ]) {
    const vm = await memoryProgram(mode, [0x04, 1, ...bytes, 0xcc]);
    map(vm.handle, 0x20000n, 8192n);
    const encoded = Array.from({ length: 4 }, (_, n) => Number((initial >> BigInt(n * 8)) & 255n));
    write(vm.handle, 0x20000n, encoded);
    reg(vm.handle, 0, 255n); reg(vm.handle, 1, 1n); reg(vm.handle, 2, 1n); reg(vm.handle, 3, 0x20000n);
    const code = await vm.install();
    assert.equal(code.run(100n), 2, name);
    assert.equal(load(vm.handle, 0x20000n), result, name);
    assert.equal(read(vm.handle, field.flags), flags, name);
    assert.equal(read(vm.handle, field.regs), accumulator, name);
    assert.equal(read(vm.handle, field.regs + 16), source, name);
    assert.equal(read(vm.handle, field.work), 2n);
    assert.deepEqual(vm.calls, { resolve: 1, load: 0, store: 0 }, `${name} ordinary RMW stays inline`);
    for (const fault of ['readonly', 'guard', 'cross-page']) {
      const target = fault === 'cross-page' ? 0x20ffen : 0x20000n;
      write(vm.handle, target, encoded);
      const protectedPage = fault === 'cross-page' ? 0x21000n : 0x20000n;
      assert.equal(core.wire_protect(vm.handle, protectedPage, 4096n, fault === 'guard' ? 0x104 : 2), 0);
      vm.reset(); reg(vm.handle, 0, 255n); reg(vm.handle, 1, 1n); reg(vm.handle, 2, 1n); reg(vm.handle, 3, target);
      const before = read(vm.handle, field.work);
      assert.equal(code.run(100n), 3, `${name} ${fault}`);
      assert.equal(core.wire_fault_access(vm.handle), 3, 'RMW requires write access even when the compare fails');
      assert.equal(read(vm.handle, field.flags), 0x257n, 'failed RMW keeps the preceding arithmetic FLAGS');
      assert.equal(read(vm.handle, field.regs), 0n);
      assert.equal(read(vm.handle, field.regs + 16), 1n);
      assert.equal(read(vm.handle, field.ip), vm.entry + 2n);
      assert.equal(read(vm.handle, field.work), before + 1n);
      assert.equal(core.wire_protect(vm.handle, protectedPage, 4096n, 4), 0);
      assert.equal(load(vm.handle, target), initial, 'failed RMW never leaves a partial write');
    }
    vm.destroy();
  }
  const aliased = await memoryProgram(mode, [0x0f, 0xc1, 0x1b, 0xcc]);
  map(aliased.handle, 0x20000n); write(aliased.handle, 0x20000n, [7, 0, 0, 0]); reg(aliased.handle, 3, 0x20000n);
  assert.equal((await aliased.install()).run(100n), 2);
  assert.equal(load(aliased.handle, 0x20000n), 0x20007n);
  assert.equal(read(aliased.handle, field.regs + 24), 7n, 'XADD stores through the original address before replacing its base register');
  aliased.destroy();
}

for (const mode of [32, 64]) {
  const locked = await memoryProgram(mode, [0xb9, 12, 0, 0, 0, 0xf0, 0x83, 0x03, 1, 0xf0, 0x0f, 0xba, 0x2b, 4, 0xe2, 0xf5, 0xcc]);
  map(locked.handle, 0x20000n); write(locked.handle, 0x20000n, [0, 0, 0, 0]); reg(locked.handle, 3, 0x20000n);
  assert.equal((await locked.install()).run(100n), 2);
  assert.equal(load(locked.handle, 0x20000n), 28n);
  assert.equal(read(locked.handle, field.work), 37n);
  assert.equal(read(locked.handle, field.regs + 8), 0n);
  assert.deepEqual(locked.calls, { resolve: 1, load: 0, store: 0 }, 'locked scalar/bit RMW and LOOP execute within Wasm');
  locked.destroy();
}

// Wide compare-exchange updates only ZF. A pending arithmetic producer makes
// accidental whole-FLAGS replacement visible; each operand may also be the EA.
const wideForms = [[32, 8], [64, 8], [64, 16]];
const encodedInteger = (value, size) => Array.from({ length: size }, (_, index) => Number((value >> BigInt(index * 8)) & 255n));
function halves(value, size) {
  const bits = BigInt(size * 4), mask = (1n << bits) - 1n;
  return [value & mask, (value >> bits) & mask];
}
function joined(low, high, size) {
  const bits = BigInt(size * 4), mask = (1n << bits) - 1n;
  return (low & mask) | ((high & mask) << bits);
}
function readWide(handle, address, size) {
  const low = load(handle, address, 8);
  return size === 8 ? low : low | (load(handle, address + 8n, 8) << 64n);
}
for (const [mode, size] of wideForms) for (const lock of [false, true]) for (const baseRegister of [0, 1, 2, 3, 6]) {
  const instruction = [...(lock ? [0xf0] : []), ...(size === 16 ? [0x48] : []), 0x0f, 0xc7, 0x08 | baseRegister];
  const vm = await memoryProgram(mode, [0x83, 0xfd, 1, ...instruction, 0xcc]);
  const data = mode === 64 ? 0x100020000n : 0x20000n;
  map(vm.handle, data);
  const code = await vm.install();
  for (const difference of [0n, 1n, 1n << BigInt(size * 4)]) {
    const registers = [0x1234567887654321n, 0xabcdef0112345678n, 0xfedcba9843217654n, 0x9876543212345678n, 0n, 0x80000000n, data];
    if (mode === 32) for (let index = 0; index < registers.length; index++) registers[index] = BigInt.asUintN(32, registers[index]);
    registers[baseRegister] = data;
    const expected = joined(registers[0], registers[2], size), old = expected ^ difference;
    const replacement = joined(registers[3], registers[1], size);
    write(vm.handle, data, encodedInteger(old, size));
    vm.reset(); set(vm.handle, field.work, 0n); set(vm.handle, field.retired, 0n); set(vm.handle, field.flags, 0x602n);
    registers.forEach((value, index) => reg(vm.handle, index, value));
    assert.equal(code.run(1n), 1);
    assert.equal(read(vm.handle, field.flags), 0xe16n);
    assert.equal(code.run(1n), 1);
    const equal = difference === 0n;
    assert.equal(readWide(vm.handle, data, size), equal ? replacement : old);
    const returned = halves(old, size);
    for (let index = 0; index < registers.length; index++) assert.equal(read(vm.handle, field.regs + index * 8), !equal && index === 0 ? returned[0] : !equal && index === 2 ? returned[1] : registers[index], `wide exchange register ${index}`);
    assert.equal(read(vm.handle, field.flags), equal ? 0xe56n : 0xe16n);
    assert.equal(read(vm.handle, field.ip), vm.entry + 3n + BigInt(instruction.length));
    assert.equal(read(vm.handle, field.work), 2n); assert.equal(read(vm.handle, field.retired), 2n);
    assert.equal(vm.wideStores, 0);
    assert.deepEqual(vm.calls, { resolve: 1, load: 0, store: 0 }, 'wide ordinary RMW uses the inline translation');
  }
  vm.destroy();
}

for (const [mode, size] of wideForms) for (const equal of [false, true]) {
  const instruction = [0xf0, ...(size === 16 ? [0x48] : []), 0x0f, 0xc7, 0x0e];
  for (const reason of size === 8 ? ['readonly', 'guard', 'cross-page'] : ['readonly', 'guard']) {
    const vm = await memoryProgram(mode, [0x83, 0xfd, 1, ...instruction, 0xcc]);
    const data = reason === 'cross-page' ? 0x20ffdn : 0x20000n, page = reason === 'cross-page' ? 0x21000n : 0x20000n;
    map(vm.handle, 0x20000n, 8192n);
    const old = 0xfedcba9876543210n | (size === 16 ? 0xabcdef0187654321n << 64n : 0n);
    write(vm.handle, data, encodedInteger(old, size));
    const expected = halves(equal ? old : old ^ 1n, size);
    reg(vm.handle, 0, expected[0]); reg(vm.handle, 2, expected[1]); reg(vm.handle, 1, 22n); reg(vm.handle, 3, 11n); reg(vm.handle, 5, 0x80000000n); reg(vm.handle, 6, data);
    const code = await vm.install();
    assert.equal(core.wire_protect(vm.handle, page, 4096n, reason === 'guard' ? 0x104 : 2), 0);
    assert.equal(code.run(10n), 3, `${size}-byte ${reason}`);
    assert.equal(core.wire_fault_access(vm.handle), 3, 'comparison failure still requires write permission');
    assert.equal(read(vm.handle, field.ip), vm.entry + 3n);
    assert.equal(read(vm.handle, field.work), 1n); assert.equal(read(vm.handle, field.retired), 1n);
    assert.equal(read(vm.handle, field.flags), 0xa16n);
    assert.equal(read(vm.handle, field.regs), expected[0]); assert.equal(read(vm.handle, field.regs + 16), expected[1]);
    assert.equal(core.wire_protect(vm.handle, page, 4096n, 4), 0);
    assert.equal(readWide(vm.handle, data, size), old, 'a fault preserves both memory halves');
    assert.equal(code.run(1n), 1);
    assert.equal(read(vm.handle, field.retired), 2n);
    assert.equal(read(vm.handle, field.flags), equal ? 0xa56n : 0xa16n);
    assert.equal(readWide(vm.handle, data, size), equal ? joined(11n, 22n, size) : old);
    vm.destroy();
  }
}

const unalignedWide = await memoryProgram(64, [0x83, 0xfd, 1, 0x48, 0x0f, 0xc7, 0x0e, 0xcc]);
map(unalignedWide.handle, 0x20000n, 8192n);
assert.equal(core.wire_protect(unalignedWide.handle, 0x21000n, 4096n, 0x104), 0);
reg(unalignedWide.handle, 5, 0x80000000n); reg(unalignedWide.handle, 6, 0x20ff8n);
const unalignedCode = await unalignedWide.install();
assert.equal(unalignedCode.run(10n), 5, 'misaligned CMPXCHG16B raises #GP before touching either page');
assert.equal(core.wire_exception(unalignedWide.handle), 14);
assert.equal(read(unalignedWide.handle, field.ip), unalignedWide.entry + 3n);
assert.equal(read(unalignedWide.handle, field.work), 1n); assert.equal(read(unalignedWide.handle, field.retired), 1n);
assert.equal(read(unalignedWide.handle, field.flags), 0xa16n);
assert.deepEqual(unalignedWide.calls, { resolve: 0, load: 0, store: 0 });
assert.equal(unalignedCode.run(0n), 1); assert.equal(core.wire_exception(unalignedWide.handle), 14);
reg(unalignedWide.handle, 6, 0x21000n);
assert.equal(unalignedCode.run(1n), 3, 'the alignment exception did not consume the second-page guard');
assert.equal(core.wire_fault_kind(unalignedWide.handle), 4); assert.equal(core.wire_exception(unalignedWide.handle), 0);
assert.equal(unalignedCode.run(1n), 1);
assert.equal(read(unalignedWide.handle, field.retired), 2n);
unalignedWide.destroy();

for (const [mode, size] of wideForms) for (const equal of [false, true]) {
  const instruction = [0xf0, ...(size === 16 ? [0x48] : []), 0x0f, 0xc7, 0x0e];
  const jumpEnd = 3 + instruction.length + 3 + 5;
  const vm = await memoryProgram(mode, [0x83, 0xfd, 1, ...instruction, 0x0f, 0x94, 0xc1, 0xe9, ...encodedInteger(BigInt(0x100 - jumpEnd), 4), 0xcc]);
  const data = vm.entry + 0x100n;
  const payload = value => [0xbf, value, 0, 0, 0, 0xcc, ...Array(size - 6).fill(0xcc)];
  const oldBytes = payload(1), newBytes = payload(2);
  const asInteger = bytes => bytes.reduceRight((value, byte) => (value << 8n) | BigInt(byte), 0n);
  const old = asInteger(oldBytes), replacement = asInteger(newBytes), expected = halves(equal ? old : old ^ 1n, size), source = halves(replacement, size);
  write(vm.handle, data, oldBytes);
  reg(vm.handle, 0, expected[0]); reg(vm.handle, 2, expected[1]); reg(vm.handle, 3, source[0]); reg(vm.handle, 1, source[1]); reg(vm.handle, 5, 0x80000000n); reg(vm.handle, 6, data);
  const code = await vm.install();
  assert.equal(code.run(10n), 4, 'whole source write commits before the revoked module exits');
  assert.equal(read(vm.handle, field.ip), vm.entry + 3n + BigInt(instruction.length));
  assert.equal(read(vm.handle, field.work), 2n); assert.equal(read(vm.handle, field.retired), 2n);
  assert.equal(read(vm.handle, field.flags), equal ? 0xa56n : 0xa16n);
  assert.equal(readWide(vm.handle, data, size), equal ? replacement : old);
  assert.equal(vm.wideStores, size === 16 ? 1 : 0);
  assert.equal(core.wire_ready(vm.handle, code.ticket), 0, 'even the mismatch writeback revokes source');
  assert.equal((await vm.install()).run(10n), 2);
  assert.equal(read(vm.handle, field.regs + 7 * 8), equal ? 2n : 1n, 'new target bytes execute after replacement installation');
  assert.equal(read(vm.handle, field.regs + 8) & 255n, equal ? 1n : 0n);
  assert.equal(read(vm.handle, field.retired), 5n);
  vm.destroy();
}

const pairStore = core.wire_create(64, 0n, 0n);
map(pairStore, 0x20000n, 8192n); write(pairStore, 0x20ff8n, Array(16).fill(0x55));
const pairState = new Uint8Array(core.memory.buffer).slice(state(pairStore), state(pairStore) + field.size);
assert.equal(core.wire_protect(pairStore, 0x21000n, 4096n, 2), 0);
assert.equal(core.wire_store128(pairStore, 0x20ff8n, 11n, 22n), 1);
assert.equal(core.wire_fault_kind(pairStore), 3);
assert.equal(core.wire_fault_access(pairStore), 2);
assert.equal(core.wire_protect(pairStore, 0x21000n, 4096n, 4), 0);
assert.equal(readWide(pairStore, 0x20ff8n, 16), BigInt('0x' + '55'.repeat(16)), 'the concrete pair store preflights both halves');
assert.equal(core.wire_store128(pairStore, 0x20ff8n, 11n, 22n), 0);
assert.equal(readWide(pairStore, 0x20ff8n, 16), 11n | (22n << 64n));
assert.deepEqual(new Uint8Array(core.memory.buffer).slice(state(pairStore), state(pairStore) + field.size), pairState, 'pair memory operations do not transact CPU state');
core.wire_destroy(pairStore);

const numericOwner = core.wire_create(64, 0n, 0n);
const numericState = new Uint8Array(core.memory.buffer).slice(state(numericOwner), state(numericOwner) + field.size);
const numericValues = [0n, 1n, 2n, 0x7fffffffffffffffn, 0x8000000000000000n, 0xfffffffffffffffen, 0xffffffffffffffffn];
const u64 = value => BigInt.asUintN(64, value);
for (const a of numericValues) for (const b of numericValues) {
  assert.equal(u64(core.wire_mul_high(a, b)), u64((a * b) >> 64n));
  assert.equal(u64(core.wire_imul_high(a, b)), u64((BigInt.asIntN(64, a) * BigInt.asIntN(64, b)) >> 64n));
  for (const high of numericValues) for (const signed of [false, true]) {
    const wide = (high << 64n) | a;
    const dividend = signed ? BigInt.asIntN(128, wide) : wide;
    const divisor = signed ? BigInt.asIntN(64, b) : b;
    const q = divisor === 0n ? null : dividend / divisor;
    const fails = q === null || (signed ? BigInt.asIntN(64, q) !== q : q > ((1n << 64n) - 1n));
    const pointer = (signed ? core.wire_idiv64 : core.wire_div64)(numericOwner, a, high, b);
    assert.equal(pointer === 0, fails);
    if (!fails) {
      const output = new DataView(core.memory.buffer);
      assert.equal(output.getBigUint64(pointer, true), u64(q));
      assert.equal(output.getBigUint64(pointer + 8, true), u64(dividend % divisor));
    }
  }
}
assert.deepEqual(new Uint8Array(core.memory.buffer).slice(state(numericOwner), state(numericOwner) + field.size), numericState, 'numeric imports operate without CPU state transactions');
core.wire_destroy(numericOwner);

for (const [opcode, signed] of [[0x23, false], [0x2b, true]]) {
  const vm = await memoryProgram(64, [0x04, 1, 0x48, 0xf7, opcode, 0xcc]);
  map(vm.handle, 0x20000n); write(vm.handle, 0x20000n, [255, 255, 255, 255, 255, 255, 255, 255]);
  assert.equal(core.wire_protect(vm.handle, 0x20000n, 4096n, 2), 0);
  reg(vm.handle, 0, 0x7fn); reg(vm.handle, 3, 0x20000n);
  assert.equal((await vm.install()).run(100n), 2, 'implicit multiplication reads a read-only operand');
  const product = signed ? -128n : 128n * ((1n << 64n) - 1n);
  assert.equal(read(vm.handle, field.regs), u64(product));
  assert.equal(read(vm.handle, field.regs + 16), u64(product >> 64n));
  assert.equal(read(vm.handle, field.flags), signed ? 0x292n : 0xa93n);
  assert.equal(read(vm.handle, field.work), 2n);
  vm.destroy();
}

for (const mode of [32, 64]) {
  for (const bits of [16, 32, 64]) {
    if (mode === 32 && bits === 64) continue;
    const prefix = bits === 16 ? [0x66] : bits === 64 ? [0x48] : [];
    for (const opcode of [0xa3, 0xab, 0xb3, 0xbb]) {
      const vm = await memoryProgram(mode, [0x04, 1, ...prefix, 0x0f, opcode, 0x0b, 0xcc]);
      map(vm.handle, 0x20000n, 8192n);
      const code = await vm.install(), width = (1n << BigInt(bits)) - 1n;
      for (const index of [-bits - 1, -1, 0, bits, bits * 2 + 1]) {
        const offset = BigInt(index) >> BigInt(Math.log2(bits));
        const target = 0x20800n + offset * BigInt(bits / 8), bit = BigInt(index) & BigInt(bits - 1);
        const initial = opcode === 0xab ? 0n : width;
        write(vm.handle, target, Array.from({ length: bits / 8 }, (_, n) => Number((initial >> BigInt(n * 8)) & 255n)));
        vm.reset(); reg(vm.handle, 0, 0x7fn); reg(vm.handle, 1, BigInt(index)); reg(vm.handle, 3, 0x20800n);
        assert.equal(code.run(100n), 2);
        const expected = opcode === 0xab ? initial | (1n << bit) : opcode === 0xb3 ? initial & ~(1n << bit) : opcode === 0xbb ? initial ^ (1n << bit) : initial;
        assert.equal(load(vm.handle, target, bits / 8), expected, 'signed bit index selects the complete memory bit string');
        assert.equal(read(vm.handle, field.flags), 0xa92n | ((initial >> bit) & 1n));
      }
      vm.destroy();
    }
  }
  const wrapped = await memoryProgram(mode, mode === 32 ? [0x64, 0x67, 0x66, 0x0f, 0xab, 0x0f, 0xcc] : [0x64, 0x67, 0x0f, 0xab, 0x0b, 0xcc]);
  map(wrapped.handle, 0x20000n);
  reg(wrapped.handle, 3, mode === 32 ? 0xffffn : 0xffffffffn); reg(wrapped.handle, 1, BigInt(mode === 32 ? 16 : 32)); set(wrapped.handle, field.fs, 0x20000n);
  assert.equal((await wrapped.install()).run(100n), 2);
  assert.equal(load(wrapped.handle, mode === 32 ? 0x20001n : 0x20003n, mode === 32 ? 2 : 4), 1n, 'bit-string offset wraps before applying the segment base');
  wrapped.destroy();
  const bitFault = await memoryProgram(mode, [0x04, 1, 0x0f, 0xb3, 0x0b, 0xcc]);
  map(bitFault.handle, 0x20000n, 8192n); write(bitFault.handle, 0x20ffen, [255, 255, 255, 255]);
  reg(bitFault.handle, 0, 0x7fn); reg(bitFault.handle, 1, 0n); reg(bitFault.handle, 3, 0x20ffen);
  assert.equal(core.wire_protect(bitFault.handle, 0x21000n, 4096n, 2), 0);
  assert.equal((await bitFault.install()).run(100n), 3);
  assert.equal(read(bitFault.handle, field.flags), 0xa92n, 'failed bit RMW retains the pending prior FLAGS');
  assert.equal(load(bitFault.handle, 0x20ffen), 0xffffffffn);
  bitFault.destroy();
}

for (const instruction of [[0xff, 0xd2], [0xff, 0xe2], [0xc3]]) {
  const vm = await memoryProgram(64, [0x04, 1, ...instruction, 0xcc]);
  map(vm.handle, 0x30000n); reg(vm.handle, 4, 0x30800n); reg(vm.handle, 0, 0x7fn);
  const target = 1n << 47n;
  reg(vm.handle, 2, target);
  write(vm.handle, 0x30800n, Array.from({ length: 8 }, (_, index) => Number((target >> BigInt(index * 8)) & 255n)));
  assert.equal((await vm.install()).run(100n), 3);
  assert.equal(core.wire_fault_kind(vm.handle), 1);
  assert.equal(BigInt.asUintN(64, core.wire_fault_address(vm.handle)), target);
  assert.equal(core.wire_fault_access(vm.handle), 4);
  assert.equal(read(vm.handle, field.regs + 4 * 8), 0x30800n, 'invalid control target cannot commit stack changes');
  assert.equal(read(vm.handle, field.flags), 0xa92n);
  assert.equal(read(vm.handle, field.ip), vm.entry + 2n);
  assert.equal(read(vm.handle, field.work), 1n);
  vm.destroy();
}
for (const [bytes, flags, fault, distance] of [
  [[0xeb, 20, 0xcc], 0x202n, true, 22n],
  [[0x74, 20, 0xcc], 0x202n, false, 22n],
  [[0x74, 20, 0xcc], 0x242n, true, 22n],
  [[0xe8, 20, 0, 0, 0, 0xcc], 0x202n, true, 25n],
]) {
  const entry = (1n << 47n) - 16n;
  const handle = core.wire_create(64, entry, 0n);
  map(handle, entry & ~4095n, 4096n, 7); write(handle, entry, bytes); set(handle, field.flags, flags);
  const compiled = candidate(handle);
  const code = await compiled.pending;
  assert.equal(core.wire_ready(handle, compiled.ticket), 1);
  assert.equal(code.run(state(handle), 100n, core.wire_cache(handle), handle), fault ? 3 : 2);
  assert.equal(read(handle, field.work), fault ? 0n : 1n);
  assert.equal(read(handle, field.ip), fault ? entry : entry + 2n);
  assert.equal(read(handle, field.regs + 4 * 8), 0n);
  if (fault) assert.equal(core.wire_fault_address(handle), entry + distance);
  core.wire_destroy(handle);
}
for (const [bytes, count, flags, fails, after] of [
  [[0xe2, 20, 0xcc], 0n, 0x202n, true, 0n],
  [[0xe2, 20, 0xcc], 1n, 0x202n, false, 0n],
  [[0xe1, 20, 0xcc], 2n, 0x242n, true, 2n],
  [[0xe1, 20, 0xcc], 2n, 0x202n, false, 1n],
  [[0x67, 0xe2, 20, 0xcc], 0x100000000n, 0x202n, true, 0x100000000n],
  [[0xe3, 20, 0xcc], 0n, 0x202n, true, 0n],
  [[0xe3, 20, 0xcc], 1n, 0x202n, false, 1n],
]) {
  const entry = (1n << 47n) - 16n, handle = core.wire_create(64, entry, 0n);
  map(handle, entry & ~4095n, 4096n, 7); write(handle, entry, bytes); reg(handle, 1, count); set(handle, field.flags, flags);
  const code = await candidate(handle).pending;
  assert.equal(code.run(state(handle), 100n, core.wire_cache(handle), handle), fails ? 3 : 2);
  assert.equal(read(handle, field.regs + 8), after, 'a faulting count branch preserves its pre-instruction counter');
  assert.equal(read(handle, field.work), fails ? 0n : 1n);
  assert.equal(read(handle, field.flags), flags);
  assert.equal(read(handle, field.ip), fails ? entry : entry + BigInt(bytes.length - 1));
  core.wire_destroy(handle);
}
console.log('core Wasm memory, scalar/wide integer RMW, count branches, precise exits, and lifecycle checks passed');
