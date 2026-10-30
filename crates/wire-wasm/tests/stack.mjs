// Explicit architectural checks for FLAGS images and multi-access stack
// operations. All bytecode and memory inputs are generated here.
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
const core = instance.exports, entry = 0x10000n;
const mask = size => (1n << BigInt(size * 8)) - 1n;
const bytes = (value, size) => Array.from({ length: size }, (_, index) => Number((value >> BigInt(index * 8)) & 255n));
const merge = (old, value, size) => size === 2 ? (old & ~mask(2)) | (value & mask(2)) : value & mask(size);
let cases = 0;
function program(code, mode = 64) {
  const calls = { resolve: 0, load: 0, store: 0, probe: 0 };
  let handle, ticket;
  const imports = { ...core,
    wire_create(...args) { handle = core.wire_create(...args); return handle; },
    wire_ticket(...args) { ticket = core.wire_ticket(...args); return ticket; },
  };
  for (const operation of Object.keys(calls)) imports[`wire_${operation}`] = (...args) => { calls[operation]++; return core[`wire_${operation}`](...args); };
  const vm = new WasmMachine(imports, { mode, ip: entry });
  vm.map(entry, 4096n, 7); vm.write(entry, [...code, 0xcc]);
  return { vm, calls, get handle() { return handle; }, get ticket() { return ticket; } };
}
const forms = [[32, 2], [32, 4], [64, 2], [64, 8]];
const prefix = size => size === 2 ? [0x66] : [];
const writableFlags = size => [0, 2, 4, 6, 7, 8, 10, 11, 14, ...(size === 2 ? [] : [18, 21])].reduce((value, bit) => value | (1n << BigInt(bit)), 0n);
const poppedFlags = (old, value, size) => ((old & ~(writableFlags(size) | 0x10000n)) | (value & writableFlags(size)) | 2n);

for (const [mode, size] of forms) {
  const test = program([0x04, 1, ...prefix(size), 0x9c, ...prefix(size), 0x5b], mode), { vm } = test;
  vm.map(0x30000n, 8192n); vm.register(4, 0x30800n); vm.register(0, 127n); vm.register(3, 0x12345678n); vm.flags = 0x10602n;
  assert.equal((await vm.run(2n)).reason, Exit.Budget);
  assert.equal(vm.register(4), 0x30800n - BigInt(size));
  assert.equal(vm.readInt(vm.register(4), size), 0xe92n & mask(size), 'saved FLAGS contain the arithmetic recipe and omit RF');
  assert.equal(vm.flags, 0x10e92n);
  assert.equal((await vm.run(1n)).reason, Exit.Budget);
  assert.equal(vm.register(3), merge(0x12345678n, 0xe92n, size));
  assert.equal(vm.register(4), 0x30800n); assert.equal(vm.work, 3n); assert.equal(vm.retired, 3n);
  assert.equal(test.calls.store, 0); assert.equal(test.calls.probe, 0);
  vm.dispose(); cases++;

  for (const value of [0n, mask(size), 0x401n]) {
    const test = program([0x04, 1, ...prefix(size), 0x9d], mode), { vm } = test;
    vm.map(0x30000n, 4096n); vm.register(4, 0x30800n); vm.register(0, 127n); vm.flags = 0x3d0202n;
    vm.write(0x30800n, bytes(value, size));
    assert.equal((await vm.run(2n)).reason, Exit.Budget);
    assert.equal(vm.flags, poppedFlags(0x3d0a92n, value, size), 'CPL3 flag permissions and operand width');
    assert.equal(vm.register(4), 0x30800n + BigInt(size)); assert.equal(vm.retired, 2n);
    vm.dispose(); cases++;
  }
  const consumer = program([0x04, 1, ...prefix(size), 0x9d, 0x83, 0xd0, 0, 0x9f], mode);
  consumer.vm.map(0x30000n, 4096n); consumer.vm.register(4, 0x30800n); consumer.vm.register(0, 127n); consumer.vm.write(0x30800n, bytes(0x401n, size));
  assert.equal((await consumer.vm.run(10n)).reason, Exit.Unsupported);
  assert.equal(consumer.vm.register(0), 0x681n, 'a following carry consumer uses the popped FLAGS instead of the previous recipe');
  assert.equal(consumer.vm.flags, 0x606n);
  consumer.vm.dispose(); cases++;
  for (const operation of ['push', 'pop']) for (const reason of ['guard', 'cross-page']) {
    const test = program([0x04, 1, ...prefix(size), operation === 'push' ? 0x9c : 0x9d], mode), { vm } = test;
    vm.map(0x30000n, 8192n); vm.register(0, 127n);
    const address = reason === 'guard' ? 0x30000n : 0x31000n - BigInt(size / 2);
    const sp = address + (operation === 'push' ? BigInt(size) : 0n);
    vm.register(4, sp); vm.write(address, bytes(0x401n, size));
    const page = reason === 'guard' ? 0x30000n : 0x31000n;
    assert.equal(core.wire_protect(test.handle, page, 4096n, reason === 'guard' ? 0x104 : operation === 'push' ? 2 : 1), 0);
    assert.equal((await vm.run(10n)).reason, Exit.Fault);
    assert.equal(vm.register(4), sp); assert.equal(vm.register(0), 128n);
    assert.equal(vm.flags, 0xa92n); assert.equal(vm.work, 1n); assert.equal(vm.retired, 1n); assert.equal(vm.ip, entry + 2n);
    assert.equal(core.wire_fault_access(test.handle), operation === 'push' ? 2 : 1);
    assert.equal(core.wire_protect(test.handle, page, 4096n, 4), 0);
    assert.equal(vm.readInt(address, size), 0x401n, 'failed flag stack access does not write memory');
    assert.equal((await vm.run(1n)).reason, Exit.Budget);
    assert.equal(vm.register(4), sp + BigInt(operation === 'push' ? -size : size));
    assert.equal(vm.retired, 2n); vm.dispose(); cases++;
  }
}

