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
const base = 0x10000n;

function tracked() {
  const result = { prepares: 0, begins: 0, links: 0, liveChecks: 0, handle: 0, ticket: 0n, onLink: null };
  result.core = { ...core,
    wire_create(...args) { result.handle = core.wire_create(...args); return result.handle; },
    wire_compile(...args) { result.prepares++; return core.wire_compile(...args); },
    wire_ticket(...args) { result.ticket = core.wire_ticket(...args); return result.ticket; },
    wire_begin(...args) { result.begins++; return core.wire_begin(...args); },
    wire_link(...args) { result.links++; result.onLink?.(...args); return core.wire_link(...args); },
    wire_live(...args) { result.liveChecks++; return core.wire_live(...args); },
  };
  return result;
}

function program(bytes, observation = tracked(), options = {}) {
  const vm = new WasmMachine(observation.core, { mode: 64, ip: base, ...options });
  vm.map(base, 4096n, 7);
  vm.write(base, bytes);
  return { vm, observation };
}

const sum = [0xb9, 100, 0, 0, 0, 0x31, 0xc0, 0x01, 0xc8, 0x83, 0xe9, 1, 0x75, 0xf9, 0xcc];
for (const mode of [32, 64]) {
  const { vm, observation } = program(sum, tracked(), { mode });
  assert.deepEqual(await vm.run(0n), { reason: Exit.Budget, work: 0n, ip: base });
  for (let step = 0; step < 302; step++) {
    const result = await vm.run(1n);
    assert.equal(result.reason, Exit.Budget);
    assert.equal(result.work, 1n);
  }
  assert.equal((await vm.run(1n)).reason, Exit.Unsupported);
  assert.equal(vm.register(0), 5050n);
  assert.equal(vm.register(1), 0n);
  assert.equal(vm.work, 302n);
  assert.equal(vm.retired, 302n);
  assert.equal(observation.prepares, 1, 'interior budget stops reuse the original module');
  assert.equal(vm.stats().modules, 1);
  assert.equal(vm.stats().instances, 1);
  vm.setCodeLimit(0);
  assert.deepEqual(vm.stats(), { modules: 0, chargedBytes: 0, instances: 0 });
  await assert.rejects(vm.run(1n), /budget exhausted/);
  vm.dispose(); vm.dispose();
}

// Ordinary eviction releases only the retired instance across the host bridge;
// bulk retirement beyond the bounded notification buffer keeps the full check.
{
  const {vm, observation} = program([0xb8, 7, 0, 0, 0, 0xcc]);
  vm.map(base + 4096n, 95n * 4096n, 7);
  for (let index = 0; index < 96; index++) {
    const pc = base + BigInt(index) * 4096n;
    vm.write(pc, [0xb8, index, 0, 0, 0, 0xcc]); vm.ip = pc;
    assert.equal((await vm.run(1n)).reason, Exit.Budget);
    assert.equal(vm.register(0), BigInt(index));
  }
  const retained = vm.stats(), checks = observation.liveChecks;
  assert.equal(retained.instances, 96);
  vm.setCodeLimit(retained.chargedBytes - 1);
  assert.equal(vm.stats().instances, 95);
  assert.equal(observation.liveChecks, checks, 'one eviction does not scan retained instances');
  vm.setCodeLimit(0);
  assert.deepEqual(vm.stats(), {modules: 0, chargedBytes: 0, instances: 0});
  assert.equal(observation.liveChecks - checks, 95, 'overflow checks and releases every remaining instance');
  vm.dispose();
}

const carry = program([0x00, 0xd8, 0xfe, 0xc0, 0x0f, 0x92, 0xc1, 0xcc]);
carry.vm.register(0, 255n); carry.vm.register(3, 1n);
for (let step = 0; step < 3; step++) assert.equal((await carry.vm.run(1n)).work, 1n);
assert.equal(carry.vm.register(0), 1n);
assert.equal(carry.vm.register(1) & 255n, 1n);
assert.equal(carry.vm.flags, 0x203n, 'an interior INC resumes with the actual saved carry');
assert.equal(carry.observation.prepares, 1);
carry.vm.dispose();

