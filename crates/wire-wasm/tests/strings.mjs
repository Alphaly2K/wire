// Explicit, deterministic checks of generated string loops against architectural
// results, including partial execution. Requires only Cargo's wasm32 target/Node.
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
const core = instance.exports;
const layout = Array.from(new Uint32Array(core.memory.buffer, core.wire_state_layout(), 8));
const entry = 0x10000n, mask = bits => (1n << BigInt(bits)) - 1n;
const put = (old, value, bits) => bits < 32 ? (old & ~mask(bits)) | (value & mask(bits)) : value & mask(bits);
let cases = 0;
function program(bytes, mode = 64) {
  const calls = { resolve: 0, load: 0, store: 0 };
  let handle;
  const exports = { ...core, wire_create(...args) { handle = core.wire_create(...args); return handle; } };
  for (const operation of Object.keys(calls)) exports[`wire_${operation}`] = (...args) => { calls[operation]++; return core[`wire_${operation}`](...args); };
  const vm = new WasmMachine(exports, { mode, ip: entry });
  vm.map(entry, 4096n, 7); vm.write(entry, [...bytes, 0xcc]);
  function set(offset, value) { new DataView(core.memory.buffer).setBigUint64(core.wire_state(handle) + offset, value, true); }
  return { vm, calls, handle, reset() { core.wire_cancel_repeat(handle); set(layout[3], entry); set(layout[4], 0n); set(layout[5], 0n); }, set };
}
function writeValues(vm, address, values, size) {
  const bytes = values.flatMap(value => Array.from({ length: size }, (_, index) => Number((value >> BigInt(index * 8)) & 255n)));
  vm.write(address, bytes);
}
function subtraction(a, b, bits, old) {
  const sign = 1n << BigInt(bits - 1), value = (a - b) & mask(bits);
  const parity = [...(value & 255n).toString(2)].filter(bit => bit === '1').length % 2 === 0;
  return (old & ~0x8d5n) | (a < b ? 1n : 0n) | (parity ? 4n : 0n) | ((a ^ b ^ value) & 16n)
    | (value === 0n ? 64n : 0n) | (value & sign ? 128n : 0n) | ((a ^ b) & (a ^ value) & sign ? 2048n : 0n);
}

for (const mode of [32, 64]) for (const addressBits of mode === 32 ? [16, 32] : [32, 64]) {
  for (const size of mode === 32 ? [1, 2, 4] : [1, 2, 4, 8]) {
    for (const [operation, opcode] of [['movs', 0xa4], ['stos', 0xaa], ['lods', 0xac], ['cmps', 0xa6], ['scas', 0xae]]) {
      for (const repeat of [0, 0xf3, 0xf2]) {
        const bytes = [...(addressBits !== mode ? [0x67] : []), ...(repeat ? [repeat] : []), ...(size === 2 ? [0x66] : size === 8 ? [0x48] : []), opcode + (size === 1 ? 0 : 1)];
        const test = program(bytes, mode), { vm } = test;
        const source = addressBits === 64 ? 0x100002000n : 0x2000n, destination = source + 0x2000n;
        vm.map(source, 4096n); vm.map(destination, 4096n);
        const width = size * 8, accumulator = 1n << BigInt(width - 1), high = 0xabcd123400000000n;
        const usesSource = ['movs', 'lods', 'cmps'].includes(operation), usesDestination = operation !== 'lods';
        const compares = ['cmps', 'scas'].includes(operation);
        for (const backwards of [false, true]) for (const count of [0n, 4n]) {
          test.reset();
          const sourceValues = [accumulator, mask(width), 0n, 7n];
          const comparison = operation === 'cmps' ? sourceValues : Array(4).fill(accumulator);
          const destinationValues = comparison.map((value, index) => index === (repeat === 0xf2 ? 0 : 1) ? value ^ 1n : value);
          writeValues(vm, source, backwards ? sourceValues.toReversed() : sourceValues, size);
          writeValues(vm, destination, backwards ? destinationValues.toReversed() : destinationValues, size);
          const upper = addressBits === 16 ? high | 0x98760000n : addressBits === 32 ? high : 0n;
          const si = upper | (source + (backwards ? 3n * BigInt(size) : 0n));
          const di = upper | (destination + (backwards ? 3n * BigInt(size) : 0n));
          const cx = upper | count, ax = size < 8 ? high | accumulator : accumulator;
          const flags = 0xa93n | (backwards ? 1024n : 0n) | (repeat === 0xf2 ? 64n : 0n);
          // Raw register setup also exercises high halves in PE32 address aliases.
          for (const [index, value] of [[0, ax], [1, cx], [6, si], [7, di]]) test.set(layout[1] + index * 8, value);
          vm.flags = flags;
          const accessBefore = { ...test.calls };
          assert.equal((await vm.run(0n)).reason, Exit.Budget);
          assert.deepEqual(test.calls, accessBefore);
          assert.equal(vm.work, 0n); assert.equal(vm.retired, 0n);
          const result = await vm.run(20n);
          const elements = !repeat ? 1 : count === 0n ? 0 : compares ? 2 : 4;
          const step = BigInt(elements * size) * (backwards ? -1n : 1n);
          const readRaw = index => new DataView(core.memory.buffer).getBigUint64(core.wire_state(test.handle) + layout[1] + index * 8, true);
          assert.equal(result.reason, Exit.Unsupported, `${operation}/${mode}/${width}/${addressBits}/${repeat}`);
          assert.equal(vm.ip, entry + BigInt(bytes.length));
          assert.equal(vm.work, BigInt(Math.max(1, elements))); assert.equal(vm.retired, 1n);
          assert.equal(readRaw(6), usesSource && elements ? put(si, si + step, addressBits) : si);
          assert.equal(readRaw(7), usesDestination && elements ? put(di, di + step, addressBits) : di);
          assert.equal(readRaw(1), repeat && elements ? put(cx, count - BigInt(elements), addressBits) : cx);
          assert.equal(readRaw(0), operation === 'lods' && elements ? put(ax, sourceValues[elements - 1], width) : ax);
          const left = operation === 'scas' ? accumulator : sourceValues[elements - 1];
          assert.equal(vm.flags, compares && elements ? subtraction(left, destinationValues[elements - 1], width, flags) : flags);
          const expectedDestination = [...destinationValues];
          if (['movs', 'stos'].includes(operation)) for (let index = 0; index < elements; index++) expectedDestination[index] = operation === 'movs' ? sourceValues[index] : accumulator;
          for (let index = 0; index < 4; index++) assert.equal(BigInt.asUintN(64, core.wire_load(test.handle, destination + BigInt(index * size), size, 1)), expectedDestination[backwards ? 3 - index : index]);
          if (repeat && count === 0n) assert.deepEqual(test.calls, accessBefore, 'zero-count strings do not resolve operands');
          cases++;
        }
        assert.equal(test.calls.load, 0); assert.equal(test.calls.store, 0, 'ordinary elements stay in Wasm');
        assert.ok(test.calls.resolve <= 2, 'translation misses depend on data pages, not REP elements');
        vm.dispose();
      }
    }
  }
}