for (const size of [2, 4]) for (const override of [false, true]) {
  const prefixes = [...prefix(size), ...(override ? [0x67] : [])];
  const test = program([...prefixes, 0x60, ...prefixes, 0x61], 32), { vm } = test;
  vm.map(0x30000n, 4096n);
  const initial = Array.from({ length: 8 }, (_, index) => 0xabcd1100n + BigInt(index)); initial[4] = 0x30800n;
  initial.forEach((value, index) => vm.register(index, value));
  assert.equal((await vm.run(1n)).reason, Exit.Budget);
  const bottom = 0x30800n - BigInt(8 * size);
  const image = [7, 6, 5, 4, 3, 2, 1, 0];
  for (let index = 0; index < 8; index++) assert.equal(vm.readInt(bottom + BigInt(index * size), size), initial[image[index]] & mask(size));
  vm.write(bottom + BigInt(3 * size), bytes(mask(size), size));
  for (const index of [0, 1, 2, 3, 5, 6, 7]) vm.register(index, 0x8765ffffn);
  assert.equal((await vm.run(1n)).reason, Exit.Budget);
  for (let index = 0; index < 8; index++) assert.equal(vm.register(index), index === 4 ? 0x30800n : merge(0x8765ffffn, initial[index], size));
  assert.equal(vm.work, 2n); assert.equal(vm.retired, 2n); assert.equal(vm.flags, 0x202n);
  assert.equal(test.calls.store, 0);
  vm.dispose(); cases++;
}

for (const size of [2, 4]) for (const operation of ['push', 'pop']) {
  const test = program([0x04, 1, ...prefix(size), operation === 'push' ? 0x60 : 0x61], 32), { vm } = test;
  vm.map(0x20000n, 8192n);
  const sp = operation === 'push' ? 0x21000n + BigInt(4 * size) : 0x21000n - BigInt(4 * size);
  const initial = [127n, 22n, 33n, 44n, sp, 55n, 66n, 77n];
  initial.forEach((value, index) => vm.register(index, value));
  if (operation === 'pop') vm.write(sp, Array.from({ length: 8 }, (_, index) => bytes(BigInt(100 + index), size)).flat());
  assert.equal(core.wire_protect(test.handle, operation === 'push' ? 0x20000n : 0x21000n, 4096n, operation === 'push' ? 2 : 0x104), 0);
  assert.equal((await vm.run(10n)).reason, Exit.Fault);
  for (let index = 0; index < 8; index++) assert.equal(vm.register(index), index === 0 ? 128n : initial[index], 'late fault retains every architectural register');
  assert.equal(vm.ip, entry + 2n); assert.equal(vm.work, 1n); assert.equal(vm.retired, 1n); assert.equal(vm.flags, 0xa92n);
  if (operation === 'push') for (let index = 0; index < 4; index++) assert.equal(vm.readInt(sp - BigInt((index + 1) * size), size), index === 0 ? 128n : initial[index], 'completed stores survive the fault');
  assert.equal(core.wire_protect(test.handle, 0x20000n, 8192n, 4), 0);
  assert.equal((await vm.run(1n)).reason, Exit.Budget);
  assert.equal(vm.retired, 2n); assert.equal(vm.register(4), sp + BigInt(operation === 'push' ? -8 * size : 8 * size));
  vm.dispose(); cases++;
}