for (const mode of [32, 64]) for (const budget of [1n, 1000n]) {
  const {vm, observation} = program([0xff, 0xc0, 0xc3], tracked(), {mode});
  assert.equal((await vm.run(1n)).reason, Exit.Budget);
  vm.map(0x20000n, 4096n, 7); vm.map(0x30000n, 4096n, 3);
  const displacement = Number(BigInt.asUintN(32, base - 0x2000cn));
  vm.write(0x20000n, [0xb8, 255, 0, 0, 0, 0x04, 1, 0xe8,
    ...Array.from({length: 4}, (_, n) => displacement >>> (n*8) & 255), 0x0f, 0x92, 0xc1, 0xcc]);
  vm.ip = 0x20000n; vm.register(4, 0x30800n);
  for (let step = 0; step < 20; step++) {
    const result = await vm.run(budget);
    if (result.reason === Exit.Unsupported) break;
    assert.equal(result.reason, Exit.Budget); assert.ok(step < 19);
  }
  assert.equal(vm.ip, 0x2000fn);
  assert.equal(vm.register(0), 1n);
  assert.equal(vm.register(1) & 255n, 1n, 'cached callee preserves carry into its caller continuation');
  assert.equal(vm.register(4), 0x30800n);
  if (budget === 1000n) assert.equal(observation.prepares, 2, 'ordinary cached edges remain resident');
  assert.equal(vm.stats().modules, 2);
  vm.dispose();
}

const dynamicBytes = Array(40).fill(0xcc);
dynamicBytes.splice(0, 14, 0x39, 0xc9, 0x75, 28, 0xb8, 0xff, 0xff, 0xff, 0x7f, 0x83, 0xc0, 1, 0xff, 0xe2);
dynamicBytes.splice(32, 7, 0x80, 0xc3, 1, 0x0f, 0x98, 0xc1, 0xcc);
const dynamic = program(dynamicBytes);
dynamic.vm.register(2, base + 35n);
assert.equal((await dynamic.vm.run(100n)).reason, Exit.Unsupported);
assert.equal(dynamic.vm.register(0), 0x80000000n);
assert.equal(dynamic.vm.register(1), 1n);
assert.equal(dynamic.vm.flags, 0xa96n, 'an interior dynamic target cannot consume a skipped 8-bit producer');
assert.equal(dynamic.vm.work, 6n);
assert.equal(dynamic.observation.prepares, 1, 'dynamic interior target stays in the existing Wasm module');
dynamic.vm.dispose();

const regions = program([0xff, 0xe2]);
regions.vm.map(0x20000n, 4096n, 7);
regions.vm.write(0x20000n, [0xb8, 7, 0, 0, 0, 0xcc]);
regions.vm.register(2, 0x20000n);
assert.equal((await regions.vm.run(1000n)).reason, Exit.Unsupported);
assert.equal(regions.vm.register(0), 7n);
assert.equal(regions.vm.work, 2n);
const retained = regions.vm.stats();
assert.equal(retained.modules, 2);
assert.equal(retained.instances, 2);
regions.vm.ip = base; regions.vm.register(0, 0n);
assert.equal((await regions.vm.run(1000n)).reason, Exit.Unsupported);
assert.equal(regions.vm.register(0), 7n);
assert.deepEqual(regions.vm.stats(), retained, 'both installed regions are reused');
regions.vm.setCodeLimit(retained.chargedBytes - 1);
assert.equal(regions.vm.stats().modules, 1);
assert.equal(regions.vm.stats().instances, 1, 'reclamation drops the corresponding JS instance reference');
assert.ok(regions.vm.stats().chargedBytes < retained.chargedBytes);
regions.vm.dispose();