// Count and pointers wrap at the address width. FS applies only to the source,
// after the narrow offset wraps; the destination remains flat ES.
for (const mode of [32, 64]) {
  const test = program([0x64, 0x67, 0xf3, 0xa4], mode), { vm } = test;
  const bits = mode === 32 ? 16 : 32, source = 0x30000n;
  vm.map(source, 4096n); vm.map(0n, 4096n);
  const last = mask(bits);
  vm.map(last & ~4095n, 4096n);
  test.set(layout[6], source); // FS base
  vm.map(source + (last & ~4095n), 4096n);
  vm.write(source + last, [11]); vm.write(source, [22]);
  vm.register(6, last); vm.register(7, last); vm.register(1, 2n);
  assert.equal((await vm.run(1n)).reason, Exit.Budget);
  assert.equal(vm.ip, entry); assert.equal(vm.work, 1n); assert.equal(vm.retired, 0n);
  assert.equal(vm.register(1), 1n); assert.equal(vm.register(6), 0n); assert.equal(vm.register(7), 0n);
  assert.equal((await vm.run(1n)).reason, Exit.Budget);
  assert.equal(vm.ip, entry + 4n); assert.equal(vm.work, 2n); assert.equal(vm.retired, 1n);
  assert.equal(vm.readInt(last, 1), 11n); assert.equal(vm.readInt(0n, 1), 22n);
  vm.dispose(); cases++;
}

for (const backwards of [false, true]) {
  const test = program([0xf3, 0xa4]), { vm } = test;
  vm.map(0x20000n, 4096n); vm.write(0x20000n, [1, 2, 3, 4, 5]);
  vm.flags = backwards ? 0x602n : 0x202n;
  vm.register(6, backwards ? 0x20004n : 0x20000n); vm.register(7, backwards ? 0x20003n : 0x20001n); vm.register(1, 4n);
  for (let index = 0; index < 4; index++) {
    assert.equal((await vm.run(1n)).reason, Exit.Budget);
    assert.equal(vm.retired, index === 3 ? 1n : 0n);
  }
  assert.equal(vm.readInt(0x20000n, 4), backwards ? 0x05050505n : 0x01010101n, 'overlapping MOVS observes each preceding element');
  assert.equal(test.calls.resolve, 1, 'an overlapping copy reuses its data page translation');
  vm.dispose(); cases++;
}

