export const Exit = Object.freeze({ Dispatch: 0, Budget: 1, Unsupported: 2, Fault: 3, SourceChanged: 4, Exception: 5, HostError: 6, Service: 7, Returned: 8, Callback: 9, ThreadReturned: 10, Waiting: 11, ProcessExited: 12, Graphics: 13, FileInput:14 });
const MAX_U64 = (1n << 64n) - 1n;
const unsigned = value => BigInt.asUintN(64, value);
const drivers = new WeakMap();
const graphicsSlots = new WeakMap();

function driverModule(core) {
  let pending = drivers.get(core);
  if (!pending) {
    const pointer = core.wire_driver();
    const length = core.wire_driver_len();
    pending = WebAssembly.compile(new Uint8Array(core.memory.buffer, pointer, length).slice());
    drivers.set(core, pending);
  }
  return pending;
}

function u64(value) {
  value = BigInt(value);
  if (value < 0n || value > MAX_U64) throw new RangeError('value is outside the 64-bit unsigned range');
  return value;
}

function byteLimit(value) {
  if (!Number.isSafeInteger(value) || value < 0 || value > 0xffffffff) throw new RangeError('invalid code budget');
  return value;
}

function defaultClock(frequency) {
  const origin = performance.now();
  return () => BigInt(Math.floor((performance.now() - origin) * (frequency / 1000)));
}

function defaultRandom(length) {
  try {
    const bytes = new Uint8Array(length);
    for (let offset = 0; offset < length; offset += 65536) globalThis.crypto.getRandomValues(bytes.subarray(offset, offset + 65536));
    return bytes;
  }
  catch { return null; }
}

/** A single guest owner in an instantiated core Wasm module. */
export class WasmMachine {
  #core;
  #handle;
  #mode;
  #entryPoints;
  #image = null;
  #layout;
  #records = new Map();
  #epoch = 0n;
  #current = 0n;
  #closed = false;
  #running = false;
  #family = {members: [this], cursor: 0, running: false, fileReaders:new Map()};
  #loading = false;
  #finished = false;
  #entryFinished = false;
  #hasChildren = false;
  #table = new WebAssembly.Table({ element: 'anyfunc', initial: 1 });
  #freeSlots = [];
  #driver = null;
  #clock;
  #wallClock;
  #cpuTimes;
  #cpuSampling = false;
  #random;
  #ticks = 0n;
  #clockError = null;
  #clockResult = [0n, 0];
  #platform = { clock: () => this.#platformClock() };
  #graphics = null;
  #graphicsSlot = 0;
  #graphicsSetup = false;
  #graphicsOptions = null;
  #inheritedGraphics = null;
  #audio = null;
  #audioSetup = false;
  #audioOptions = null;
  #inheritedAudio = null;