for (const mode of [32, 64]) {
  const loop = program([0x83, 0xc0, 1, 0xff, 0xe2], tracked(), { mode });
  const { vm, observation } = loop, other = 0x20000n;
  vm.map(other, 4096n, 7);
  vm.write(other, [0x83, 0xe9, 1, 0x74, 2, 0xff, 0xe3, 0xcc]);
  vm.register(2, other); vm.register(3, base); vm.register(1, 1n);
  assert.equal((await vm.run(10n)).reason, Exit.Unsupported);
  const installed = vm.stats();
  for (const budget of [4999n, 111n]) {
    const count = 1000n, before = vm.work;
    vm.ip = base; vm.register(0, 0n); vm.register(1, count); vm.flags = 0x202n;
    observation.begins = 0; observation.links = 0;
    observation.onLink = () => {
      assert.equal(vm.register(0), 0n, 'GPRs stay resident across the module edge');
      assert.equal(vm.register(1), count);
      assert.equal(vm.flags, 0x202n, 'pending FLAGS stay resident');
      assert.equal(vm.work, before, 'counters commit at the outer exit');
      assert.equal(vm.ip, base);
    };
    const result = await vm.run(budget);
    assert.equal(result.reason, Exit.Budget);
    assert.equal(result.work, budget);
    assert.equal(observation.begins, 1, 'warm execution enters Wasm once');
    assert.equal(observation.links, 2, 'only two initial links enter the core resolver');
    assert.deepEqual(vm.stats(), installed, 'resident dispatch reuses both installed modules');
    assert.equal(vm.register(0), budget === 4999n ? count : 23n);
    assert.equal(vm.register(1), budget === 4999n ? 0n : 978n);
    assert.equal(vm.retired, vm.work);
  }
  observation.onLink = null;
  // Saturated authority must use validation even when cached tags compare equal.
  for (const pointer of [core.wire_links(observation.handle), core.wire_cache(observation.handle)]) {
    new DataView(core.memory.buffer).setBigUint64(pointer, (1n << 64n) - 1n, true);
    vm.ip = base; vm.register(0, 0n); vm.register(1, 8n);
    observation.begins = 0; observation.links = 0;
    assert.equal((await vm.run(100n)).reason, Exit.Unsupported);
    assert.equal(vm.register(0), 8n);
    assert.equal(observation.begins, 1);
    assert.equal(observation.links, 16, 'saturated epochs never authorize the fast link');
  }
  vm.dispose();
}

const lazy = program([0xb8, 0xff, 0xff, 0xff, 0x7f, 0x83, 0xc0, 1, 0xff, 0xe2]);
lazy.vm.map(0x20000n, 4096n, 7);
lazy.vm.write(0x20000n, [0x80, 0xc3, 1, 0x0f, 0x90, 0xc1, 0xcc]);
lazy.vm.ip = 0x20000n;
assert.equal((await lazy.vm.run(100n)).reason, Exit.Unsupported);
lazy.vm.ip = base; lazy.vm.register(1, 0n); lazy.vm.register(2, 0x20003n);
lazy.observation.begins = 0;
assert.equal((await lazy.vm.run(100n)).reason, Exit.Unsupported);
assert.equal(lazy.vm.register(1), 1n, 'an interior target consumes the incoming 32-bit overflow');
assert.equal(lazy.vm.flags, 0xa96n);
assert.equal(lazy.observation.begins, 1);
lazy.vm.dispose();

const carryRegions = program([0x04, 1, 0xff, 0xe2]);
carryRegions.vm.map(0x20000n, 4096n, 7); carryRegions.vm.map(0x30000n, 4096n, 7);
carryRegions.vm.write(0x20000n, [0x10, 0xd8, 0xd2, 0xe0, 0xff, 0xe6]);
carryRegions.vm.write(0x30000n, [0x0f, 0x92, 0xc3, 0xcc]);
carryRegions.vm.register(2, 0x20000n); carryRegions.vm.register(6, 0x30000n);
for (const [index, count] of [0n, 0n, 1n].entries()) {
  carryRegions.vm.ip = base; carryRegions.vm.register(0, 255n); carryRegions.vm.register(3, 255n); carryRegions.vm.register(1, count);
  carryRegions.observation.begins = 0;
  assert.equal((await carryRegions.vm.run(100n)).reason, Exit.Unsupported);
  assert.equal(carryRegions.vm.register(0), 0n);
  assert.equal(carryRegions.vm.register(3), count === 0n ? 1n : 0n);
  assert.equal(carryRegions.vm.flags, count === 0n ? 0x257n : 0x256n, 'carry and zero-count shift recipes survive both module edges');
  assert.equal(carryRegions.observation.begins, index === 0 ? 3 : 1);
}
carryRegions.vm.dispose();