for (const [mode, size] of forms) for (const levelByte of [0, 1, 2, 31, 32, 255]) for (const override of [false, true]) {
  const allocation = 23, level = levelByte & 31;
  const test = program([...prefix(size), ...(override ? [0x67] : []), 0xc8, allocation, 0, levelByte], mode), { vm } = test;
  const high = mode === 64 ? 0x100000000n : 0n, sp = high + 0x30800n, bp = high + 0x40800n;
  vm.map(high + 0x30000n, 4096n); vm.map(high + 0x40000n, 4096n); vm.register(4, sp); vm.register(5, bp);
  for (let depth = 1; depth < level; depth++) vm.write(bp - BigInt(depth * size), bytes(0x80n + BigInt(depth), size));
  assert.equal((await vm.run(1n)).reason, Exit.Budget);
  const frame = sp - BigInt(size), end = frame - BigInt(level * size + allocation);
  assert.equal(vm.register(4), end); assert.equal(vm.register(5), merge(bp, frame, size));
  assert.equal(vm.readInt(frame, size), bp & mask(size));
  for (let depth = 1; depth < level; depth++) assert.equal(vm.readInt(frame - BigInt(depth * size), size), 0x80n + BigInt(depth));
  if (level) assert.equal(vm.readInt(frame - BigInt(level * size), size), frame & mask(size));
  assert.equal(vm.readInt(end, size), 0n, 'the final stack permission probe does not write');
  assert.equal(vm.work, 1n); assert.equal(vm.retired, 1n); assert.equal(vm.flags, 0x202n);
  assert.equal(test.calls.store, 0); assert.equal(test.calls.probe, 0, 'ordinary final stack probe stays inline');
  vm.dispose(); cases++;
}

for (const [mode, size] of forms) for (const fault of ['parent', 'push', 'probe']) {
  const allocation = fault === 'probe' ? 4096 : 0, level = fault === 'probe' ? 0 : 2;
  const test = program([0x04, 1, ...prefix(size), 0xc8, allocation & 255, allocation >> 8, level], mode), { vm } = test;
  vm.map(0x30000n, 8192n); vm.map(0x40000n, 4096n);
  const sp = fault === 'push' ? 0x31000n + BigInt(size) : 0x31800n, bp = 0x40800n;
  vm.register(0, 127n); vm.register(4, sp); vm.register(5, bp); vm.write(bp - BigInt(size), bytes(123n, size));
  assert.equal(core.wire_protect(test.handle, fault === 'parent' ? 0x40000n : 0x30000n, 4096n, fault === 'parent' ? 1 : 2), 0);
  assert.equal((await vm.run(10n)).reason, Exit.Fault);
  assert.equal(vm.register(4), sp); assert.equal(vm.register(5), bp); assert.equal(vm.flags, 0xa92n);
  assert.equal(vm.work, 1n); assert.equal(vm.retired, 1n); assert.equal(vm.ip, entry + 2n);
  assert.equal(core.wire_fault_kind(test.handle), 3);
  assert.equal(core.wire_fault_access(test.handle), fault === 'parent' ? 1 : 2);
  assert.equal(vm.readInt(sp - BigInt(size), size), bp & mask(size), 'the first ENTER store precedes a later failure');
  assert.equal(core.wire_protect(test.handle, fault === 'parent' ? 0x40000n : 0x30000n, 4096n, 4), 0);
  assert.equal((await vm.run(1n)).reason, Exit.Budget); assert.equal(vm.retired, 2n);
  vm.dispose(); cases++;
}

const overlap = program([0xc8, 0, 0, 3]);
overlap.vm.map(0x30000n, 4096n); overlap.vm.register(4, 0x30800n); overlap.vm.register(5, 0x30800n);
assert.equal((await overlap.vm.run(1n)).reason, Exit.Budget);
for (let depth = 1; depth <= 3; depth++) assert.equal(overlap.vm.readInt(0x30800n - BigInt(depth * 8), 8), 0x30800n, 'nested ENTER reads observe its preceding writes');
assert.equal(overlap.vm.readInt(0x307e0n, 8), 0x307f8n);
overlap.vm.dispose(); cases++;