  constructor(core, { mode = 64, ip = 0n, sp = 0n, codeLimit, clock, wallClock, cpuTimes, random, timeZone } = {}) {
    this.#core = core.exports ?? core;
    if (!(this.#core.memory instanceof WebAssembly.Memory)) throw new TypeError('expected a core Wasm instance');
    if (mode !== 32 && mode !== 64) throw new RangeError('unsupported guest architecture');
    if (codeLimit !== undefined) byteLimit(codeLimit);
    if (clock !== undefined && typeof clock !== 'function') throw new TypeError('expected a clock function');
    if (wallClock !== undefined && typeof wallClock !== 'function') throw new TypeError('expected a wall clock function');
    if (cpuTimes !== undefined && typeof cpuTimes !== 'function') throw new TypeError('expected a CPU times function');
    if (random !== undefined && typeof random !== 'function') throw new TypeError('expected a random byte function');
    if (timeZone !== undefined && (!(timeZone instanceof Uint8Array) || timeZone.length !== 172)) throw new TypeError('expected a 172-byte timezone profile');
    const zone = timeZone?.slice();
    this.#clock = clock ?? defaultClock(this.#core.wire_clock_frequency());
    this.#wallClock = wallClock ?? (() => BigInt(Date.now()) * 10000n + 116444736000000000n);
    this.#cpuTimes = cpuTimes ?? (() => null);
    this.#random = random ?? defaultRandom;
    this.#mode = mode;
    const [services, serviceBytes, returned, callback, threadReturned] = new BigUint64Array(this.#core.memory.buffer, this.#core.wire_entry_layout(mode), 5);
    this.#entryPoints = Object.freeze({ services, serviceBytes, returned, callback, threadReturned });
    this.#layout = Array.from(new Uint32Array(this.#core.memory.buffer, this.#core.wire_state_layout(), 11));
    this.#handle = this.#core.wire_create(mode, u64(ip), u64(sp));
    if (!this.#handle) throw new Error('could not create machine');
    try {
      if (zone) {
        const pointer=this.#core.wire_input(this.#handle,zone.length);
        if(!pointer)throw this.#error(1);
        new Uint8Array(this.#core.memory.buffer,pointer,zone.length).set(zone);
        if(this.#core.wire_configure_timezone(this.#handle))throw this.#error(1);
      }
      if (codeLimit !== undefined) this.setCodeLimit(codeLimit);
    } catch (error) { this.dispose(); throw error; }
  }

  #assertOpen() {
    if (this.#closed || !this.#core.wire_state(this.#handle)) throw new Error('machine has been disposed');
  }

  #assertCPUWritable() {
    if (this.#cpuSampling) throw new Error('CPU changes require completing the pending CPU sample');
    if (this.#core.wire_file_pending?.(this.#handle)) throw new Error('CPU changes require completing the pending file input');
    if (this.#core.wire_graphics_pending?.(this.#handle)) throw new Error('CPU changes require completing the pending graphics operation');
  }

  async enableGraphics(options = {}) {
    this.#assertOpen();
    if (this.#running || this.#family.running || this.#graphics || this.#graphicsSetup) throw new Error('graphics setup requires an idle machine without an attached backend');
    return this.#attachGraphics(options);
  }

  configureOutputs({graphics = null, audio = null} = {}) {
    this.#assertOpen();
    if (!this.#image || this.#running || this.#family.running || this.work !== 0n || this.#graphics || this.#audio || this.#graphicsSetup || this.#audioSetup) throw new Error('output configuration requires an unstarted image');
    for (const options of [graphics,audio]) if (options !== null && (typeof options !== 'object' || Array.isArray(options))) throw new TypeError('expected output options or null');
    if (graphics?.getCanvas !== undefined && typeof graphics.getCanvas !== 'function') throw new TypeError('expected a presentation canvas function');
    if (audio?.createSink !== undefined && typeof audio.createSink !== 'function') throw new TypeError('expected an audio sink factory');
    if (audio?.sink !== undefined) throw new TypeError('process family audio requires a sink factory');
    this.#inheritedGraphics = graphics && {...graphics};
    this.#inheritedAudio = audio && {...audio};
  }

  async #attachGraphics(options) {
    this.#assertOpen();
    if (this.#graphics || this.#graphicsSetup) throw new Error('graphics backend is already attached or being configured');
    if (this.#mode !== 64 || !this.#image) throw new Error('graphics setup requires an AMD64 image');
    if (!options || typeof options !== 'object') throw new TypeError('expected graphics options');
    if (options.getCanvas !== undefined && typeof options.getCanvas !== 'function') throw new TypeError('expected a presentation canvas function');
    const configured = {...options};
    const effective = {...configured};
    const process = this.processId;
    if (configured.getCanvas) effective.getCanvas = window => configured.getCanvas(window, process);
    if (this.#core.wire_graphics_abi?.() !== 1 || !(this.#core.__indirect_function_table instanceof WebAssembly.Table)) throw new Error('graphics bridge is unavailable');
    this.#graphicsSetup = true;
    let graphics;
    const table = this.#core.__indirect_function_table;
    try {
      if (!globalThis.WireWebGPU) {
        await import('./draw.js');
        await import('./gpu.js');
      }
      graphics = await globalThis.WireWebGPU.create(this.#core.memory, effective);
      this.#assertOpen();
      const bytes = new Uint8Array([0,97,115,109,1,0,0,0,1,7,1,96,2,127,127,1,127,2,17,1,4,119,105,114,101,8,100,105,115,112,97,116,99,104,0,0,3,2,1,0,7,8,1,4,99,97,108,108,0,1,10,10,1,8,0,32,0,32,1,16,0,11]);
      const shim = new WebAssembly.Instance(new WebAssembly.Module(bytes), {wire: {dispatch: (op, at) => graphics.dispatch(op >>> 0, at >>> 0)}});
      let free = graphicsSlots.get(table);
      if (!free) graphicsSlots.set(table, free = []);
      this.#graphicsSlot = free.length ? free.pop() : table.grow(1);
      table.set(this.#graphicsSlot, shim.exports.call);
      const profile = graphics.profile(), at = this.#core.wire_input(this.#handle, profile.length);
      if (!at) throw new Error('graphics profile allocation failed');
      new Uint8Array(this.#core.memory.buffer, at, profile.length).set(profile);
      if (this.#core.wire_attach_graphics(this.#handle, this.#graphicsSlot)) throw this.#error(2);
      this.#graphics = graphics;
      this.#graphicsOptions = configured;
    } catch (error) {
      graphics?.close();
      if (this.#graphicsSlot) { table.set(this.#graphicsSlot, null); graphicsSlots.get(table).push(this.#graphicsSlot); this.#graphicsSlot = 0; }
      throw error;
    } finally { this.#graphicsSetup = false; }
  }

  async enableAudio(options = {}) {
    this.#assertOpen();
    if (this.#running || this.#family.running) throw new Error('audio setup requires an idle machine');
    return this.#attachAudio(options);
  }

  async #attachAudio(options, running = false) {
    this.#assertOpen();
    if (this.#mode !== 32 || !this.#image || this.#audio || this.#audioSetup) throw new Error('audio setup requires a PE32 image without an attached output');
    if (!options || typeof options !== 'object') throw new TypeError('expected audio options');
    if (options.createSink !== undefined && typeof options.createSink !== 'function') throw new TypeError('expected an audio sink factory');
    const configured = {...options};
    if (typeof this.#core.wire_audio_enable !== 'function') throw new Error('audio bridge is unavailable');
    this.#audioSetup = true;
    let sink;
    try {
      sink = configured.sink ?? (configured.createSink
        ? await configured.createSink(this.processId)
        : await (await import('./audio.mjs')).createAudioSink(configured));
      if (typeof sink?.write !== 'function' || typeof sink?.close !== 'function') throw new TypeError('audio sink requires write and close functions');
      this.#assertOpen();
      if (!running && (this.#running || this.#family.running)) throw new Error('audio setup requires an idle machine');
      if (this.#core.wire_audio_enable(this.#handle, this.#readClock())) throw new Error('audio output could not be attached');
      this.#audio = sink;
      this.#audioOptions = configured;
    } catch (error) { if (typeof sink?.close === 'function') sink.close(); throw error; }
    finally { this.#audioSetup = false; }
  }

  #retireAudio() {
    if (this.#audio && !this.#core.wire_audio_enabled(this.#handle)) { this.#audio.close(); this.#audio=null; }
  }

  #pumpAudio() {
    this.#retireAudio();
    if (!this.#audio) return 0;
    const drain = () => {
      const count=this.#core.wire_audio_sample_count(this.#handle);
      if (!count) return true;
      const at=this.#core.wire_audio_samples(this.#handle);
      const samples=new Int16Array(this.#core.memory.buffer,at,count).slice();
      const accepted=this.#audio.write(samples);
      if (typeof accepted !== 'boolean') throw new TypeError('audio sink write must synchronously return a boolean');
      if (accepted) this.#core.wire_audio_ack(this.#handle);
      return accepted;
    };
    if (!drain()) return 0;
    const status=this.#core.wire_audio_pump(this.#handle,this.#readClock());
    if (status===1) return {kind:Number(this.#core.wire_audio_fault(this.#handle,0)),address:unsigned(this.#core.wire_audio_fault(this.#handle,1)),access:Number(this.#core.wire_audio_fault(this.#handle,2))};
    if (status) throw this.#error(status);
    drain();
    return 0;
  }

  audioStats() {
    this.#assertOpen();
    if (!this.#audio) return null;
    return {mixedFrames:unsigned(this.#core.wire_audio_stat(this.#handle,0)),
      droppedFrames:unsigned(this.#core.wire_audio_stat(this.#handle,1)),
      lockedPlaybackFrames:unsigned(this.#core.wire_audio_stat(this.#handle,2)),
      pendingFrames:this.#core.wire_audio_sample_count(this.#handle)/2};
  }

  async flushGraphics() {
    this.#assertOpen();
    if (this.#running || this.#family.running) throw new Error('machine is already running');
    if (!this.#graphics) throw new Error('graphics backend is not attached');
    await this.#graphics.flush();
    this.#assertOpen();
  }

  #error(status) {
    const pointer = this.#core.wire_error(this.#handle);
    const length = this.#core.wire_error_len(this.#handle);
    const message = new TextDecoder().decode(new Uint8Array(this.#core.memory.buffer, pointer, length));
    return new Error(message || `machine operation failed (${status})`);
  }

  #readClock() {
    const ticks = this.#clock();
    if (typeof ticks !== 'bigint') throw new TypeError('clock must return BigInt ticks');
    u64(ticks);
    if (ticks < this.#ticks) throw new RangeError('clock moved backwards');
    this.#ticks = ticks;
    return ticks;
  }

  #platformClock() {
    try {
      this.#clockResult[0] = this.#readClock();
      this.#clockResult[1] = 0;
    } catch (error) {
      this.#clockError = { error };
      this.#clockResult[0] = 0n;
      this.#clockResult[1] = 1;
    }
    return this.#clockResult;
  }

  /** Read elapsed 100ns ticks from the same source used by RDTSC/RDTSCP. */
  now() { this.#assertOpen(); return this.#readClock(); }
  get clockFrequency() { this.#assertOpen(); return this.#core.wire_clock_frequency(); }
  get entryPoints() { this.#assertOpen(); return this.#entryPoints; }
  get image() { this.#assertOpen(); return this.#image; }
  get processId() { this.#assertOpen(); return this.#core.wire_process_id(this.#handle); }
  get totalWork() { this.#assertOpen(); return this.#family.members.reduce((sum, vm) => sum + vm.work, 0n); }
  get totalRetired() { this.#assertOpen(); return this.#family.members.reduce((sum, vm) => sum + vm.retired, 0n); }

  processes() {
    this.#assertOpen();
    return this.#family.members.map(vm => ({id: vm.processId, mode: vm.#mode,
      work: vm.work, retired: vm.retired, exitCode: vm.processExitCode}));
  }

  finishProcessEntry() {
    this.#assertOpen();
    if (!this.#image || this.#running || this.#family.running) throw new Error('process entry completion requires an idle image');
    if (this.#entryFinished) return;
    const boundary = this.#core.wire_boundary(this.#handle);
    if (boundary !== Exit.Returned && boundary !== Exit.ThreadReturned) throw new Error('process entry has not returned');
    const status = this.#core.wire_finish_process_entry(this.#handle);
    if (status) throw this.#error(status);
    this.#entryFinished = true;
  }

  #processOwner(processId) {
    if (processId === undefined) return this;
    if (!Number.isInteger(processId) || processId < 1 || processId > 0xffffffff) throw new RangeError('invalid process identity');
    const owner = this.#family.members.find(vm => vm.processId === processId);
    if (!owner) throw new RangeError('unknown process identity');
    return owner;
  }

  supplyModule(name, bytes) {
    this.#supply(name, bytes, this.#core.wire_supply_module);
  }

  supplyFile(path, bytes) {
    this.#supply(path, bytes, this.#core.wire_supply_file);
  }

  supplyDirectory(path) {
    this.#supply(path, [], this.#core.wire_supply_directory);
  }

  supplyFileSource(path,size,read) {
    this.#assertOpen();
    if(this.#running || this.#family.running)throw Error('machine is already running');
    if(typeof path!=='string' || path.includes('\0') || typeof read!=='function')throw new TypeError('expected a file path and range reader');
    size=u64(size);if(size>0x7fffffffffffffffn)throw new RangeError('file source size exceeds the signed 64-bit range');
    const text=new TextEncoder().encode(path),at=this.#core.wire_input(this.#handle,text.length);
    if(!at)throw this.#error(1);
    new Uint8Array(this.#core.memory.buffer,at,text.length).set(text);
    const id=this.#core.wire_supply_file_source(this.#handle,text.length,size);
    if(!id)throw this.#error(1);
    this.#family.fileReaders.set(id,read);
  }

  async #readFileInput() {
    this.#assertOpen();
    if(this.#core.wire_file_request(this.#handle)!==24)throw Error('file input request is unavailable');
    const at=this.#core.wire_input(this.#handle,24),view=new DataView(this.#core.memory.buffer,at,24);
    const source=Number(view.getBigUint64(0,true)),offset=view.getBigUint64(8,true),length=Number(view.getBigUint64(16,true));
    const read=this.#family.fileReaders.get(source);
    if(!read || !Number.isSafeInteger(length) || length<1 || length>256*1024*1024)throw Error('invalid file input request');
    const bytes=await read(offset,length);
    this.#assertOpen();
    if(!(bytes instanceof Uint8Array) || bytes.length!==length)throw Error('file range reader returned an unexpected byte count');
    const input=bytes.buffer===this.#core.memory.buffer?bytes.slice():bytes;
    const pointer=this.#core.wire_input(this.#handle,length);if(!pointer)throw this.#error(1);
    new Uint8Array(this.#core.memory.buffer,pointer,length).set(input);
    if(this.#core.wire_supply_file_input(this.#handle,source,offset))throw this.#error(1);
  }

  fileStats(){this.#assertOpen();return{cachedBytes:Number(this.#core.wire_file_cache_bytes(this.#handle)),ownedBytes:Number(this.#core.wire_file_owned_bytes(this.#handle)),sources:this.#family.fileReaders.size};}

  writeInput(bytes, {end = false} = {}, processId) {
    this.#assertOpen();
    if (this.#running || this.#family.running) throw new Error('machine is already running');
    const owner = this.#processOwner(processId);
    if (owner !== this) return owner.writeInput(bytes, {end});
    if (typeof end !== 'boolean') throw new TypeError('input end must be boolean');
    const input = Uint8Array.from(bytes);
    if (input.length > 8 * 1024 * 1024) throw new RangeError('input batch exceeds capacity');
    const pointer = this.#core.wire_input(this.#handle, input.length);
    if (!pointer) throw this.#error(1);
    new Uint8Array(this.#core.memory.buffer, pointer, input.length).set(input);
    const status = this.#core.wire_write_input(this.#handle, Number(end));
    if (status) throw this.#error(status);
  }

  readOutput(endpoint = 1, limit = 65536, processId) {
    this.#assertOpen();
    if (this.#running || this.#family.running) throw new Error('machine is already running');
    const owner = this.#processOwner(processId);
    if (owner !== this) return owner.readOutput(endpoint, limit);
    if (endpoint !== 1 && endpoint !== 2) throw new RangeError('invalid output endpoint');
    if (!Number.isSafeInteger(limit) || limit < 0 || limit > 1024 * 1024) throw new RangeError('invalid output batch size');
    const length = this.#core.wire_read_output(this.#handle, endpoint, limit);
    if (length === 0xffffffff) throw this.#error(1);
    if (!length) return new Uint8Array();
    const pointer = this.#core.wire_input(this.#handle, length);
    if (!pointer) throw this.#error(1);
    return new Uint8Array(this.#core.memory.buffer, pointer, length).slice();
  }

  windows(processId) {
    this.#assertOpen();
    if (this.#running || this.#family.running) throw new Error('machine is already running');
    const owner = this.#processOwner(processId);
    if (owner !== this) return owner.windows();
    const length = this.#core.wire_windows(this.#handle);
    if (length === 0xffffffff) throw this.#error(1);
    if (!length) return [];
    const pointer = this.#core.wire_input(this.#handle, length);
    if (!pointer) throw this.#error(1);
    const bytes = new Uint8Array(this.#core.memory.buffer, pointer, length);
    const view = new DataView(bytes.buffer, bytes.byteOffset, length), result = [];
    for (let at = 0; at < length;) {
      const handle = view.getBigUint64(at, true), width = view.getUint32(at+8, true), height = view.getUint32(at+12, true);
      const flags = view.getUint32(at+16, true), size = view.getUint32(at+20, true);
      const title = new TextDecoder().decode(bytes.subarray(at+24, at+24+size));
      result.push({handle, width, height, visible: !!(flags&1), topmost: !!(flags&2), title});
      at += 24+size;
    }
    return result;
  }

  windowPixels(window, processId) {
    this.#assertOpen();
    if (this.#running || this.#family.running) throw new Error('machine is already running');
    const owner = this.#processOwner(processId);
    if (owner !== this) return owner.windowPixels(window);
    if (typeof window !== 'bigint') throw new TypeError('window handle must be BigInt');
    const length = this.#core.wire_window_pixels(this.#handle, u64(window));
    if (length === 0xffffffff) throw this.#error(1);
    if (!length) return null;
    const pointer = this.#core.wire_input(this.#handle, length);
    if (!pointer) throw this.#error(1);
    const view = new DataView(this.#core.memory.buffer, pointer, length);
    const width = view.getUint32(0, true), height = view.getUint32(4, true), stride = view.getUint32(8, true);
    return {width, height, stride, rgba: new Uint8Array(this.#core.memory.buffer, pointer+16, length-16).slice()};
  }

  windowOrigin(window, processId) {
    this.#assertOpen();
    if (this.#running || this.#family.running) throw new Error('machine is already running');
    const owner = this.#processOwner(processId);
    if (owner !== this) return owner.windowOrigin(window);
    if (typeof window !== 'bigint') throw new TypeError('window handle must be BigInt');
    const length = this.#core.wire_window_origin(this.#handle, u64(window));
    if (length === 0xffffffff) throw this.#error(1);
    if (!length) return null;
    const pointer = this.#core.wire_input(this.#handle, length);
    if (!pointer) throw this.#error(1);
    const view = new DataView(this.#core.memory.buffer, pointer, 8);
    return [view.getInt32(0, true), view.getInt32(4, true)];
  }

  queueWindowEvent(event, processId) { this.queueWindowEvents([event], processId); }

  queueWindowEvents(events, processId) {
    this.#assertOpen();
    if (this.#running || this.#family.running) throw new Error('machine is already running');
    const owner = this.#processOwner(processId);
    if (owner !== this) return owner.queueWindowEvents(events);
    if (!Array.isArray(events) || events.length > 256) throw new RangeError('invalid host event batch');
    const integer = (value, min, max) => {
      if (!Number.isInteger(value) || value < min || value > max) throw new RangeError('invalid host event integer');
      return value;
    };
    const pointerValue = value => {
      if (typeof value !== 'bigint') throw new TypeError('host event pointers must be BigInt');
      return u64(value);
    };
    const bytes = new Uint8Array(events.length * 48), view = new DataView(bytes.buffer);
    let time;
    for (let n = 0; n < events.length; n++) {
      const event = events[n], at = n*48;
      view.setBigUint64(at, pointerValue(event.window), true);
      view.setUint32(at+8, integer(event.message, 0, 0xffffffff), true);
      view.setBigUint64(at+16, pointerValue(event.wparam ?? 0n), true);
      view.setBigUint64(at+24, pointerValue(event.lparam ?? 0n), true);
      const stamp = event.time === undefined ? (time ??= Number((this.#readClock()/10000n)&0xffffffffn)) : integer(event.time,0,0xffffffff);
      view.setUint32(at+32, stamp, true);
      if (event.point !== undefined) {
        if (!Array.isArray(event.point) || event.point.length !== 2) throw new TypeError('invalid host event point');
        view.setUint32(at+12, 1, true);
        view.setInt32(at+36, integer(event.point[0], -2147483648, 2147483647), true);
        view.setInt32(at+40, integer(event.point[1], -2147483648, 2147483647), true);
      }
    }
    const pointer = this.#core.wire_input(this.#handle, bytes.length);
    if (!pointer) throw this.#error(1);
    new Uint8Array(this.#core.memory.buffer, pointer, bytes.length).set(bytes);
    const status = this.#core.wire_queue_window_events(this.#handle);
    if (status) throw this.#error(status);
  }

  #supply(name, bytes, commit) {
    this.#assertOpen();
    if (this.#running || this.#family.running) throw new Error('machine is already running');
    if (typeof name !== 'string' || name.includes('\0')) throw new TypeError('invalid module identity');
    const input = Uint8Array.from(bytes), text = new TextEncoder().encode(name);
    const pointer = this.#core.wire_input(this.#handle, text.length + input.length);
    if (!pointer) throw this.#error(1);
    const buffer = new Uint8Array(this.#core.memory.buffer, pointer, text.length + input.length);
    buffer.set(text); buffer.set(input, text.length);
    const status = commit(this.#handle, text.length);
    if (status) throw this.#error(status);
  }

  moduleBase(name) {
    this.#assertOpen();
    if (typeof name !== 'string' || name.includes('\0')) throw new TypeError('invalid module identity');
    const input = new TextEncoder().encode(name);
    const pointer = this.#core.wire_input(this.#handle, input.length);
    if (!pointer) throw this.#error(1);
    new Uint8Array(this.#core.memory.buffer, pointer, input.length).set(input);
    return unsigned(this.#core.wire_image_base(this.#handle));
  }

  /** Initialize one PE executable; positive run budgets include its services. */
  load(bytes, { base = 0n, command = '"C:\\program.exe"', diagnosticImports = false } = {}) {
    this.#assertOpen();
    if (this.#running || this.#family.running) throw new Error('machine is already running');
    return this.#loadImage(bytes,{base,command,diagnosticImports});
  }

  async loadAsync(bytes,options={}) {
    this.#assertOpen();
    if(this.#running || this.#family.running || this.#loading)throw Error('machine is already running');
    const input=Uint8Array.from(bytes),configured={...options};
    this.#loading=true;this.#family.running=true;
    try{for(;;){try{return this.#loadImage(input,configured);}catch(error){if(!error.fileInput)throw error;await this.#readFileInput();}}}
    finally{this.#loading=false;this.#family.running=false;}
  }

  #loadImage(bytes,{base=0n,command='"C:\\program.exe"',diagnosticImports=false}={}) {
    if (typeof command !== 'string' || command.includes('\0')) throw new TypeError('invalid command line');
    if (typeof diagnosticImports !== 'boolean') throw new TypeError('diagnosticImports must be boolean');
    base = u64(base);
    const input = Uint8Array.from(bytes), text = new TextEncoder().encode(command);
    const pointer = this.#core.wire_input(this.#handle, text.length + input.length);
    if (!pointer) throw this.#error(1);
    const buffer = new Uint8Array(this.#core.memory.buffer, pointer, text.length + input.length);
    buffer.set(text); buffer.set(input, text.length);
    const status = diagnosticImports
      ? this.#core.wire_load_pe_diagnostic(this.#handle, base, text.length)
      : this.#core.wire_load_pe(this.#handle, base, text.length);
    if(status===3){const error=Error('PE loading requires file input');error.fileInput=true;throw error;}
    if (status) throw this.#error(status);
    this.#image = this.#imageInfo();
    this.#current = 0n;
    this.#sync();
    return this.#image;
  }

  #imageInfo() {
    const info = Array.from(new BigUint64Array(this.#core.memory.buffer, this.#core.wire_image_info(this.#handle), 10));
    const [imageBase, size, entry, sp, teb, peb, commandA, commandW, tls, tlsArray] = info;
    return Object.freeze({ base: imageBase, size, entry, sp, teb, peb, commandA, commandW, tls, tlsArray,
      importTraps: this.#core.wire_import_trap_count(this.#handle) });
  }

  #collectChildren() {
    for (;;) {
      const mode = this.#core.wire_child_mode(this.#handle);
      if (!mode) { this.#hasChildren = false; return; }
      const child = new WasmMachine(this.#core, {mode, clock: this.#clock, wallClock: this.#wallClock, cpuTimes: this.#cpuTimes, random: this.#random});
      try {
        if (this.#core.wire_adopt_child(this.#handle, child.#handle)) throw new Error('could not adopt child process');
        child.#image = child.#imageInfo();
        child.#family = this.#family;
        child.#inheritedGraphics = this.#graphicsOptions ?? this.#inheritedGraphics;
        child.#inheritedAudio = this.#audioOptions ?? this.#inheritedAudio;
        this.#family.members.push(child);
      } catch (error) { child.#disposeSelf(); throw error; }
    }
  }

  bind(dll, name) {
    this.#assertOpen();
    if (typeof dll !== 'string' || typeof name !== 'string' || dll.includes('\0') || name.includes('\0')) throw new TypeError('invalid Windows export identity');
    const input = new TextEncoder().encode(`${dll}\0${name}`);
    const pointer = this.#core.wire_input(this.#handle, input.length);
    if (!pointer) throw this.#error(1);
    new Uint8Array(this.#core.memory.buffer, pointer, input.length).set(input);
    const address = unsigned(this.#core.wire_bind(this.#handle));
    if (!address) throw this.#error(1);
    return address;
  }

  /** Complete one bound service while execution is suspended at its gateway. */
  service() {
    this.#assertOpen();
    if (this.#running || this.#family.running) throw new Error('machine is already running');
    this.#running = true;
    try {
      const audioFault=this.#audio && this.#pumpAudio();
      if (audioFault) return {reason:Exit.Fault,work:0n,ip:this.ip,fault:audioFault,audio:true};
      const result = this.#dispatchService();
      delete result.loader;
      return result;
    } finally { this.#running = false; this.#sync(); this.#retireAudio(); this.#retireGraphics(); this.#graphics?.submit(); }
  }

  #dispatchService(request = this.#core.wire_service_source(this.#handle), cpuTimes = this.#cpuTimes) {
    const source = request&127;
    if (!source) throw new Error('unbound Windows service gateway');
    let sample = 0n, wall = 0n;
    if (source === 2 || source === 4) sample = this.#readClock();
    if (source === 3 || source === 4) {
      wall = this.#wallClock();
      if (typeof wall !== 'bigint') throw new TypeError('wall clock must return BigInt FILETIME');
      u64(wall);
      if (source === 3) sample = wall;
    }
    const before = this.work;
    let status;
    const length = request&0x80 ? (request >>> 8) + 1 : request&0x100 ? 16 : 0;
    if (length) {
      const bytes = this.#random(length);
      if (bytes === null) status = this.#core.wire_service_random(this.#handle, 0n, 0n, 0);
      else {
        if (!(bytes instanceof Uint8Array) || bytes.length !== length) throw new TypeError('random byte function must return the requested Uint8Array or null');
        if (length === 16) {
          const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
          status = this.#core.wire_service_random(this.#handle, view.getBigUint64(0,true), view.getBigUint64(8,true), 1);
        } else {
          const input = bytes.buffer === this.#core.memory.buffer ? bytes.slice() : bytes;
          const pointer = this.#core.wire_input(this.#handle, length);
          if (!pointer) throw this.#error(1);
          new Uint8Array(this.#core.memory.buffer, pointer, length).set(input);
          status = this.#core.wire_service_random_bytes(this.#handle, 1);
        }
      }
    } else if (source === 5) {
      let times;
      this.#cpuSampling = true;
      try { times = cpuTimes(); }
      finally { this.#cpuSampling = false; }
      if (times === null) status = this.#core.wire_service_cpu(this.#handle, 0n, 0n, 0n, 50);
      else if (!Array.isArray(times) && times && Number.isInteger(times.error) && times.error > 0 && times.error <= 0xffffffff) status = this.#core.wire_service_cpu(this.#handle, 0n, 0n, 0n, times.error);
      else {
        if (times && typeof times.then === 'function') { Promise.resolve(times).catch(() => {}); throw new TypeError('asynchronous CPU times require run()'); }
        if (!Array.isArray(times) || times.length !== 3 || [...times].some(n => typeof n !== 'bigint')) throw new TypeError('CPU times must return three BigInt durations or null');
        const [idle, kernel, user] = times.map(u64);
        if (kernel < idle) throw new RangeError('CPU kernel time must include idle time');
        status = this.#core.wire_service_cpu(this.#handle, idle, kernel, user, 0);
      }
    } else status = source === 4 ? this.#core.wire_service_timed(this.#handle, sample, wall) : this.#core.wire_service(this.#handle, sample);
    if (status === 1) return this.#stopped(Exit.Fault, 0n);
    if (status === 7) return this.#stopped(Exit.Graphics, 0n);
    if (status === 8) return this.#stopped(Exit.FileInput, 0n);
    if (status === 3 || status === 4) return { ...this.#stopped(Exit.Waiting, 0n), loader: status === 4 };
    if (status === 5) return this.#stopped(Exit.ProcessExited, unsigned(this.work - before));
    if (status === 6) {
      this.#hasChildren = true;
      this.#collectChildren();
      return this.#stopped(Exit.Dispatch, unsigned(this.work - before));
    }
    if (status) throw this.#error(status);
    return this.#stopped(Exit.Dispatch, unsigned(this.work - before));
  }

  #field(offset, value) {
    this.#assertOpen();
    const view = new DataView(this.#core.memory.buffer);
    const pointer = this.#core.wire_state(this.#handle) + offset;
    if (value === undefined) return view.getBigUint64(pointer, true);
    const integer = u64(value);
    this.#assertCPUWritable();
    this.#core.wire_cancel_repeat(this.#handle);
    view.setBigUint64(pointer, integer, true);
  }

  get ip() { return this.#field(this.#layout[3]); }
  set ip(value) { this.#field(this.#layout[3], BigInt.asUintN(this.#mode, BigInt(value))); }
  get flags() { return this.#field(this.#layout[2]); }
  set flags(value) { this.#field(this.#layout[2], value); }
  get work() { return this.#field(this.#layout[4]); }
  get retired() { return this.#field(this.#layout[5]); }
  get processExitCode() {
    this.#assertOpen();
    const code = unsigned(this.#core.wire_process_exit_code(this.#handle));
    return code === MAX_U64 ? null : Number(code);
  }

  get mxcsr() {
    this.#assertOpen();
    return new DataView(this.#core.memory.buffer).getUint32(this.#core.wire_state(this.#handle)+this.#layout[10],true);
  }
  set mxcsr(value) {
    this.#assertOpen();
    if (!Number.isInteger(value) || value < 0 || value > 0xffff) throw new RangeError('invalid MXCSR');
    this.#assertCPUWritable();
    this.#core.wire_cancel_repeat(this.#handle);
    new DataView(this.#core.memory.buffer).setUint32(this.#core.wire_state(this.#handle)+this.#layout[10],value,true);
  }

  register(index, value) {
    if (!Number.isInteger(index) || index < 0 || index >= (this.#mode === 32 ? 8 : 16)) throw new RangeError('invalid register');
    return this.#field(this.#layout[1] + index * 8, value === undefined ? undefined : BigInt.asUintN(this.#mode, BigInt(value)));
  }

  xmm(index, value) {
    this.#assertOpen();
    if (!Number.isInteger(index) || index < 0 || index >= (this.#mode === 32 ? 8 : 16)) throw new RangeError('invalid XMM register');
    const input = value === undefined ? null : Uint8Array.from(value);
    if (input && input.length !== 16) throw new RangeError('expected 16 XMM bytes');
    const pointer = this.#core.wire_state(this.#handle) + this.#layout[9] + index * 16;
    const bytes = new Uint8Array(this.#core.memory.buffer, pointer, 16);
    if (!input) return bytes.slice();
    this.#assertCPUWritable();
    this.#core.wire_cancel_repeat(this.#handle);
    bytes.set(input);
  }

  map(address, size, permissions = 3) {
    this.#assertOpen();
    if (!Number.isInteger(permissions) || permissions < 0 || permissions > 7) throw new RangeError('invalid memory permissions');
    const status = this.#core.wire_map(this.#handle, u64(address), u64(size), permissions);
    if (status) throw this.#error(status);
  }

  write(address, bytes) {
    this.#assertOpen();
    // Copy before a core allocation can grow/detach an input view of its own memory.
    const input = Uint8Array.from(bytes);
    const pointer = this.#core.wire_input(this.#handle, input.length);
    if (!pointer) throw this.#error(1);
    new Uint8Array(this.#core.memory.buffer, pointer, input.length).set(input);
    const status = this.#core.wire_write(this.#handle, u64(address));
    if (status) throw this.#error(status);
  }

  readInt(address, bytes = 4) {
    this.#assertOpen();
    if (![1, 2, 4, 8].includes(bytes)) throw new RangeError('invalid integer width');
    const value = unsigned(this.#core.wire_load(this.#handle, u64(address), bytes, 1));
    if (this.#core.wire_fault_kind(this.#handle)) throw new Error(`memory access failed at ${BigInt(address).toString(16)}`);
    return value;
  }

  setCodeLimit(bytes) {
    this.#assertOpen();
    const status = this.#core.wire_code_limit(this.#handle, byteLimit(bytes));
    if (status) throw this.#error(status);
    this.#sync();
  }

  stats() {
    this.#assertOpen();
    this.#sync();
    return { modules: this.#core.wire_code_count(this.#handle), chargedBytes: this.#core.wire_code_used(this.#handle), instances: this.#records.size };
  }

  #sync() {
    if (this.#closed) return;
    const epoch = unsigned(this.#core.wire_code_epoch(this.#handle));
    if (epoch !== this.#epoch || epoch === MAX_U64) {
      const pointer = this.#core.wire_code_retired(this.#handle);
      if (!pointer) throw this.#error(1);
      const view = new DataView(this.#core.memory.buffer);
      const count = view.getBigUint64(pointer, true);
      if (count === MAX_U64) {
        for (const [ticket, record] of this.#records) {
          if (!this.#core.wire_live(this.#handle, ticket)) { this.#clearSlot(record); this.#records.delete(ticket); }
        }
      } else {
        for (let index = 0; index < Number(count); index++) {
          const ticket = view.getBigUint64(pointer + 8 + index * 8, true);
          const record = this.#records.get(ticket);
          if (record) { this.#clearSlot(record); this.#records.delete(ticket); }
        }
      }
      this.#epoch = unsigned(this.#core.wire_code_epoch(this.#handle));
    }
  }

  #clearSlot(record) {
    if (!record.slot) return;
    this.#table.set(record.slot, null);
    this.#freeSlots.push(record.slot);
    record.slot = 0;
  }

  #resident() {
    if (!this.#driver) this.#driver = driverModule(this.#core).then(module => WebAssembly.instantiate(module, { core: this.#core, jit: { functions: this.#table } }));
    return this.#driver;
  }

  #install(ticket) {
    this.#sync();
    const retained = this.#records.get(ticket);
    if (retained) return retained.promise;
    if (!this.#core.wire_pin(this.#handle, ticket)) return Promise.resolve(null);
    // Publish one promise per immutable module, and retain its Rust owner until
    // compilation and source revalidation finish, including during disposal.
    const record = { slot: 0, promise: null };
    record.promise = (async () => {
      try {
        const pointer = this.#core.wire_module(this.#handle, ticket);
        const length = this.#core.wire_module_len(this.#handle, ticket);
        const bytes = new Uint8Array(this.#core.memory.buffer, pointer, length).slice();
        const resident = await this.#resident();
        if (this.#closed) return null;
        const module = new WebAssembly.Module(bytes);
        const instance = new WebAssembly.Instance(module, { core: this.#core, jit: resident.exports, platform: this.#platform });
        if (this.#closed || !this.#core.wire_live(this.#handle, ticket)) return null;
        record.slot = this.#freeSlots.pop() ?? this.#table.grow(1);
        this.#table.set(record.slot, instance.exports.chunk);
        if (!this.#core.wire_publish(this.#handle, ticket, record.slot)) { this.#clearSlot(record); return null; }
        return instance;
      } catch (error) {
        this.#clearSlot(record);
        this.#records.delete(ticket);
        throw error;
      } finally {
        this.#core.wire_release(this.#handle, ticket);
        this.#sync();
      }
    })();
    this.#records.set(ticket, record);
    return record.promise;
  }

  #stopped(reason, work) {
    if (reason === Exit.HostError) {
      const failure = this.#clockError;
      this.#clockError = null;
      if (!failure) throw new Error('host clock failed');
      throw failure.error;
    }
    const result = { reason, work, ip: this.ip };
    if (reason === Exit.ProcessExited) result.exitCode = this.processExitCode;
    if (reason === Exit.Fault) result.fault = { kind: this.#core.wire_fault_kind(this.#handle), address: unsigned(this.#core.wire_fault_address(this.#handle)), access: this.#core.wire_fault_access(this.#handle) };
    if (reason === Exit.Exception) {
      const code = unsigned(this.#core.wire_exception_code(this.#handle));
      result.exception = code ? { code: Number(code - 1n) } : { vector: this.#core.wire_exception(this.#handle) - 1 };
    }
    return result;
  }

  /** Execute the process family within one shared work budget. */
  async run(budget) {
    this.#assertOpen(); budget = u64(budget);
    if (this.#family.running) throw new Error('machine is already running');
    if (budget === 0n) return this.#stopped(Exit.Budget, 0n);
    const family = this.#family, waiting = new Set();
    let completed = 0n;
    family.running = true;
    try {
      for (;;) {
        this.#assertOpen();
        for (const member of family.members) if (member.#hasChildren) member.#collectChildren();
        const available = family.members.filter(member => !member.#finished);
        if (!available.length) return {...family.members[0].#stopped(Exit.ProcessExited, completed), processId: family.members[0].processId};
        if (available.every(member => waiting.has(member))) {
          const member = available[0];
          return {...member.#stopped(Exit.Waiting, completed), processId: member.processId};
        }
        let member;
        do { member = family.members[family.cursor++ % family.members.length]; }
        while (member.#finished || waiting.has(member));
        const remaining = budget-completed;
        const allowance = family.members.length === 1 || remaining < 10000n ? remaining : 10000n;
        let result;
        try { result = await member.#runProcess(allowance); }
        catch (error) { if (family.members.length > 1) error.processId = member.processId; throw error; }
        completed += result.work;
        if (family.members.length === 1) return result;
        if (result.work) waiting.clear();
        if (result.reason === Exit.ProcessExited) { member.#finished = true; waiting.clear(); }
        else if (result.reason === Exit.Returned || result.reason === Exit.ThreadReturned) {
          if (!member.#entryFinished) {
            const status = member.#core.wire_finish_process_entry(member.#handle);
            if (status) throw member.#error(status);
            member.#entryFinished = true; waiting.clear();
          } else throw new Error('process shutdown returned without an exit');
        } else if (result.reason === Exit.Waiting) waiting.add(member);
        else if (result.reason !== Exit.Budget && result.reason !== Exit.Dispatch) {
          return {...result, work: completed, processId: member.processId};
        }
        if (completed === budget) return {...member.#stopped(Exit.Budget, completed), processId: member.processId};
      }
    } finally { family.running = false; }
  }

  async #runProcess(budget) {
    this.#assertOpen();
    budget = u64(budget);
    if (this.#running) throw new Error('machine is already running');
    if (budget === 0n) return this.#stopped(Exit.Budget, 0n);
    this.#running = true;
    let completed = 0n;
    let resume = true;
    const schedule = flags => {
      const status = this.#core.wire_schedule(this.#handle, flags);
      if (status === 1) return Exit.Fault;
      if (status === 3) return Exit.Waiting;
      if (status === 4) return Exit.ThreadReturned;
      if (status === 5) return Exit.ProcessExited;
      if (status === 6) return Exit.FileInput;
      if (status) throw this.#error(status);
      return Exit.Dispatch;
    };
    try {
      for (;;) {
        this.#assertOpen();
        this.#sync();
        const audioFault=this.#audio && this.#pumpAudio();
        if (audioFault) return {reason:Exit.Fault,work:completed,ip:this.ip,fault:audioFault,audio:true};
        const scheduled = schedule(resume ? 2 : 0);
        resume = false;
        if(scheduled===Exit.FileInput){await this.#readFileInput();continue;}
        if (scheduled !== Exit.Dispatch) return this.#stopped(scheduled, completed);
        const startup = this.#core.wire_startup(this.#handle);
        if(startup===4){await this.#readFileInput();continue;}
        if (startup === 1) return this.#stopped(Exit.Fault, completed);
        if (startup === 3) return this.#stopped(Exit.Exception, completed);
        if (startup) throw this.#error(startup);
        const boundary = this.#core.wire_boundary(this.#handle);
        if (boundary === Exit.Service && this.#image) {
          if (!this.#audio && this.#inheritedAudio && this.#core.wire_audio_required(this.#handle)) {
            await this.#attachAudio({...this.#inheritedAudio, sink: undefined}, true);
            this.#inheritedAudio = null;
          }
          if (!this.#graphics && this.#inheritedGraphics && this.#core.wire_graphics_required(this.#handle)) {
            await this.#attachGraphics(this.#inheritedGraphics);
            this.#inheritedGraphics = null;
          }
          const request = this.#core.wire_service_source(this.#handle);
          let cpuTimes = this.#cpuTimes;
          if ((request&127) === 5) {
            let sample;
            this.#cpuSampling = true;
            try { sample = await cpuTimes(); }
            finally { this.#cpuSampling = false; }
            this.#assertOpen();
            cpuTimes = () => sample;
          }
          const result = this.#dispatchService(request, cpuTimes);
          completed += result.work;
          if(result.reason===Exit.FileInput){await this.#readFileInput();continue;}
          if (result.reason === Exit.Graphics) {
            if (!this.#graphics) throw new Error('graphics continuation has no backend');
            await this.#graphics.flush();
            this.#assertOpen();
            continue;
          }
          if (result.reason === Exit.Waiting) {
            const scheduled = schedule(result.loader ? 3 : 1);
            if(scheduled===Exit.FileInput){await this.#readFileInput();continue;}
            if (scheduled !== Exit.Dispatch) return this.#stopped(scheduled, completed);
            continue;
          }
          if (result.reason !== Exit.Dispatch) return this.#stopped(result.reason, completed);
          if (completed === budget) return this.#stopped(Exit.Budget, completed);
          if (this.#family.members.length > 1) return this.#stopped(Exit.Dispatch, completed);
          continue;
        }
        if (boundary === Exit.ThreadReturned && this.#image) continue;
        if ((boundary === Exit.Callback || boundary === Exit.Returned) && this.#core.wire_startup_pending(this.#handle)) continue;
        if (boundary) return this.#stopped(boundary, completed);
        let ticket = this.#current;
        if (!this.#core.wire_ready(this.#handle, ticket)) {
          const status = this.#core.wire_compile(this.#handle);
          if (status === 1) {
            if (this.#core.wire_fetch_fault(this.#handle)) {
              const delivery = this.#core.wire_deliver_exception(this.#handle, Exit.Fault);
              if (delivery === 1) continue;
              if (delivery === 3) throw this.#error(delivery);
              return this.#stopped(Exit.Fault, completed);
            }
            continue;
          }
          if (status) throw this.#error(status);
          ticket = unsigned(this.#core.wire_ticket(this.#handle));
        }
        const instance = await this.#install(ticket);
        this.#assertOpen();
        if (!instance) { this.#current = 0n; continue; }
        const resident = await this.#resident();
        this.#assertOpen();
        const begin = this.#core.wire_begin(this.#handle, ticket);
        if (begin === 0) { this.#current = 0n; continue; }
        if (begin !== 1) throw this.#error(begin);
        this.#current = ticket;
        let before, reason;
        try {
          before = this.work;
          const quantum = unsigned(this.#core.wire_quantum(this.#handle));
          const allowance = quantum < budget - completed ? quantum : budget - completed;
          reason = resident.exports.run(this.#core.wire_state(this.#handle), allowance, this.#core.wire_cache(this.#handle), this.#handle);
        } finally { this.#core.wire_end(this.#handle); }
        const delta = unsigned(this.work - before);
        if (delta > budget - completed) throw new Error('generated module exceeded its work budget');
        completed += delta;
        if (reason === Exit.Budget && completed < budget) continue;
        if (reason === Exit.ThreadReturned && this.#image) {
          if (completed === budget) return this.#stopped(Exit.Budget, completed);
          continue;
        }
        if ((reason === Exit.Service && this.#image) || (reason === Exit.Callback && this.#core.wire_startup_pending(this.#handle))) {
          if (completed === budget) return this.#stopped(Exit.Budget, completed);
          continue;
        }
        if (this.#image && (reason === Exit.Fault || reason === Exit.Exception)) {
          if (completed === budget) return this.#stopped(Exit.Budget, completed);
          const delivery = this.#core.wire_deliver_exception(this.#handle, reason);
          if (delivery === 1) continue;
          if (delivery === 2) return this.#stopped(Exit.Fault, completed);
          if (delivery === 3) throw this.#error(delivery);
        }
        if (reason === Exit.Budget || reason === Exit.Unsupported || reason === Exit.Fault || reason === Exit.Exception || reason === Exit.HostError || reason === Exit.Service || reason === Exit.Returned || reason === Exit.Callback || reason === Exit.ThreadReturned) {
          return this.#stopped(reason, completed);
        }
        if (reason !== Exit.Dispatch && reason !== Exit.SourceChanged) throw new Error('unknown generated exit');
        if (completed === budget) return { reason: Exit.Budget, work: completed, ip: this.ip };
        if (delta === 0n) {
          if (!this.#core.wire_live(this.#handle, ticket)) { this.#current = 0n; continue; }
          throw new Error('generated module made no progress');
        }
      }
    } finally { this.#running = false; this.#sync(); this.#retireAudio(); this.#retireGraphics(); this.#graphics?.submit(); }
  }

  dispose() {
    for (const member of this.#family.members) member.#disposeSelf();
    this.#family.fileReaders.clear();
  }

  #releaseGraphics() {
    this.#graphics?.close();
    this.#graphics = null;
    if (this.#graphicsSlot) {
      const table = this.#core.__indirect_function_table;
      table.set(this.#graphicsSlot, null);
      graphicsSlots.get(table).push(this.#graphicsSlot);
      this.#graphicsSlot = 0;
    }
  }

  #retireGraphics() {
    if (this.#graphics && !this.#core.wire_graphics_attached(this.#handle)) this.#releaseGraphics();
  }

  #disposeSelf() {
    if (this.#closed) return;
    this.#closed = true;
    for (const record of this.#records.values()) this.#clearSlot(record);
    this.#records.clear();
    this.#driver = null;
    this.#core.wire_destroy(this.#handle);
    this.#audio?.close();
    this.#audio = null;
    this.#audioOptions = null;
    this.#inheritedAudio = null;
    this.#releaseGraphics();
    this.#graphicsOptions = null;
    this.#inheritedGraphics = null;
  }
}