const partialFlags = program([0x04, 1, 0xff, 0xe2]);
partialFlags.vm.map(0x20000n, 4096n, 7); partialFlags.vm.map(0x30000n, 4096n, 7);
partialFlags.vm.write(0x20000n, [0x0f, 0xba, 0xf1, 1, 0xf5, 0x0f, 0xbc, 0xdf, 0xff, 0xe6]);
partialFlags.vm.write(0x30000n, [0x0f, 0x90, 0xc2, 0x0f, 0x92, 0xc3, 0x0f, 0x94, 0xc1, 0x9f, 0xcc]);
for (const [index, a] of [0x7fn, 0xffn].entries()) {
  const vm = partialFlags.vm;
  vm.ip = base; vm.register(0, a); vm.register(1, 2n); vm.register(2, 0x20000n); vm.register(6, 0x30000n); vm.register(7, 1n); vm.flags = 0x202n;
  partialFlags.observation.begins = 0;
  const result = await vm.run(100n), expectedFlags = a === 0x7fn ? 0xa92n : 0x216n;
  assert.equal(result.reason, Exit.Unsupported);
  assert.equal(result.work, 10n);
  assert.equal(vm.flags, expectedFlags, 'partial flag overrides retain the other pending arithmetic flags');
  assert.equal(vm.register(0), (((expectedFlags & 0xd5n) | 2n) << 8n) | ((a + 1n) & 255n));
  assert.equal(vm.register(1), 0n); assert.equal(vm.register(3), 0n);
  assert.equal(vm.register(2), 0x20000n | (a === 0x7fn ? 1n : 0n));
  assert.equal(partialFlags.observation.begins, index === 0 ? 3 : 1);
}
partialFlags.vm.dispose();

const collision = program([0x83, 0xc0, 1, 0xff, 0xe2]);
const high = base + (1n << 40n);
collision.vm.map(high, 4096n, 7);
collision.vm.write(high, [0x83, 0xe9, 1, 0x74, 2, 0xff, 0xe3, 0xcc]);
collision.vm.register(2, high); collision.vm.register(3, base); collision.vm.register(1, 1n);
assert.equal((await collision.vm.run(100n)).reason, Exit.Unsupported);
collision.vm.ip = base; collision.vm.register(0, 0n); collision.vm.register(1, 8n);
collision.observation.begins = 0; collision.observation.links = 0;
assert.equal((await collision.vm.run(100n)).reason, Exit.Unsupported);
assert.equal(collision.vm.register(0), 8n);
assert.equal(collision.vm.register(1), 0n);
assert.equal(collision.observation.begins, 1);
assert.equal(collision.observation.links, 16, 'colliding links compare the entire 64-bit PC');
collision.vm.dispose();

const rewriting = program([0x83, 0xf9, 1, 0x75, 3, 0xc6, 0x06, 9, 0xff, 0xe2]);
rewriting.vm.map(0x20000n, 4096n, 7); rewriting.vm.map(0x30000n, 4096n);
rewriting.vm.write(0x20000n, [0xb8, 1, 0, 0, 0, 0x83, 0xe9, 1, 0x74, 2, 0xff, 0xe3, 0xcc]);
rewriting.vm.register(2, 0x20000n); rewriting.vm.register(3, base); rewriting.vm.register(6, 0x30000n);
rewriting.vm.register(1, 1n);
assert.equal((await rewriting.vm.run(100n)).reason, Exit.Unsupported);
const revoked = rewriting.observation.ticket;
rewriting.vm.ip = base; rewriting.vm.register(0, 0n); rewriting.vm.register(1, 2n); rewriting.vm.register(6, 0x20001n);
let retainedDuringRun = false;
rewriting.observation.onLink = (handle, pc) => {
  if (pc === base + 8n) {
    assert.equal(core.wire_live(handle, revoked), 0, 'source write revokes the installed target');
    assert.ok(core.wire_module_len(handle, revoked) > 0, 'visited target stays pinned until the resident run ends');
    retainedDuringRun = true;
  }
};
const rewritten = await rewriting.vm.run(100n);
assert.equal(rewritten.reason, Exit.Unsupported);
assert.equal(rewritten.work, 14n);
assert.equal(rewriting.vm.register(0), 9n, 'the previously visited target executes only its new source');
assert.ok(retainedDuringRun);
assert.equal(core.wire_module_len(rewriting.observation.handle, revoked), 0);
assert.equal(rewriting.vm.stats().instances, 2);
rewriting.vm.dispose();

const trapped = program([0xb8, 7, 0, 0, 0, 0xcc]);
trapped.observation.onLink = () => { throw new Error('resolver failure'); };
await assert.rejects(trapped.vm.run(10n), /resolver failure/);
trapped.vm.setCodeLimit(0);
assert.deepEqual(trapped.vm.stats(), { modules: 0, chargedBytes: 0, instances: 0 }, 'a trapped driver releases every run pin');
trapped.observation.onLink = null;
trapped.vm.setCodeLimit(65536);
assert.equal((await trapped.vm.run(10n)).reason, Exit.Unsupported);
trapped.vm.dispose();