// Completed MOVS elements survive a later source fault, and a failed element
// does not advance either pointer or the count.
const copy = program([0xf3, 0xa4]);
copy.vm.map(0x20000n, 4096n); copy.vm.map(0x30000n, 4096n);
copy.vm.write(0x20ffen, [11, 22]); copy.vm.register(6, 0x20ffen); copy.vm.register(7, 0x30000n); copy.vm.register(1, 4n);
assert.equal((await copy.vm.run(1n)).reason, Exit.Budget);
assert.deepEqual(await copy.vm.run(10n), { reason: Exit.Fault, work: 1n, ip: entry, fault: { kind: 2, address: 0x21000n, access: 1 } });
assert.equal(copy.vm.register(6), 0x21000n); assert.equal(copy.vm.register(7), 0x30002n); assert.equal(copy.vm.register(1), 2n);
assert.equal(copy.vm.work, 2n); assert.equal(copy.vm.retired, 0n); assert.equal(copy.vm.readInt(0x30000n, 4), 0x160bn);
copy.vm.map(0x21000n, 4096n); copy.vm.write(0x21000n, [33, 44]);
assert.equal((await copy.vm.run(10n)).reason, Exit.Unsupported);
assert.equal(copy.vm.work, 4n); assert.equal(copy.vm.retired, 1n); assert.equal(copy.vm.readInt(0x30000n, 4), 0x2c21160bn);
copy.vm.dispose(); cases++;

const storeFault = program([0xf3, 0xab]);
storeFault.vm.map(0x30000n, 8192n); storeFault.vm.write(0x30ffan, Array(10).fill(0x55));
assert.equal(core.wire_protect(storeFault.handle, 0x31000n, 4096n, 2), 0);
storeFault.vm.register(7, 0x30ffan); storeFault.vm.register(1, 3n); storeFault.vm.register(0, 0x12345678n);
assert.equal((await storeFault.vm.run(10n)).reason, Exit.Fault);
assert.equal(storeFault.vm.work, 1n); assert.equal(storeFault.vm.retired, 0n);
assert.equal(storeFault.vm.register(7), 0x30ffen); assert.equal(storeFault.vm.register(1), 2n);
assert.equal(storeFault.vm.readInt(0x30ffen, 4), 0x55555555n, 'a faulting cross-page element cannot partially write');
storeFault.vm.dispose(); cases++;

for (const opcode of [0xa6, 0xae]) for (const repeat of [0xf3, 0xf2]) for (const changed of ['eviction', 'bytes', 'context']) {
  const test = program([0x04, 1, repeat, opcode]), { vm } = test;
  vm.map(0x20000n, 4096n); vm.map(0x30000n, 4096n);
  const a = 128, b = repeat === 0xf3 ? a : 0;
  vm.write(0x20ffen, [a, a]); vm.write(0x30fffn, [b]);
  vm.register(0, 127n); vm.register(6, 0x20ffen); vm.register(7, 0x30fffn); vm.register(1, 3n);
  assert.equal((await vm.run(2n)).reason, Exit.Budget);
  assert.equal(vm.work, 2n); assert.equal(vm.retired, 1n); assert.equal(vm.ip, entry + 2n);
  const partialFlags = vm.flags;
  assert.notEqual(partialFlags, 0xa92n);
  assert.equal((await vm.run(0n)).reason, Exit.Budget);
  if (changed === 'eviction') { vm.setCodeLimit(0); vm.setCodeLimit(64 * 1024 * 1024); }
  if (changed === 'bytes') vm.write(entry + 2n, [repeat === 0xf3 ? 0xf2 : 0xf3]);
  if (changed === 'context') vm.flags = 0x203n;
  assert.deepEqual(await vm.run(10n), { reason: Exit.Fault, work: 0n, ip: entry + 2n, fault: { kind: 2, address: 0x31000n, access: 1 } });
  assert.equal(vm.flags, changed === 'eviction' ? 0xa92n : changed === 'bytes' ? partialFlags : 0x203n, 'fault FLAGS respect original instruction identity and explicit context edits');
  assert.equal(vm.register(1), 2n); assert.equal(vm.register(7), 0x31000n); assert.equal(vm.work, 2n); assert.equal(vm.retired, 1n);
  vm.map(0x31000n, 4096n); vm.write(0x31000n, [b, b]);
  vm.map(0x21000n, 4096n); vm.write(0x21000n, [a]);
  assert.equal((await vm.run(10n)).reason, Exit.Unsupported);
  assert.equal(vm.retired, 2n);
  vm.dispose(); cases++;
}

for (const count of [1n, 2n]) {
  const test = program([0xf3, 0xa4]), { vm } = test;
  vm.map(0x20000n, 4096n); vm.write(0x20000n, [0xaa, 0x55]);
  vm.register(6, 0x20000n); vm.register(7, entry + 1n); vm.register(1, count); vm.register(0, 0xccn);
  const result = await vm.run(10n);
  assert.equal(result.reason, Exit.Unsupported);
  assert.equal(vm.work, count); assert.equal(vm.retired, 1n); assert.equal(vm.register(1), 0n);
  assert.equal(vm.register(6), 0x20001n, 'revoked MOVS must not execute another element after it becomes STOS');
  assert.equal(vm.register(7), entry + 1n + count); assert.equal(vm.ip, entry + 2n);
  assert.equal(vm.readInt(entry + 1n, 1), 0xaan);
  vm.dispose(); cases++;
}
console.log(`${cases} string/REP cases passed: widths, FLAGS, short budgets, precise faults, restart identity, inline memory, and source revocation`);