const probe = program([0xc8, 0, 0x10, 0]);
probe.vm.map(entry + 4096n, 4096n); probe.vm.register(4, entry + 4104n); probe.vm.register(5, 123n);
assert.equal((await probe.vm.run(1n)).reason, Exit.Budget);
assert.equal(probe.vm.register(4), entry); assert.equal(probe.vm.register(5), entry + 4096n);
assert.equal(probe.vm.readInt(entry, 4), 0x001000c8n);
assert.equal(core.wire_ready(probe.handle, probe.ticket), 1, 'a probe into watched source does not revoke it');
assert.equal(probe.calls.probe, 1); assert.equal(probe.calls.store, 0);
probe.vm.dispose(); cases++;

const writeOnly = program([0xc8, 4, 0, 0]);
writeOnly.vm.map(0x30000n, 4096n, 2); writeOnly.vm.map(0x31000n, 4096n);
writeOnly.vm.register(4, 0x31008n); writeOnly.vm.register(5, 77n);
assert.equal((await writeOnly.vm.run(1n)).reason, Exit.Budget);
assert.equal(writeOnly.vm.register(4), 0x30ffcn); assert.equal(writeOnly.calls.probe, 1, 'a cross-page final probe checks write permission without reading');
writeOnly.vm.dispose(); cases++;

for (const enter of [false, true]) {
  const test = program(enter ? [0xc8, 0, 0, 2] : [0x60], 32), { vm } = test;
  vm.register(4, entry + 0x120n); vm.register(5, entry + 0x200n);
  vm.write(entry + 0x1fcn, bytes(123n, 4));
  for (const index of [0, 1, 2, 3, 6, 7]) vm.register(index, BigInt(index + 11));
  assert.equal((await vm.run(1n)).reason, Exit.Budget);
  assert.equal(vm.retired, 1n); assert.equal(vm.work, 1n);
  assert.equal(vm.register(4), entry + (enter ? 0x114n : 0x100n));
  assert.equal(vm.ip, entry + (enter ? 4n : 1n));
  assert.equal(core.wire_ready(test.handle, test.ticket), 0, 'all stores finish before a source-writing stack instruction exits');
  if (enter) {
    assert.equal(vm.readInt(entry + 0x11cn), entry + 0x200n);
    assert.equal(vm.readInt(entry + 0x118n), 123n);
    assert.equal(vm.readInt(entry + 0x114n), entry + 0x11cn);
  } else {
    assert.equal(vm.readInt(entry + 0x10cn), entry + 0x120n, 'saved ESP is the original pointer');
    assert.equal(vm.readInt(entry + 0x100n), 18n);
  }
  vm.dispose(); cases++;
}

const partialSource = program([0x60], 32);
partialSource.vm.map(entry - 4096n, 4096n, 1);
const original = [11n, 22n, 33n, 44n, entry + 16n, 55n, 66n, 77n];
original.forEach((value, index) => partialSource.vm.register(index, value));
assert.equal((await partialSource.vm.run(1n)).reason, Exit.Fault);
assert.equal(partialSource.vm.ip, entry); assert.equal(partialSource.vm.work, 0n); assert.equal(partialSource.vm.retired, 0n);
original.forEach((value, index) => assert.equal(partialSource.vm.register(index), value));
for (let index = 0; index < 4; index++) assert.equal(partialSource.vm.readInt(entry + BigInt(12 - index * 4)), original[index]);
assert.equal(core.wire_ready(partialSource.handle, partialSource.ticket), 0, 'partial stores revoke source even when the whole instruction faults');
partialSource.vm.dispose(); cases++;

for (const instruction of [0x60, 0x61]) {
  const test = program([0x66, instruction], 32), { vm } = test;
  vm.map(0xfffff000n, 4096n); vm.map(0n, 4096n); vm.register(4, instruction === 0x60 ? 8n : 0xfffffff8n);
  for (const index of [0, 1, 2, 3, 5, 6, 7]) vm.register(index, BigInt(index + 11));
  assert.equal((await vm.run(1n)).reason, Exit.Budget);
  assert.equal(vm.register(4), instruction === 0x60 ? 0xfffffff8n : 8n, 'batch stack operations wrap at ESP width');
  vm.dispose(); cases++;
}
console.log(`${cases} FLAGS/batch stack/ENTER cases passed: widths, permissions, nested frames, precise faults, source probes, and budgets`);