const closingResident = program([0xb8, 7, 0, 0, 0, 0xcc]);
const residentHandle = closingResident.observation.handle;
let whileResident;
closingResident.observation.onLink = () => {
  closingResident.vm.dispose();
  whileResident = program([0xcc]);
  assert.notEqual(whileResident.observation.handle, residentHandle, 'the closed active owner retains its allocation');
};
await assert.rejects(closingResident.vm.run(10n), /disposed/);
const afterResident = program([0xcc]);
assert.equal(afterResident.observation.handle, residentHandle, 'the balanced run end releases the closed owner');
afterResident.vm.dispose(); whileResident.vm.dispose();

for (const [signed, high, divisor] of [[false, 0n, 0n], [false, 2n, 2n], [true, 0n, 0n], [true, 1n, 1n]]) {
  const observation = tracked();
  const helper = signed ? 'wire_idiv64' : 'wire_div64';
  let activeSnapshot = null, vm;
  observation.core[helper] = (...args) => {
    if (activeSnapshot) {
      assert.equal(vm.register(0), activeSnapshot.rax, 'wide helpers receive values without a CPU spill');
      assert.equal(vm.flags, activeSnapshot.flags);
      assert.equal(vm.work, activeSnapshot.work);
    }
    return core[helper](...args);
  };
  ({ vm } = program([0x04, 1, 0xff, 0xe6], observation));
  const target = 0x20000n;
  vm.map(target, 4096n, 7); vm.write(target, [0x48, 0xf7, signed ? 0xfb : 0xf3, 0xcc]);
  vm.ip = target; vm.register(3, 1n);
  assert.equal((await vm.run(1n)).reason, Exit.Budget);
  vm.ip = base; vm.register(0, 0x7fn); vm.register(2, high); vm.register(3, divisor); vm.register(6, target); vm.flags = 0x202n;
  activeSnapshot = { rax: vm.register(0), flags: vm.flags, work: vm.work };
  observation.begins = 0;
  assert.deepEqual(await vm.run(100n), { reason: Exit.Exception, work: 2n, ip: target, exception: { vector: 0 } });
  assert.equal(observation.begins, 1);
  assert.equal(vm.register(0), 0x80n); assert.equal(vm.register(2), high);
  assert.equal(vm.flags, 0xa92n);
  assert.equal(core.wire_exception(observation.handle), 1);
  assert.equal((await vm.run(0n)).reason, Exit.Budget);
  assert.equal(core.wire_exception(observation.handle), 1, 'zero budget preserves the pending exception');
  activeSnapshot = null;
  vm.register(2, 0n); vm.register(3, 2n);
  assert.equal((await vm.run(1n)).reason, Exit.Budget);
  assert.equal(vm.register(0), 0x40n); assert.equal(vm.register(2), 0n);
  assert.equal(core.wire_exception(observation.handle), 0, 'resumed execution clears the delivered exception record');
  vm.dispose();
}

const exchangeSource = program([0xf0, 0x0f, 0xb1, 0x13, 0xb9, 1, 0, 0, 0, 0x0f, 0x94, 0xc0, 0xcc]);
exchangeSource.vm.register(0, 1n); exchangeSource.vm.register(2, 9n); exchangeSource.vm.register(3, base + 5n);
assert.equal((await exchangeSource.vm.run(100n)).reason, Exit.Unsupported);
assert.equal(exchangeSource.vm.register(1), 9n, 'CMPXCHG source revocation precedes execution of the modified successor');
assert.equal(exchangeSource.vm.register(0), 1n, 'the successor consumes the compare FLAGS after recompilation');
assert.equal(exchangeSource.vm.flags, 0x246n);
assert.equal(exchangeSource.vm.work, 3n);
assert.equal(exchangeSource.observation.begins, 2);
assert.equal(exchangeSource.vm.stats().instances, 1);
exchangeSource.vm.dispose();

const failedInstall = program([0xb8, 7, 0, 0, 0, 0xcc]);
const Module = WebAssembly.Module;
try {
  WebAssembly.Module = new Proxy(Module, { construct() { throw new WebAssembly.CompileError('installation failure'); } });
  await assert.rejects(failedInstall.vm.run(10n), /installation failure/);
} finally { WebAssembly.Module = Module; }
failedInstall.vm.setCodeLimit(0);
assert.deepEqual(failedInstall.vm.stats(), { modules: 0, chargedBytes: 0, instances: 0 }, 'failed installation releases its source pin and host record');
failedInstall.vm.setCodeLimit(65536);
assert.equal((await failedInstall.vm.run(10n)).reason, Exit.Unsupported);
assert.equal(failedInstall.vm.register(0), 7n);
failedInstall.vm.dispose();

const changed = program([0xb8, 1, 0, 0, 0, 0xcc]);
const installing = changed.vm.run(10n);
assert.throws(() => changed.vm.setCodeLimit(0), /budget exhausted/, 'pending installation stays charged');
changed.vm.write(base, [0xb8, 9, 0, 0, 0, 0xcc]);
assert.equal((await installing).reason, Exit.Unsupported);
assert.equal(changed.vm.register(0), 9n);
assert.equal(changed.vm.work, 1n, 'old source is never executed after the async installation');
assert.equal(changed.vm.stats().modules, 1);
assert.equal(changed.vm.stats().instances, 1);
assert.equal(changed.observation.prepares, 2);
changed.vm.dispose();

const fault = program([0x8b, 0x03, 0xcc]);
fault.vm.register(3, 0x20000n);
assert.deepEqual(await fault.vm.run(10n), { reason: Exit.Fault, work: 0n, ip: base, fault: { kind: 2, address: 0x20000n, access: 1 } });
fault.vm.dispose();

const executeGuard = program([0xb8, 7, 0, 0, 0, 0xcc]);
assert.equal(core.wire_protect(executeGuard.observation.handle, base, 4096n, 0x120), 0);
assert.deepEqual(await executeGuard.vm.run(0n), { reason: Exit.Budget, work: 0n, ip: base });
assert.equal(executeGuard.observation.prepares, 0, 'zero work never attempts a fetch');
assert.deepEqual(await executeGuard.vm.run(10n), { reason: Exit.Fault, work: 0n, ip: base, fault: { kind: 4, address: base, access: 4 } });
assert.equal((await executeGuard.vm.run(10n)).reason, Exit.Unsupported);
assert.equal(executeGuard.vm.register(0), 7n, 'execute guard is consumed by the failed run');
executeGuard.vm.dispose();

const splitObservation = tracked();
const split = new WasmMachine(splitObservation.core, { ip: base + 4095n });
split.map(base, 8192n, 7);
split.write(base + 4095n, [0x48, 0xb8, 1, 2, 3, 4, 5, 6, 7, 8, 0xcc]);
assert.equal(core.wire_protect(splitObservation.handle, base + 4096n, 4096n, 0x120), 0);
assert.deepEqual(await split.run(10n), { reason: Exit.Fault, work: 0n, ip: base + 4095n, fault: { kind: 4, address: base + 4096n, access: 4 } });
assert.equal((await split.run(10n)).reason, Exit.Unsupported);
assert.equal(split.register(0), 0x0807060504030201n);
assert.equal(split.work, 1n);
split.dispose();

const missingTarget = program([0xff, 0xd2, 0xcc]);
missingTarget.vm.map(0x30000n, 4096n);
missingTarget.vm.register(4, 0x30800n); missingTarget.vm.register(2, 0x40000n);
assert.deepEqual(await missingTarget.vm.run(10n), { reason: Exit.Fault, work: 1n, ip: 0x40000n, fault: { kind: 2, address: 0x40000n, access: 4 } });
assert.equal(missingTarget.vm.register(4), 0x307f8n);
assert.equal(missingTarget.vm.readInt(0x307f8n, 8), base + 2n);
missingTarget.vm.dispose();

const closing = program([0xb8, 1, 0, 0, 0, 0xcc]);
const closedHandle = closing.observation.handle;
const closingRun = closing.vm.run(10n);
const closedTicket = closing.observation.ticket;
closing.vm.dispose();
assert.equal(core.wire_state(closedHandle), 0);
assert.equal(core.wire_live(closedHandle, closedTicket), 0);
const whilePinned = program([0xcc]);
assert.notEqual(whilePinned.observation.handle, closedHandle, 'closed owner remains allocated until its installation finishes');
await assert.rejects(closingRun, /disposed/);
const reused = program([0xb8, 7, 0, 0, 0, 0xcc]);
assert.equal(reused.observation.handle, closedHandle);
assert.equal((await reused.vm.run(10n)).reason, Exit.Unsupported);
assert.notEqual(reused.observation.ticket, closedTicket);
core.wire_release(closedHandle, closedTicket);
assert.equal(core.wire_ready(closedHandle, closedTicket), 0);
assert.equal(reused.vm.register(0), 7n);
reused.vm.dispose(); whilePinned.vm.dispose();
console.log('resident state/partial FLAGS, precise exceptions, link authority, source revocation, and lifecycle checks passed');
